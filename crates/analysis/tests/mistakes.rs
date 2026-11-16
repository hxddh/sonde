//! The S3 mistakes people make most (24.0): small objects written one
//! each, deletes one at a time, a read and write back with no condition, a
//! key polled while it is not there, and a key read again and again between
//! its writes. Each is found with its fix for every SDK, priced, said the
//! same on an import, and reads fixed in the fix loop once its fix is made.

use chrono::DateTime;
use reqlog::{Header, Record};
use sonde_analysis::episode::{Observation, episodes, observations_of};
use sonde_analysis::fixloop::State;
use sonde_analysis::profile::{RequestPrices, Run, compare_runs};
use sonde_analysis::run::RunHeader;

fn rec(id: u64, at_ms: i64, op: &str, key: &str, status: u16) -> Record {
    let method = match op {
        "HeadObject" => "HEAD",
        "PutObject" => "PUT",
        "DeleteObject" => "DELETE",
        "DeleteObjects" => "POST",
        _ => "GET",
    };
    Record {
        id,
        time: DateTime::from_timestamp_millis(1_700_000_000_000 + at_ms).unwrap(),
        method: method.into(),
        kind: op.into(),
        bucket: Some("data".into()),
        key: (!key.is_empty()).then(|| key.into()),
        status: Some(status),
        latency_ms: 2.0,
        request_bytes: Some(if op == "PutObject" { 900 } else { 0 }).into(),
        response_bytes: Some(10).into(),
        ..Default::default()
    }
}

/// Requests one after another, 5 ms apart, numbered from 1.
struct Job(Vec<Record>);

impl Job {
    fn new() -> Job {
        Job(Vec::new())
    }

    fn add(&mut self, op: &str, key: &str, status: u16) -> &mut Record {
        let id = self.0.len() as u64 + 1;
        self.0.push(rec(id, id as i64 * 5, op, key, status));
        self.0.last_mut().unwrap()
    }

    /// `n` reads of other keys, each once: a run's other work.
    fn other(&mut self, n: usize) {
        for i in 0..n {
            self.add("GetObject", &format!("in/part-{i}.parquet"), 200);
        }
    }
}

fn header(imported: bool) -> RunHeader {
    let mut h = RunHeader::new(
        "demo",
        "http://demo",
        "us-east-1",
        Some("python3 job.py".into()),
    );
    h.whole_seconds = imported;
    h
}

fn found(rs: &[Record], kind: &str) -> Vec<Observation> {
    observations_of(&header(false), rs, &episodes(rs))
        .into_iter()
        .filter(|o| o.kind == kind)
        .collect()
}

fn run(rs: &[Record], before: Option<&Run>) -> Run {
    let mut r = Run::analysed(&header(false), rs, &RequestPrices::default());
    if let Some(b) = before {
        r.measure(rs, b);
    }
    r
}

/// Where the finding of `kind` stands in `after` against `before`.
fn state(before: &[Record], after: &[Record], kind: &str) -> State {
    let a = run(before, None);
    let b = run(after, Some(&a));
    let c = compare_runs(&a, &b);
    c.findings
        .iter()
        .find(|f| f.identity.starts_with(&format!("{kind}|")))
        .unwrap_or_else(|| panic!("{kind} in {:#?}", c.findings))
        .state
}

/// Every new kind has its fix in each SDK's language.
fn has_every_fix(o: &Observation) {
    for sdk in ["python", "java", "go", "js"] {
        assert!(o.fixes.iter().any(|f| f.sdk == sdk), "{} {sdk}", o.kind);
    }
}

#[test]
fn a_folder_s_small_objects_are_found_beside_folders_of_the_same_names() {
    // out/000, out/000/meta, out/001, out/001/meta, …: by key they alternate between out/ and its children
    let mut job = Job::new();
    for i in 0..100 {
        job.add("PutObject", &format!("out/{i:03}"), 200);
        job.add("PutObject", &format!("out/{i:03}/meta"), 200);
    }
    job.other(20);
    let small = found(&job.0, "obs.small");
    assert!(
        small.iter().any(|o| o.text.contains("data/out/,")),
        "{small:?}"
    );
}

