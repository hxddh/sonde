//! A run as one HTML file anyone opens in a browser, from a disk or a
//! web server alike: sonde's viewer (the app's own views and Time pane, built
//! with Native Shell's web target and embedded at build time) and the
//! run, everything already computed, all in the one file. `sonde share`
//! and the app's Share both write it.

use std::io::Write;
use std::path::Path;

use base64::Engine;
use serde_json::Value;

use crate::episode::{episodes, shared_notes};
use crate::run::RunFile;
use crate::summary::summarize;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/viewer_files.rs"));
}

/// Whether this build carries the viewer (a development build may not).
pub fn has_viewer() -> bool {
    embedded::VIEWER.iter().any(|(p, _, _)| *p == "index.html")
}
/// A JSON document written as it is made: a run of a million requests
/// is never one tree in memory and then one string. Objects and arrays are
/// opened and closed; a value is written whole, its keys in order (as
/// `serde_json::Value` orders them, so a document reads the same whichever
/// way it was made). Pretty or on one line.
pub struct JsonOut<W: std::io::Write> {
    out: W,
    pretty: bool,
    /// For each open object or array: whether it has anything yet.
    open: Vec<bool>,
    /// A key was written: what follows is its value, on the same line.
    keyed: bool,
}

impl JsonOut<Vec<u8>> {
    /// How much has been written so far.
    pub fn written(&self) -> usize {
        self.out.len()
    }
}

impl<W: std::io::Write> JsonOut<W> {
    pub fn new(out: W, pretty: bool) -> Self {
        JsonOut {
            out,
            pretty,
            open: Vec::new(),
            keyed: false,
        }
    }

    fn indent(&mut self, depth: usize) -> std::io::Result<()> {
        if self.pretty {
            self.out.write_all(b"\n")?;
            for _ in 0..depth {
                self.out.write_all(b"  ")?;
            }
        }
        Ok(())
    }

    /// Before a key, or a value of an array: the comma and the line.
    fn place(&mut self) -> std::io::Result<()> {
        if self.keyed {
            self.keyed = false;
            return Ok(());
        }
        let depth = self.open.len();
        if let Some(any) = self.open.last_mut() {
            if *any {
                self.out.write_all(b",")?;
            }
            *any = true;
            self.indent(depth)?;
        }
        Ok(())
    }

    pub fn key(&mut self, name: &str) -> std::io::Result<()> {
        self.place()?;
        serde_json::to_writer(&mut self.out, name)?;
        self.out.write_all(if self.pretty { b": " } else { b":" })?;
        self.keyed = true;
        Ok(())
    }

    pub fn begin_object(&mut self) -> std::io::Result<()> {
        self.place()?;
        self.open.push(false);
        self.out.write_all(b"{")
    }

    pub fn begin_array(&mut self) -> std::io::Result<()> {
        self.place()?;
        self.open.push(false);
        self.out.write_all(b"[")
    }

    fn end(&mut self, close: &[u8]) -> std::io::Result<()> {
        if self.open.pop() == Some(true) {
            let depth = self.open.len();
            self.indent(depth)?;
        }
        self.out.write_all(close)
    }

    pub fn end_object(&mut self) -> std::io::Result<()> {
        self.end(b"}")
    }

    pub fn end_array(&mut self) -> std::io::Result<()> {
        self.end(b"]")
    }

    /// A value, whole.
    pub fn value(&mut self, v: &impl serde::Serialize) -> std::io::Result<()> {
        let v = serde_json::to_value(v)?;
        self.json(&v)
    }

