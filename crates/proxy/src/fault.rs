//! Faults: what the proxy answers in the store's place, so that a program
//! meets the failures S3 documents before production does.
//!
//! A rule matches by operation and key, never by count of arrival ("the third
//! UploadPart" is a race in a concurrent program). What is counted is the
//! attempts at one request (its method, place and part): a rule with
//! `times = 1` fails each request's first attempt and lets its retry through,
//! so a program that retries passes and one that does not is seen not to.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// What a rule does to a request it matches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Effect {
    /// Answered with this error; the store never sees the request.
    Status { status: u16, code: String },
    /// Sent to the store, and its answer replaced by a 500: the write
    /// happened and the program is not told (the ambiguous write).
    LostAnswer,
    /// Sent to the store after this long.
    Delay { ms: u64 },
    /// S3's documented rate for one prefix: past it, 503 SlowDown.
    Rate { reads: u32, writes: u32 },
}

/// One rule: which requests, what happens to them, how many times each.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    /// Operation names; one ending in `*` matches by its start. Empty: all.
    #[serde(default)]
    pub on: Vec<String>,
    /// Keys under this prefix only (empty: all; a request without a key
    /// matches only an empty prefix).
    #[serde(default)]
    pub prefix: String,
    pub effect: Effect,
    /// Attempts at one request that meet the fault; 0 is every attempt.
    #[serde(default)]
    pub times: u32,
}

/// A named set of rules: what one trial run is put through.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Scenario {
    pub name: String,
    /// What it does, in a line.
    pub about: String,
    pub rules: Vec<Rule>,
}

const WRITES: &[&str] = &[
    "PutObject",
    "CopyObject",
    "DeleteObject",
    "DeleteObjects",
    "UploadPart",
    "UploadPartCopy",
    "CompleteMultipartUpload",
];

fn rule(on: &[&str], effect: Effect, times: u32) -> Rule {
    Rule {
        on: on.iter().map(|s| s.to_string()).collect(),
        prefix: String::new(),
        effect,
        times,
    }
}

fn status(status: u16, code: &str) -> Effect {
    Effect::Status {
        status,
        code: code.into(),
    }
}

/// The scenarios sonde knows by name.
pub fn scenarios() -> Vec<Scenario> {
    let s = |name: &str, about: &str, rules: Vec<Rule>| Scenario {
        name: name.into(),
        about: about.into(),
        rules,
    };
    vec![
        s(
            "slowdown",
            "503 SlowDown on each request's first attempt",
            vec![rule(&[], status(503, "SlowDown"), 1)],
        ),
        s(
            "error",
            "500 InternalError on each request's first attempt",
            vec![rule(&[], status(500, "InternalError"), 1)],
        ),
        s(
            "lost-answer",
            "each write reaches the store, and its first answer is lost",
            vec![rule(WRITES, Effect::LostAnswer, 1)],
        ),
        s(
            "fail-complete",
            "CompleteMultipartUpload fails once",
            vec![rule(
                &["CompleteMultipartUpload"],
                status(500, "InternalError"),
                1,
            )],
        ),
        s(
            "slow",
            "every request waits 300 ms more",
            vec![rule(&[], Effect::Delay { ms: 300 }, 0)],
        ),
        s(
            "s3-limits",
            "S3's rate for one prefix: 5,500 reads and 3,500 writes a second",
            vec![rule(
                &[],
                Effect::Rate {
                    reads: 5500,
                    writes: 3500,
                },
                0,
            )],
        ),
    ]
}

/// A scenario by its name.
pub fn scenario(name: &str) -> Option<Scenario> {
    scenarios().into_iter().find(|s| s.name == name)
}