#[test]
fn small_objects_written_one_each_are_found_priced_and_fixed_by_a_batch() {
    // hot5: 100 tasks, each 19 objects of 900 bytes in a folder of its own
    let mut job = Job::new();
    for t in 0..100 {
        for j in 0..19 {
            job.add("PutObject", &format!("h5/out/{t}/{j}.json"), 200);
        }
    }
    job.other(100);
    let small = found(&job.0, "obs.small");
    assert_eq!(small.len(), 1);
    let o = &small[0];
    assert_eq!(
        o.text,
        "1,900 objects of less than 64 KiB were written to data/h5/out/, one PUT each (1.6 MiB in all): write them as fewer, larger objects"
    );
    // a folder's 17 KiB fit in one object: 18 of each 19 PUTs go
    assert_eq!(o.saves_requests, 1800);
    has_every_fix(o);
    // priced: 1,800 writes at S3's price
    let mut ranked = vec![o.clone()];
    sonde_analysis::fixes::rank_by_saving(&mut ranked, &job.0, &RequestPrices::default());
    assert!(
        (ranked[0].saves_cost - 0.009).abs() < 1e-9,
        "{}",
        ranked[0].saves_cost
    );

    // one object each task: still small, 100 of them (less)
    let mut fewer = Job::new();
    for t in 0..100 {
        fewer.add("PutObject", &format!("h5/out/{t}.jsonl"), 200);
    }
    fewer.other(100);
    assert_eq!(state(&job.0, &fewer.0, "obs.small"), State::Less);
    // one object for the run: fixed
    let mut one = Job::new();
    one.add("PutObject", "h5/out/all.jsonl", 200);
    one.other(100);
    assert_eq!(state(&job.0, &one.0, "obs.small"), State::Fixed);
    // one key written again and again is no small file; nor a few of them
    let mut rewritten = Job::new();
    for _ in 0..100 {
        rewritten.add("PutObject", "state/counter.json", 200);
    }
    assert!(found(&rewritten.0, "obs.small").is_empty());
    let mut few = Job::new();
    for j in 0..20 {
        few.add("PutObject", &format!("in/{j}.json"), 200);
    }
    few.other(100);
    assert!(found(&few.0, "obs.small").is_empty());
}

#[test]
fn deletes_one_at_a_time_are_found_and_fixed_by_delete_objects() {
    let mut job = Job::new();
    job.other(300);
    for i in 0..300 {
        job.add("DeleteObject", &format!("job/events/e-{i:05}.json"), 204);
    }
    let del = found(&job.0, "obs.deletes");
    assert_eq!(del.len(), 1);
    assert_eq!(
        del[0].text,
        "300 objects under data/job/events/ were deleted one request each: DeleteObjects removes up to 1,000 in one"
    );
    // one request for all 300
    assert_eq!(del[0].saves_requests, 299);
    // about its bucket: the folder its keys share moves with its counts
    assert_eq!(del[0].identity, "obs.deletes|data");
    has_every_fix(&del[0]);
    let mut fixed = Job::new();
    fixed.other(300);
    let batch = fixed.add("DeleteObjects", "", 200);
    batch.rare_mut().request_body = Some(
        (0..300)
            .map(|i| format!("<Object><Key>job/events/e-{i:05}.json</Key></Object>"))
            .collect::<String>(),
    );
    assert_eq!(state(&job.0, &fixed.0, "obs.deletes"), State::Fixed);
    // a few, among much else: under 1% of the run, no finding
    let mut few = Job::new();
    few.other(3000);
    for i in 0..25 {
        few.add("DeleteObject", &format!("tmp/{i}"), 204);
    }
    assert!(found(&few.0, "obs.deletes").is_empty());
}

