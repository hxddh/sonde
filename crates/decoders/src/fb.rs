//! A minimal, bounds-checked FlatBuffers reader: tables, scalars,
//! vectors, strings. Enough to decode SlateDB manifests from their schema
//! without generated code; every access is checked so a corrupt manifest
//! is an error, never a panic.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("flatbuffer: {0}")]
pub struct FbError(pub String);

type R<T> = Result<T, FbError>;

#[derive(Clone, Copy)]
pub struct Buf<'a>(pub &'a [u8]);

impl<'a> Buf<'a> {
    fn read<const N: usize>(&self, at: usize) -> R<[u8; N]> {
        self.0
            .get(at..at + N)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| FbError(format!("read of {N} bytes at {at} past {}", self.0.len())))
    }
    pub fn u8(&self, at: usize) -> R<u8> {
        Ok(self.read::<1>(at)?[0])
    }
    pub fn u16(&self, at: usize) -> R<u16> {
        Ok(u16::from_le_bytes(self.read(at)?))
    }
    pub fn u32(&self, at: usize) -> R<u32> {
        Ok(u32::from_le_bytes(self.read(at)?))
    }
    pub fn i32(&self, at: usize) -> R<i32> {
        Ok(i32::from_le_bytes(self.read(at)?))
    }
    pub fn u64(&self, at: usize) -> R<u64> {
        Ok(u64::from_le_bytes(self.read(at)?))
    }
    /// The root table.
    pub fn root(self) -> R<Table<'a>> {
        let off = self.u32(0)? as usize;
        Table::at(self, off)
    }
}

#[derive(Clone, Copy)]
pub struct Table<'a> {
    buf: Buf<'a>,
    pos: usize,
    vtable: usize,
    vlen: u16,
}

impl<'a> Table<'a> {
    pub fn at(buf: Buf<'a>, pos: usize) -> R<Table<'a>> {
        let soff = buf.i32(pos)? as i64;
        let vtable = pos as i64 - soff;
        if vtable < 0 || vtable as usize >= buf.0.len() {
            return Err(FbError(format!("vtable out of range at {pos}")));
        }
        let vtable = vtable as usize;
        let vlen = buf.u16(vtable)?;
        if vlen < 4 {
            return Err(FbError("vtable too short".into()));
        }
        Ok(Table {
            buf,
            pos,
            vtable,
            vlen,
        })
    }

    /// The field's absolute position, if present.
    fn field(&self, id: usize) -> R<Option<usize>> {
        let entry = 4 + 2 * id;
        if entry + 2 > self.vlen as usize {
            return Ok(None);
        }
        let off = self.buf.u16(self.vtable + entry)? as usize;
        Ok(if off == 0 { None } else { Some(self.pos + off) })
    }

    pub fn u64(&self, id: usize, default: u64) -> R<u64> {
        self.field(id)?.map_or(Ok(default), |p| self.buf.u64(p))
    }
    pub fn i64(&self, id: usize, default: i64) -> R<i64> {
        Ok(self.u64(id, default as u64)? as i64)
    }
    pub fn u32(&self, id: usize, default: u32) -> R<u32> {
        self.field(id)?.map_or(Ok(default), |p| self.buf.u32(p))
    }
    pub fn u16(&self, id: usize) -> R<Option<u16>> {
        self.field(id)?.map(|p| self.buf.u16(p)).transpose()
    }
    pub fn u8(&self, id: usize, default: u8) -> R<u8> {
        self.field(id)?.map_or(Ok(default), |p| self.buf.u8(p))
    }
    pub fn bool(&self, id: usize) -> R<bool> {
        Ok(self.u8(id, 0)? != 0)
    }

    fn indirect(&self, id: usize) -> R<Option<usize>> {
        match self.field(id)? {
            None => Ok(None),
            Some(p) => Ok(Some(p + self.buf.u32(p)? as usize)),
        }
    }

    pub fn table(&self, id: usize) -> R<Option<Table<'a>>> {
        self.indirect(id)?
            .map(|p| Table::at(self.buf, p))
            .transpose()
    }

    pub fn bytes(&self, id: usize) -> R<Option<&'a [u8]>> {
        match self.indirect(id)? {
            None => Ok(None),
            Some(p) => {
                let len = self.buf.u32(p)? as usize;
                self.buf
                    .0
                    .get(p + 4..p + 4 + len)
                    .map(Some)
                    .ok_or_else(|| FbError("vector past the end".into()))
            }
        }
    }

    pub fn string(&self, id: usize) -> R<Option<String>> {
        Ok(self
            .bytes(id)?
            .map(|b| String::from_utf8_lossy(b).to_string()))
    }

    /// A vector of tables.
    pub fn tables(&self, id: usize) -> R<Vec<Table<'a>>> {
        let Some(p) = self.indirect(id)? else {
            return Ok(vec![]);
        };
        let len = self.buf.u32(p)? as usize;
        if len > self.buf.0.len() / 4 {
            return Err(FbError(format!("vector length {len} too large")));
        }
        (0..len)
            .map(|i| {
                let e = p + 4 + 4 * i;
                Table::at(self.buf, e + self.buf.u32(e)? as usize)
            })
            .collect()
    }
}
