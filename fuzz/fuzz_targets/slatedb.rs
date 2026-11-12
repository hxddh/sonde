//! SlateDB: a manifest (FlatBuffers) and an SSTable.
#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let _ = decoders::slatedb::decode_manifest(data);
    let _ = decoders::sstable::read_all(data);
    let _ = decoders::sstable::protobuf(data);
});
