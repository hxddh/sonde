//! A reader for the LevelDB / RocksDB block-based table format that Pebble
//! writes (lakeFS stores its ranges and metaranges in it): the footer, the
//! index block, data blocks (uncompressed or Snappy), prefix-compressed
//! entries. Read-only and bounds-checked.

pub const ROCKSDB_MAGIC: u64 = 0x88e2_41b7_85f4_cff7;
pub const LEVELDB_MAGIC: u64 = 0xdb47_7524_8b80_fb57;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("sstable: {0}")]
pub struct SstError(pub String);

fn varint(b: &[u8], pos: &mut usize) -> Result<u64, SstError> {
    let mut v: u64 = 0;
    for shift in (0..64).step_by(7) {
        let byte = *b
            .get(*pos)
            .ok_or_else(|| SstError("varint past the end".into()))?;
        *pos += 1;
        v |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(SstError("varint too long".into()))
}

/// Go's `binary.Varint`: zigzag-encoded.
pub fn svarint(b: &[u8], pos: &mut usize) -> Result<i64, SstError> {
    let u = varint(b, pos)?;
    Ok(((u >> 1) as i64) ^ -((u & 1) as i64))
}

#[derive(Debug, Clone, Copy)]
struct Handle {
    offset: u64,
    size: u64,
}

fn handle(b: &[u8], pos: &mut usize) -> Result<Handle, SstError> {
    Ok(Handle {
        offset: varint(b, pos)?,
        size: varint(b, pos)?,
    })
}

/// Which table format the file is in.
pub fn magic(file: &[u8]) -> Option<u64> {
    let m = u64::from_le_bytes(file.get(file.len().checked_sub(8)?..)?.try_into().ok()?);
    (m == ROCKSDB_MAGIC || m == LEVELDB_MAGIC).then_some(m)
}

fn block(file: &[u8], h: Handle) -> Result<Vec<u8>, SstError> {
    let start = h.offset as usize;
    let end = start + h.size as usize;
    let data = file
        .get(start..end)
        .ok_or_else(|| SstError(format!("block {start}+{} past the file", h.size)))?;
    let kind = *file
        .get(end)
        .ok_or_else(|| SstError("block trailer missing".into()))?;
    match kind {
        0 => Ok(data.to_vec()),
        1 => snap::raw::Decoder::new()
            .decompress_vec(data)
            .map_err(|e| SstError(format!("snappy: {e}"))),
        k => Err(SstError(format!(
            "block compression {k} is not supported (none and snappy are)"
        ))),
    }
}

/// Entries of one block: (key, value).
fn entries(b: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, SstError> {
    if b.len() < 4 {
        return Err(SstError("block too short".into()));
    }
    let restarts = u32::from_le_bytes(b[b.len() - 4..].try_into().unwrap()) as usize;
    let end = b
        .len()
        .checked_sub(4 + 4 * restarts)
        .ok_or_else(|| SstError("bad restart count".into()))?;
    let mut out = vec![];
    let mut pos = 0;
    let mut key: Vec<u8> = vec![];
    while pos < end {
        let shared = varint(b, &mut pos)? as usize;
        let unshared = varint(b, &mut pos)? as usize;
        let vlen = varint(b, &mut pos)? as usize;
        if shared > key.len() {
            return Err(SstError(
                "shared prefix longer than the previous key".into(),
            ));
        }
        key.truncate(shared);
        key.extend_from_slice(
            b.get(pos..pos + unshared)
                .ok_or_else(|| SstError("key past the block".into()))?,
        );
        pos += unshared;
        let v = b
            .get(pos..pos + vlen)
            .ok_or_else(|| SstError("value past the block".into()))?
            .to_vec();
        pos += vlen;
        out.push((key.clone(), v));
    }
    Ok(out)
}

/// Every entry of the table, in order. Internal keys keep their 8-byte
/// trailer stripped: these are user keys.
pub fn read_all(file: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>, SstError> {
    let m = magic(file).ok_or_else(|| SstError("no table magic".into()))?;
    // RocksDB format_version ≥ 1: checksum(1) handles(40) version(4) magic(8); LevelDB: handles(40) magic(8)
    let footer_len = if m == ROCKSDB_MAGIC { 53 } else { 48 };
    let footer = file
        .get(
            file.len()
                .checked_sub(footer_len)
                .ok_or_else(|| SstError("file shorter than a footer".into()))?..,
        )
        .unwrap();
    let mut pos = if m == ROCKSDB_MAGIC { 1 } else { 0 };
    let _meta = handle(footer, &mut pos)?;
    let index = handle(footer, &mut pos)?;
    let mut out = vec![];
    for (_, v) in entries(&block(file, index)?)? {
        let mut p = 0;
        let h = handle(&v, &mut p)?;
        for (k, val) in entries(&block(file, h)?)? {
            let user = if k.len() >= 8 {
                k[..k.len() - 8].to_vec()
            } else {
                k
            };
            out.push((user, val));
        }
    }
    Ok(out)
}

/// A protobuf field: number, and its value (varint, or bytes).
#[derive(Debug, Clone, PartialEq)]
pub enum Pb {
    Varint(u64),
    Bytes(Vec<u8>),
    Fixed(u64),
}

/// Fields of one protobuf message (no schema needed).
pub fn protobuf(b: &[u8]) -> Result<Vec<(u32, Pb)>, SstError> {
    let mut out = vec![];
    let mut pos = 0;
    while pos < b.len() {
        let tag = varint(b, &mut pos)?;
        let (num, wire) = ((tag >> 3) as u32, tag & 7);
        let v = match wire {
            0 => Pb::Varint(varint(b, &mut pos)?),
            1 => {
                let x = u64::from_le_bytes(
                    b.get(pos..pos + 8)
                        .ok_or_else(|| SstError("fixed64 past the end".into()))?
                        .try_into()
                        .unwrap(),
                );
                pos += 8;
                Pb::Fixed(x)
            }
            2 => {
                let len = varint(b, &mut pos)? as usize;
                let x = b
                    .get(pos..pos + len)
                    .ok_or_else(|| SstError("bytes past the end".into()))?
                    .to_vec();
                pos += len;
                Pb::Bytes(x)
            }
            5 => {
                let x = u32::from_le_bytes(
                    b.get(pos..pos + 4)
                        .ok_or_else(|| SstError("fixed32 past the end".into()))?
                        .try_into()
                        .unwrap(),
                );
                pos += 4;
                Pb::Fixed(x as u64)
            }
            w => return Err(SstError(format!("protobuf wire type {w}"))),
        };
        out.push((num, v));
    }
    Ok(out)
}

pub fn pb_bytes(fields: &[(u32, Pb)], n: u32) -> Option<&[u8]> {
    fields.iter().find_map(|(k, v)| match v {
        Pb::Bytes(b) if *k == n => Some(b.as_slice()),
        _ => None,
    })
}

pub fn pb_int(fields: &[(u32, Pb)], n: u32) -> Option<u64> {
    fields.iter().find_map(|(k, v)| match v {
        Pb::Varint(x) | Pb::Fixed(x) if *k == n => Some(*x),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zigzag() {
        let mut p = 0;
        assert_eq!(svarint(&[0x80, 0x01], &mut p).unwrap(), 64);
        let mut p = 0;
        assert_eq!(svarint(&[0x4e], &mut p).unwrap(), 39);
        let mut p = 0;
        assert_eq!(svarint(&[0x01], &mut p).unwrap(), -1);
    }

    #[test]
    fn protobuf_fields() {
        let f = protobuf(&[0x0a, 0x02, b'h', b'i', 0x18, 0x06]).unwrap();
        assert_eq!(pb_bytes(&f, 1), Some(&b"hi"[..]));
        assert_eq!(pb_int(&f, 3), Some(6));
    }
}
