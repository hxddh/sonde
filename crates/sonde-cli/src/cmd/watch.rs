//! A proxy left running (`sonde proxy --rotate/--alert`), and production
//! read through its logs (`sonde import s3://bucket/prefix [--follow]`).

use sonde_proxy::program::{money_text, ms_text};
use std::io::IsTerminal;
use std::path::Path;
use std::sync::Arc;

use reqlog::Record;
use sonde_core::Sonde;
use sonde_proxy::episode::{episodes, observations};
use sonde_proxy::profile::{RequestPrices, profile};
use sonde_proxy::session::{Session, SessionHeader, SessionWriter};

use crate::output;

use super::check::url_shown;
use super::proxy::{ProxyArgs, check_listen, print_env};
use super::record::*;

/// `sonde import s3://bucket/prefix`: what to read, from where, how often.
pub(super) struct Logs<'a> {
    pub(super) endpoint: Option<&'a str>,
    pub(super) src: &'a str,
    pub(super) every: &'a str,
    /// One pass, then exit (no --follow).
    pub(super) once: bool,
    pub(super) alert: Option<&'a str>,
}

/// Where an import of logs got to: the newest log file's time read, and
/// the files read since a little before it, each with when it was written
/// (what its forgetting goes by).
#[derive(serde::Serialize, serde::Deserialize)]
struct LogsMark {
    watermark: chrono::DateTime<chrono::Utc>,
    seen: std::collections::BTreeMap<String, chrono::DateTime<chrono::Utc>>,
    /// The requests of uploads started and not finished yet: an upload
    /// whose completion is in a later pass's logs is read with it, not
    /// called unfinished in the pass it started in ([`hold_unfinished`]).
    #[serde(default)]
    pending: Vec<Record>,
}

/// How long an upload is waited for, in the logs' own time: past it, one
/// still not completed is unfinished.
const UPLOAD_WAITED: chrono::Duration = chrono::Duration::hours(24);

/// Takes out of `records` the requests of each multipart upload not
/// completed in them and started less than [`UPLOAD_WAITED`] before their
/// newest: what a later pass may finish. `records` are put in time order,
/// numbered from 1.
fn hold_unfinished(records: &mut Vec<Record>) -> Vec<Record> {
    records.sort_by_key(|r| r.time);
    for (i, r) in records.iter_mut().enumerate() {
        r.id = i as u64 + 1;
    }
    let Some(newest) = records.last().map(|r| r.time) else {
        return Vec::new();
    };
    let held: std::collections::HashSet<u64> = episodes(records)
        .iter()
        .filter(|e| {
            e.kind == sonde_proxy::episode::Kind::Multipart
                && e.title().ends_with("(not completed)")
                && newest - e.start < UPLOAD_WAITED
        })
        .flat_map(|e| e.records.iter().copied())
        .collect();
    let (out, kept): (Vec<Record>, Vec<Record>) = std::mem::take(records)
        .into_iter()
        .partition(|r| held.contains(&r.id));
    *records = kept;
    out
}

/// How long before the mark a file is still listed, and so still remembered.
const LOGS_GRACE: chrono::Duration = chrono::Duration::hours(2);

impl LogsMark {
    /// Whether a file written at `at` is to be read: new enough to be looked
    /// at, and not read yet.
    fn wants(&self, key: &str, at: chrono::DateTime<chrono::Utc>) -> bool {
        at > self.watermark - LOGS_GRACE && !self.seen.contains_key(key)
    }

    /// A file read (or given up on): not read again.
    fn read(&mut self, key: &str, at: chrono::DateTime<chrono::Utc>) {
        self.seen.insert(key.to_string(), at);
    }

    /// Forgets the files too old to be listed again, and only those: one
    /// still within the grace would be read a second time.
    fn forget_old(&mut self) {
        let keep_after = self.watermark - LOGS_GRACE;
        self.seen.retain(|_, at| *at > keep_after);
    }
}

/// What a single pass over the logs exits with: the listing failed, or files were
/// listed and not one could be downloaded (nothing was read: 70), the pass is
/// over the budget (1, as `sonde check`), or 0.
/// "None read" is also a prefix with no files, or none a log.
fn once_exit(listing_failed: bool, none_read: bool, over_budget: bool) -> i32 {
    if listing_failed || none_read {
        70
    } else {
        i32::from(over_budget)
    }
}

/// The program key of a log source's sessions: its runs make a history and
/// `sonde check` holds them to a baseline like any program's.
pub const LOGS_OF: &str = "logs of ";

