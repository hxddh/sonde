//! Milvus Woodpecker: a block's records, its footer and headers.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    for r in decoders::woodpecker::records(data) {
        let _ = r;
    }
    let _ = decoders::woodpecker::footer(data);
    let _ = decoders::woodpecker::block_header(data);
    let _ = decoders::woodpecker::index_record(data);
});
