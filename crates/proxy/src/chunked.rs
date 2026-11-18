//! `aws-chunked` bodies whose chunks are signed with the client's key. The
//! proxy cannot re-sign chunks it did not produce the seed signature for, so
//! it decodes them as they arrive ([`Decoder`]) and forwards the payload
//! unsigned: as it is, or (with a trailing checksum) framed again as an
//! unsigned `aws-chunked` body that carries the client's checksum to the
//! store ([`Encoder`]). Neither holds more than a chunk.

/// The `x-amz-content-sha256` values whose body carries chunk signatures.
pub fn is_signed_streaming(content_sha256: &str) -> bool {
    content_sha256.starts_with("STREAMING-AWS4-HMAC-SHA256-PAYLOAD")
        || content_sha256.starts_with("STREAMING-AWS4-ECDSA-P256-SHA256-PAYLOAD")
}

/// A decoded body: the payload and the trailing headers (without the
/// trailer signature).
#[derive(Debug, PartialEq, Eq)]
pub struct Decoded {
    pub payload: Vec<u8>,
    pub trailers: Vec<(String, String)>,
}

/// Decodes an `aws-chunked` body held whole.
pub fn decode(body: &[u8]) -> Result<Decoded, String> {
    let mut d = Decoder::default();
    let mut payload = Vec::with_capacity(body.len());
    d.feed(body, &mut payload)?;
    d.finish()?;
    Ok(Decoded {
        payload,
        trailers: d.trailers,
    })
}

/// An `aws-chunked` body decoded a piece at a time, as it arrives.
#[derive(Debug, Default)]
pub struct Decoder {
    /// What arrived and is not read yet: part of a chunk's header, its
    /// CRLF or a trailing line (data is passed on as it comes).
    held: Vec<u8>,
    state: State,
    /// The trailing headers (lower-case, without the trailer signature),
    /// once the body has ended.
    pub trailers: Vec<(String, String)>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Head,
    /// Data of the chunk still to come.
    Data(usize),
    /// The CRLF after a chunk's data.
    DataEnd,
    Trailers,
    Done,
}

/// The longest line a chunk header or a trailer may be.
const LINE_MAX: usize = 8 << 10;

impl Decoder {
    /// Reads `input`, appending the payload in it to `out`.
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) -> Result<(), String> {
        let mut buf = std::mem::take(&mut self.held);
        buf.extend_from_slice(input);
        let mut at = 0;
        let read = self.read(&buf, &mut at, out);
        buf.drain(..at);
        self.held = buf;
        read?;
        if self.held.len() > LINE_MAX && matches!(self.state, State::Head | State::Trailers) {
            return Err("aws-chunked: a line without CRLF".into());
        }
        Ok(())
    }

    /// Reads `buf` from `at` as far as it goes, moving `at` past what is read.
    fn read(&mut self, buf: &[u8], at: &mut usize, out: &mut Vec<u8>) -> Result<(), String> {
        loop {
            let rest = &buf[*at..];
            match self.state {
                State::Done => return Ok(()),
                State::Data(left) => {
                    let n = left.min(rest.len());
                    if n == 0 {
                        return Ok(());
                    }
                    out.extend_from_slice(&rest[..n]);
                    *at += n;
                    self.state = if n == left {
                        State::DataEnd
                    } else {
                        State::Data(left - n)
                    };
                }
                State::DataEnd => {
                    if rest.len() < 2 {
                        return Ok(());
                    }
                    if &rest[..2] != b"\r\n" {
                        return Err("aws-chunked: a chunk without its CRLF".into());
                    }
                    *at += 2;
                    self.state = State::Head;
                }
                State::Head | State::Trailers => {
                    let Some(end) = rest.windows(2).position(|w| w == b"\r\n") else {
                        return Ok(());
                    };
                    let line = &rest[..end];
                    *at += end + 2;
                    if self.state == State::Trailers {
                        if line.is_empty() {
                            self.state = State::Done;
                            continue;
                        }
                        let l = String::from_utf8_lossy(line);
                        if let Some((k, v)) = l.split_once(':') {
                            let k = k.trim().to_ascii_lowercase();
                            if k != "x-amz-trailer-signature" {
                                self.trailers.push((k, v.trim().to_string()));
                            }
                        }
                        continue;
                    }
                    let head = std::str::from_utf8(line)
                        .map_err(|_| "aws-chunked: chunk header not UTF-8")?;
                    let size_hex = head.split(';').next().unwrap_or("").trim();
                    let size = usize::from_str_radix(size_hex, 16)
                        .map_err(|_| format!("aws-chunked: chunk size {size_hex:?}"))?;
                    self.state = if size == 0 {
                        State::Trailers
                    } else {
                        State::Data(size)
                    };
                }
            }
        }
    }

    /// Whether the body ended where it should: its last chunk read, and
    /// its trailers (or none) to their blank line.
    pub fn finish(&self) -> Result<(), String> {
        match self.state {
            State::Done => Ok(()),
            // no trailer: the last chunk is followed by one blank line, or
            // by nothing at all
            State::Trailers if self.held.is_empty() => Ok(()),
            _ => Err("aws-chunked: the body stops before its last chunk".into()),
        }
    }
}

