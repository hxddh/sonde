//! An import read as a stream and written as it is read: a CloudTrail file
//! or a HAR of several GiB is never held, as text or as JSON.
//!
//! The source is decompressed and parsed as it comes (an event or an entry
//! at a time), and each request is written at once to a file beside the
//! run to be ([`Spool`]), through [`RunWriter`] as every record is. What is
//! kept in memory is a request's time and place in that file (16 bytes
//! each, 8 more for where its line starts): a run is in time order, numbered
//! in it and named for the host most of it went to, which only the end of
//! the source says. [`Sealed::save`] then writes the run from the file, in
//! order. The run is, byte for byte, the one [`super::Parts`] makes with
//! everything in memory.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use reqlog::{Record, Sink};
use serde::Deserialize;
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

use super::{BROKEN, EMPTY, Source, access_log, detect, har_entries, spread_seconds, trail_events};
use crate::run::{RunHeader, RunWriter};

/// How much of a source is read at a time.
const CHUNK: usize = 1 << 16;

/// What says which source a text is: its first characters ([`detect`]).
const HEAD: usize = 4096;

/// A source's text from its start, as often as it is asked for: a file
/// (gzip or plain), or text in memory.
#[derive(Clone, Copy)]
enum Input<'a> {
    File(&'a Path),
    Text(&'a [u8]),
}

impl Input<'_> {
    /// Its text as a stream: gunzipped when it is gzip (as S3 delivers
    /// CloudTrail), what is not UTF-8 replaced as
    /// [`String::from_utf8_lossy`] replaces it. With whether it is gzip.
    fn open(&self) -> std::io::Result<(Box<dyn BufRead + '_>, bool)> {
        let mut raw: Box<dyn BufRead + '_> = match self {
            Input::File(p) => Box::new(BufReader::with_capacity(CHUNK, File::open(p)?)),
            Input::Text(t) => Box::new(*t),
        };
        let gzip = raw.fill_buf()?.starts_with(&[0x1f, 0x8b]);
        let text: Box<dyn Read + '_> = if gzip {
            Box::new(Capped {
                inner: flate2::bufread::MultiGzDecoder::new(raw),
                left: crate::run::MAX_TEXT,
            })
        } else {
            raw
        };
        Ok((
            Box::new(BufReader::with_capacity(CHUNK, Lossy::new(text))),
            gzip,
        ))
    }
}

/// A gzip file's text, to a limit: a few bytes of gzip can say they are any
/// size, and each request of it is written to disk.
struct Capped<R> {
    inner: R,
    left: u64,
}

impl<R: Read> Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n as u64 > self.left {
            return Err(std::io::Error::other(format!(
                "it holds more than {} MiB",
                crate::run::MAX_TEXT >> 20
            )));
        }
        self.left -= n as u64;
        Ok(n)
    }
}

/// Bytes as UTF-8 text, as they are read: each run of bytes that is no
/// character becomes U+FFFD, as [`String::from_utf8_lossy`] of the whole
/// would make it.
struct Lossy<R> {
    inner: R,
    /// Read and not yet text: a character's first bytes, cut by the read.
    raw: Vec<u8>,
    text: Vec<u8>,
    at: usize,
    done: bool,
}

impl<R: Read> Lossy<R> {
    fn new(inner: R) -> Self {
        Lossy {
            inner,
            raw: Vec::new(),
            text: Vec::new(),
            at: 0,
            done: false,
        }
    }