#[test]
fn a_read_and_write_back_with_no_condition_is_found_and_fixed_by_if_match() {
    let cycle = |job: &mut Job, conditional: bool| {
        job.add("GetObject", "job/config.json", 200);
        let put = job.add("PutObject", "job/config.json", 200);
        if conditional {
            put.lists_mut().request_headers = vec![Header {
                name: "If-Match".into(),
                value: "\"e1\"".into(),
            }];
        }
    };
    let mut job = Job::new();
    job.other(20);
    for _ in 0..5 {
        cycle(&mut job, false);
    }
    let rmw = found(&job.0, "obs.rmw");
    assert_eq!(rmw.len(), 1);
    assert_eq!(
        rmw[0].text,
        "an object was read and then written back with no If-Match 5 times (data/job/config.json the most): another writer's change in between is lost"
    );
    // nothing saved: what it is about is a lost update
    assert_eq!(rmw[0].saves_requests, 0);
    has_every_fix(&rmw[0]);
    assert!(rmw[0].fixes[0].code.contains("IfMatch="));
    let mut fixed = Job::new();
    fixed.other(20);
    for _ in 0..5 {
        cycle(&mut fixed, true);
    }
    assert_eq!(state(&job.0, &fixed.0, "obs.rmw"), State::Fixed);
    // a write that follows no read of its key is no read-modify-write
    let mut blind = Job::new();
    for i in 0..5 {
        blind.add("GetObject", &format!("in/{i}"), 200);
        blind.add("PutObject", &format!("out/{i}"), 200);
    }
    assert!(found(&blind.0, "obs.rmw").is_empty());
    // an import: the log keeps no headers, so it says it does not know
    let imported = observations_of(&header(true), &job.0, &episodes(&job.0));
    let o = imported.iter().find(|o| o.kind == "obs.rmw").unwrap();
    assert_eq!(o.phrases[0].key, "obs.rmw.log");
    assert!(
        o.text
            .contains("the log does not say whether with If-Match")
    );
    assert_eq!(o.identity, rmw[0].identity);
}

#[test]
fn a_key_polled_while_it_is_not_there_is_found_and_fixed_by_backing_off() {
    let mut job = Job::new();
    job.other(500);
    for _ in 0..10 {
        job.add("HeadObject", "job/_DONE", 404);
    }
    let polled = found(&job.0, "obs.polled");
    assert_eq!(polled.len(), 1);
    assert_eq!(
        polled[0].text,
        "data/job/_DONE was asked for 10 times while it was not there (404): back off between tries, or have its writer send an event"
    );
    // ten tries in 50 ms: backing off from a second asks once
    assert_eq!(polled[0].saves_requests, 9);
    has_every_fix(&polled[0]);
    // backing off: a try, a second, another
    let mut fixed = Job::new();
    fixed.other(500);
    fixed.add("HeadObject", "job/_DONE", 404);
    let late = fixed.add("HeadObject", "job/_DONE", 404);
    late.time += chrono::Duration::seconds(1);
    assert_eq!(state(&job.0, &fixed.0, "obs.polled"), State::Fixed);
    // a format's own lookups (SlateDB's newest manifest, its GC boundary)
    // are its way, not a loop to back off from
    let mut slate = Job::new();
    slate.other(20);
    for _ in 0..40 {
        slate.add("HeadObject", "slate/db/gc/manifest.boundary", 404);
        slate.add(
            "GetObject",
            "slate/db/manifest/00000000000000000005.manifest",
            404,
        );
    }
    assert!(found(&slate.0, "obs.polled").is_empty());
    // a day of polls (an import's): what backing off would still ask is
    // kept, a try a minute
    let mut day = Job::new();
    day.other(100);
    for i in 0..2880 {
        let r = day.add("HeadObject", "tables/_SUCCESS", 404);
        r.time = DateTime::from_timestamp_millis(1_700_000_000_000 + i * 30_000).unwrap();
    }
    let o = &found(&day.0, "obs.polled")[0];
    assert!(
        (1400..1460).contains(&o.saves_requests),
        "{}",
        o.saves_requests
    );
}

