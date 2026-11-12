//! Iceberg's table metadata (JSON): the newest `*.metadata.json` of the
//! sample table is the fuzzer's.
#![no_main]

#[path = "common.rs"]
mod common;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    common::iceberg_with(data, |k| {
        k.ends_with("00004-a9f29147-7a98-4522-a8e7-63e01130e611.metadata.json")
    });
});
