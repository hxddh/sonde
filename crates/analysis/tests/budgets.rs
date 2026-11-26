//! What reading a large run may cost, held by a test: runs of any
//! size are a promise only while something fails when it is broken.
//!
//! Each test holds its peak memory to a budget: within a few percent from run
//! to run of the same code (the reading's threads place memory a little
//! differently each time), so a miss is a change, never the runner. Its time, which a
//! shared runner moves by half and more, is said (a `budget` line each) and
//! held only at several times what it takes, so a test fails on a time
//! that is wrong, not on a slow machine. docs/dev/performance.md has the figures.
//!
//! Ignored by default (they make runs of up to a million requests); CI
//! runs them optimised, each in a process of its own, with the runs made
//! once for all of them in `SONDE_BUDGET_RUNS`:
//! `cargo test --release -p sonde-proxy --test budgets -- --ignored --exact NAME`.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sonde_analysis::profile::RequestPrices;
use sonde_analysis::run::RunFile;
use sonde_analysis::view::Analysis;

/// One at a time: the peak of memory is the process's, and the clock is
/// kinder to a test that has the cores to itself. The million goes first
/// (the others give it a moment to take its turn): its peak is then its own,
/// not what the tests before it left behind.
static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn alone(first: bool) -> std::sync::MutexGuard<'static, ()> {
    if !first {
        std::thread::sleep(Duration::from_millis(500));
    }
    ONE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The run `name`, made by `make` the first time: kept in
/// `SONDE_BUDGET_RUNS` when it is set, so that tests run each in a
/// process of its own make each run once; else made in `dir` for this
/// test alone.
fn run(dir: &Path, name: &str, make: impl FnOnce(&Path) -> u64) -> PathBuf {
    let Some(kept) = std::env::var_os("SONDE_BUDGET_RUNS") else {
        let path = dir.join(name);
        make(&path);
        return path;
    };
    let path = PathBuf::from(kept).join(name);
    if !path.is_file() {
        std::fs::create_dir_all(path.parent().expect("a folder")).expect("the runs' folder");
        // whole or not there: a test stopped half-way leaves no run
        let part = path.with_extension("part");
        make(&part);
        std::fs::rename(&part, &path).expect("the run kept");
    }
    path
}

/// Starts the count of [`peak_mib`] again from what is held now (Linux):
/// a test's peak is then its own, not the one before it.
fn peak_from_now() {
    let _ = std::fs::write("/proc/self/clear_refs", "5");
}

/// The most memory this process has held at once, in MiB (Linux; none elsewhere).
fn peak_mib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// Says what a test cost (`budget NAME: …`, the line CI puts in its
/// summary) and holds it: the peak (since [`peak_from_now`]) under `mib`,
/// the time under `room` (several times what it takes: a time past it is a
/// change, not a busy runner).
fn held(name: &str, took: Duration, room: Duration, mib: u64) {
    let peak = peak_mib();
    eprintln!(
        "budget {name}: peak {} MiB of {mib}; {:.2} s (held at {:.0} s)",
        peak.map_or("?".into(), |p| p.to_string()),
        took.as_secs_f64(),
        room.as_secs_f64()
    );
    if let Some(peak) = peak {
        assert!(peak < mib, "{name}: peak memory {peak} MiB, past its {mib}");
    }
    assert!(took < room, "{name}: took {took:?}, past {room:?}");
}

/// What `sonde show` does with a run: read it (lean: what the analyses
/// read), then count it, group it into operations, find what stands out,
/// reckon the four signals and where the time went, and what acting on the
/// findings would leave ([`Analysis::of`]: `show`, `check` and the app's
/// view all go through it), and the run against its program's run before
/// it ([`Analysis::since_last`]). Returns how long it took, and the analysis.
fn read_as_show(path: &std::path::Path) -> (Duration, usize, Analysis) {
    let started = Instant::now();
    let s = RunFile::open_lean(path).expect("the run opens");
    let opened = started.elapsed();
    let a = Analysis::of(&s, &RequestPrices::default());
    // and, a run of a program, against the run before it (none here)
    let since = a.since_last(
        &sonde_analysis::fixloop::folder_of(path),
        path,
        &s.header,
        &s.records,
        &|_| RequestPrices::default(),
    );
    assert!(since.is_some_and(|c| c.previous.is_none() && !c.findings.is_empty()));
    let took = started.elapsed();
    eprintln!(
        "  read {:.2} s, analysed {:.2} s, peak {} MiB",
        opened.as_secs_f64(),
        (took - opened).as_secs_f64(),
        peak_mib().unwrap_or(0)
    );
    assert_eq!(a.summary.requests as usize, s.records.len());
    assert!(!a.episodes.is_empty() && a.signals.rate.requests > 0);
    (took, s.records.len(), a)
}