/// A duration as `--every` writes it: "15m", "1h", "30s", "90" (seconds).
fn every(v: &str) -> anyhow::Result<std::time::Duration> {
    let t = v.trim();
    let (n, unit) = t.split_at(t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len()));
    let n: u64 = n
        .parse()
        .map_err(|_| anyhow::anyhow!("--every {v:?}: say 30s, 15m or 1h"))?;
    let secs = match unit.trim() {
        "" | "s" => n,
        "m" | "min" => n * 60,
        "h" => n * 3600,
        _ => anyhow::bail!("--every {v:?}: say 30s, 15m or 1h"),
    };
    Ok(std::time::Duration::from_secs(secs.max(10)))
}

/// `sonde import s3://bucket/prefix`: production seen through its
/// logs. Each pass reads the log files written since the last one (through
/// the endpoint's credentials, GET and LIST only), keeps their requests as a
/// session of the program `logs of s3://…`, says each new warning, and holds
/// the pass to sonde.toml's budget; `--alert` posts both. Where it got to is
/// kept, so a restart goes on from there.
pub(super) async fn watch_logs(s: &Arc<Sonde>, a: &Logs<'_>) -> anyhow::Result<i32> {
    let src = a.src;
    use std::collections::HashSet;
    let Some((bucket, prefix)) = src
        .strip_prefix("s3://")
        .map(|r| r.split_once('/').unwrap_or((r, "")))
        .filter(|(b, _)| !b.is_empty())
    else {
        eprintln!("sonde: logs are read from s3://bucket/prefix");
        return Ok(64);
    };
    let ep = super::endpoint(s, a.endpoint)?;
    let client = s.client(&ep)?;
    let pause = every(a.every)?;
    let dir = s.paths().sessions();
    std::fs::create_dir_all(&dir)?;
    let key = format!("{LOGS_OF}s3://{bucket}/{prefix}");
    // where the last pass got to, per source
    let mark_file = dir.join(format!(
        "logs-{:016x}.mark",
        key.bytes()
            .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64)
                .wrapping_mul(0x100_0000_01b3))
    ));
    // where it got to: the newest file's time read, and the files read near
    // it (logs arrive late, and prefixes of regions or days list in any
    // order, so a time and the files seen, not the last key listed)
    let mut mark: LogsMark = std::fs::read(&mark_file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| LogsMark {
            // a first pass reads what arrived in the last interval, not the
            // whole history under the prefix
            watermark: chrono::Utc::now() - chrono::Duration::from_std(pause).unwrap_or_default(),
            seen: Default::default(),
            pending: Vec::new(),
        });
    let mut warned: HashSet<String> = HashSet::new();
    let mut sent = Vec::new();
    if !a.once {
        eprintln!("reading {src} every {}s", pause.as_secs());
    }
    // the last pass's outcome: what `--once` exits with
    let (mut listing_failed, mut none_read, mut over);
    loop {
        // the files written since the mark (less a grace for late ones), not read yet
        let mut keys: Vec<(String, u64, chrono::DateTime<chrono::Utc>)> = Vec::new();
        let mut token = None;
        let mut files = 0usize;
        let listed = loop {
            let page = client
                .list_objects_v2(
                    bucket,
                    &s3raw::ops::ListV2 {
                        prefix: Some(prefix.to_string()),
                        continuation_token: token.clone(),
                        url_encoding: true,
                        ..Default::default()
                    },
                )
                .await;
            let (page, _) = match page {
                Ok(p) => p,
                Err(e) => break Err(e),
            };
            files += page.contents.len();
            keys.extend(page.contents.iter().filter_map(|o| {
                let at = o
                    .last_modified
                    .as_deref()
                    .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())?
                    .with_timezone(&chrono::Utc);
                mark.wants(&o.key, at).then(|| (o.key.clone(), o.size, at))
            }));
            token = page.next_continuation_token.filter(|_| page.is_truncated);
            if token.is_none() {
                break Ok(());
            }
        };
        // a store that did not answer this time is asked again next time
        listing_failed = listed.is_err();
        over = false;
        if let Err(e) = listed {
            eprintln!(
                "{} listing {src}: {e}",
                paint("!", "33", std::io::stderr().is_terminal())
            );
            keys.clear();
        }
        let mut records = Vec::new();
        // of the files asked for: how many answered, how many did not
        let (mut downloaded, mut unread, mut logs) = (0usize, 0usize, 0usize);
        for (k, size, at) in &keys {
            // a log file is small; something this large is not one
            if *size > 256 << 20 {
                mark.read(k, *at);
                continue;
            }
            let r = match client.get_object(bucket, k, None).await {
                Ok(r) => r,
                Err(e) => {
                    // gone (a lifecycle rule) or not reachable now: tried again next pass
                    eprintln!(
                        "{} {k}: {e}",
                        paint("!", "33", std::io::stderr().is_terminal())
                    );
                    unread += 1;
                    continue;
                }
            };
            downloaded += 1;
            let text = sonde_proxy::import::text_of(&r.body)
                .unwrap_or_else(|_| String::from_utf8_lossy(&r.body).into_owned());
            if let Ok(i) = sonde_proxy::import::import_counted(&text, k) {
                logs += 1;
                if i.malformed > 0 {
                    eprintln!(
                        "{} {k}: {} line{} could not be read",
                        paint("!", "33", std::io::stderr().is_terminal()),
                        i.malformed,
                        if i.malformed == 1 { "" } else { "s" }
                    );
                }
                records.extend(i.session.records);
            }
            mark.read(k, *at);
            if *at > mark.watermark {
                mark.watermark = *at;
            }
        }
        // every file failed: a pass that read nothing did not find nothing
        // (no permission to GET, say); one file gone among many is not that
        none_read = unread > 0 && downloaded == 0;
        let bang = paint("!", "33", std::io::stderr().is_terminal());
        if none_read {
            eprintln!(
                "{bang} none of the {unread} log file{} listed under {src} could be read",
                if unread == 1 { "" } else { "s" }
            );
        } else if !listing_failed && files == 0 {
            // a prefix with nothing under it is a wrong prefix, not a quiet hour
            none_read = true;
            eprintln!("{bang} no files under {src}: is it where the logs are delivered?");
        } else if downloaded > 0 && logs == 0 {
            none_read = true;
            eprintln!(
                "{bang} none of the {} read under {src} is an S3 access log or CloudTrail file",
                output::count(downloaded, "file")
            );
        } else if !listing_failed && keys.is_empty() {
            eprintln!(
                "{} nothing new under {src} since the last pass",
                paint("·", "2", std::io::stderr().is_terminal())
            );
        }
        // uploads the passes before left open are read with this one's; the
        // ones still open wait for the next
        if !records.is_empty() {
            records.splice(0..0, std::mem::take(&mut mark.pending));
            mark.pending = hold_unfinished(&mut records);
        }
        // the seen files old enough to be past the grace are forgotten
        mark.forget_old();
        if let Ok(b) = serde_json::to_vec(&mark) {
            let _ = reqlog::private::write(&mark_file, b);
        }
        // the requests are the endpoint's: priced and checked as its
        for r in records.iter_mut() {
            r.endpoint = ep.as_str().into();
        }
        if !records.is_empty() {
            records.sort_by_key(|r| r.time);
            for (i, r) in records.iter_mut().enumerate() {
                r.id = i as u64 + 1;
            }
            let mut header = SessionHeader::new(&ep, "", "", Some(key.clone()));
            header.created = chrono::Utc::now();
            let path = sonde_proxy::session::fresh_path(
                &dir,
                &format!(
                    "{}-logs-{bucket}",
                    chrono::Local::now().format("%Y%m%d-%H%M%S")
                ),
            );
            let session = Session {
                header,
                records,
                logs: Vec::new(),
            };
            sonde_proxy::import::save(&session, &path)?;
            let records = &session.records;
            eprintln!(
                "{} {} requests · {}",
                paint("·", "2", std::io::stderr().is_terminal()),
                output::grouped(records.len() as u64),
                path.display()
            );
            for o in observations(records, &episodes(records)) {
                if o.level == "warn" && warned.insert(sonde_proxy::profile::finding_identity(&o)) {
                    let place = o
                        .callers
                        .first()
                        .map(|c| format!(" — {}", c.place))
                        .unwrap_or_default();
                    eprintln!(
                        "{} {}{place}",
                        paint("!", "33", std::io::stderr().is_terminal()),
                        o.text
                    );
                    if let Some(url) = a.alert {
                        sent.push(post_alert(url, format!("sonde ({src}): {}{place}", o.text)));
                    }
                }
            }
            // said here whether or not anything is told elsewhere
            if let Some(what) = over_budget(s, records) {
                over = true;
                let text = format!("sonde ({src}): {} is over budget: {what}", path.display());
                eprintln!(
                    "{} {text}",
                    paint("!", "33", std::io::stderr().is_terminal())
                );
                if let Some(url) = a.alert {
                    sent.push(post_alert(url, text));
                }
            }
        }
        sent.retain(|h: &tokio::task::JoinHandle<()>| !h.is_finished());
        if a.once {
            break;
        }
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tokio::time::sleep(pause) => {}
        }
    }
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        futures::future::join_all(sent),
    )
    .await;
    // a watch stopped by hand is done; a single pass answers for itself
    Ok(if a.once {
        once_exit(listing_failed, none_read, over)
    } else {
        0
    })
}