    /// A value already a tree.
    pub fn json(&mut self, v: &Value) -> std::io::Result<()> {
        self.place()?;
        if !self.pretty {
            return Ok(serde_json::to_writer(&mut self.out, v)?);
        }
        // its lines, moved in to where it stands (a string's own line
        // breaks are escaped: every line break here is the printer's)
        let text = serde_json::to_string_pretty(v)?;
        let depth = self.open.len();
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                self.indent(depth)?;
            }
            self.out.write_all(line.as_bytes())?;
        }
        Ok(())
    }

    /// A key and its value.
    pub fn field(&mut self, name: &str, v: &impl serde::Serialize) -> std::io::Result<()> {
        self.key(name)?;
        self.value(v)
    }

    /// A key and an array of the values `items` gives, one at a time.
    pub fn array<T: serde::Serialize>(
        &mut self,
        name: &str,
        items: impl IntoIterator<Item = T>,
    ) -> std::io::Result<()> {
        self.key(name)?;
        self.begin_array()?;
        for item in items {
            self.value(&item)?;
        }
        self.end_array()
    }

    /// The writer back, flushed, once everything opened is closed.
    pub fn finish(mut self) -> std::io::Result<W> {
        self.out.flush()?;
        Ok(self.out)
    }
}

/// A run's records as they are in its file, each handed to `each` with
/// the id the run read gave it: its place in the file where the run was
/// numbered `afresh` (a file joined from two recordings), else its own. For
/// writing every record out in full beside an analysis made of
/// [`RunFile::open_lean`].
pub fn each_record(
    path: &Path,
    afresh: bool,
    each: &mut dyn FnMut(reqlog::Record) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let mut place = 0u64;
    let mut lines = crate::run::Lines::open(path)?;
    // the header
    lines.line()?;
    while let Some(raw) = lines.line()? {
        if !raw.starts_with(b"{") || raw.starts_with(b"{\"log\":") {
            continue;
        }
        let text = String::from_utf8_lossy(raw);
        // a torn last line: the run read skipped it too
        let Ok(mut r) = serde_json::from_str::<reqlog::Record>(text.trim_end_matches('\r')) else {
            continue;
        };
        place += 1;
        if afresh {
            r.id = place;
        }
        each(r)?;
    }
    Ok(())
}

/// A record as the viewer has it: with its short target.
fn shared_record(r: &reqlog::Record) -> Value {
    let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
    if let Value::Object(m) = &mut v {
        m.insert("target".into(), Value::String(r.summary().target));
    }
    v
}

/// A run compared with another (its baseline, the run before it): what the
/// page shows first, as the app's Compare shows it.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Compared {
    /// The run before, as `check` names it.
    pub name: String,
    pub change: crate::profile::RunChange,
    /// Both runs' signals and profiles, the run before first: the tiles'
    /// lines and the two over time.
    pub signals: [crate::signals::Signals; 2],
    pub profiles: [crate::profile::Profile; 2],
}

/// Everything the viewer shows, computed: header, summary, operations,
/// observations, records (with their short target) and output lines.
pub fn run_json(s: &RunFile, prices: &crate::profile::RequestPrices) -> Value {
    let mut doc = Vec::new();
    // (into memory: nothing to fail)
    let _ = write_run_json(s, None, prices, None, &mut doc);
    serde_json::from_slice(&doc).unwrap_or(Value::Null)
}

/// [`run_json`], written as it is made. `s` may be a run read with
/// [`RunFile::open_lean`] when `full` is its file: the records are then
/// written from the file, one at a time, and never all held.
pub fn write_run_json(
    s: &RunFile,
    full: Option<&Path>,
    prices: &crate::profile::RequestPrices,
    compared: Option<&Compared>,
    out: impl std::io::Write,
) -> std::io::Result<()> {
    let eps = episodes(&s.records);
    // a page meant for others: a variable set before the program (a key,
    // say) never reaches it
    let mut header = s.header.clone();
    header.command = header.command.as_deref().map(crate::program::scrub_command);
    // the folder names this machine's user, and so may a file's path
    header.cwd = None;
    header.ca_file = None;
    // the findings as the app shows a run's: its plan of diagnoses, ranked by the time they save, and where the
    // program's time went
    let summary = summarize(&s.records);
    let profile = crate::profile::profile(&s.records, prices);
    let timed = !s.header.whole_seconds;
    let mut observations = crate::episode::observations_of(&s.header, &s.records, &eps);
    let reckoning = crate::fixes::Reckoning::of(&s.records, &profile, timed);
    crate::fixes::rank_by_saving(&mut observations, &reckoning);
    let month = crate::month::month(
        &s.header,
        summary.seconds,
        profile.cost.amount,
        &profile.cost.currency,
    );
    let diagnoses = crate::fixes::diagnose(&mut observations, &eps, &reckoning, month.as_ref());
    drop(reckoning);
    let time = crate::summary::time_account(&s.records, &summary, &profile, &s.header);
    let mut w = JsonOut::new(out, false);
    w.begin_object()?;
    if let Some(c) = compared {
        w.field("compare", c)?;
    }
    w.field("diagnoses", &diagnoses)?;
    w.array("operations", &eps)?;
    w.field("format", &"sonde-share")?;
    w.field("header", &header)?;
    w.field("lines", &crate::logs::lines(&s.logs))?;
    w.field("observations", &observations)?;
    w.field("profile", &profile)?;
    w.key("records")?;
    w.begin_array()?;
    match full {
        Some(path) => each_record(path, s.afresh, &mut |r| w.json(&shared_record(&r)))?,
        None => {
            for r in &s.records {
                w.json(&shared_record(r))?;
            }
        }
    }
    w.end_array()?;
    w.field(
        "sharedNotes",
        &shared_notes(&eps)
            .into_iter()
            .map(|(_, n, _, _)| n)
            .collect::<Vec<_>>(),
    )?;
    w.field("signals", &crate::signals::signals(&s.records, prices))?;
    w.field("summary", &summary)?;
    w.field("time", &time)?;
    w.field("version", &2)?;
    w.end_object()?;
    w.finish()?;
    Ok(())
}

