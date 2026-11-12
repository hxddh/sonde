//! lakeFS: a range or metarange (an SSTable of Graveler's).
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = decoders::lakefs::decode(data);
});