/// What a million requests of the lake job may hold at once: 400 MiB, what
/// 22.0 set out to hold them in. Each budget is what was measured
/// (docs/dev/performance.md) and about an eighth more, for another machine's
/// threads and allocator: not room for a request held whole again.
const MILLION_MIB: u64 = 400;

#[test]
#[ignore = "a million requests: run in release, as CI does"]
fn a_million_requests_are_read_within_their_memory() {
    let _alone = alone(true);
    let dir = tempfile::tempdir().expect("a folder");
    let path = run(dir.path(), "lake-1000000-1.sonde", |p| {
        support::lake(1_000_000, p, 1)
    });
    let before = peak_mib();
    peak_from_now();
    let (took, read, analysis) = read_as_show(&path);
    assert!(read >= 1_000_000);
    eprintln!(
        "a million requests: {:.1} s, peak {:?} MiB (before reading: {before:?}), {} operations, {} findings",
        took.as_secs_f64(),
        peak_mib(),
        analysis.episodes.len(),
        analysis.observations.len()
    );
    // the lake job has what a real one has: the findings are there to be found (three since 21.0, whose
    // advice holds only for a key read alike every time)
    assert!(
        analysis.observations.len() >= 3 && analysis.what_if.is_some(),
        "{} findings",
        analysis.observations.len()
    );
    held(
        "show of a million",
        took,
        Duration::from_secs(45),
        MILLION_MIB,
    );
}

/// What `sonde show --since` does with two runs: each read (lean), reckoned and
/// let go before the other is read ([`Run::analysed`]), then compared.
#[test]
#[ignore = "two runs of a million requests: run in release, as CI does"]
fn two_runs_of_a_million_requests_compare_within_what_one_costs() {
    use sonde_analysis::profile::{Run, compare_runs};
    let _alone = alone(false);
    let dir = tempfile::tempdir().expect("a folder");
    let a = run(dir.path(), "lake-1000000-1.sonde", |p| {
        support::lake(1_000_000, p, 1)
    });
    let b = run(dir.path(), "lake-1000000-2.sonde", |p| {
        support::lake(1_000_000, p, 2)
    });
    peak_from_now();
    let started = Instant::now();
    let run = |p: &std::path::Path| {
        let s = RunFile::open_lean(p).expect("the run opens");
        Run::analysed(&s.header, &s.records, &RequestPrices::default())
    };
    let (ra, rb) = (run(&a), run(&b));
    let c = compare_runs(&ra, &rb);
    let took = started.elapsed();
    eprintln!(
        "two runs of a million compared: {:.1} s, peak {:?} MiB",
        took.as_secs_f64(),
        peak_mib()
    );
    assert!(c.requests.0 >= 1_000_000 && c.requests.1 >= 1_000_000);
    assert!(!c.groups.is_empty() && !c.prefixes.is_empty());
    // the second run is read once the first is let go: one run's memory
    held(
        "compare of two millions",
        took,
        Duration::from_secs(120),
        560,
    );
}

#[test]
#[ignore = "three hundred thousand requests: run in release, as CI does"]
fn an_outage_of_three_hundred_thousand_requests_is_read_within_its_memory() {
    let _alone = alone(false);
    let dir = tempfile::tempdir().expect("a folder");
    let path = run(dir.path(), "outage-300000.sonde", |p| {
        support::outage(300_000, p)
    });
    peak_from_now();
    let (took, read, _) = read_as_show(&path);
    assert_eq!(read, 300_000);
    held(
        "show of an outage of 300 thousand",
        took,
        Duration::from_secs(30),
        240,
    );
}