    fn refill(&mut self) -> std::io::Result<()> {
        const REPLACEMENT: &[u8] = "\u{FFFD}".as_bytes();
        self.text.clear();
        self.at = 0;
        let carried = self.raw.len();
        self.raw.resize(carried + CHUNK, 0);
        let n = loop {
            match self.inner.read(&mut self.raw[carried..]) {
                Ok(n) => break n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    self.raw.truncate(carried);
                    return Err(e);
                }
            }
        };
        self.raw.truncate(carried + n);
        self.done = n == 0;
        let mut rest = &self.raw[..];
        while !rest.is_empty() {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    self.text.extend_from_slice(s.as_bytes());
                    rest = &[];
                }
                Err(e) => {
                    let (valid, after) = rest.split_at(e.valid_up_to());
                    self.text.extend_from_slice(valid);
                    match e.error_len() {
                        Some(bad) => {
                            self.text.extend_from_slice(REPLACEMENT);
                            rest = &after[bad..];
                        }
                        // a character cut by the read: the next has its rest
                        None if !self.done => {
                            rest = after;
                            break;
                        }
                        None => {
                            self.text.extend_from_slice(REPLACEMENT);
                            rest = &[];
                        }
                    }
                }
            }
        }
        let kept = self.raw.len() - rest.len();
        self.raw.drain(..kept);
        Ok(())
    }
}

impl<R: Read> Read for Lossy<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while self.at == self.text.len() {
            if self.done {
                return Ok(0);
            }
            self.refill()?;
        }
        let n = buf.len().min(self.text.len() - self.at);
        buf[..n].copy_from_slice(&self.text[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// White space as [`str::trim`] knows it, of one byte.
fn is_space(b: u8) -> bool {
    b.is_ascii_whitespace() || b == 0x0b
}

/// Passes over the white space a text begins with. Returns whether
/// anything follows.
fn skip_space(r: &mut dyn BufRead) -> std::io::Result<bool> {
    loop {
        let buf = r.fill_buf()?;
        if buf.is_empty() {
            return Ok(false);
        }
        let n = buf.iter().take_while(|b| is_space(**b)).count();
        let more = n < buf.len();
        r.consume(n);
        if more {
            return Ok(true);
        }
    }
}

/// A text's first characters, past the white space before them.
fn head(r: &mut dyn BufRead) -> std::io::Result<String> {
    skip_space(r)?;
    let mut bytes = Vec::new();
    r.take(HEAD as u64 * 4).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).chars().take(HEAD).collect())
}

/// A text's first line that is not empty, alone: what a file of one event
/// a line begins with.
struct FirstLine<'a> {
    inner: &'a mut dyn BufRead,
    done: bool,
}

impl Read for FirstLine<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        let from = self.inner.fill_buf()?;
        let n = from.len().min(buf.len());
        let (n, ended) = match from[..n].iter().position(|b| *b == b'\n') {
            Some(at) => (at, true),
            None => (n, n == 0),
        };
        buf[..n].copy_from_slice(&from[..n]);
        self.inner.consume(n);
        self.done = ended;
        Ok(n)
    }
}

/// Each line of a text with its line end, one at a time. A line past
/// [`crate::run::MAX_LINE`] is no request's: `each` is given none of it,
/// and `long` counts it.
fn each_line(
    r: &mut dyn BufRead,
    long: &mut usize,
    mut each: impl FnMut(&str),
) -> std::io::Result<()> {
    let mut line: Vec<u8> = Vec::new();
    loop {
        line.clear();
        let mut over = false;
        let mut read = false;
        loop {
            let buf = r.fill_buf()?;
            if buf.is_empty() {
                break;
            }
            read = true;
            let (n, ended) = match buf.iter().position(|b| *b == b'\n') {
                Some(at) => (at + 1, true),
                None => (buf.len(), false),
            };
            if line.len() + n > crate::run::MAX_LINE {
                over = true;
                line.clear();
            } else if !over {
                line.extend_from_slice(&buf[..n]);
            }
            r.consume(n);
            if ended {
                break;
            }
        }
        if !read {
            return Ok(());
        }
        if over {
            *long += 1;
        } else {
            // (text already: `Lossy` made it so)
            each(&String::from_utf8_lossy(&line));
        }
    }
}

/// What a JSON value is when it is not what was looked for: passed over,
/// whatever it is, and `$none` returned.
macro_rules! pass_over {
    ($none:expr) => {
        fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
            Ok($none)
        }
        fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
            Ok($none)
        }
        fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
            Ok($none)
        }
        fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
            Ok($none)
        }
        fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
            Ok($none)
        }
        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok($none)
        }
    };
}

