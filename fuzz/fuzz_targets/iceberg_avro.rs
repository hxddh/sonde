//! Iceberg's manifest lists and manifests (Avro): every `.avro` of the sample
//! table is the fuzzer's, so both are read from it.
#![no_main]

#[path = "common.rs"]
mod common;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    common::iceberg_with(data, |k| k.ends_with(".avro"));
});