#[test]
#[ignore = "three hundred thousand requests: run in release, as CI does"]
fn small_uploads_of_three_hundred_thousand_requests_are_read_within_their_memory() {
    let _alone = alone(false);
    let dir = tempfile::tempdir().expect("a folder");
    let path = run(dir.path(), "uploads-300000.sonde", |p| {
        support::small_uploads(300_000, p)
    });
    peak_from_now();
    let (took, read, analysis) = read_as_show(&path);
    // every upload is in the finding, and acting on it leaves one PUT each
    let small = analysis
        .observations
        .iter()
        .find(|o| o.phrases[0].key == "obs.small.parts")
        .expect("small files uploaded in parts");
    assert_eq!(small.saves_requests, 200_000);
    assert_eq!(read, 300_000);
    held(
        "show of 300 thousand small uploads",
        took,
        Duration::from_secs(30),
        270,
    );
}

#[test]
#[ignore = "a hundred thousand requests: run in release, as CI does"]
fn the_apps_first_view_of_a_hundred_thousand_requests_is_built_within_its_memory() {
    let _alone = alone(false);
    let dir = tempfile::tempdir().expect("a folder");
    let path = run(dir.path(), "lake-100000-2.sonde", |p| {
        support::lake(100_000, p, 2)
    });
    peak_from_now();
    // what the bridge does when a window opens a run: read it, analyse
    // it, and build the view that is sent (the first page and the totals)
    let started = Instant::now();
    let s = RunFile::open(&path).expect("the run opens");
    let a = Analysis::of(&s, &RequestPrices::default());
    let view = serde_json::Value::Object(a.view(&path.to_string_lossy(), &s.header));
    let took = started.elapsed();
    let n = s.records.len();
    let sent = serde_json::to_string(&view)
        .expect("the view as text")
        .len();
    eprintln!(
        "the first view of {n} requests: {:.2} s, {} KiB sent for {} operations",
        took.as_secs_f64(),
        sent / 1024,
        a.episodes.len()
    );
    held(
        "the app's first view of 100 thousand",
        took,
        Duration::from_secs(6),
        250,
    );
    // and it does not grow with the run: a page of operations, not all
    assert_eq!(
        view["operations"].as_array().map(Vec::len),
        Some(sonde_analysis::view::PAGE)
    );
    assert_eq!(view["operationsTotal"], a.episodes.len());
    assert!(sent < 2 << 20, "{sent} bytes sent");
    // the next page is a lookup, not another analysis
    let started = Instant::now();
    let q: sonde_analysis::view::Query = serde_json::from_value(
        serde_json::json!({ "offset": 50_000, "sort": "duration", "descending": true }),
    )
    .expect("a query");
    let page = a.page(&q);
    assert_eq!(
        page["rows"].as_array().map(Vec::len),
        Some(sonde_analysis::view::PAGE)
    );
    let first = started.elapsed();
    let started = Instant::now();
    let _ = a.page(&sonde_analysis::view::Query {
        offset: 50_500,
        ..q
    });
    eprintln!(
        "a sorted page: {:.0} ms the first time, {:.1} ms the next",
        first.as_secs_f64() * 1000.0,
        started.elapsed().as_secs_f64() * 1000.0
    );
    // (a lookup: held at a few times what it takes, as the view is)
    assert!(first < Duration::from_millis(1500), "a page took {first:?}");
}