/// A JSON array's elements, each handed over as it is read and let go:
/// the array is never held. Whether the value was an array.
struct Elements<'f, F>(&'f mut F);

impl<'de, F: FnMut(Value)> DeserializeSeed<'de> for Elements<'_, F> {
    type Value = bool;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<bool, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de, F: FnMut(Value)> Visitor<'de> for Elements<'_, F> {
    type Value = bool;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<bool, A::Error> {
        while let Some(v) = seq.next_element::<Value>()? {
            (self.0)(v);
        }
        Ok(true)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<bool, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(false)
    }
    pass_over!(false);
}

/// What `inner` makes of the value at `key` of a JSON object, the rest of
/// the object passed over; none when there is no such key, or no object.
struct Within<S> {
    key: &'static str,
    inner: S,
}

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Within<S> {
    type Value = Option<S::Value>;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de, S: DeserializeSeed<'de>> Visitor<'de> for Within<S> {
    type Value = Option<S::Value>;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut inner = Some(self.inner);
        let mut found = None;
        while let Some(key) = map.next_key::<std::borrow::Cow<str>>()? {
            match inner.take() {
                Some(seed) if key == self.key => found = Some(map.next_value_seed(seed)?),
                kept => {
                    inner = kept;
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(found)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(None)
    }
    pass_over!(None);
}

/// A CloudTrail document: the events of its `Records` handed over as they
/// are read, or, when it has no such array, the document itself (one event,
/// as a file of one event a line begins).
struct TrailDocument<'f, F>(&'f mut F);

impl<'de, F: FnMut(Value)> DeserializeSeed<'de> for TrailDocument<'_, F> {
    type Value = Option<Value>;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de, F: FnMut(Value)> Visitor<'de> for TrailDocument<'_, F> {
    type Value = Option<Value>;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a CloudTrail document")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut event = serde_json::Map::new();
        let mut records = false;
        while let Some(key) = map.next_key::<String>()? {
            if key == "Records" {
                records |= map.next_value_seed(Elements(&mut *self.0))?;
            } else if records {
                // beside its records, nothing is read
                map.next_value::<IgnoredAny>()?;
            } else {
                event.insert(key, map.next_value()?);
            }
        }
        Ok((!records).then_some(Value::Object(event)))
    }
}

/// A request's id as the index holds it: 8 bytes, never 0.
fn id_hash(id: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut h);
    h.finish() | 1
}

/// An import under way: the requests of each source read so far, written to
/// a file of its own beside the run to be, and where each is in it.
pub struct Spool {
    source: Option<Source>,
    /// Where the requests are until the run is written: a run file's lines,
    /// in the order they were read. Removed when the import ends.
    spill: PathBuf,
    writer: Option<RunWriter>,
    /// Lines written to it, those of a file given up on too.
    written: u32,
    /// Each request's time and its line.
    index: Vec<(DateTime<Utc>, u32)>,
    /// An access log's requests' ids ([`id_hash`]; 0 without one), as the
    /// index is: a copy's source is found by it.
    ids: Vec<u64>,
    malformed: usize,
    not_requests: usize,
    other_hosts: usize,
    other_services: usize,
    hosts: BTreeMap<String, usize>,
    sources: HashMap<String, String>,
    /// Why the requests could not be kept, the first time.
    failed: Option<std::io::Error>,
}

/// What of an import a file given up on must not leave behind.
struct Mark {
    requests: usize,
    counts: [usize; 4],
    hosts: BTreeMap<String, usize>,
}

impl Drop for Spool {
    fn drop(&mut self) {
        self.writer = None;
        let _ = std::fs::remove_file(&self.spill);
    }
}

