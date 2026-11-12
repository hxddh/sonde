//! The table the app is tested against too (`cases.json`, written from
//! `app/src/format.ts`): the same number, the same figure.

use serde_json::Value;

fn cases() -> Value {
    serde_json::from_str(include_str!("../cases.json")).expect("cases.json")
}

#[test]
fn sizes_are_written_as_the_app_writes_them() {
    for c in cases()["bytes"].as_array().unwrap() {
        let n = c[0].as_u64().unwrap();
        assert_eq!(sonde_fmt::bytes(n), c[1].as_str().unwrap(), "{n} B");
    }
}

#[test]
fn counts_are_written_as_the_app_writes_them() {
    for c in cases()["counts"].as_array().unwrap() {
        let n = c[0].as_u64().unwrap();
        assert_eq!(sonde_fmt::count(n), c[1].as_str().unwrap(), "{n}");
    }
}

#[test]
fn lengths_of_time_are_the_apps_figures_in_the_terminals_words() {
    for c in cases()["durations"].as_array().unwrap() {
        let ms = c["ms"].as_f64().unwrap();
        assert_eq!(
            sonde_fmt::duration(ms),
            c["cli"].as_str().unwrap(),
            "{ms} ms"
        );
    }
}

/// The app's test of the same table: every case is in it, in the app's
/// words (`app/tests/desktop.test.tsx`, "one way to write a figure").
#[test]
fn the_apps_test_has_every_case() {
    let app = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../app/tests/desktop.test.tsx"
    ))
    .expect("the app's tests");
    let cases = cases();
    for c in cases["bytes"].as_array().unwrap() {
        let line = format!("[{}, \"{}\"]", c[0], c[1].as_str().unwrap());
        assert!(app.contains(&line), "{line} is not in the app's test");
    }
    for c in cases["durations"].as_array().unwrap() {
        let line = format!("[{}, \"{}\"]", c["ms"], c["app"].as_str().unwrap());
        assert!(app.contains(&line), "{line} is not in the app's test");
    }
}