/// Posts `text` to a webhook, in the background; a failure is said once on stderr.
fn post_alert(url: &str, text: String) -> tokio::task::JoinHandle<()> {
    let url = url.to_string();
    tokio::spawn(async move {
        let r = reqwest::Client::new()
            .post(&url)
            .json(&serde_json::json!({ "text": text }))
            .send()
            .await
            .and_then(|r| r.error_for_status());
        if let Err(e) = r {
            static SAID: std::sync::Once = std::sync::Once::new();
            SAID.call_once(|| {
                eprintln!("alert to {} failed: {}", url_shown(&url), e.without_url())
            });
        }
    })
}

/// The project's budget (`sonde.toml`'s `[check]`), when there is one.
fn project_budget() -> Option<sonde_proxy::program::Budget> {
    let here = std::env::current_dir().ok()?;
    let project = sonde_proxy::project::of(&here).ok()??;
    project.file.check.budget().ok()
}

/// A finished file held to the project's budget: what went over, when
/// anything did.
fn over_budget(s: &Arc<Sonde>, records: &[Record]) -> Option<String> {
    let budget = project_budget()?;
    let endpoint = records.first().map(|r| r.endpoint.as_str()).unwrap_or("");
    let prof = profile(records, &request_prices_for(s, endpoint));
    let obs = observations(records, &episodes(records));
    let point = sonde_proxy::program::run_point("", "", records, &prof, &obs);
    over_limits(&point, &budget)
}

