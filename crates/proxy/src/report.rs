//! A session as one HTML file: what `sonde share --html` writes. Nothing to
//! serve and nothing to install — the summary, the findings, where the time
//! went, the operations on a timeline and in a table (each with its own
//! `#op-N` link), in light and dark. `redact` first makes it safe to hand to
//! someone outside: names of buckets and keys become stable tokens, and
//! everything a request carried besides its shape is dropped.

use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};

use crate::episode::{Episode, Outcome, episodes};
use crate::profile::{RequestPrices, profile};
use crate::session::Session;
use crate::summary::summarize;

/// Path segments that say what a layout is, not what the data is: kept.
const STRUCTURAL: &[&str] = &[
    "_delta_log",
    "_last_checkpoint",
    "metadata",
    "data",
    "manifest",
    "manifests",
    "wal",
    "compacted",
    "compactions",
    "snapshots",
    "_lakefs",
    "tmp",
];

/// File types a key keeps when redacted: the kind of data, not its name.
/// Anything else after a dot (`john.smith`) is part of the name.
const FILE_TYPES: &[&str] = &[
    "parquet",
    "orc",
    "avro",
    "arrow",
    "ipc",
    "json",
    "jsonl",
    "ndjson",
    "csv",
    "tsv",
    "txt",
    "log",
    "xml",
    "yaml",
    "yml",
    "md",
    "html",
    "sst",
    "wal",
    "manifest",
    "metadata",
    "checkpoint",
    "crc",
    "idx",
    "db",
    "sqlite",
    "pb",
    "bin",
    "dat",
    "gz",
    "zst",
    "lz4",
    "snappy",
    "bz2",
    "xz",
    "zip",
    "tar",
    "png",
    "jpg",
    "jpeg",
    "gif",
    "webp",
    "mov",
    "mp4",
    "pdf",
];

/// Query parameters whose values say how, not what: kept as they are.
const PLAIN_QUERY: &[&str] = &[
    "list-type",
    "max-keys",
    "max-uploads",
    "max-parts",
    "delimiter",
    "encoding-type",
    "fetch-owner",
    "partNumber",
    "part-number-marker",
    "uploads",
    "versions",
    "versioning",
    "x-id",
    "acl",
    "tagging",
    "lifecycle",
    "location",
    "policy",
    "cors",
    "delete",
    "attributes",
    "restore",
    "object-lock",
    "retention",
    "legal-hold",
    "torrent",
    "select",
    "select-type",
];

struct Redactor {
    salt: u64,
}

impl Redactor {
    fn token(&self, s: &str) -> String {
        let mut h = DefaultHasher::new();
        self.salt.hash(&mut h);
        s.hash(&mut h);
        format!("{:08x}", h.finish() as u32)
    }

    fn segment(&self, seg: &str) -> String {
        if seg.is_empty()
            || STRUCTURAL.contains(&seg)
            || seg
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == '_')
        {
            return seg.to_string();
        }
        // keep what kind of file it is: `.parquet`, `.json`, `.sst`
        match seg.rsplit_once('.') {
            Some((stem, ext))
                if !stem.is_empty() && FILE_TYPES.contains(&ext.to_ascii_lowercase().as_str()) =>
            {
                // a numbered file keeps its number: 00000000000000000010.json
                if stem.chars().all(|c| c.is_ascii_digit()) {
                    seg.to_string()
                } else {
                    format!("{}.{ext}", self.token(stem))
                }
            }
            _ => self.token(seg),
        }
    }

    fn path(&self, p: &str) -> String {
        p.split('/')
            .map(|s| self.segment(s))
            .collect::<Vec<_>>()
            .join("/")
    }
}