impl Spool {
    /// An import whose run will be written in `folder`: its requests are
    /// kept there meanwhile, in a file of its own (0600, as a run is).
    pub fn create(folder: &Path) -> std::io::Result<Spool> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let folder = if folder.as_os_str().is_empty() {
            Path::new(".")
        } else {
            folder
        };
        let spill = folder.join(format!(
            ".import.{}.{}.part",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let writer = RunWriter::create(&spill, &RunHeader::new("", "", "", None))?;
        Ok(Spool {
            source: None,
            spill,
            writer: Some(writer),
            written: 0,
            index: Vec::new(),
            ids: Vec::new(),
            malformed: 0,
            not_requests: 0,
            other_hosts: 0,
            other_services: 0,
            hosts: BTreeMap::new(),
            sources: HashMap::new(),
            failed: None,
        })
    }

    /// Requests read so far.
    pub fn requests(&self) -> usize {
        self.index.len()
    }

    /// Reads one file, gzip or plain. An error leaves what was read before
    /// as it was: the file is no source sonde reads, another than the
    /// first's, or broken.
    pub fn add_file(&mut self, path: &Path) -> Result<(), String> {
        self.add(Input::File(path))
    }

    /// Reads one file's bytes, as [`Spool::add_file`] reads a file.
    pub fn add_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.add(Input::Text(bytes))
    }

    /// Reads every file under `dir` as one import, as [`super::import_dir`]
    /// does. Returns the files read, and those left out with why.
    pub fn add_dir(&mut self, dir: &Path) -> Result<(usize, Vec<(String, String)>), String> {
        let files = super::files_under(dir)?;
        let (mut read, mut left_out) = (0, Vec::new());
        for path in &files {
            match self.add_file(path) {
                Ok(()) => read += 1,
                Err(why) => left_out.push((
                    path.strip_prefix(dir)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .into_owned(),
                    why,
                )),
            }
        }
        if read == 0 {
            return Err(super::none_among(files.len()));
        }
        Ok((read, left_out))
    }

    fn mark(&self) -> Mark {
        Mark {
            requests: self.index.len(),
            counts: [
                self.malformed,
                self.not_requests,
                self.other_hosts,
                self.other_services,
            ],
            hosts: self.hosts.clone(),
        }
    }

    /// Lets go of what was read since `mark`: its lines stay in the file,
    /// named by nothing.
    fn back_to(&mut self, mark: Mark) {
        self.index.truncate(mark.requests);
        self.ids.truncate(mark.requests.min(self.ids.len()));
        [
            self.malformed,
            self.not_requests,
            self.other_hosts,
            self.other_services,
        ] = mark.counts;
        self.hosts = mark.hosts;
    }

    fn push(&mut self, source: Source, r: Record) {
        let Some(writer) = self.writer.as_ref().filter(|_| self.failed.is_none()) else {
            return;
        };
        if self.written == u32::MAX {
            self.failed = Some(std::io::Error::other("more requests than an import holds"));
            return;
        }
        writer.write(&r);
        self.index.push((r.time, self.written));
        self.written += 1;
        if source == Source::AccessLog {
            self.ids
                .push(r.rare().request_id.as_deref().map_or(0, id_hash));
        }
    }

    fn add(&mut self, input: Input) -> Result<(), String> {
        // (a file that cannot be read says so as the old reading did)
        let said = |gzip: bool, e: &dyn std::fmt::Display| {
            if gzip {
                format!("gzip: {e}")
            } else {
                e.to_string()
            }
        };
        let (mut reader, gzip) = input.open().map_err(|e| e.to_string())?;
        let head = head(&mut reader).map_err(|e| said(gzip, &e))?;
        drop(reader);
        if head.is_empty() {
            return Err(format!("{EMPTY}: the file is empty"));
        }
        let source = detect(&head).ok_or("not an S3 server access log, CloudTrail file or HAR")?;
        match self.source {
            Some(first) if first != source => {
                return Err(format!("{} among {} files", source.a(), first.name()));
            }
            _ => {}
        }
        let mark = self.mark();
        let read = match source {
            Source::AccessLog => self.access_log(input),
            Source::CloudTrail => self.cloudtrail(input, &head),
            Source::Har => self.har(input),
        };
        match read {
            Ok(()) => {
                self.source = Some(source);
                Ok(())
            }
            Err(e) => {
                self.back_to(mark);
                Err(match e {
                    Refused::Read(e) => said(gzip, &e),
                    Refused::Said(e) => e,
                })
            }
        }
    }