/// The same from a file's figures alone: without a baseline a budget's
/// limits are all on them (requests, waiting, cost, errors, p95).
fn over_limits(
    point: &sonde_proxy::program::RunPoint,
    budget: &sonde_proxy::program::Budget,
) -> Option<String> {
    let none = profile(&[], &RequestPrices::default());
    let c = sonde_proxy::program::check((point, &[], &none, &[]), None, budget);
    let over: Vec<String> = c
        .verdicts
        .iter()
        .filter(|v| !v.ok)
        .map(|v| match v.what {
            "cost" => format!(
                "cost {} > {}",
                money_text(v.value, &point.currency),
                money_text(v.limit, &point.currency)
            ),
            "waiting" | "p95" => format!("{} {} > {}", v.what, ms_text(v.value), ms_text(v.limit)),
            what => format!("{what} {:.0} > {:.0}", v.value, v.limit),
        })
        .collect();
    (!over.is_empty()).then(|| over.join(", "))
}

/// How many of a file's newest records a proxy left running reads for its findings.
const WATCH_RECENT: usize = 50_000;

/// A proxy left running (`--rotate`, `--alert`): one status line for the
/// last minute, each new warning finding as it appears, and a new session
/// file every --rotate minutes.
pub(super) async fn watch(s: &Arc<Sonde>, a: &ProxyArgs) -> anyhow::Result<i32> {
    use std::collections::{HashSet, VecDeque};
    use std::time::{Duration, Instant};
    check_listen(a)?;
    let started = start(s, &a.rec, a.listen, None, a.allow_remote).await?;
    print_env(&started.proxy);
    let header = sonde_proxy::session::read_header(&started.path)
        .ok_or_else(|| anyhow::anyhow!("cannot read {}", started.path.display()))?;
    let tty = std::io::stderr().is_terminal();
    let dir = started
        .path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let mut rx = started.log.subscribe();
    let mut writer = started.writer.clone();
    let mut path = started.path.clone();
    // a file's records are on disk: held here are the figures its budget is
    // on and the newest records, which the warnings are read from
    let mut figures: Option<sonde_proxy::program::RunFigures> = None;
    let mut recent: VecDeque<Record> = VecDeque::new();
    let take = |figures: &mut Option<sonde_proxy::program::RunFigures>,
                recent: &mut VecDeque<Record>,
                r: Record| {
        figures
            .get_or_insert_with(|| {
                sonde_proxy::program::RunFigures::new(request_prices_for(s, &r.endpoint))
            })
            .add(&r);
        if recent.len() == WATCH_RECENT {
            recent.pop_front();
        }
        recent.push_back(r);
    };
    let over = |figures: Option<sonde_proxy::program::RunFigures>| {
        over_limits(&figures?.point(), &project_budget()?)
    };
    let mut minute: VecDeque<(Instant, Record)> = VecDeque::new();
    let mut warned: HashSet<String> = HashSet::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut rotated = Instant::now();
    let mut seconds = 0u64;
    // alerts on their way: waited for (a little) before sonde exits
    let mut sent: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    let finish = |w: Arc<SessionWriter>, p: &Path| {
        let _ = w.flush();
        drop(w);
        let _ = sonde_proxy::session::compress(p);
    };
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            got = rx.recv() => match got {
                Ok(id) => {
                    if let Some(r) = started.log.get(id) {
                        minute.push_back((Instant::now(), r.clone()));
                        take(&mut figures, &mut recent, r);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => break,
            },
            _ = tick.tick() => {
                seconds += 1;
                while minute.front().is_some_and(|(t, _)| t.elapsed() > Duration::from_secs(60)) {
                    minute.pop_front();
                }
                let n = minute.len();
                let errors = minute.iter().filter(|(_, r)| r.is_error() && !sonde_proxy::summary::is_miss(r)).count();
                let mut lat: Vec<f64> = minute.iter().map(|(_, r)| r.latency_ms).collect();
                lat.sort_by(f64::total_cmp);
                let p95 = sonde_proxy::profile::percentile(&lat, 0.95);
                let line = format!(
                    "last minute: {} requests ({:.1}/s) · {} errors · p95 {} · {} in this file",
                    output::grouped(n as u64),
                    n as f64 / 60.0,
                    output::grouped(errors as u64),
                    ms_text(p95),
                    output::grouped(figures.as_ref().map_or(0, |f| f.requests()) as u64)
                );
                if tty {
                    eprint!("\r\x1b[2K{line}");
                } else if seconds.is_multiple_of(30) {
                    eprintln!("{line}");
                }
                // what this file's requests say, every five seconds: each warning once
                if seconds.is_multiple_of(5) && !recent.is_empty() {
                    let recent = &*recent.make_contiguous();
                    for o in observations(recent, &episodes(recent)) {
                        if o.level == "warn" && warned.insert(sonde_proxy::profile::finding_identity(&o)) {
                            if tty {
                                eprint!("\r\x1b[2K");
                            }
                            eprintln!("{} {}", paint("!", "33", tty), o.text);
                            if let Some(url) = &a.alert {
                                sent.push(post_alert(url, format!("sonde proxy: {}", o.text)));
                            }
                        }
                    }
                }
                if a.rotate.is_some_and(|m| rotated.elapsed() >= Duration::from_secs(m.max(1) * 60)) {
                    let next = sonde_proxy::session::fresh_path(
                        &dir,
                        &format!("{}-proxy", chrono::Local::now().format("%Y%m%d-%H%M%S")),
                    );
                    let mut h = header.clone();
                    h.created = chrono::Utc::now();
                    let w = Arc::new(SessionWriter::create(&next, &h)?);
                    started.log.set_sink(Some(w.clone()));
                    let old = std::mem::replace(&mut writer, w);
                    let old_path = std::mem::replace(&mut path, next);
                    finish(old, &old_path);
                    sent.retain(|h| !h.is_finished());
                    if tty {
                        eprint!("\r\x1b[2K");
                    }
                    eprintln!("{} {}", paint("·", "2", tty), old_path.display());
                    // said here whether or not anything is told elsewhere
                    if let Some(over) = over(figures.take()) {
                        let text = format!("sonde proxy: {} is over budget: {over}", old_path.display());
                        eprintln!("{} {text}", paint("!", "33", tty));
                        if let Some(url) = &a.alert {
                            sent.push(post_alert(url, text));
                        }
                    }
                    recent.clear();
                    warned.clear();
                    rotated = Instant::now();
                }
            }
        }
    }
    if tty {
        eprintln!();
    }
    // requests still in flight are recorded before the file is packed
    let Started { mut proxy, log, .. } = started;
    shut_down(&mut proxy, SHUTDOWN_GRACE).await;
    while let Ok(id) = rx.try_recv() {
        if let Some(r) = log.get(id) {
            take(&mut figures, &mut recent, r);
        }
    }
    log.set_sink(None);
    drop(proxy);
    finish(writer, &path);
    // the last file is held to the budget too
    if let Some(over) = over(figures.take()) {
        let text = format!("sonde proxy: {} is over budget: {over}", path.display());
        eprintln!("{} {text}", paint("!", "33", tty));
        if let Some(url) = &a.alert {
            sent.push(post_alert(url, text));
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(10), futures::future::join_all(sent)).await;
    eprintln!("{}", path.display());
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One pass over the logs at `src` through `endpoint`.
    fn logs<'a>(endpoint: &'a str, src: &'a str) -> Logs<'a> {
        Logs {
            endpoint: Some(endpoint),
            src,
            every: "15m",
            once: true,
            alert: None,
        }
    }

    #[test]
    fn a_watched_file_is_held_to_the_budget_by_its_figures() {
        use sonde_proxy::program::{Budget, RunFigures};
        let mut f = RunFigures::new(RequestPrices::default());
        for i in 0..3 {
            f.add(&Record {
                id: i,
                method: "GET".into(),
                operation: "GetObject".into(),
                status: Some(200),
                latency_ms: 2000.0,
                ..Default::default()
            });
        }
        let budget = |max_requests, max_p95_ms| Budget {
            max_requests,
            max_p95_ms,
            ..Default::default()
        };
        assert_eq!(over_limits(&f.point(), &budget(Some(3), None)), None);
        assert_eq!(
            over_limits(&f.point(), &budget(Some(2), Some(500.0))).as_deref(),
            Some("requests 3 > 2, p95 2.00 s > 500 ms")
        );
    }

    #[test]
    fn every_reads_seconds_minutes_and_hours() {
        assert_eq!(every("15m").unwrap().as_secs(), 900);
        assert_eq!(every("1h").unwrap().as_secs(), 3600);
        assert_eq!(every("90").unwrap().as_secs(), 90);
        assert!(every("soon").is_err());
    }

    #[tokio::test]
    async fn watching_logs_keeps_each_pass_as_a_run_of_the_source() {
        let dir = tempfile::tempdir().unwrap();
        let s = Arc::new(Sonde::open(sonde_core::Paths::rooted(dir.path())).unwrap());
        sonde_core::demo::install(&s).await.unwrap();
        // an access log written into the bucket
        let log = "79a59df900b949e55d96a1e698fbacedfd6e09d98eacf8f8d5218e7cd47ef2be demo [06/Feb/2026:00:00:38 +0000] 192.0.2.3 - 3E57427F3EXAMPLE REST.GET.OBJECT data/a.csv \"GET /demo/data/a.csv HTTP/1.1\" 200 - 113 113 7 6 \"-\" \"boto3/1.34\" - x SigV4 ECDHE TLSv1.2 - -\n\
                   79a59df900b949e55d96a1e698fbacedfd6e09d98eacf8f8d5218e7cd47ef2be demo [06/Feb/2026:00:00:39 +0000] 192.0.2.3 - 3E57427F3EXAMPLF REST.HEAD.OBJECT data/missing \"HEAD /demo/data/missing HTTP/1.1\" 404 NoSuchKey - - 4 4 \"-\" \"boto3/1.34\" - x SigV4 ECDHE TLSv1.2 - -\n";
        s.client("demo")
            .unwrap()
            .put_object(
                "demo",
                "logs/2026-02-06-00-00-40-AB",
                bytes::Bytes::from(log),
                &[],
            )
            .await
            .unwrap();
        assert_eq!(
            watch_logs(&s, &logs("demo", "s3://demo/logs/"))
                .await
                .unwrap(),
            0
        );
        let sessions: Vec<_> = std::fs::read_dir(s.paths().sessions())
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "sonde"))
            .collect();
        assert_eq!(sessions.len(), 1);
        let session = Session::open(&sessions[0]).unwrap();
        assert_eq!(session.records.len(), 2);
        assert_eq!(
            session.header.command.as_deref(),
            Some("logs of s3://demo/logs/")
        );
        // a second pass has nothing new: no second session
        assert_eq!(
            watch_logs(&s, &logs("demo", "s3://demo/logs/"))
                .await
                .unwrap(),
            0
        );
        let count = || {
            std::fs::read_dir(s.paths().sessions())
                .unwrap()
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "sonde"))
                .count()
        };
        assert_eq!(count(), 1);
        // a file that lists before the ones read (another region, a late
        // delivery) is still read
        s.client("demo")
            .unwrap()
            .put_object(
                "demo",
                "logs/0-earlier/2026-02-06-00-00-41-CD",
                bytes::Bytes::from(log),
                &[],
            )
            .await
            .unwrap();
        assert_eq!(
            watch_logs(&s, &logs("demo", "s3://demo/logs/"))
                .await
                .unwrap(),
            0
        );
        assert_eq!(count(), 2);
    }

    /// An upload started in one pass's logs and completed in the next is
    /// read in the second, whole: never warned of as unfinished.
    #[tokio::test]
    async fn an_upload_completed_in_a_later_pass_is_not_unfinished() {
        let dir = tempfile::tempdir().unwrap();
        let s = Arc::new(Sonde::open(sonde_core::Paths::rooted(dir.path())).unwrap());
        sonde_core::demo::install(&s).await.unwrap();
        let line = |at: &str, op: &str, uri: &str| {
            format!(
                "79a5 demo [06/Feb/2026:00:{at} +0000] 192.0.2.3 - 3E57 {op} big.bin \"{uri} HTTP/1.1\" 200 - 100 100 7 6 \"-\" \"boto3/1.34\" - x SigV4 ECDHE TLSv1.2 - -\n"
            )
        };
        let put = |name: &str, text: String| {
            let c = s.client("demo").unwrap();
            let name = name.to_string();
            async move {
                c.put_object("demo", &name, bytes::Bytes::from(text), &[])
                    .await
                    .unwrap();
            }
        };
        put(
            "uplogs/2026-02-06-00-01-00-AB",
            line("00:38", "REST.POST.UPLOADS", "POST /demo/big.bin?uploads")
                + &line(
                    "00:39",
                    "REST.PUT.PART",
                    "PUT /demo/big.bin?partNumber=1&uploadId=u1",
                )
                + &line("00:40", "REST.GET.OBJECT", "GET /demo/other"),
        )
        .await;
        assert_eq!(
            watch_logs(&s, &logs("demo", "s3://demo/uplogs/"))
                .await
                .unwrap(),
            0
        );
        let sessions = || {
            let mut all: Vec<_> = std::fs::read_dir(s.paths().sessions())
                .unwrap()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "sonde"))
                .collect();
            all.sort();
            all
        };
        let unfinished = |path: &std::path::Path| {
            let session = Session::open(path).unwrap();
            let eps = episodes(&session.records);
            let obs = observations(&session.records, &eps);
            (
                session.records.len(),
                obs.iter().any(|o| o.phrases[0].key == "obs.unfinished"),
            )
        };
        let first = sessions();
        assert_eq!(first.len(), 1);
        assert_eq!(unfinished(&first[0]), (1, false), "the upload waits");
        put(
            "uplogs/2026-02-06-00-02-00-CD",
            line(
                "01:10",
                "REST.POST.UPLOAD",
                "POST /demo/big.bin?uploadId=u1",
            ),
        )
        .await;
        assert_eq!(
            watch_logs(&s, &logs("demo", "s3://demo/uplogs/"))
                .await
                .unwrap(),
            0
        );
        let all = sessions();
        assert_eq!(all.len(), 2);
        let second = all.iter().find(|p| **p != first[0]).unwrap();
        assert_eq!(unfinished(second), (3, false), "read whole, finished");
    }

    #[test]
    fn hold_unfinished_lets_go_of_an_upload_waited_for_a_day() {
        let at = |h: i64| chrono::DateTime::<chrono::Utc>::UNIX_EPOCH + chrono::Duration::hours(h);
        let rec = |h: i64, method: &str, op: &str, q: &[(&str, &str)]| Record {
            time: at(h),
            method: method.into(),
            operation: op.into(),
            bucket: Some("b".into()),
            key: Some("big".into()),
            status: Some(200),
            query: q.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect(),
            ..Default::default()
        };
        let started = || rec(0, "POST", "CreateMultipartUpload", &[("uploads", "")]);
        let mut fresh = vec![started(), rec(1, "GET", "GetObject", &[])];
        assert_eq!(hold_unfinished(&mut fresh).len(), 1);
        assert_eq!(fresh.len(), 1);
        let mut stale = vec![started(), rec(30, "GET", "GetObject", &[])];
        assert!(
            hold_unfinished(&mut stale).is_empty(),
            "a day on: unfinished"
        );
        assert_eq!(stale.len(), 2);
    }

    #[test]
    fn a_single_pass_over_the_logs_answers_for_itself() {
        assert_eq!(once_exit(false, false, false), 0);
        assert_eq!(once_exit(false, false, true), 1);
        assert_eq!(once_exit(true, false, false), 70);
        assert_eq!(once_exit(true, false, true), 70);
        assert_eq!(once_exit(false, true, false), 70);
    }

    #[test]
    fn a_log_file_read_is_remembered_until_it_is_too_old_to_be_listed() {
        let t = |h: i64| chrono::DateTime::from_timestamp(1_770_000_000 + h * 3600, 0).unwrap();
        let mut mark = LogsMark {
            watermark: t(0),
            seen: Default::default(),
            pending: Vec::new(),
        };
        // more files than the old limit, all within the grace
        for i in 0..50_010 {
            mark.read(&format!("logs/{i:06}"), t(0));
        }
        // a pass with one new file, an hour on
        assert!(mark.wants("logs/new", t(1)));
        mark.read("logs/new", t(1));
        mark.watermark = t(1);
        mark.forget_old();
        assert_eq!(mark.seen.len(), 50_011);
        assert!(!mark.wants("logs/000000", t(0)), "read a second time");
        // two hours past them they are not listed, so not remembered
        mark.read("logs/later", t(3));
        mark.watermark = t(3);
        mark.forget_old();
        assert_eq!(mark.seen.len(), 1);
        assert!(!mark.wants("logs/000000", t(0)));
    }

    #[tokio::test]
    async fn a_single_pass_that_could_not_list_the_logs_does_not_exit_0() {
        let dir = tempfile::tempdir().unwrap();
        let s = Arc::new(Sonde::open(sonde_core::Paths::rooted(dir.path())).unwrap());
        sonde_core::demo::install(&s).await.unwrap();
        let src = "s3://no-such-bucket-of-logs/logs/";
        assert_eq!(watch_logs(&s, &logs("demo", src)).await.unwrap(), 70);
    }

    /// A store that lists `n` log files (0 or 2) and refuses to hand over any.
    async fn store_that_lists_and_refuses_gets(n: u8) -> std::net::SocketAddr {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    loop {
                        let mut chunk = [0u8; 4096];
                        while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                            match sock.read(&mut chunk).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                            }
                        }
                        let head = String::from_utf8_lossy(&buf).into_owned();
                        buf.clear();
                        let (status, body) = if head.contains("list-type=2") {
                            let now = chrono::Utc::now().to_rfc3339();
                            let file = |n: u8| {
                                format!(
                                    "<Contents><Key>logs/2026-02-06-00-00-4{n}-AB</Key><LastModified>{now}</LastModified><ETag>\"x\"</ETag><Size>300</Size></Contents>"
                                )
                            };
                            (
                                "200 OK",
                                format!(
                                    "<ListBucketResult><Name>lake</Name><Prefix>logs/</Prefix><KeyCount>{n}</KeyCount><IsTruncated>false</IsTruncated>{}</ListBucketResult>",
                                    (0..n).map(file).collect::<String>()
                                ),
                            )
                        } else {
                            (
                                "403 Forbidden",
                                "<Error><Code>AccessDenied</Code><Message>Access Denied</Message></Error>".to_string(),
                            )
                        };
                        let resp = format!(
                            "HTTP/1.1 {status}\r\ncontent-type: application/xml\r\ncontent-length: {}\r\n\r\n{body}",
                            body.len()
                        );
                        if sock.write_all(resp.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn a_single_pass_that_could_read_none_of_the_files_listed_does_not_exit_0() {
        for n in [2, 0] {
            assert_eq!(pass_over(n).await, 70, "{n} files");
        }
    }

    async fn pass_over(n: u8) -> i32 {
        let dir = tempfile::tempdir().unwrap();
        let s = Arc::new(Sonde::open(sonde_core::Paths::rooted(dir.path())).unwrap());
        let addr = store_that_lists_and_refuses_gets(n).await;
        let mut e = sonde_core::EndpointConfig::new(
            "refusing",
            "refusing",
            &format!("http://{addr}"),
            "us-east-1",
            sonde_core::Vendor::Minio,
        );
        e.path_style = true;
        e.credentials = sonde_core::CredentialSource::Static {
            access_key_id: "k".into(),
            secret_access_key: "s".into(),
        };
        s.add_transient_endpoint(e).unwrap();
        let src = "s3://lake/logs/";
        watch_logs(&s, &logs("refusing", src)).await.unwrap()
    }
}
