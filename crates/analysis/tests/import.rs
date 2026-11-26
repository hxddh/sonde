//! `sonde import` reads its source as a stream and writes the run from a
//! file ([`Spool`]): the run is, byte for byte, the one the import that
//! holds everything in memory makes ([`import_counted`], the app's and what
//! `sonde import` did up to 32.0), and what it refuses it refuses in the
//! same words. The sources are `tests/import/`.

use std::path::{Path, PathBuf};

use sonde_analysis::import::{self, Saved, Spool};

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/import")
        .join(name)
}

fn gzipped(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(bytes).unwrap();
    z.finish().unwrap()
}

/// The import that holds the text, its JSON and every record: what it
/// left out, or why it refused.
fn held(bytes: &[u8], name: &str, out: &Path) -> Result<Saved, String> {
    let text = import::text_of(bytes)?;
    let i = import::import_counted(&text, name)?;
    import::save(&i.run, out).unwrap();
    Ok(Saved {
        source: i.source,
        requests: i.run.records.len(),
        malformed: i.malformed,
        not_requests: i.not_requests,
        other_hosts: i.other_hosts,
        other_services: i.other_services,
    })
}

/// The import that streams.
fn streamed(file: &Path, name: &str, out: &Path) -> Result<Saved, String> {
    let mut spool = Spool::create(out.parent().unwrap()).unwrap();
    spool.add_file(file)?;
    Ok(spool.seal(name)?.save(out).unwrap())
}

/// What a folder holds, by name.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn text_of_run(path: &Path) -> String {
    use std::io::Read;
    let mut text = String::new();
    flate2::read::MultiGzDecoder::new(std::fs::File::open(path).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    text
}

#[test]
fn a_streamed_import_writes_the_run_the_import_in_memory_writes() {
    let dir = tempfile::tempdir().unwrap();
    for (name, requests) in [
        ("access.log", 7),
        ("trail.json", 5),
        ("trail.jsonl", 4),
        ("page.har", 4),
    ] {
        let bytes = std::fs::read(sample(name)).unwrap();
        let zipped = dir.path().join(format!("{name}.gz"));
        std::fs::write(&zipped, gzipped(&bytes)).unwrap();
        for (file, bytes) in [(sample(name), bytes.clone()), (zipped, gzipped(&bytes))] {
            let (a, b) = (
                dir.path().join("held.sonde"),
                dir.path().join("streamed.sonde"),
            );
            let held = held(&bytes, "sample", &a).unwrap();
            let streamed = streamed(&file, "sample", &b).unwrap();
            assert_eq!(streamed, held, "{name}");
            assert_eq!(held.requests, requests, "{name}");
            // as text first: a difference then says where
            assert_eq!(text_of_run(&b), text_of_run(&a), "{name}");
            assert_eq!(
                std::fs::read(&b).unwrap(),
                std::fs::read(&a).unwrap(),
                "{name}: the files"
            );
            // and it is a run: in time order, numbered in it
            let run = sonde_analysis::run::RunFile::open(&b).unwrap();
            assert_eq!(run.records.len(), requests);
            assert!(run.records.windows(2).all(|w| w[0].time <= w[1].time));
            assert!(
                run.records
                    .iter()
                    .enumerate()
                    .all(|(i, r)| r.id == i as u64 + 1)
            );
            std::fs::remove_file(&a).unwrap();
            std::fs::remove_file(&b).unwrap();
        }
    }
    // nothing of the import is left beside its run
    assert_eq!(
        names(dir.path()),
        [
            "access.log.gz",
            "page.har.gz",
            "trail.json.gz",
            "trail.jsonl.gz"
        ]
    );
    // what the samples hold is what each source can: a copy named by the
    // store's own line, lines that are no request, other hosts and services
    let a = dir.path().join("a.sonde");
    let log = streamed(&sample("access.log"), "sample", &a).unwrap();
    assert_eq!((log.malformed, log.not_requests), (1, 2));
    let text = text_of_run(&a);
    assert!(text.contains("x-amz-copy-source") && text.contains("/lake/t/part-01.parquet"));
    assert!(text.contains("\"wholeSeconds\":true"), "{text}");
    let trail = streamed(&sample("trail.json"), "sample", &a).unwrap();
    assert_eq!((trail.malformed, trail.other_services), (1, 1));
    let lines = streamed(&sample("trail.jsonl"), "sample", &a).unwrap();
    assert_eq!((lines.malformed, lines.other_services), (1, 1));
    let har = streamed(&sample("page.har"), "sample", &a).unwrap();
    assert_eq!((har.malformed, har.other_hosts), (1, 2));
    let text = text_of_run(&a);
    assert!(
        !text.contains("0123abcd") && !text.contains("Signature=abcdef"),
        "{text}"
    );
}