    fn access_log(&mut self, input: Input) -> Result<(), Refused> {
        let (mut reader, _) = input.open()?;
        let mut long = 0;
        each_line(&mut reader, &mut long, |line| {
            let (mut malformed, mut not_requests) = (0, 0);
            let records = access_log(
                line,
                &mut malformed,
                &mut not_requests,
                &mut self.sources,
                &mut self.hosts,
            );
            self.malformed += malformed;
            self.not_requests += not_requests;
            for r in records {
                self.push(Source::AccessLog, r);
            }
        })?;
        self.malformed += long;
        Ok(())
    }

    /// One CloudTrail event, or what is none: counted.
    fn event(&mut self, event: Value) {
        let records = trail_events(
            vec![event],
            &mut self.malformed,
            &mut self.other_services,
            &mut self.hosts,
        );
        for r in records {
            self.push(Source::CloudTrail, r);
        }
    }

    /// CloudTrail, as [`super::cloudtrail`] reads it: one document of
    /// `Records`, or one event a line.
    fn cloudtrail(&mut self, input: Input, head: &str) -> Result<(), Refused> {
        let mark = self.mark();
        let (reader, _) = input.open()?;
        let mut de = serde_json::Deserializer::from_reader(reader);
        let document = TrailDocument(&mut |event| self.event(event))
            .deserialize(&mut de)
            .and_then(|one| de.end().map(|()| one));
        drop(de);
        let e = match document {
            Ok(one) => {
                one.into_iter().for_each(|event| self.event(event));
                return Ok(());
            }
            Err(e) if e.is_io() => return Err(Refused::Read(e.into())),
            Err(e) => e,
        };
        // not one document: what it gave before it broke is let go
        self.back_to(mark);
        if head.contains("\"Records\"") {
            // one document, not one event a line: cut short or damaged
            let (mut reader, _) = input.open()?;
            let alone = skip_space(&mut reader)? && {
                let mut line = serde_json::Deserializer::from_reader(FirstLine {
                    inner: &mut reader,
                    done: false,
                });
                match IgnoredAny::deserialize(&mut line).and_then(|_| line.end()) {
                    Ok(()) => true,
                    Err(e) if e.is_io() => return Err(Refused::Read(e.into())),
                    Err(_) => false,
                }
            };
            if !alone {
                return Err(Refused::Said(format!(
                    "{BROKEN} at line {}, column {}: a CloudTrail file's JSON does not read to its end ({e})",
                    e.line(),
                    e.column()
                )));
            }
        }
        // JSON lines (as Athena or a log pipeline writes them)
        let (mut reader, _) = input.open()?;
        let mut long = 0;
        each_line(&mut reader, &mut long, |line| {
            for l in line.lines().filter(|l| !l.trim().is_empty()) {
                match serde_json::from_str::<Value>(l) {
                    Ok(event) => self.event(event),
                    Err(_) => self.malformed += 1,
                }
            }
        })?;
        self.malformed += long;
        Ok(())
    }

    /// A HAR, as [`super::har`] reads it: the entries of its `log`.
    fn har(&mut self, input: Input) -> Result<(), Refused> {
        let (reader, _) = input.open()?;
        let mut de = serde_json::Deserializer::from_reader(reader);
        let mut entry = |entry: Value| {
            let records = har_entries(
                std::slice::from_ref(&entry),
                &mut self.malformed,
                &mut self.other_hosts,
                &mut self.hosts,
            );
            for r in records {
                self.push(Source::Har, r);
            }
        };
        let entries = Within {
            key: "log",
            inner: Within {
                key: "entries",
                inner: Elements(&mut entry),
            },
        }
        .deserialize(&mut de)
        .and_then(|found| de.end().map(|()| found));
        match entries {
            Ok(Some(Some(true))) => Ok(()),
            Ok(_) => Err(Refused::Said("a HAR without log.entries".into())),
            Err(e) if e.is_io() => Err(Refused::Read(e.into())),
            Err(e) => Err(Refused::Said(e.to_string())),
        }
    }