/// A fault as written in `sonde.toml` or on the command line: `503 SlowDown`,
/// `500`, `lost-answer`, `delay 200ms`.
pub fn parse_effect(text: &str) -> Result<Effect, String> {
    let t = text.trim();
    let mut words = t.split_whitespace();
    let first = words.next().unwrap_or("");
    let second = words.next();
    if first == "lost-answer" && second.is_none() {
        return Ok(Effect::LostAnswer);
    }
    if first == "delay" {
        let v = second.ok_or("delay: how long? (delay 200ms)")?;
        let ms = match v.strip_suffix("ms") {
            Some(n) => n.parse::<u64>().ok(),
            None => v
                .strip_suffix('s')
                .and_then(|n| n.parse::<f64>().ok())
                .filter(|s| *s >= 0.0 && *s <= 3600.0)
                .map(|s| (s * 1000.0) as u64),
        };
        return ms
            .map(|ms| Effect::Delay { ms })
            .ok_or_else(|| format!("delay {v}: a time such as 200ms or 2s"));
    }
    if let Ok(code) = first.parse::<u16>() {
        if !(400..600).contains(&code) {
            return Err(format!("{code}: a fault's status is 400 to 599"));
        }
        let name = second.map(str::to_string).unwrap_or_else(|| {
            match code {
                503 => "SlowDown",
                500 => "InternalError",
                403 => "AccessDenied",
                404 => "NoSuchKey",
                412 => "PreconditionFailed",
                _ => "InternalError",
            }
            .to_string()
        });
        return Ok(Effect::Status {
            status: code,
            code: name,
        });
    }
    Err(format!(
        "{t}: a fault is a status (503 SlowDown), lost-answer, or delay 200ms"
    ))
}

/// What the proxy does with one request.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Answer this error now.
    Answer { status: u16, code: String },
    /// Send it on, then answer 500 whatever the store said.
    LoseAnswer,
    /// Wait, then send it on.
    Wait(Duration),
}

/// A scenario's rules at work in one proxy, with what they have counted.
pub struct Faults {
    pub name: String,
    rules: Vec<Rule>,
    /// Attempts met, by rule and request.
    met: Mutex<HashMap<(usize, String), u32>>,
    /// Requests in the current second, by rule, prefix and kind.
    rate: Mutex<HashMap<(usize, String, bool), (Instant, u32)>>,
}

fn matches_op(on: &[String], op: &str) -> bool {
    on.is_empty()
        || on.iter().any(|p| match p.strip_suffix('*') {
            Some(start) => op.starts_with(start),
            None => p == op,
        })
}

/// A key's prefix as S3 partitions by it for its rate: the bucket and the
/// key's folder.
fn rate_prefix(bucket: Option<&str>, key: Option<&str>) -> String {
    let folder = key
        .and_then(|k| k.rfind('/').map(|i| &k[..=i]))
        .unwrap_or("");
    format!("{}/{folder}", bucket.unwrap_or(""))
}

impl Faults {
    pub fn new(scenario: &Scenario) -> Self {
        Faults {
            name: scenario.name.clone(),
            rules: scenario.rules.clone(),
            met: Mutex::new(HashMap::new()),
            rate: Mutex::new(HashMap::new()),
        }
    }