/// What a JSON document is written through inside the page's `<script>`:
/// every `<` becomes `\u003c` (the same text to JSON: JSON has `<` only in
/// strings), so no string in it opens a comment or closes the script.
struct InScript<W: Write> {
    out: W,
}

impl<W: Write> Write for InScript<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut from = 0;
        for (i, &b) in buf.iter().enumerate() {
            if b == b'<' {
                self.out.write_all(&buf[from..i])?;
                self.out.write_all(b"\\u003c")?;
                from = i + 1;
            }
        }
        self.out.write_all(&buf[from..])?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

/// Text for the page's markup: `&`, `<`, `>` and `"` escaped.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The files of the viewer the page carries, by path, as `[how, base64]`:
/// `"g"` gzipped (decompressed in the browser), `"r"` as they are. Native
/// Shell's page, manifest and prefetch script are the page's own business,
/// and its formula renderer is never used by the viewer: none is carried; its
/// page's styles are in the page ([`page_parts`]).
fn files_json() -> String {
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut out = String::from("{");
    for (path, gz, bytes) in embedded::VIEWER {
        let skip = *path == "index.html"
            || path.ends_with(".webmanifest")
            || path.ends_with("LICENSES.txt")
            || path.starts_with("nsh-prefetch.")
            || path.starts_with("nsh-page.")
            || path.starts_with("nsh-math.");
        if skip {
            continue;
        }
        if out.len() > 1 {
            out.push(',');
        }
        out.push_str(&serde_json::to_string(path).unwrap_or_default());
        out.push_str(":[\"");
        out.push_str(if *gz { "g" } else { "r" });
        out.push_str("\",\"");
        out.push_str(&b64.encode(bytes));
        out.push_str("\"]");
    }
    out.push('}');
    out
}