/// What the Time pane of a million requests is given, and what choosing a
/// stretch of it costs: the profile and the signals the pane draws are
/// bounded by its slices (not its requests), so the pane's JavaScript has
/// the same rows to draw whatever the run's size; and a tenth of the run,
/// reckoned alone on the run's axis as the bridge does for a stretch, adds
/// that tenth of a copy, not a second run.
#[test]
#[ignore = "a million requests: run in release, as CI does"]
fn the_time_pane_of_a_million_requests_draws_bounded_rows() {
    use sonde_analysis::profile::{Timeline, profile, profile_from};
    use sonde_analysis::signals::{signals, signals_from};
    let _alone = alone(false);
    let dir = tempfile::tempdir().expect("a folder");
    let path = run(dir.path(), "lake-1000000-1.sonde", |p| {
        support::lake(1_000_000, p, 1)
    });
    peak_from_now();
    let started = Instant::now();
    let s = RunFile::open_lean(&path).expect("the run opens");
    let prices = RequestPrices::default();
    let (p, g) = (profile(&s.records, &prices), signals(&s.records, &prices));
    // what the bridge sends the pane: its rows bounded by the slices
    let sent = serde_json::to_vec(&(&p, &g)).expect("as JSON").len();
    assert!(
        p.rate.len() <= 91 && p.samples.len() <= 4000,
        "{} slices, {} samples",
        p.rate.len(),
        p.samples.len()
    );
    assert!(p.latency_heat.len() <= 91 * 16 && p.heat.len() <= 91 * 9);
    assert!(sent < 2 << 20, "{sent} bytes for the pane");
    // a stretch of a tenth, mid-run: its own slices, on the run's axis
    let tl = Timeline::of(&s.records).expect("a timeline");
    let (from, to) = (
        tl.t0_us + tl.wall_us * 45 / 100,
        tl.t0_us + tl.wall_us * 55 / 100,
    );
    let tenth: Vec<reqlog::Record> = s
        .records
        .iter()
        .filter(|r| (from..to).contains(&r.time.timestamp_micros()))
        .cloned()
        .collect();
    let zoom = profile_from(&tenth, &prices, Some(tl.t0_us));
    let zsig = signals_from(&tenth, &prices, Some(tl.t0_us));
    assert!(zoom.start_ms > 0.0 && zoom.bin_ms < p.bin_ms && zsig.rate.requests == tenth.len());
    let took = started.elapsed();
    eprintln!(
        "the Time pane of a million: {:.1} s, {} KiB sent, a tenth ({} requests) reckoned alone, peak {:?} MiB",
        took.as_secs_f64(),
        sent / 1024,
        tenth.len(),
        peak_mib()
    );
    // the run, its profile and signals, and a tenth of it held again
    held(
        "the Time pane of a million",
        took,
        Duration::from_secs(45),
        310,
    );
}

