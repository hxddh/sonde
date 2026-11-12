//! S3's XML: every answer s3raw parses, and an error's body.
#![no_main]

use s3raw::xml;

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let _ = xml::parse_error(text);
    let _ = xml::list_buckets(text);
    let _ = xml::list_objects_v2(text);
    let _ = xml::list_object_versions(text);
    let _ = xml::list_multipart_uploads(text);
    let _ = xml::list_parts(text);
    let _ = xml::initiate_multipart_upload(text);
    let _ = xml::complete_multipart_upload(text);
});
