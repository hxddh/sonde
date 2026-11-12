//! S3 server access logs: the fuzzer's lines after an owner and a bucket, so
//! that every input is read as a log (gzip as S3 delivers it, too).
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let Ok(text) = sonde_proxy::import::text_of(data) else {
        return;
    };
    let _ = sonde_proxy::import::import_counted(&format!("79a5 lake [{text}"), "fuzz");
});