/// A CloudTrail file as S3 delivers one (gzip, one `Records` document on a
/// line) of `n` data events, a few of another service's among them, their
/// times mostly in order as a trail's are. Returns how many are S3's, and
/// the bytes of its text.
fn trail(path: &Path, n: usize) -> (usize, u64) {
    use std::io::Write;
    let mut rng = support::Rng::new(7);
    let file = std::fs::File::create(path).expect("the trail");
    let mut gz =
        flate2::write::GzEncoder::new(std::io::BufWriter::new(file), flate2::Compression::new(1));
    let t0 = chrono::DateTime::parse_from_rfc3339("2026-11-03T00:00:00Z")
        .expect("a time")
        .with_timezone(&chrono::Utc);
    let (mut s3, mut bytes) = (0usize, 0u64);
    let mut put = |gz: &mut dyn Write, text: &str| {
        gz.write_all(text.as_bytes()).expect("written");
        bytes += text.len() as u64;
    };
    put(&mut gz, "{\"Records\":[");
    for i in 0..n {
        // delivered a little out of order: a second or two either way
        let at = t0 + chrono::Duration::milliseconds(i as i64 * 20 + rng.below(3000) as i64);
        let other = i % 97 == 96;
        let (name, read) = match rng.below(10) {
            0 => ("PutObject", false),
            1 => ("HeadObject", true),
            2 => ("ListObjectsV2", true),
            _ => ("GetObject", true),
        };
        let key = format!(
            "warehouse/db{}/t{:02}/data/dt=2026-11-{:02}/part-{:05}-6c5e0a1b-1f2e-4d3c-8b7a-{:012x}.c000.snappy.parquet",
            rng.below(4),
            rng.below(15),
            1 + rng.below(28),
            rng.below(4000),
            rng.next() & 0xffff_ffff_ffff
        );
        let event = serde_json::json!({
            "eventVersion": "1.09",
            "userIdentity": {
                "type": "AssumedRole",
                "principalId": format!("AROAEXAMPLEEXAMPLE:job-{}", rng.below(16)),
                "arn": format!("arn:aws:sts::123456789012:assumed-role/etl/job-{}", rng.below(16)),
                "accountId": "123456789012",
                "accessKeyId": "ASIAEXAMPLEEXAMPLE",
                "sessionContext": {
                    "sessionIssuer": {
                        "type": "Role",
                        "principalId": "AROAEXAMPLEEXAMPLE",
                        "arn": "arn:aws:iam::123456789012:role/etl",
                        "accountId": "123456789012",
                        "userName": "etl"
                    },
                    "attributes": { "creationDate": "2026-11-02T23:50:00Z", "mfaAuthenticated": "false" }
                }
            },
            "eventTime": at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            "eventSource": if other { "kms.amazonaws.com" } else { "s3.amazonaws.com" },
            "eventName": if other { "Decrypt" } else { name },
            "awsRegion": "eu-west-1",
            "sourceIPAddress": format!("10.0.{}.{}", rng.below(8), rng.below(250)),
            "userAgent": "[aws-sdk-java/2.21.0 Linux/6.1.0 OpenJDK_64-Bit_Server_VM/17.0.9+9 Java/17.0.9 vendor/Eclipse_Adoptium io/sync http/Apache cfg/retry-mode/legacy]",
            "requestParameters": {
                "bucketName": "lake",
                "Host": "lake.s3.eu-west-1.amazonaws.com",
                "key": key,
                "prefix": "warehouse/",
                "list-type": "2"
            },
            "responseElements": null,
            "additionalEventData": {
                "SignatureVersion": "SigV4",
                "CipherSuite": "TLS_AES_128_GCM_SHA256",
                "bytesTransferredIn": if read { 0 } else { 8 << 20 },
                "AuthenticationMethod": "AuthHeader",
                "x-amz-id-2": format!("{:064x}{:012x}", rng.next(), rng.next() & 0xffff_ffff_ffff),
                "bytesTransferredOut": if read { 1 << 20 } else { 0 }
            },
            "requestID": format!("{:016X}", rng.next()),
            "eventID": format!("{:032x}", rng.next()),
            "readOnly": read,
            "resources": [
                { "type": "AWS::S3::Object", "ARN": format!("arn:aws:s3:::lake/{key}") },
                { "accountId": "123456789012", "type": "AWS::S3::Bucket", "ARN": "arn:aws:s3:::lake" }
            ],
            "eventType": "AwsApiCall",
            "managementEvent": false,
            "recipientAccountId": "123456789012",
            "eventCategory": "Data",
            "tlsDetails": {
                "tlsVersion": "TLSv1.3",
                "cipherSuite": "TLS_AES_128_GCM_SHA256",
                "clientProvidedHostHeader": "lake.s3.eu-west-1.amazonaws.com"
            }
        });
        s3 += !other as usize;
        if i > 0 {
            put(&mut gz, ",");
        }
        put(&mut gz, &event.to_string());
    }
    put(&mut gz, "]}");
    gz.finish().expect("finished").flush().expect("flushed");
    (s3, bytes)
}

/// What `sonde import` does with a CloudTrail file: read as a stream
/// (decompressed and parsed an event at a time), each request written as
/// it is read, and the run written from that file in time order
/// ([`sonde_analysis::import::Spool`]). What it holds is 24 bytes a request
/// (its time, and where it is), not the file: neither its text, nor its
/// JSON, nor its records. Before 33.0 the same import held all three: the
/// text twice (bytes, then a string), the document, its `Records` again,
/// and every record.
#[test]
#[ignore = "a CloudTrail file of 300 thousand events: run in release, as CI does"]
fn an_import_of_three_hundred_thousand_events_is_written_within_its_memory() {
    use sonde_analysis::import::Spool;
    let _alone = alone(false);
    let dir = tempfile::tempdir().expect("a folder");
    let source = dir.path().join("trail.json.gz");
    let (events, text) = trail(&source, 300_000);
    assert!(text > 400 << 20, "{text} bytes of text");
    peak_from_now();
    let started = Instant::now();
    let out = dir.path().join("trail.sonde");
    let mut spool = Spool::create(dir.path()).expect("a place for it");
    spool.add_file(&source).expect("the trail reads");
    let saved = spool
        .seal("trail")
        .expect("requests in it")
        .save(&out)
        .expect("the run is written");
    let took = started.elapsed();
    assert_eq!(saved.requests, events);
    assert_eq!(saved.other_services, 300_000 - events);
    let peak = peak_mib();
    eprintln!(
        "an import of {events} requests from {} MiB of text: {:.1} s, peak {peak:?} MiB",
        text >> 20,
        took.as_secs_f64()
    );
    held(
        "import of 300 thousand CloudTrail events",
        took,
        Duration::from_secs(180),
        IMPORT_MIB,
    );
    // and it does not grow as the file does: far below what the file holds
    if let Some(peak) = peak {
        assert!(
            peak * 8 < text >> 20,
            "peak {peak} MiB for {} MiB of text",
            text >> 20
        );
    }
    // the run it wrote is a run: every request, in time order
    let run = RunFile::open_lean(&out).expect("the run opens");
    assert_eq!(run.records.len(), events);
    assert!(run.records.windows(2).all(|w| w[0].time <= w[1].time));
    assert_eq!(run.header.endpoint, "s3.eu-west-1.amazonaws.com");
}