/// A payload framed as an unsigned `aws-chunked` body
/// (`STREAMING-UNSIGNED-PAYLOAD-TRAILER`): chunks of one size, the last
/// shorter, then the trailing headers.
#[derive(Debug)]
pub struct Encoder {
    chunk: usize,
    pending: Vec<u8>,
}

impl Encoder {
    pub fn new(chunk: usize) -> Encoder {
        Encoder {
            chunk,
            pending: Vec::with_capacity(chunk),
        }
    }

    fn frame(out: &mut Vec<u8>, data: &[u8]) {
        out.extend_from_slice(format!("{:x}\r\n", data.len()).as_bytes());
        out.extend_from_slice(data);
        out.extend_from_slice(b"\r\n");
    }

    /// The whole chunks `data` completes, framed.
    pub fn push(&mut self, data: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(data);
        let mut out = Vec::new();
        let whole = self.pending.len() / self.chunk * self.chunk;
        for c in self.pending[..whole].chunks(self.chunk) {
            Encoder::frame(&mut out, c);
        }
        self.pending.drain(..whole);
        out
    }

    /// The rest: the last chunk, the empty one, and `trailers`.
    pub fn finish(&mut self, trailers: &[(String, String)]) -> Vec<u8> {
        let mut out = Vec::new();
        if !self.pending.is_empty() {
            Encoder::frame(&mut out, &self.pending);
            self.pending.clear();
        }
        out.extend_from_slice(b"0\r\n");
        for (k, v) in trailers {
            out.extend_from_slice(format!("{k}:{v}\r\n").as_bytes());
        }
        out.extend_from_slice(b"\r\n");
        out
    }

    /// The length of a payload of `decoded` bytes framed in chunks of
    /// `chunk`, with trailers of these name and value lengths.
    pub fn len(decoded: u64, chunk: u64, trailers: &[(usize, usize)]) -> u64 {
        let framed = |n: u64| format!("{n:x}").len() as u64 + 2 + n + 2;
        let (whole, rest) = (decoded / chunk, decoded % chunk);
        whole * framed(chunk)
            + if rest > 0 { framed(rest) } else { 0 }
            + 3
            + trailers
                .iter()
                .map(|(k, v)| (k + 1 + v + 2) as u64)
                .sum::<u64>()
            + 2
    }
}