#[test]
fn a_streamed_import_of_a_folder_is_the_folders_import() {
    let dir = tempfile::tempdir().unwrap();
    let logs = dir.path().join("logs");
    std::fs::create_dir_all(logs.join("2026/02")).unwrap();
    let log = std::fs::read_to_string(sample("access.log")).unwrap();
    // the copy's request in one file, the store's line of its source in another
    let (copy, rest): (Vec<&str>, Vec<&str>) = log
        .lines()
        .partition(|l| l.contains("REST.COPY.OBJECT_GET"));
    std::fs::write(logs.join("2026/02/b.log"), rest.join("\n")).unwrap();
    std::fs::write(
        logs.join("2026/02/c.log.gz"),
        gzipped(copy.join("\n").as_bytes()),
    )
    .unwrap();
    std::fs::write(logs.join("2026/a.log"), log.lines().nth(1).unwrap()).unwrap();
    std::fs::write(logs.join("README.txt"), "the logs of lake").unwrap();
    std::fs::copy(sample("page.har"), logs.join("z.har")).unwrap();
    std::fs::write(logs.join("empty.log"), " \n").unwrap();

    let (a, b) = (
        dir.path().join("held.sonde"),
        dir.path().join("streamed.sonde"),
    );
    let held = import::import_dir(&logs, "logs").unwrap();
    import::save(&held.imported.run, &a).unwrap();
    let mut spool = Spool::create(dir.path()).unwrap();
    let (files, left_out) = spool.add_dir(&logs).unwrap();
    assert_eq!(spool.requests(), held.imported.run.records.len());
    let saved = spool.seal("logs").unwrap().save(&b).unwrap();
    assert_eq!((files, &left_out), (held.files, &held.left_out));
    assert_eq!(files, 3);
    assert_eq!(left_out.len(), 3, "{left_out:?}");
    assert_eq!(saved.requests, 8);
    assert_eq!(saved.not_requests, held.imported.not_requests);
    assert_eq!(text_of_run(&b), text_of_run(&a));
    assert_eq!(std::fs::read(&b).unwrap(), std::fs::read(&a).unwrap());
    assert!(text_of_run(&b).contains("x-amz-copy-source"));
    assert_eq!(names(dir.path()), ["held.sonde", "logs", "streamed.sonde"]);
}

#[test]
fn a_streamed_import_refuses_what_the_import_in_memory_refuses_in_its_words() {
    let dir = tempfile::tempdir().unwrap();
    let trail = std::fs::read_to_string(sample("trail.json")).unwrap();
    let har = std::fs::read_to_string(sample("page.har")).unwrap();
    let run = {
        let out = dir.path().join("run.sonde");
        streamed(&sample("page.har"), "sample", &out).unwrap();
        let text = text_of_run(&out);
        std::fs::remove_file(&out).unwrap();
        text
    };
    let cut_gzip = {
        let mut z = gzipped(trail.as_bytes());
        z.truncate(z.len() / 2);
        z
    };
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("white space", b" \n\t \n".to_vec()),
        ("no source", b"hello, lake".to_vec()),
        ("a run", run.into_bytes()),
        // a document cut short, on several lines and on one
        ("cut", trail.as_bytes()[..trail.len() * 2 / 3].to_vec()),
        (
            "cut on one line",
            trail.replace('\n', "").as_bytes()[..trail.len() / 2].to_vec(),
        ),
        ("damaged", trail.replacen("\"eventName\":", "\"eventName\"", 1).into_bytes()),
        ("more after its end", format!("{trail}{trail}").into_bytes()),
        (
            "no S3 events",
            br#"{"Records":[{"eventTime":"2026-02-06T00:00:01Z","eventSource":"ec2.amazonaws.com"}]}"#
                .to_vec(),
        ),
        ("no records", br#"{"Records":{}}"#.to_vec()),
        ("a HAR cut short", har.as_bytes()[..har.len() / 2].to_vec()),
        ("a HAR without entries", br#"{"log":{"version":"1.2"},"entries":3}"#.to_vec()),
        ("a HAR whose log is no object", br#"{"log":[],"entries":[]}"#.to_vec()),
        ("a HAR of nothing", br#"{"log":{"entries":[]}}"#.to_vec()),
        ("gzip cut short", cut_gzip),
    ];
    for (what, bytes) in cases {
        let file = dir.path().join("source");
        std::fs::write(&file, &bytes).unwrap();
        let out = dir.path().join("out.sonde");
        let held = held(&bytes, "sample", &out);
        let streamed = streamed(&file, "sample", &out);
        assert!(held.is_err(), "{what}: {held:?}");
        assert_eq!(streamed, held, "{what}");
        let why = streamed.unwrap_err();
        assert_eq!(
            import::is_bad_data(&why),
            matches!(
                what,
                "empty"
                    | "white space"
                    | "cut"
                    | "cut on one line"
                    | "damaged"
                    | "more after its end"
                    | "no S3 events"
                    | "no records"
                    | "a HAR of nothing"
            ),
            "{what}: {why}"
        );
        // nothing written, nothing left
        assert_eq!(names(dir.path()), ["source"], "{what}");
    }
    // a file of another source after the first is left out, and what was read stays
    let mut spool = Spool::create(dir.path()).unwrap();
    spool.add_file(&sample("trail.json")).unwrap();
    let before = spool.requests();
    let why = spool.add_file(&sample("page.har")).unwrap_err();
    assert_eq!(why, "a HAR among CloudTrail files");
    let cut = dir.path().join("cut.json");
    std::fs::write(&cut, &trail.as_bytes()[..trail.len() * 2 / 3]).unwrap();
    assert!(import::is_bad_data(&spool.add_file(&cut).unwrap_err()));
    assert_eq!(
        spool.requests(),
        before,
        "a file given up on leaves no request"
    );
    spool.add_bytes(trail.as_bytes()).unwrap();
    let out = dir.path().join("out.sonde");
    let saved = spool.seal("sample").unwrap().save(&out).unwrap();
    assert_eq!(saved.requests, before * 2);
    assert_eq!((saved.malformed, saved.other_services), (2, 2));
    let run = sonde_analysis::run::RunFile::open(&out).unwrap();
    assert_eq!(run.records.len(), before * 2);
}