/// What an import of 300 thousand events may hold at once: what was measured
/// (18 MiB, docs/dev/performance.md) and room for another allocator, far
/// below its 544 MiB of text.
const IMPORT_MIB: u64 = 32;

/// A lean reading of a talkative program's run: the output sonde recorded
/// (here 200 MiB of it, four chunks a request) is read past, not kept.
/// `show` and `check` never read it; before 33.0 they held it all.
#[test]
#[ignore = "fifty thousand requests and 200 MiB of output: run in release, as CI does"]
fn a_talkative_programs_output_is_not_held_by_a_lean_reading() {
    use std::io::{BufRead, Write};
    let _alone = alone(false);
    let dir = tempfile::tempdir().expect("a folder");
    let quiet = run(dir.path(), "lake-50000-3.sonde", |p| {
        support::lake(50_000, p, 3)
    });
    // the same run, with what the program printed between its requests
    let path = dir.path().join("talkative.sonde");
    let mut output = 0u64;
    {
        let lines = std::io::BufReader::new(flate2::read::MultiGzDecoder::new(
            std::fs::File::open(&quiet).expect("the run"),
        ));
        let mut gz = flate2::write::GzEncoder::new(
            std::io::BufWriter::new(std::fs::File::create(&path).expect("the run")),
            flate2::Compression::new(1),
        );
        let said = "x".repeat(1000);
        for (i, line) in lines.lines().enumerate() {
            let line = line.expect("a line");
            writeln!(gz, "{line}").expect("written");
            if i == 0 {
                continue;
            }
            let time: String = serde_json::from_str::<serde_json::Value>(&line).expect("a record")
                ["time"]
                .as_str()
                .expect("its time")
                .into();
            for k in 0..4 {
                let chunk = serde_json::json!({ "log": { "time": time, "text": format!("{i}.{k} {said}\n") } });
                let chunk = chunk.to_string();
                output += chunk.len() as u64;
                writeln!(gz, "{chunk}").expect("written");
            }
        }
        gz.finish().expect("finished").flush().expect("flushed");
    }
    assert!(output > 200 << 20, "{output} bytes of output");
    peak_from_now();
    let started = Instant::now();
    let s = RunFile::open_lean(&path).expect("the run opens");
    assert!(s.logs.is_empty() && s.records.len() >= 50_000);
    let a = Analysis::of(&s, &RequestPrices::default());
    let took = started.elapsed();
    assert!(!a.episodes.is_empty());
    held(
        "show of 50 thousand requests and 200 MiB of output",
        took,
        Duration::from_secs(30),
        TALKATIVE_MIB,
    );
    drop((a, s));
    // asked for, it is there (what the app's run and a shared page show)
    let s = RunFile::open_lean_with_logs(&path).expect("the run opens");
    assert_eq!(s.logs.len(), s.records.len() * 4);
}

/// What a lean reading of fifty thousand requests may hold, whatever their
/// program printed (40 MiB measured; its 200 MiB of output would be 200 more).
const TALKATIVE_MIB: u64 = 64;
