//! The whole suite against live services (`SONDE_IT_TARGETS`).

mod common;

use probes::Verdict;
use probes::runner::{self, RunOptions};

#[tokio::test(flavor = "multi_thread")]
async fn suite_runs_and_cleans_up() {
    for t in common::targets() {
        let s = &t.sonde;
        let c = s.client(&t.name).unwrap();
        let b = common::bucket_name("probe");
        c.create_bucket(&b, false).await.unwrap();
        let opts = RunOptions {
            bucket: b.clone(),
            allow_temp_buckets: true,
            include_slow: true,
            ..Default::default()
        };
        let ids = runner::spawn(s, std::slice::from_ref(&t.name), opts);
        let v = s
            .jobs()
            .wait(&ids[0])
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", t.name));
        let run: probes::Run = serde_json::from_value(v).unwrap();
        println!("== {} ==", t.name);
        for r in &run.results {
            println!("{} {:<34} {}", r.verdict.symbol(), r.id, r.summary);
        }
        assert!(run.results.len() >= 40);
        // every probe made requests and kept them
        for r in run.results.iter().filter(|r| r.verdict != Verdict::Skipped) {
            assert!(
                !r.exchanges.is_empty(),
                "{}: {} kept no exchanges",
                t.name,
                r.id
            );
        }
        // nothing left behind
        let left = c
            .list_object_versions(&b, Some("sonde-probe/"), None, None, None)
            .await
            .unwrap();
        assert!(
            left.version.is_empty() && left.delete_marker.is_empty(),
            "{}: leftovers {:?}",
            t.name,
            left.version.iter().map(|v| &v.key).collect::<Vec<_>>()
        );
        let ups = c
            .list_multipart_uploads(&b, None, None, None)
            .await
            .unwrap();
        assert!(ups.upload.is_empty(), "{}: uploads left", t.name);
        let saved = probes::store::list(s.paths());
        assert_eq!(saved.len(), 1);
        let md = probes::report::markdown(std::slice::from_ref(&run));
        assert!(md.contains("| `cond.put-if-none-match` |"));
        let _ = c.delete_bucket(&b).await;
    }
}
