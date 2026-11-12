//! CloudTrail: the fuzzer's JSON as a log's records.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = sonde_proxy::import::import_counted(&format!("{{\"Records\":{text}"), "fuzz");
});
