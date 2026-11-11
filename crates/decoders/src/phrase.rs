//! What sonde says, as data: a phrase is a key and its arguments, so the app
//! says it in the reader's language (`app/src/corephrases.ts`) while the CLI
//! prints the English beside it. A text is phrases said one after the other.
//!
//! The type is here because every crate that says something depends on this
//! one; `sonde-core` and `probes` each keep their own table of templates and
//! build on [`Phrase::fill`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Phrase {
    pub key: String,
    #[serde(default)]
    pub args: BTreeMap<String, String>,
}

impl Phrase {
    pub fn new(key: impl Into<String>, args: &[(&str, String)]) -> Phrase {
        Phrase {
            key: key.into(),
            args: args
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }

    /// `template` with each `{name}` replaced by the argument of that name.
    pub fn fill(&self, template: &str) -> String {
        // one pass over the template: an argument's own text (a key named
        // "{n}.json") is never read as a placeholder
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let after = &rest[open + 1..];
            match after.find('}') {
                Some(close) if self.args.contains_key(&after[..close]) => {
                    out.push_str(&self.args[&after[..close]]);
                    rest = &after[close + 1..];
                }
                _ => {
                    out.push('{');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }
}

// a single phrase where a text is wanted
impl From<Phrase> for Vec<Phrase> {
    fn from(p: Phrase) -> Self {
        vec![p]
    }
}

/// `"s"` after a count other than one, for English plurals (`{s}`).
pub fn plural(n: usize) -> String {
    if n == 1 { String::new() } else { "s".into() }
}

/// A phrase: `ph!("key", name = value, …)`, each value by its `Display`.
#[macro_export]
macro_rules! ph {
    ($key:expr $(, $name:ident = $val:expr)* $(,)?) => {
        $crate::phrase::Phrase::new($key, &[$((stringify!($name), $val.to_string())),*])
    };
}

/// Words sonde does not translate (a key, an error a library worded).
pub fn raw(text: impl Into<String>) -> Phrase {
    Phrase::new("raw", &[("text", text.into())])
}

/// A fact's label: the key is the label itself under `fact.`.
pub fn fact_label(label: &str) -> Phrase {
    debug_assert!(
        FACTS.contains(&label),
        "fact label {label:?} is not in decoders::phrase::FACTS"
    );
    Phrase::new(format!("fact.{label}"), &[])
}

/// This crate's phrase in English.
pub fn say(p: &Phrase) -> String {
    match p.key.strip_prefix("fact.") {
        Some(label) => label.to_string(),
        None => p.fill(template(&p.key).unwrap_or("")),
    }
}

/// A text in English: its phrases one after the other.
pub fn en(text: &[Phrase]) -> String {
    text.iter().map(say).collect()
}

pub fn template(key: &str) -> Option<&'static str> {
    TEMPLATES.iter().find(|(k, _)| *k == key).map(|(_, t)| *t)
}

/// Every key this crate says and its English: the table and the fact labels.
pub fn templates() -> Vec<(String, String)> {
    TEMPLATES
        .iter()
        .map(|(k, t)| (k.to_string(), t.to_string()))
        .chain(FACTS.iter().map(|l| (format!("fact.{l}"), l.to_string())))
        .collect()
}

/// The names in a template's `{…}`.
fn placeholders(template: &str) -> Vec<&str> {
    let mut out = vec![];
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        rest = &rest[open + 1..];
        if let Some(close) = rest.find('}') {
            let name = &rest[..close];
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric()) {
                out.push(name);
            }
        }
    }
    out
}

/// What a translation file (`app/src/corephrases.ts`: a `"key": "template",`
/// a line) lacks for `templates`: a key with no line, or a line that names
/// an argument the English does not have. Empty when it is whole.
pub fn untranslated(ts: &str, templates: &[(String, String)]) -> Vec<String> {
    let said: BTreeMap<String, String> = ts
        .lines()
        .filter_map(|l| {
            let l = l.trim().trim_end_matches(',');
            let (k, v) = l.split_once("\": \"")?;
            Some((
                serde_json::from_str(&format!("{k}\"")).ok()?,
                serde_json::from_str(&format!("\"{v}")).ok()?,
            ))
        })
        .collect();
    let mut out = vec![];
    for (key, en) in templates {
        match said.get(key) {
            None => out.push(format!("{key}: no template")),
            Some(t) => {
                let has = placeholders(en);
                for name in placeholders(t) {
                    if !has.contains(&name) {
                        out.push(format!("{key}: {{{name}}} is not an argument"));
                    }
                }
            }
        }
    }
    out
}