#[test]
fn a_key_read_again_between_its_writes_is_judged_stretch_by_stretch_with_a_floor() {
    // hot5: a schema read once by each of 100 tasks, among 1,900 writes: 5%
    let mut job = Job::new();
    job.add("PutObject", "h5/schema.json", 200);
    for t in 0..100 {
        job.add("GetObject", "h5/schema.json", 200);
        for j in 0..19 {
            job.add("PutObject", &format!("h5/out/{t}/{j}.json"), 200);
        }
    }
    let hot = found(&job.0, "obs.hot");
    assert_eq!(hot.len(), 1);
    assert_eq!(hot[0].saves_requests, 99);
    // rewritten now and then: each stretch between writes is read 9 times,
    // and a cache told of the program's own writes reads each once
    let mut stretches = Job::new();
    for _ in 0..10 {
        stretches.add("PutObject", "state/conf.json", 200);
        for _ in 0..9 {
            stretches.add("GetObject", "state/conf.json", 200);
        }
    }
    stretches.other(50);
    let hot = found(&stretches.0, "obs.hot");
    assert_eq!(hot.len(), 1);
    assert_eq!(
        (hot[0].phrases[0].args["n"].as_ref(), hot[0].saves_requests),
        ("90", 80)
    );
    // ten reads that would save under 1% of the requests (and of the cost)
    let mut little = Job::new();
    for _ in 0..10 {
        little.add("GetObject", "conf.json", 200);
    }
    little.other(2000);
    assert!(found(&little.0, "obs.hot").is_empty());
    // cached: read once, fixed
    let mut cached = Job::new();
    cached.add("PutObject", "h5/schema.json", 200);
    cached.add("GetObject", "h5/schema.json", 200);
    for t in 0..100 {
        for j in 0..19 {
            cached.add("PutObject", &format!("h5/out/{t}/{j}.json"), 200);
        }
    }
    assert_eq!(state(&job.0, &cached.0, "obs.hot"), State::Fixed);
}

/// 24.0 named a read written back by its key with the most cycles, and
/// deletes and small writes by the folder all their keys share: fixed in
/// part, each read fixed and new. One finding a run, about its bucket.
#[test]
fn a_finding_fixed_in_part_reads_less_whatever_its_text_names_now() {
    // read-modify-write: a.json 5 times, b.json 3; then a.json 2 times
    let rmw = |a: usize, b: usize| {
        let mut job = Job::new();
        job.other(20);
        for (key, n) in [("a.json", a), ("b.json", b)] {
            for _ in 0..n {
                job.add("GetObject", key, 200);
                job.add("PutObject", key, 200);
            }
        }
        job.0
    };
    let (before, after) = (rmw(5, 3), rmw(2, 3));
    assert!(
        found(&before, "obs.rmw")[0]
            .text
            .contains("data/a.json the most")
    );
    assert!(
        found(&after, "obs.rmw")[0]
            .text
            .contains("data/b.json the most")
    );
    assert_eq!(state(&before, &after, "obs.rmw"), State::Less);
    // deletes under out/a/, then a delete under tmp/ too: data/ is named
    let deletes = |tmp: bool| {
        let mut job = Job::new();
        job.other(100);
        for i in 0..60 {
            job.add("DeleteObject", &format!("out/a/{i}"), 204);
        }
        if tmp {
            job.add("DeleteObject", "tmp/x", 204);
        }
        job.0
    };
    let (one, two) = (deletes(false), deletes(true));
    let t = &found(&two, "obs.deletes")[0].text;
    assert!(t.starts_with("61 objects under data/ were"), "{t}");
    let (a, b) = (run(&one, None), run(&two, None));
    let c = compare_runs(&a, &b);
    let del: Vec<_> = c
        .findings
        .iter()
        .filter(|f| f.identity.starts_with("obs.deletes"))
        .collect();
    assert_eq!(del.len(), 1, "{del:#?}");
    assert_eq!(del[0].state, State::More);
    // small writes to out/, then fewer, and two to tmp/ besides: less
    let small = |n: usize, tmp: bool| {
        let mut job = Job::new();
        for i in 0..n {
            job.add("PutObject", &format!("out/{i}.json"), 200);
        }
        if tmp {
            job.add("PutObject", "tmp/1.json", 200);
            job.add("PutObject", "tmp/2.json", 200);
        }
        job.other(20);
        job.0
    };
    let (before, after) = (small(200, false), small(100, true));
    assert!(
        found(&after, "obs.small")[0]
            .text
            .contains("written to data/,")
    );
    assert_eq!(state(&before, &after, "obs.small"), State::Less);
}