/// What the page runs first: each file of the viewer made a `blob:` URL (a
/// module's relative imports pointed at its neighbours' URLs, its
/// dependencies first), Native Shell's description of the app pointed at
/// them, then Native Shell's own start script. Nothing is fetched from
/// anywhere, so the page opens from a disk (`file://`) as from a server.
const BOOT: &str = r#"(async () => {
  const status = document.getElementById("status");
  const files = JSON.parse(document.getElementById("sonde-files").textContent);
  const TYPES = { js: "text/javascript", wasm: "application/wasm", css: "text/css", json: "application/json", woff2: "font/woff2", png: "image/png", svg: "image/svg+xml" };
  const typeOf = (p) => TYPES[p.slice(p.lastIndexOf(".") + 1)] ?? "application/octet-stream";
  const bytesOf = async ([how, b64]) => {
    const raw = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    if (how !== "g") return raw;
    return new Uint8Array(await new Response(new Blob([raw]).stream().pipeThrough(new DecompressionStream("gzip"))).arrayBuffer());
  };
  const urls = {};
  const text = {};
  for (const [p, f] of Object.entries(files)) {
    if (p.endsWith(".js")) text[p] = new TextDecoder().decode(await bytesOf(f));
    else urls[p] = URL.createObjectURL(new Blob([await bytesOf(f)], { type: typeOf(p) }));
  }
  const dir = (p) => p.slice(0, p.lastIndexOf("/") + 1);
  const resolve = (from, rel) => {
    const out = [];
    for (const x of (dir(from) + rel).split("/")) {
      if (x === "" || x === ".") continue;
      if (x === "..") out.pop();
      else out.push(x);
    }
    return out.join("/");
  };
  const moduleUrl = (p, seen = new Set()) => {
    if (urls[p]) return urls[p];
    if (seen.has(p)) throw new Error(`a cycle of imports at ${p}`);
    seen.add(p);
    const src = text[p].replace(/(["'`])(\.{1,2}\/[^"'`\s$]+)\1/g, (whole, q, rel) => {
      const to = resolve(p, rel);
      return to in files ? q + (to.endsWith(".js") ? moduleUrl(to, seen) : urls[to]) + q : whole;
    });
    return (urls[p] = URL.createObjectURL(new Blob([src], { type: "text/javascript" })));
  };
  for (const p of Object.keys(text)) moduleUrl(p);
  const blob = (v) => (typeof v === "string" ? (urls[v.replace(/^\.\//, "")] ?? v) : v);
  const each = (o) => { for (const k in o ?? {}) o[k] = blob(o[k]); };
  const node = document.getElementById("nsh-app");
  const app = JSON.parse(node.textContent);
  for (const k of ["wasm", "glue", "code", "host", "runtime"]) if (app[k]) app[k] = blob(app[k]);
  each(app.assets); each(app.data); each(app.chunks);
  for (const name in app.sides ?? {}) app.sides[name].url = blob(app.sides[name].url);
  node.textContent = JSON.stringify(app);
  // nothing was requested ahead: Native Shell's start script takes what it asks for
  window.fetches = new Map();
  const start = document.createElement("script");
  start.type = "module";
  start.src = urls[Object.keys(text).find((p) => /^nsh-start\.[^/]*\.js$/.test(p))];
  document.body.append(start);
})().catch((e) => {
  const s = document.getElementById("status");
  s.className = "failed";
  s.textContent = `This page could not start: ${e?.message ?? e}`;
});"#;

/// A file of the embedded viewer, unpacked.
fn viewer_file(path: &str) -> Option<Vec<u8>> {
    let (_, gz, bytes) = embedded::VIEWER.iter().find(|(p, _, _)| *p == path)?;
    if !gz {
        return Some(bytes.to_vec());
    }
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(*bytes), &mut out).ok()?;
    Some(out)
}

/// What the page takes from Native Shell's (`index`): its look while the
/// viewer loads, the stylesheet it links (`nsh-page.<digest>.css`, read with
/// `file`) set inline, so the one file needs no other; and its description
/// of the app. The app's modules import one another by their files' names
/// (`linked`), which the page's script points at its `blob:` URLs ([`BOOT`]).
fn page_parts(
    index: &str,
    file: impl Fn(&str) -> Option<Vec<u8>>,
) -> std::io::Result<(String, String)> {
    let between = |a: &str, b: &str| -> std::io::Result<(usize, usize)> {
        let i = index
            .find(a)
            .ok_or_else(|| std::io::Error::other(format!("the viewer's page has no {a}")))?;
        let j = index[i + a.len()..]
            .find(b)
            .map(|j| i + a.len() + j)
            .ok_or_else(|| std::io::Error::other(format!("the viewer's page has no {b}")))?;
        Ok((i + a.len(), j))
    };
    let (i, j) = between("<link rel=\"stylesheet\" href=\"", "\"")?;
    let css = &index[i..j];
    let css = file(css)
        .ok_or_else(|| std::io::Error::other(format!("the viewer has no {css}")))
        .map(|b| String::from_utf8_lossy(&b).replace("</", "<\\/"))?;
    let open = "<script type=\"application/json\" id=\"nsh-app\">";
    let (i, j) = between(open, "</script>")?;
    Ok((
        format!("<style>\n{}\n</style>", css.trim_end()),
        format!("{open}{}</script>", &index[i..j]),
    ))
}

/// Writes the page: one HTML file holding the viewer and the run
/// ([`write_run_json`], given to the viewer as its parameters), titled
/// by the run's program. Refused when this build has no viewer.
pub fn write_page(
    s: &RunFile,
    full: Option<&Path>,
    prices: &crate::profile::RequestPrices,
    compared: Option<&Compared>,
    out: impl Write,
) -> std::io::Result<()> {
    let index = embedded::VIEWER
        .iter()
        .find(|(p, _, _)| *p == "index.html")
        .map(|(_, _, b)| String::from_utf8_lossy(b).into_owned())
        .ok_or_else(|| {
            std::io::Error::other(
                "this build of sonde has no web viewer: build it first (`cd viewer && nsh package App.tsx --target web`), or write the run as data with `sonde show FILE -f json`",
            )
        })?;
    let (style, app) = page_parts(&index, viewer_file)?;
    let title = crate::program::key_of(&s.header).unwrap_or_else(|| "run".into());
    let mut out = std::io::BufWriter::new(out);
    write!(
        out,
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1, viewport-fit=cover\">\n<title>{} · sonde</title>\n<meta name=\"color-scheme\" content=\"light dark\">\n{style}\n</head>\n<body>\n<div id=\"app\"></div>\n<div id=\"status\" role=\"status\" aria-live=\"polite\"><div class=\"spinner\" aria-label=\"Loading\"></div></div>\n<noscript><div id=\"nojs\">This page needs JavaScript.</div></noscript>\n{app}\n<script type=\"application/json\" id=\"nsh-params\">",
        esc(&title)
    )?;
    {
        let mut inside = InScript { out: &mut out };
        write_run_json(s, full, prices, compared, &mut inside)?;
    }
    write!(
        out,
        "</script>\n<script type=\"application/json\" id=\"sonde-files\">{}</script>\n<script>{BOOT}</script>\n</body>\n</html>\n",
        files_json()
    )?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_text_in_the_document_opens_or_closes_markup_in_the_page() {
        let doc = serde_json::json!({
            "k": "<!-- <script>x</script> </SCRIPT <ScRiPt",
            "n": ["<", "<<", "a</", "<!--"],
        });
        let text = serde_json::to_vec(&doc).unwrap();
        // written in pieces cut anywhere
        for cut in 0..text.len() {
            let mut page = Vec::new();
            {
                let mut inside = InScript { out: &mut page };
                inside.write_all(&text[..cut]).unwrap();
                inside.write_all(&text[cut..]).unwrap();
            }
            assert!(!page.contains(&b'<'), "{}", String::from_utf8_lossy(&page));
            let back: Value = serde_json::from_slice(&page).unwrap();
            assert_eq!(back, doc);
        }
    }

    #[test]
    fn the_page_sets_the_viewers_linked_stylesheet_inline() {
        // the page as Native Shell 0.6 makes it: its styles a file of their own, linked
        let index = "<head>\n<link rel=\"modulepreload\" href=\"nsh.1.js\">\n<link rel=\"stylesheet\" href=\"nsh-page.2.css\">\n</head><body>\n<script type=\"application/json\" id=\"nsh-app\">{\"code\":\"app.3.js\",\"linked\":true}</script>\n</body>";
        let file = |p: &str| (p == "nsh-page.2.css").then(|| b"#status{color:red}\n".to_vec());
        let (style, app) = page_parts(index, file).unwrap();
        assert_eq!(style, "<style>\n#status{color:red}\n</style>");
        assert_eq!(
            app,
            "<script type=\"application/json\" id=\"nsh-app\">{\"code\":\"app.3.js\",\"linked\":true}</script>"
        );
        // a stylesheet that is not there, or a page that links none, is said
        assert!(page_parts(index, |_| None).is_err());
        assert!(page_parts("<style>x</style>", file).is_err());
        // what would end the style element early cannot
        let (style, _) = page_parts(index, |_| Some(b"a{}</style><b>".to_vec())).unwrap();
        assert!(!style[7..style.len() - 8].contains("</"), "{style}");
    }

    #[test]
    fn a_document_written_as_it_is_made_reads_as_one_built_whole() {
        let v = serde_json::json!({
            "b": [1, {"z": "two\nlines", "a": []}, [], {}],
            "a": {"k": null, "deep": {"x": [true]}},
            "empty": [],
        });
        for pretty in [true, false] {
            let mut w = JsonOut::new(Vec::new(), pretty);
            w.begin_object().unwrap();
            w.field("a", &v["a"]).unwrap();
            w.key("b").unwrap();
            w.begin_array().unwrap();
            w.value(&1).unwrap();
            w.begin_object().unwrap();
            w.array("a", Vec::<u8>::new()).unwrap();
            w.field("z", &"two\nlines").unwrap();
            w.end_object().unwrap();
            w.begin_array().unwrap();
            w.end_array().unwrap();
            w.begin_object().unwrap();
            w.end_object().unwrap();
            w.end_array().unwrap();
            w.array("empty", Vec::<u8>::new()).unwrap();
            w.end_object().unwrap();
            let text = String::from_utf8(w.finish().unwrap()).unwrap();
            let whole = if pretty {
                serde_json::to_string_pretty(&v).unwrap()
            } else {
                serde_json::to_string(&v).unwrap()
            };
            assert_eq!(text, whole);
        }
    }

    #[test]
    fn a_run_shared_from_a_lean_reading_and_its_file_is_the_same_document() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sdk-java.sonde");
        let prices = crate::profile::RequestPrices::default();
        let full = RunFile::open(&path).unwrap();
        let mut whole = Vec::new();
        write_run_json(&full, None, &prices, None, &mut whole).unwrap();
        let lean = RunFile::open_lean(&path).unwrap();
        let mut streamed = Vec::new();
        write_run_json(&lean, Some(&path), &prices, None, &mut streamed).unwrap();
        assert!(whole == streamed, "the two documents differ");
        let doc: Value = serde_json::from_slice(&streamed).unwrap();
        assert_eq!(doc["records"].as_array().unwrap().len(), full.records.len());
        // every header is there, though the analysis never held them
        assert!(
            doc["records"][0]["requestHeaders"]
                .as_array()
                .unwrap()
                .iter()
                .any(|h| h["name"]
                    .as_str()
                    .unwrap()
                    .eq_ignore_ascii_case("authorization"))
        );
        assert_eq!(run_json(&full, &prices), doc);
        // its findings as the app shows the run's: the plan of diagnoses, where the time went
        let a = crate::view::Analysis::of(&full, &prices);
        assert_eq!(
            doc["diagnoses"],
            serde_json::to_value(&a.diagnoses).unwrap()
        );
        assert_eq!(doc["time"], serde_json::to_value(&a.time).unwrap());
        assert_eq!(
            doc["observations"],
            serde_json::to_value(&a.observations).unwrap()
        );
    }

    #[test]
    fn a_shared_page_never_carries_a_secret_set_before_the_program() {
        let s = crate::run::RunFile {
            header: crate::run::RunHeader {
                format: "sonde-run".into(),
                version: 1,
                sonde: None,
                created: chrono::Utc::now(),
                endpoint: "e".into(),
                url: String::new(),
                region: String::new(),
                command: Some("AWS_SECRET_ACCESS_KEY=hunter2 python etl.py".into()),
                name: None,
                cwd: None,
                git: None,
                scenario: None,
                trial_of: None,
                whole_seconds: false,
                path_style: false,
                ca_file: None,
                tls: false,
                policy: None,
            },
            records: vec![],
            logs: vec![],
            afresh: false,
        };
        let doc = super::run_json(&s, &Default::default()).to_string();
        assert!(!doc.contains("hunter2"), "{doc}");
        assert!(doc.contains("AWS_SECRET_ACCESS_KEY=… python etl.py"));
        let mut page = Vec::new();
        // (a build without the viewer refuses the page: the document is what it holds)
        if write_page(&s, None, &Default::default(), None, &mut page).is_ok() {
            assert!(!String::from_utf8_lossy(&page).contains("hunter2"));
        }
    }
}