/// The labels of facts (`fact.<label>`).
pub const FACTS: &[&str] = &[
    // FACTS-BEGIN
    "Active",
    "Bytes written by then",
    "Bytes",
    "Checkpoints",
    "Compactor epoch",
    "Compactor state files",
    "Data files",
    "Entries",
    "Fencing events",
    "First write",
    "Format version",
    "L0 SSTs",
    "Last L0 sequence",
    "Last updated",
    "Last write",
    "Live bytes",
    "Live records",
    "Location",
    "Logs",
    "Longest pause",
    "Median interval",
    "Metadata",
    "Metaranges (committed trees)",
    "Objects written by then",
    "Objects",
    "Open",
    "Operation",
    "Ranges",
    "Sealed",
    "Segments started",
    "Segments",
    "Snapshot",
    "Snapshots",
    "Sorted runs",
    "Table UUID",
    "WAL gaps",
    "WAL last seen",
    "WAL not yet in L0",
    "WAL objects written by then",
    "WAL replay after",
    "Writer epoch",
    "p99 interval",
    "Busiest interval",
    "Compacted SSTs",
    "Current snapshot",
    "Data objects",
    "Distinct paths",
    "Expirable snapshots",
    "GC lag",
    "Latest manifest",
    "Live data files",
    "Live delete files",
    "Metadata files",
    "Orphan data files",
    "Records",
    "Refs",
    "Unreferenced data",
    "WAL SSTs",
    "Writes per second",
    // FACTS-END
];