    /// The import of everything added, ready to be written: an error when
    /// there is nothing in it to write.
    pub fn seal(self, name: &str) -> Result<Sealed, String> {
        let source = self
            .source
            .ok_or("not an S3 server access log, CloudTrail file or HAR")?;
        if self.index.is_empty() {
            return Err(format!("{EMPTY}: no S3 requests in this {}", source.name()));
        }
        Ok(Sealed {
            spool: self,
            source,
            name: name.to_string(),
        })
    }
}

/// Why a file was not read: it could not be (the disk's error, gzip's), or
/// it is not what it says.
enum Refused {
    Read(std::io::Error),
    Said(String),
}

impl From<std::io::Error> for Refused {
    fn from(e: std::io::Error) -> Self {
        Refused::Read(e)
    }
}

/// An import read to its end, with requests in it ([`Spool::seal`]).
pub struct Sealed {
    spool: Spool,
    source: Source,
    name: String,
}

/// An import written as a run: what it came from, and what of the source
/// was left out of it (as [`super::Imported`] says).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Saved {
    pub source: Source,
    pub requests: usize,
    pub malformed: usize,
    pub not_requests: usize,
    pub other_hosts: usize,
    pub other_services: usize,
}

/// The lines of the spill, read by where they start: one after another
/// when the source was in time order, as most are.
struct Spilled {
    file: BufReader<File>,
    at: u64,
    /// Where each line starts, the header's first, and where the last ends.
    starts: Vec<u64>,
    line: Vec<u8>,
}

impl Spilled {
    fn open(path: &Path) -> std::io::Result<Spilled> {
        let mut file = BufReader::with_capacity(CHUNK, File::open(path)?);
        let mut starts = vec![0u64];
        let mut at = 0u64;
        loop {
            let buf = file.fill_buf()?;
            if buf.is_empty() {
                break;
            }
            starts.extend(
                buf.iter()
                    .enumerate()
                    .filter(|(_, b)| **b == b'\n')
                    .map(|(i, _)| at + i as u64 + 1),
            );
            let n = buf.len();
            at += n as u64;
            file.consume(n);
        }
        Ok(Spilled {
            file,
            at,
            starts,
            line: Vec::new(),
        })
    }

    /// The record on the spill's line `n` (its requests' lines from 0).
    fn record(&mut self, n: u32) -> std::io::Result<Record> {
        let bad = |why: String| std::io::Error::new(std::io::ErrorKind::InvalidData, why);
        let (Some(&from), Some(&to)) = (
            self.starts.get(n as usize + 1),
            self.starts.get(n as usize + 2),
        ) else {
            return Err(bad(format!("request {n} of the import was not kept")));
        };
        if from != self.at {
            self.file.seek(SeekFrom::Start(from))?;
        }
        self.line.resize((to - from) as usize, 0);
        self.file.read_exact(&mut self.line)?;
        self.at = to;
        serde_json::from_slice(&self.line).map_err(|e| bad(e.to_string()))
    }
}

impl Sealed {
    /// Requests it holds.
    pub fn requests(&self) -> usize {
        self.spool.index.len()
    }