/// Identities kept under the rules of 24.0 (4) are made again by these: a
/// read written back is about its bucket, and stays the one finding.
#[test]
fn a_kept_finding_of_a_bucket_s_kind_is_made_again_by_the_rules_of_now() {
    use sonde_analysis::fixloop::Program;
    let kept = serde_json::json!({
        "rules": 4,
        "findings": {
            "obs.rmw|data/a.json": {"kind": "obs.rmw", "text": "", "state": "open", "since": "r1", "predicted": {"requests": 0, "ms": 0.0}},
            "obs.hot|data/k": {"kind": "obs.hot", "text": "", "state": "open", "since": "r1", "predicted": {"requests": 0, "ms": 0.0}}
        }
    });
    let mut p: Program = serde_json::from_value(kept).unwrap();
    assert_eq!(p.rekey(), 0);
    let ids: Vec<&str> = p.findings.keys().map(String::as_str).collect();
    assert_eq!(ids, ["obs.hot|data/k", "obs.rmw|data"]);
}

/// 24.0 counted a key written 6 times as 6 small objects.
#[test]
fn small_writes_count_objects_and_leave_a_key_s_rewrites_out() {
    let mut job = Job::new();
    for i in 0..61 {
        job.add("PutObject", &format!("out/{i}.json"), 200);
    }
    for _ in 0..6 {
        job.add("PutObject", "out/state.json", 200);
    }
    job.other(20);
    let o = &found(&job.0, "obs.small")[0];
    assert!(
        o.text.starts_with("62 objects of less than 64 KiB"),
        "{}",
        o.text
    );
    // one object for all 62: 61 PUTs go, none of the rewrites counted
    assert_eq!(o.saves_requests, 61);
}

/// 24.0 took every read of a key away when a cache and HEAD-before-GET
/// were both acted on: `61 → 1`. The cache keeps one read.
#[test]
fn acting_on_a_cache_and_get_alone_keeps_one_read_of_the_key() {
    let mut job = Job::new();
    job.add("PutObject", "cfg/x.json", 200);
    for _ in 0..30 {
        job.add("HeadObject", "cfg/x.json", 200);
        job.add("GetObject", "cfg/x.json", 200);
    }
    let rs = &job.0;
    let os = observations_of(&header(false), rs, &episodes(rs));
    assert!(os.iter().any(|o| o.kind == "obs.hot"), "{os:#?}");
    assert!(os.iter().any(|o| o.kind == "obs.headget"), "{os:#?}");
    let p = sonde_analysis::profile::profile(rs, &RequestPrices::default());
    let w = sonde_analysis::fixes::what_if(rs, &p, &os, true).unwrap();
    assert_eq!(w.requests, (61, 2));
}

/// 24.0 said a saving of 29 of 1,500 requests was 1% of the run, beside
/// the 30 reads it is of said as 2%.
#[test]
fn a_saving_s_share_of_the_run_is_rounded_as_the_reads_it_is_of() {
    let mut job = Job::new();
    job.add("PutObject", "cfg/x.json", 200);
    for _ in 0..30 {
        job.add("GetObject", "cfg/x.json", 200);
    }
    job.other(1469);
    assert_eq!(job.0.len(), 1500);
    let o = &found(&job.0, "obs.hot")[0];
    assert!(o.text.contains("(2% of all requests)"), "{}", o.text);
    assert_eq!(
        o.saving_text.as_deref(),
        Some("29 fewer requests (2% of the run)")
    );
}
