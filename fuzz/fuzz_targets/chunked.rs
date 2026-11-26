//! An `aws-chunked` body, as a program sends one through the proxy: read
//! whole and a piece at a time, which must agree, and a payload framed again
//! reads back as it was.
#![no_main]

use sonde_proxy::chunked::{self, Decoder, Encoder};

libfuzzer_sys::fuzz_target!(|data: &[u8]| {
    // the first byte: where the body is cut into pieces as it arrives
    let Some((&step, body)) = data.split_first() else {
        return;
    };
    let whole = chunked::decode(body);

    let mut d = Decoder::default();
    let mut payload = Vec::new();
    let mut fed = Ok(());
    for piece in body.chunks(usize::from(step).max(1)) {
        fed = d.feed(piece, &mut payload);
        if fed.is_err() {
            break;
        }
    }
    let pieces = fed.and_then(|()| d.finish()).map(|()| (payload, d.trailers));
    match (&whole, &pieces) {
        (Ok(w), Ok((p, t))) => {
            assert_eq!(&w.payload, p);
            assert_eq!(&w.trailers, t);
        }
        (Err(_), Err(_)) => {}
        _ => panic!("whole {whole:?}, in pieces {pieces:?}"),
    }

    // measured alone, it counts what it would decode
    let mut m = Decoder::default();
    if m.measure(body).and_then(|()| m.finish()).is_ok()
        && let Ok(w) = &whole
    {
        assert_eq!(m.decoded, w.payload.len() as u64);
    }

    // the body's bytes as a payload, framed again and read back
    let chunk = usize::from(step % 64) + 1;
    let trailers = vec![("x-amz-checksum-crc32".to_string(), "AAAAAA==".to_string())];
    let mut e = Encoder::new(chunk);
    let mut framed = e.push(body);
    framed.extend(e.finish(&trailers));
    assert_eq!(
        framed.len() as u64,
        Encoder::len(body.len() as u64, chunk as u64, &[(20, 8)])
    );
    let back = chunked::decode(&framed).expect("a body framed by the encoder reads");
    assert_eq!(back.payload, body);
    assert_eq!(back.trailers, trailers);
});