/// Makes a session safe to hand on: bucket names and every key segment
/// become tokens (the same name, the same token, within this session; the
/// layout's own names and file types are kept), and headers, bodies,
/// request ids, URLs, the program's output and its arguments are dropped.
pub fn redact(s: &mut Session) {
    let r = Redactor {
        salt: rand::random(),
    };
    s.header.url = String::new();
    s.header.endpoint = "endpoint".into();
    s.header.region = String::new();
    s.header.cwd = None;
    // the commit says which change; a branch's name may say whose
    if let Some(g) = &mut s.header.git {
        g.branch = None;
    }
    // the program's name only: never a variable set before it (a key, say)
    s.header.command = s
        .header
        .command
        .as_deref()
        .map(|c| crate::program::program_name(c).unwrap_or_default());
    s.logs.clear();
    for rec in &mut s.records {
        rec.endpoint = "endpoint".into();
        rec.url = String::new();
        rec.job = None;
        // what failed, not where: the message names the URL
        rec.transport_error = rec
            .transport_error
            .as_ref()
            .map(|_| "the request did not reach the store".to_string());
        rec.bucket = rec
            .bucket
            .as_deref()
            .map(|b| format!("bucket-{}", r.token(b)).into());
        rec.key = rec.key.as_deref().map(|k| r.path(k));
        for (k, v) in &mut rec.query {
            if matches!(
                k.as_str(),
                "prefix" | "start-after" | "marker" | "key-marker"
            ) {
                *v = r.path(v).into();
            } else if !PLAIN_QUERY.contains(&k.as_str()) && !v.is_empty() {
                // ids, tokens, response-* overrides (a file name), an app's own
                *v = r.token(v).into();
            }
        }
        rec.request_headers.clear();
        rec.response_headers.clear();
        rec.request_body = None;
        rec.request_id = None;
        rec.host_id = None;
        // who asked (an IAM ARN: account and user) and from where
        rec.requester = None;
        rec.region = None;
        // the S3 error code says what went wrong; the message may name things
        rec.error_body = rec.error_body.as_deref().and_then(|b| {
            let code = b.split("<Code>").nth(1)?.split("</Code>").next()?;
            Some(format!("<Error><Code>{code}</Code></Error>"))
        });
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn ms(v: f64) -> String {
    if v >= 1000.0 {
        format!("{:.1} s", v / 1000.0)
    } else if v >= 0.95 {
        format!("{v:.0} ms")
    } else {
        format!("{v:.1} ms")
    }
}

fn bytes(b: u64) -> String {
    // the one ladder of sizes
    crate::signals::bytes_text(b as f64)
}

const STYLE: &str = r#"
:root{--bg:#fbfbfa;--panel:#fff;--fg:#1d1d1f;--muted:#6e6e73;--line:#e5e5e7;--accent:#2f6fde;--warn:#b25e09;--bad:#c4312b;--ok:#2e7d4f;--bar:#2f6fde;--bar2:#9ab8ee}
@media (prefers-color-scheme:dark){:root{--bg:#161618;--panel:#1f1f22;--fg:#f2f2f4;--muted:#9a9aa1;--line:#323236;--accent:#6c9cf0;--warn:#e9a24c;--bad:#ef6a62;--ok:#5cc489;--bar:#6c9cf0;--bar2:#34507f}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:14px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",system-ui,sans-serif}
main{max-width:1040px;margin:0 auto;padding:32px 16px 64px}
h1{font-size:22px;margin:0 0 4px}h2{font-size:15px;margin:32px 0 10px}
.sub{color:var(--muted);margin:0 0 20px}
.stats{display:grid;grid-template-columns:repeat(auto-fit,minmax(120px,1fr));gap:8px}
.stat{background:var(--panel);border:1px solid var(--line);border-radius:10px;padding:10px 12px}
.stat b{display:block;font-size:20px;font-variant-numeric:tabular-nums}.stat span{color:var(--muted);font-size:12px}
.card{background:var(--panel);border:1px solid var(--line);border-radius:10px;padding:4px 14px}
.finding{display:flex;gap:10px;padding:10px 0;border-bottom:1px solid var(--line)}.finding:last-child{border:0}
.dot{flex:none;width:8px;height:8px;border-radius:50%;margin-top:7px;background:var(--muted)}.warn .dot{background:var(--warn)}
.saving{color:var(--ok);font-size:12px}
.row{display:grid;grid-template-columns:minmax(120px,220px) 1fr 70px;gap:10px;align-items:center;padding:6px 0}
.track{height:10px;background:var(--line);border-radius:5px;overflow:hidden}.fill{height:100%;background:var(--bar);border-radius:5px}
.num{text-align:right;font-variant-numeric:tabular-nums;color:var(--muted)}
svg{display:block;width:100%;height:auto}
table{width:100%;border-collapse:collapse;font-variant-numeric:tabular-nums}th,td{text-align:left;padding:6px 8px;border-bottom:1px solid var(--line);vertical-align:top}
th{color:var(--muted);font-weight:500;font-size:12px}td.r,th.r{text-align:right}
tr:target{background:color-mix(in srgb,var(--accent) 14%,transparent)}
a{color:var(--accent);text-decoration:none}.bad{color:var(--bad)}.muted{color:var(--muted)}
.wrap{overflow-x:auto}footer{margin-top:40px;color:var(--muted);font-size:12px}
"#;

/// The operations a page lists and draws at most (and those the findings
/// name, wherever they are): past it the page says how many it left out.
pub const PAGE_OPERATIONS: usize = 2000;

/// The page.
pub fn html(s: &Session, prices: &RequestPrices) -> String {
    let recs = &s.records;
    let eps = episodes(recs);
    let obs = crate::episode::observations_of(&s.header, recs, &eps);
    let sum = summarize(recs);
    let prof = profile(recs, prices);
    let command = s
        .header
        .command
        .as_deref()
        .map(crate::program::scrub_command);
    // a program's name titles its page; its command is said under it
    let named = s
        .header
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty());
    let title = named
        .map(String::from)
        .or_else(|| command.clone())
        .unwrap_or_else(|| format!("Session on {}", s.header.endpoint));
    let said_under = match (named, &command) {
        (Some(_), Some(c)) => format!("{} · ", esc(c)),
        _ => String::new(),
    };
    let mut o = String::new();
    let _ = write!(
        o,
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{} · sonde</title><style>{STYLE}</style></head><body><main>",
        esc(&title)
    );
    let _ = write!(
        o,
        "<h1>{}</h1><p class=\"sub\">{said_under}{} · {}</p>",
        esc(&title),
        esc(&s.header.endpoint),
        s.header.created.format("%Y-%m-%d %H:%M UTC")
    );
    let stat = |o: &mut String, v: String, l: &str| {
        let _ = write!(o, "<div class=\"stat\"><b>{v}</b><span>{l}</span></div>");
    };
    o.push_str("<div class=\"stats\">");
    stat(&mut o, sum.requests.to_string(), "requests");
    stat(&mut o, eps.len().to_string(), "operations");
    stat(&mut o, ms(prof.wall_ms), "took");
    stat(
        &mut o,
        format!("{:.0}%", prof.busy_ms * 100.0 / prof.wall_ms.max(1e-9)),
        "waiting on the store",
    );
    stat(&mut o, sum.errors.to_string(), "errors");
    stat(
        &mut o,
        format!("↑{} ↓{}", bytes(sum.bytes_up), bytes(sum.bytes_down)),
        "sent · received",
    );
    o.push_str("</div>");

    if !obs.is_empty() {
        o.push_str("<h2>Findings</h2><div class=\"card\">");
        for f in &obs {
            let _ = write!(
                o,
                "<div class=\"finding {}\"><span class=\"dot\"></span><div>{}",
                if f.level == "warn" { "warn" } else { "" },
                esc(&f.text)
            );
            if let Some(sv) = &f.saving_text {
                let _ = write!(o, "<div class=\"saving\">{}</div>", esc(sv));
            }
            for c in &f.callers {
                let _ = write!(
                    o,
                    "<div class=\"caller\"><code>{}</code> · {}</div>",
                    esc(&c.place),
                    c.requests
                );
            }
            let links: Vec<String> = f
                .episodes
                .iter()
                .take(5)
                .map(|i| format!("<a href=\"#op-{i}\">#{}</a>", i + 1))
                .collect();
            if !links.is_empty() {
                let _ = write!(o, "<div class=\"muted\">{}</div>", links.join(" "));
            }
            o.push_str("</div></div>");
        }
        o.push_str("</div>");
    }

    if !prof.groups.is_empty() {
        o.push_str("<h2>Where the time went</h2><div class=\"card\">");
        for g in prof.groups.iter().take(8) {
            let name = if g.missed {
                format!("{} (not found)", g.operation)
            } else {
                g.operation.clone()
            };
            let _ = write!(
                o,
                "<div class=\"row\"><span>{}</span><div class=\"track\"><div class=\"fill\" style=\"width:{:.1}%\"></div></div><span class=\"num\">{}</span></div>",
                esc(&name),
                g.share * 100.0,
                ms(g.waited_ms)
            );
        }
        o.push_str("</div>");
    }

    // a page is read, not scrolled for ever: the first operations and every
    // one a finding links to are on it, and it says how many are not
    let mut on_page: Vec<usize> = (0..eps.len().min(PAGE_OPERATIONS)).collect();
    on_page.extend(
        obs.iter()
            .flat_map(|f| f.episodes.iter().take(5).copied())
            .filter(|i| *i >= PAGE_OPERATIONS && *i < eps.len()),
    );
    on_page.sort_unstable();
    on_page.dedup();
    let left_out = eps.len() - on_page.len();
    let left_out_text = (left_out > 0).then(|| {
        format!(
            "<p class=\"muted\">{} of its {} operations are on this page: the first {} and those the findings name. <code>sonde show --all</code> prints every one; <code>sonde share</code> without <code>--html</code> makes the whole session a site.</p>",
            on_page.len(),
            eps.len(),
            PAGE_OPERATIONS
        )
    });
    o.push_str(&timeline(&eps, &on_page));
    if let Some(t) = &left_out_text {
        o.push_str(t);
    }

    o.push_str("<h2>Operations</h2><div class=\"card wrap\"><table><thead><tr><th>#</th><th>at</th><th>operation</th><th class=\"r\">requests</th><th class=\"r\">took</th></tr></thead><tbody>");
    let t0 = eps.first().map(|e| e.start);
    for (i, e) in on_page.iter().map(|i| (*i, &eps[*i])) {
        let at = t0
            .map(|t| ms((e.start - t).num_microseconds().unwrap_or(0) as f64 / 1000.0))
            .unwrap_or_default();
        let took = (e.end - e.start).num_microseconds().unwrap_or(0) as f64 / 1000.0;
        let failed = !matches!(e.outcome, Outcome::Ok);
        let _ = write!(
            o,
            "<tr id=\"op-{i}\"><td><a href=\"#op-{i}\">{}</a></td><td class=\"muted\">+{at}</td><td{}>{}</td><td class=\"r\">{}</td><td class=\"r\">{}</td></tr>",
            i + 1,
            if failed { " class=\"bad\"" } else { "" },
            esc(&e.title()),
            e.records.len(),
            ms(took)
        );
    }
    o.push_str("</tbody></table></div>");
    if let Some(t) = &left_out_text {
        o.push_str(t);
    }
    o.push_str("<footer>Recorded with sonde.</footer></main></body></html>\n");
    o
}

/// The operations on one time axis: each a bar from its start to its end,
/// in rows so that none overlap; a failed one in red. Each links to its row.
fn timeline(eps: &[Episode], on_page: &[usize]) -> String {
    let (Some(first), Some(_)) = (eps.first(), eps.last()) else {
        return String::new();
    };
    let t0 = first.start;
    let end = eps.iter().map(|e| e.end).max().unwrap_or(t0);
    let span = ((end - t0).num_microseconds().unwrap_or(1) as f64).max(1.0);
    let x = |t: chrono::DateTime<chrono::Utc>| {
        (t - t0).num_microseconds().unwrap_or(0) as f64 / span * 1000.0
    };
    // lanes: the first lane free at an operation's start (at most 12, then they stack)
    let mut lanes: Vec<f64> = Vec::new();
    let mut bars = String::new();
    for (i, e) in on_page.iter().map(|i| (*i, &eps[*i])) {
        let (a, b) = (x(e.start), x(e.end).max(x(e.start) + 2.0));
        let lane = match lanes.iter().position(|&free| free <= a) {
            Some(l) => l,
            None if lanes.len() < 12 => {
                lanes.push(0.0);
                lanes.len() - 1
            }
            None => lanes.len() - 1,
        };
        lanes[lane] = b + 1.0;
        let color = if matches!(e.outcome, Outcome::Ok) {
            "var(--bar)"
        } else {
            "var(--bad)"
        };
        let _ = write!(
            bars,
            "<a href=\"#op-{i}\"><rect x=\"{a:.1}\" y=\"{}\" width=\"{:.1}\" height=\"10\" rx=\"2\" fill=\"{color}\"><title>{}</title></rect></a>",
            lane * 14 + 4,
            b - a,
            esc(&e.title())
        );
    }
    let h = lanes.len().max(1) * 14 + 22;
    let total = span / 1000.0;
    format!(
        "<h2>Timeline</h2><div class=\"card\"><svg viewBox=\"0 0 1000 {h}\" preserveAspectRatio=\"none\" role=\"img\" aria-label=\"operations over time\">{bars}<text x=\"0\" y=\"{}\" font-size=\"11\" fill=\"var(--muted)\">0</text><text x=\"1000\" y=\"{}\" font-size=\"11\" text-anchor=\"end\" fill=\"var(--muted)\">{}</text></svg></div>",
        h - 4,
        h - 4,
        ms(total)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionHeader;
    use chrono::DateTime;
    use reqlog::Record;

    fn session() -> Session {
        let rec = |id: u64, key: &str| {
            Record {
            id,
            time: DateTime::from_timestamp_millis(1_700_000_000_000 + id as i64 * 10).unwrap(),
            method: "GET".into(),
            operation: "GetObject".into(),
            bucket: Some("acme-payroll".into()),
            key: Some(key.into()),
            url: "https://acme-payroll.s3.amazonaws.com/x".into(),
            status: Some(404),
            latency_ms: 4.0,
            error_body: Some("<Error><Code>NoSuchKey</Code><Message>acme-payroll/2026/alice.parquet</Message></Error>".into()),
            request_id: Some("REQ".into()),
            ..Default::default()
        }
        };
        Session {
            header: SessionHeader::new(
                "prod",
                "https://s3.amazonaws.com",
                "us-east-1",
                Some("/opt/etl/run --secret=x".into()),
            ),
            records: vec![
                rec(1, "2026/alice.parquet"),
                rec(2, "2026/alice.parquet"),
                rec(3, "tables/t/_delta_log/00000000000000000010.json"),
            ],
            logs: vec![],
        }
    }

    #[test]
    fn a_redacted_session_keeps_its_shape_and_none_of_its_names() {
        let mut s = session();
        redact(&mut s);
        let page = html(&s, &RequestPrices::default());
        for secret in [
            "acme",
            "alice",
            "payroll",
            "prod",
            "--secret",
            "REQ",
            "amazonaws",
        ] {
            assert!(!page.contains(secret), "{secret} leaked");
        }
        // the same key, the same token; the layout's names and numbers kept
        assert_eq!(s.records[0].key, s.records[1].key);
        assert!(s.records[0].key.as_deref().unwrap().starts_with("2026/"));
        assert!(s.records[0].key.as_deref().unwrap().ends_with(".parquet"));
        assert!(
            s.records[2]
                .key
                .as_deref()
                .unwrap()
                .ends_with("/_delta_log/00000000000000000010.json")
        );
        assert_eq!(
            s.records[0].error_body.as_deref(),
            Some("<Error><Code>NoSuchKey</Code></Error>")
        );
        assert_eq!(s.header.command.as_deref(), Some("run"));
    }

    #[test]
    fn nothing_that_names_the_data_survives_redaction() {
        let mut s = session();
        let r = &mut s.records[0];
        r.key = Some("hr/john.smith".into());
        r.job = Some("payroll-alice".into());
        r.transport_error = Some(
            "error sending request for url (https://acme-payroll.s3.amazonaws.com/hr/alice)".into(),
        );
        r.query = vec![
            (
                "response-content-disposition".into(),
                "attachment; filename=alice-salary.csv".into(),
            ),
            ("list-type".into(), "2".into()),
        ];
        redact(&mut s);
        let doc = serde_json::to_string(&s.records).unwrap();
        for secret in ["smith", "alice", "payroll", "amazonaws"] {
            assert!(!doc.contains(secret), "{secret} leaked: {doc}");
        }
        assert!(doc.contains("\"list-type\",\"2\""));
    }

    #[test]
    fn the_page_links_each_operation() {
        let page = html(&session(), &RequestPrices::default());
        assert!(page.starts_with("<!doctype html>"));
        assert!(page.contains("id=\"op-0\"") && page.contains("href=\"#op-0\""));
        assert!(page.contains("prefers-color-scheme:dark"));
    }

    #[test]
    fn a_long_session_is_a_page_of_its_first_operations_and_those_a_finding_names() {
        // 2600 writes, then one key read 400 times: a finding about the reads
        let mut s = session();
        s.header.name = Some("etl".into());
        s.records = (0..3000u64)
            .map(|i| Record {
                id: i + 1,
                time: DateTime::from_timestamp_millis(1_700_000_000_000 + i as i64 * 10).unwrap(),
                method: if i < 2600 { "PUT" } else { "GET" }.into(),
                operation: if i < 2600 { "PutObject" } else { "GetObject" }.into(),
                bucket: Some("b".into()),
                key: Some(if i < 2600 {
                    format!("out/{i}")
                } else {
                    "conf.json".into()
                }),
                status: Some(200),
                latency_ms: 4.0,
                ..Default::default()
            })
            .collect();
        let page = html(&s, &RequestPrices::default());
        let rows = page.matches("<tr id=\"op-").count();
        let bars = page.matches("<rect ").count();
        // the first 2000, and the five the finding links to (past them)
        assert_eq!((rows, bars), (PAGE_OPERATIONS + 5, PAGE_OPERATIONS + 5));
        assert!(page.contains("id=\"op-2600\"") && !page.contains("id=\"op-2599\""));
        assert!(
            page.contains("2005 of its 3000 operations are on this page")
                && page.contains("sonde show --all"),
            "it says what it left out, and how to see it"
        );
        // a program's name is the page's title; its command is said under it
        assert!(page.contains("<title>etl · sonde</title>"));
        assert!(
            page.contains("<h1>etl</h1><p class=\"sub\">/opt/etl/run ")
                && page.contains(" · prod · "),
            "{}",
            &page[page.find("<h1>").unwrap_or(0)..][..200]
        );
        // a short session's page says nothing of the kind
        assert!(!html(&session(), &RequestPrices::default()).contains("are on this page"));
    }

    #[test]
    fn who_sent_a_request_and_from_which_region_do_not_survive_redaction() {
        let mut s = session();
        s.records[0].requester = Some("arn:aws:iam::123456789012:user/alice".into());
        s.records[0].region = Some("eu-west-3".into());
        s.header.region = "eu-west-3".into();
        redact(&mut s);
        let doc = serde_json::to_string(&crate::share::session_json(&s, &RequestPrices::default()))
            .unwrap();
        for secret in ["123456789012", "alice", "eu-west-3"] {
            assert!(!doc.contains(secret), "{secret} leaked");
        }
    }
}