    /// Writes the run at `path`, compressed as a finished recording is:
    /// its records in time order and numbered in it, as [`super::save`] of
    /// [`super::Parts::finish`] writes them.
    pub fn save(mut self, path: &Path) -> std::io::Result<Saved> {
        let spool = &mut self.spool;
        if let Some(e) = spool.failed.take() {
            return Err(e);
        }
        if let Some(w) = spool.writer.take() {
            w.flush()?;
            if let Some(why) = w.not_written() {
                return Err(std::io::Error::other(why));
            }
        }
        let mut spilled = Spilled::open(&spool.spill)?;
        // a copy's source, said by the store's own line of it: the first
        // request of that id has it
        let mut copied: HashMap<u32, String> = HashMap::new();
        if !spool.sources.is_empty() {
            let wanted: HashSet<u64> = spool.sources.keys().map(|id| id_hash(id)).collect();
            for (place, id) in spool.ids.iter().enumerate() {
                if !wanted.contains(id) {
                    continue;
                }
                let line = spool.index[place].1;
                let r = spilled.record(line)?;
                if let Some(source) = r
                    .rare()
                    .request_id
                    .as_ref()
                    .and_then(|id| spool.sources.remove(id))
                {
                    copied.insert(line, source);
                }
            }
        }
        spool.ids = Vec::new();
        // (stable: requests of one moment stay in the log's order)
        let mut index = std::mem::take(&mut spool.index);
        index.sort_by_key(|(time, _)| *time);
        let whole_seconds = spread_seconds(&mut index, |entry| &mut entry.0);
        // where the requests went, when the source says: the store's host
        // (the file's name otherwise, which is all there is)
        let endpoint = spool
            .hosts
            .iter()
            .max_by_key(|(_, n)| **n)
            .map(|(h, _)| h.clone())
            .unwrap_or_else(|| self.name.clone());
        let mut header = RunHeader::new(
            &endpoint,
            "",
            "",
            Some(format!("imported from {}", self.source.name())),
        );
        header.created = index[0].0;
        header.whole_seconds = whole_seconds;
        {
            let w = RunWriter::create(path, &header)?;
            for (i, (time, line)) in index.iter().enumerate() {
                let mut r = spilled.record(*line)?;
                if let Some(source) = copied.remove(line) {
                    r.lists_mut()
                        .request_headers
                        .push(reqlog::Header::new("x-amz-copy-source", source));
                }
                r.time = *time;
                r.id = i as u64 + 1;
                r.rare_mut().endpoint = endpoint.as_str().into();
                w.write(&r);
            }
            w.flush()?;
            if let Some(why) = w.not_written() {
                return Err(std::io::Error::other(why));
            }
        }
        crate::run::compress(path)?;
        Ok(Saved {
            source: self.source,
            requests: index.len(),
            malformed: spool.malformed,
            not_requests: spool.not_requests,
            other_hosts: spool.other_hosts,
            other_services: spool.other_services,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_that_are_no_text_are_replaced_as_the_whole_would_be() {
        // a character cut at every place a read can cut it, and bytes that are none
        let mut bytes = "é—😀 a".as_bytes().to_vec();
        bytes.extend_from_slice(&[0xff, b'b', 0xe2, 0x82, b'c', 0xf0, 0x9f]);
        let whole = String::from_utf8_lossy(&bytes).into_owned();
        struct Drip<'a>(&'a [u8], usize);
        impl Read for Drip<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n = self.1.min(self.0.len()).min(buf.len());
                buf[..n].copy_from_slice(&self.0[..n]);
                self.0 = &self.0[n..];
                Ok(n)
            }
        }
        for step in 1..=bytes.len() {
            let mut text = String::new();
            Lossy::new(Drip(&bytes, step))
                .read_to_string(&mut text)
                .expect("text");
            assert_eq!(text, whole, "{step} bytes a read");
        }
    }

    #[test]
    fn lines_are_read_one_at_a_time_with_their_ends() {
        let mut lines = Vec::new();
        let mut long = 0;
        let text = "a\r\n\nb\nlast";
        each_line(&mut text.as_bytes(), &mut long, |l| {
            lines.push(l.to_string())
        })
        .unwrap();
        assert_eq!(lines, ["a\r\n", "\n", "b\n", "last"]);
        assert_eq!(long, 0);
        let mut reader = "  \n\n {\"a\":1}\n{\"b\"".as_bytes();
        assert!(skip_space(&mut reader).unwrap());
        let mut first = String::new();
        FirstLine {
            inner: &mut reader,
            done: false,
        }
        .read_to_string(&mut first)
        .unwrap();
        assert_eq!(first, "{\"a\":1}");
    }
}