/// The English templates: `{name}` takes the argument of that name.
pub const TEMPLATES: &[(&str, &str)] = &[
    ("raw", "{text}"),
    // TEMPLATES-BEGIN
    ("cadence.closed", "The {format} format is not published: sonde does not decode it. Shown: prefix statistics and write cadence from the listing only."),
    ("cadence.objects", "{n} objects"),
    ("cadence.pause", "a write pause of {s} s against a median interval of {ms} ms"),
    ("cadence.busiest", "{n} objects per {min} min"),
    ("cadence.headline", "{format}: {n} objects ({size}), cadence only"),
    ("cadence.headline.closed", " — format not published, not decoded"),
    ("decode.failed", "{key}: cannot decode: {error}"),
    ("unexpected.object", "unexpected object {key} ({size})"),
    ("headline.errors", " — errors found"),
    ("slate.skipped", "{n} older manifests not decoded (only the newest {max})"),
    ("slate.headline.none", "no manifest found"),
    ("slate.headline.undecoded", "no manifest could be decoded"),
    ("slate.manifest", "manifest {id}"),
    ("slate.manifest.detail", "writer epoch {writer}, compactor epoch {compactor}, L0 {l0}, runs {runs}, WAL replay after {wal}"),
    ("slate.fence.writer", "writer epoch {from} → {to}"),
    ("slate.fence.writer.detail", "a new writer fenced the previous one"),
    ("slate.fence.compactor", "compactor epoch {from} → {to}"),
    ("slate.fence.compactor.detail", "a new compactor took over"),
    ("slate.wal", "WAL {id}"),
    ("slate.wal.flushed", "flushed to L0 (GC-able)"),
    ("slate.wal.pending", "not yet in L0"),
    ("slate.gap.one", "WAL {id} is missing"),
    ("slate.gap.many", "WAL {from}–{to} are missing"),
    ("slate.gap.lost", ": entries not yet in L0 are lost"),
    ("slate.gap.flushed", " (already flushed; collected out of order)"),
    ("slate.sst.missing", "compacted/{name}.sst is referenced by the manifest but missing"),
    ("slate.orphans", "{n} compacted SSTs ({size}) are referenced by no manifest or checkpoint: orphans awaiting GC"),
    ("slate.gclag", "{n} WAL SSTs ({size}) were flushed to L0 but not collected; the oldest is {hours} h old (GC lag)"),
    ("slate.gclag.fact", "{n} WAL SSTs ({size}) flushed but kept; {orphans} orphan SSTs; {manifests} superseded manifests"),
    ("slate.headline", "SlateDB: manifest {id}, writer epoch {epoch}, {wals} WAL SSTs ({pending} pending), {l0} L0 + {runs} sorted runs"),
    ("none", "none"),
    ("referenced.missing", "{key} is referenced but missing"),
    ("iceberg.headline.none", "no metadata.json"),
    ("iceberg.notjson", "{key}: not JSON: {error}"),
    ("iceberg.headline.unreadable", "unreadable metadata.json"),
    ("iceberg.version.unknown", "unknown Iceberg format-version {v} (sonde decodes 1, 2 and 3); not decoded"),
    ("iceberg.headline.unknown", "Iceberg format-version {v}: unknown, not decoded"),
    ("iceberg.inline", "snapshot {id} uses inline manifests (format v1 `manifests`)"),
    ("iceberg.list.unreadable", "snapshot {id}: manifest list {key} unreadable: {error}"),
    ("iceberg.manifest.bad", "manifest {key}: {error}"),
    ("iceberg.manifest.missing", "manifest {key} is missing: {error}"),
    ("iceberg.orphans", "{n} data files ({size}) are referenced by no snapshot: orphans (failed writes, or files of expired snapshots not deleted); remove_orphan_files reclaims them"),
    ("iceberg.expirable", "{n} snapshots are older than {days} days and not referenced: expire_snapshots would drop them and free {size} in {files} files"),
    ("iceberg.expirable.fact", "{n} (max age {days} d, keep {keep})"),
    ("iceberg.snapshot", "snapshot {id} ({op})"),
    ("iceberg.snapshot.detail", "{n} live files"),
    ("iceberg.headline", "Iceberg v{v}: {n} snapshots, "),
    ("iceberg.headline.current", "current {id}"),
    ("iceberg.headline.nocurrent", "current none"),
    ("iceberg.headline.rest", ", {files} live files ({size}), {orphans} orphans, {expirable} expirable"),
    ("lakefs.notgraveler", "{key}: not a graveler table ({why})"),
    ("lakefs.range.missing", "range {id} is listed by a metarange but missing"),
    ("lakefs.unreferenced", "{n} data objects ({size}) are referenced by no committed range: uncommitted (staged) writes, or garbage for lakeFS's GC"),
    ("lakefs.kv", "Commit history, branches and tags are kept in lakeFS's KV store, not in object storage: each metarange below is one committed tree"),
    ("lakefs.tree", "tree {id}"),
    ("lakefs.tree.detail", "{ranges} ranges, {entries} entries"),
    ("lakefs.unreferenced.detail", "unreferenced"),
    ("lakefs.committed.detail", "committed"),
    ("lakefs.headline", "lakeFS: {trees} committed trees, {ranges} ranges, {data} data objects ({unreferenced} unreferenced)"),
    ("wp.unexpected", "unexpected object {key}"),
    ("wp.crc", "{key}: a record fails its CRC32"),
    ("wp.footer.version", "{key}: unknown footer version {v} (sonde decodes 5 and 6); not decoded"),
    ("wp.state.unknown", "unknown version"),
    ("wp.index.count", "{key}: {n} index records for {blocks} blocks"),
    ("wp.blocks.missing", "log {log} segment {seg}: footer lists {n} blocks, {have} exist"),
    ("wp.state.compacted", "sealed, compacted"),
    ("wp.state.sealed", "sealed"),
    ("wp.sealed", "log {log} segment {seg} sealed"),
    ("wp.footer.none", "{key}: no footer record"),
    ("wp.state.corrupt", "corrupt footer"),
    ("wp.blocks.gaps", "log {log} segment {seg}: block ids {from}–{to} have gaps"),
    ("wp.version", "{key}: unknown format version {v} (sonde decodes 5 and 6); not decoded"),
    ("wp.block.name", "{key}: names block {n} but holds block {holds}"),
    ("wp.block.noheader", "{key}: no block header"),
    ("wp.state.locked", "active (locked)"),
    ("wp.state.active", "active"),
    ("wp.started", "log {log} segment {seg} started"),
    ("wp.headline", "Woodpecker: {logs} logs, {segs} segments ({sealed} sealed, {active} active), {entries} entries"),
    // TEMPLATES-END
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_argument_is_never_read_as_a_placeholder() {
        let x = ph!("raw", text = "b/{text}.bin");
        assert_eq!(say(&x), "b/{text}.bin");
    }

    #[test]
    fn no_key_is_listed_twice() {
        let all = templates();
        for (i, (k, _)) in all.iter().enumerate() {
            assert!(
                all[..i].iter().all(|(o, _)| o != k),
                "{k} is listed twice"
            );
        }
    }

    #[test]
    fn a_phrase_read_back_from_json_says_the_same() {
        let x = raw("a");
        let back: Phrase = serde_json::from_str(&serde_json::to_string(&x).unwrap()).unwrap();
        assert_eq!(back, x);
        assert_eq!(say(&back), "a");
    }
}