    /// What happens to this request: the first rule that matches and still
    /// has attempts to meet.
    pub fn decide(
        &self,
        method: &str,
        op: &str,
        bucket: Option<&str>,
        key: Option<&str>,
        query: &[(String, String)],
    ) -> Option<Verdict> {
        for (i, r) in self.rules.iter().enumerate() {
            if !matches_op(&r.on, op) {
                continue;
            }
            if !r.prefix.is_empty() && !key.is_some_and(|k| k.starts_with(&r.prefix)) {
                continue;
            }
            if let Effect::Rate { reads, writes } = r.effect {
                let read = method == "GET" || method == "HEAD";
                let limit = if read { reads } else { writes };
                let mut rate = self.rate.lock();
                let slot = rate
                    .entry((i, rate_prefix(bucket, key), read))
                    .or_insert((Instant::now(), 0));
                if slot.0.elapsed() >= Duration::from_secs(1) {
                    *slot = (Instant::now(), 0);
                }
                slot.1 += 1;
                if slot.1 > limit {
                    return Some(Verdict::Answer {
                        status: 503,
                        code: "SlowDown".into(),
                    });
                }
                continue;
            }
            if r.times > 0 {
                // one request: its method, its place, its part. An upload's
                // id is the store's and differs between attempts at the
                // upload, so it is not part of the name.
                let part = query
                    .iter()
                    .find(|(k, _)| k == "partNumber")
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("");
                let listing: String = if key.is_none() {
                    query
                        .iter()
                        .filter(|(k, _)| {
                            k == "prefix" || k == "continuation-token" || k == "marker"
                        })
                        .map(|(k, v)| format!("&{k}={v}"))
                        .collect()
                } else {
                    String::new()
                };
                let id = format!(
                    "{method} {op} {}/{} {part}{listing}",
                    bucket.unwrap_or(""),
                    key.unwrap_or("")
                );
                let mut met = self.met.lock();
                let n = met.entry((i, id)).or_insert(0);
                if *n >= r.times {
                    continue;
                }
                *n += 1;
            }
            return Some(match &r.effect {
                Effect::Status { status, code } => Verdict::Answer {
                    status: *status,
                    code: code.clone(),
                },
                Effect::LostAnswer => Verdict::LoseAnswer,
                Effect::Delay { ms } => Verdict::Wait(Duration::from_millis(*ms)),
                Effect::Rate { .. } => unreachable!("handled above"),
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decide(f: &Faults, method: &str, op: &str, key: &str) -> Option<Verdict> {
        f.decide(method, op, Some("b"), Some(key), &[])
    }

    #[test]
    fn each_requests_first_attempt_fails_and_its_retry_passes() {
        let f = Faults::new(&scenario("slowdown").unwrap());
        let slow = Some(Verdict::Answer {
            status: 503,
            code: "SlowDown".into(),
        });
        assert_eq!(decide(&f, "PUT", "PutObject", "a"), slow);
        assert_eq!(decide(&f, "PUT", "PutObject", "a"), None, "the retry");
        // another request, whenever it arrives, meets it once too
        assert_eq!(decide(&f, "PUT", "PutObject", "b"), slow);
        assert_eq!(decide(&f, "GET", "GetObject", "a"), slow);
        assert_eq!(decide(&f, "GET", "GetObject", "a"), None);
    }

    #[test]
    fn parts_of_one_upload_are_requests_of_their_own() {
        let f = Faults::new(&scenario("error").unwrap());
        let part = |n: &str| {
            f.decide(
                "PUT",
                "UploadPart",
                Some("b"),
                Some("k"),
                &[
                    ("partNumber".into(), n.into()),
                    ("uploadId".into(), "u".into()),
                ],
            )
        };
        assert!(part("1").is_some() && part("2").is_some());
        assert!(part("1").is_none() && part("2").is_none());
    }

    #[test]
    fn a_rule_is_for_its_operations_and_its_prefix() {
        let f = Faults::new(&Scenario {
            name: "t".into(),
            about: String::new(),
            rules: vec![Rule {
                on: vec!["Put*".into()],
                prefix: "wal/".into(),
                effect: Effect::LostAnswer,
                times: 0,
            }],
        });
        assert_eq!(
            decide(&f, "PUT", "PutObject", "wal/1"),
            Some(Verdict::LoseAnswer)
        );
        assert_eq!(
            decide(&f, "PUT", "PutObject", "wal/1"),
            Some(Verdict::LoseAnswer)
        );
        assert_eq!(decide(&f, "PUT", "PutObject", "sst/1"), None);
        assert_eq!(decide(&f, "GET", "GetObject", "wal/1"), None);
    }

    #[test]
    fn past_a_prefixes_rate_the_answer_is_slow_down() {
        let f = Faults::new(&Scenario {
            name: "t".into(),
            about: String::new(),
            rules: vec![rule(
                &[],
                Effect::Rate {
                    reads: 2,
                    writes: 1,
                },
                0,
            )],
        });
        assert_eq!(decide(&f, "GET", "GetObject", "p/a"), None);
        assert_eq!(decide(&f, "GET", "GetObject", "p/b"), None);
        assert!(decide(&f, "GET", "GetObject", "p/c").is_some());
        assert_eq!(
            decide(&f, "GET", "GetObject", "q/a"),
            None,
            "another prefix"
        );
        assert_eq!(
            decide(&f, "PUT", "PutObject", "p/a"),
            None,
            "writes have their own"
        );
        assert!(decide(&f, "PUT", "PutObject", "p/b").is_some());
    }

    #[test]
    fn faults_as_written() {
        assert_eq!(
            parse_effect("503 SlowDown").unwrap(),
            status(503, "SlowDown")
        );
        assert_eq!(parse_effect("500").unwrap(), status(500, "InternalError"));
        assert_eq!(parse_effect("lost-answer").unwrap(), Effect::LostAnswer);
        assert_eq!(
            parse_effect("delay 200ms").unwrap(),
            Effect::Delay { ms: 200 }
        );
        assert_eq!(
            parse_effect("delay 2s").unwrap(),
            Effect::Delay { ms: 2000 }
        );
        assert!(parse_effect("200").is_err());
        assert!(parse_effect("delay").is_err());
        assert!(parse_effect("explode").is_err());
    }
}