/// How long a checksum header's value is (base64 of the digest), by its
/// name: what an [`Encoder`]'s length needs before the value arrives.
pub fn checksum_len(name: &str) -> Option<usize> {
    s3raw::checksum::Algorithm::from_header(name).map(|a| a.header_len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_chunks_and_trailer() {
        let sig = "a".repeat(64);
        let body = format!(
            "5;chunk-signature={sig}\r\nhello\r\n6;chunk-signature={sig}\r\n world\r\n0;chunk-signature={sig}\r\nx-amz-checksum-crc32:AAAAAA==\r\nx-amz-trailer-signature:{sig}\r\n\r\n"
        );
        let d = decode(body.as_bytes()).unwrap();
        assert_eq!(d.payload, b"hello world");
        assert_eq!(
            d.trailers,
            vec![("x-amz-checksum-crc32".into(), "AAAAAA==".into())]
        );
    }

    #[test]
    fn without_trailer() {
        let d = decode(b"3;chunk-signature=x\r\nabc\r\n0;chunk-signature=y\r\n\r\n").unwrap();
        assert_eq!(d.payload, b"abc");
        assert!(d.trailers.is_empty());
    }

    #[test]
    fn a_chunk_size_near_the_largest_number_is_an_error_not_a_panic() {
        for size in ["ffffffffffffffff", "fffffffffffffffe", "7fffffffffffffff"] {
            let body = format!("{size};chunk-signature=x\r\nabc\r\n0\r\n\r\n");
            assert!(decode(body.as_bytes()).is_err(), "{size}");
        }
    }

    /// Read a byte at a time or whole, a body decodes the same.
    #[test]
    fn decoded_as_it_arrives_in_pieces_of_any_size() {
        let sig = "a".repeat(64);
        let body = format!(
            "5;chunk-signature={sig}\r\nhello\r\n6;chunk-signature={sig}\r\n world\r\n0;chunk-signature={sig}\r\nx-amz-checksum-crc32:AAAAAA==\r\nx-amz-trailer-signature:{sig}\r\n\r\n"
        );
        for piece in [1, 2, 3, 7, 64, body.len()] {
            let mut d = Decoder::default();
            let mut out = Vec::new();
            for p in body.as_bytes().chunks(piece) {
                d.feed(p, &mut out).unwrap();
            }
            d.finish().unwrap();
            assert_eq!(out, b"hello world", "pieces of {piece}");
            assert_eq!(
                d.trailers,
                vec![("x-amz-checksum-crc32".into(), "AAAAAA==".into())]
            );
        }
        // without a trailer, and cut short
        let mut d = Decoder::default();
        let mut out = Vec::new();
        d.feed(
            b"3;chunk-signature=x\r\nabc\r\n0;chunk-signature=y\r\n\r\n",
            &mut out,
        )
        .unwrap();
        assert!(d.finish().is_ok() && out == b"abc");
        let mut d = Decoder::default();
        d.feed(b"10;chunk-signature=x\r\nabc", &mut out).unwrap();
        assert!(d.finish().is_err());
        assert!(
            Decoder::default()
                .feed(b"3;chunk-signature=x\r\nabcXY", &mut out)
                .is_err()
        );
    }

    /// The framed body is as long as said before it was made, and decodes
    /// to what went in.
    #[test]
    fn framed_again_unsigned_its_length_known_before() {
        let payload: Vec<u8> = (0..100_000u32).map(|i| i as u8).collect();
        let trailers = vec![(
            "x-amz-checksum-crc64nvme".to_string(),
            "AAAAAAAAAAA=".to_string(),
        )];
        for chunk in [1000usize, 4096, 65536, 100_000, 1 << 20] {
            let mut e = Encoder::new(chunk);
            let mut framed = Vec::new();
            for p in payload.chunks(777) {
                framed.extend(e.push(p));
            }
            framed.extend(e.finish(&trailers));
            let said = Encoder::len(payload.len() as u64, chunk as u64, &[(24, 12)]);
            assert_eq!(framed.len() as u64, said, "chunks of {chunk}");
            let d = decode(&framed).unwrap();
            assert_eq!((d.payload, d.trailers), (payload.clone(), trailers.clone()));
        }
        assert_eq!(checksum_len("x-amz-checksum-crc64nvme"), Some(12));
        assert_eq!(checksum_len("x-amz-checksum-sha256"), Some(44));
        // the newer ones, which boto3 sends as trailers too
        assert_eq!(checksum_len("x-amz-checksum-xxhash128"), Some(24));
        assert_eq!(checksum_len("x-amz-checksum-sha512"), Some(88));
        assert_eq!(checksum_len("x-amz-meta-other"), None);
    }

    #[test]
    fn truncated() {
        assert!(decode(b"10;chunk-signature=x\r\nabc").is_err());
        assert!(is_signed_streaming(
            "STREAMING-AWS4-HMAC-SHA256-PAYLOAD-TRAILER"
        ));
        assert!(!is_signed_streaming("STREAMING-UNSIGNED-PAYLOAD-TRAILER"));
    }
}
