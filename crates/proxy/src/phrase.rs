//! What sonde says about a session, as data: a phrase is a key and its
//! arguments, so the app and the shared page say it in the reader's language
//! (`app/src/phrases.ts`) while the CLI and the session's English text come
//! from the templates here. A text is phrases said one after the other.

use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Phrase {
    pub key: &'static str,
    pub args: BTreeMap<&'static str, String>,
}

/// A phrase: `key`'s template with `args`.
pub fn p(key: &'static str, args: &[(&'static str, String)]) -> Phrase {
    Phrase {
        key,
        args: args.iter().cloned().collect(),
    }
}

/// `"s"` after a count other than one, for English plurals (`{s}`).
pub fn plural(n: usize) -> String {
    if n == 1 { String::new() } else { "s".into() }
}

/// The English templates: `{name}` takes the argument of that name.
fn template(key: &str) -> &'static str {
    match key {
        "request" => "{op} {target}",
        "request.prefix" => "{prefix}",
        "size" => " · {size}",
        "cond.create" => "conditional create (If-None-Match: *)",
        "cond.write" => "conditional write (If-Match)",
        "race" => "lost a race: 412 Precondition Failed",
        "probe" => "{n} lookups in {dir}, none found",
        "listing" => "listed {bucket}/{prefix} in {n} pages",
        "mpu" => "multipart upload {key} · {n} part{s} · {size}",
        "mpu.aborted" => " (aborted)",
        "mpu.open" => " (not completed)",
        "mpu.billed" => "the upload was never completed or aborted: its parts are billed",
        "retry" => "{op} {target} retried {n}× after {first}",
        "retry.wait" => "{ms} ms until it succeeded or gave up",
        "step.deleted" => "deleted {file}",
        "slate.manifest" => "manifest {n} written",
        "slate.wal.empty" => "WAL {n} written empty (fencing marker)",
        "slate.wal" => "WAL {n} flushed · {size}",
        "slate.sst" => "SST flushed to L0 · {size}",
        "slate.compactions" => "compaction state {n} written",
        "iceberg.metadata.v" => "commit: table metadata v{v}",
        "iceberg.metadata" => "commit: table metadata",
        "delta.commit" => "commit {n}",
        "delta.checkpoint" => "checkpoint written",
        "delta.last" => "_last_checkpoint updated",
        "step.other" => "{op} {key}",
        "step.files" => " · {n} file{s} ({size})",
        "after.listing" => "after listing {dir}/",
        "after.reads" => "after {n} read{s}",
        "after.misses" => "after {n} lookup{s} that found nothing",
        "after.mixed" => "after {n} reads ({m} found nothing)",
        "loose.uncommitted" => "{n} file{s} written, no commit followed ({size})",
        "slate.walprobe" => "looked for the newest WAL: probed ids {lo}…{hi}",
        "read.manifest" => "read manifest {n}",
        "read.compactions" => "read compaction state {n}",
        "read.metadata" => "read table metadata v{v}",
        "read.commit" => "read commit {n}",
        "read.file" => "read {file}",
        "read.count" => " ({n} requests)",
        "loose.listed" => "listed {dir}/",
        "loose.misses" => "{n} lookup{s} that found nothing",
        "loose.reads" => "{n} reads",
        "obs.shared" => "{n} {format} writes: ",
        "obs.unfinished" => {
            "{n} multipart upload{s} never completed or aborted: the parts stay billed until a lifecycle rule removes them"
        }
        "obs.orphans.one" => {
            "files were written with no commit after them: orphans unless a later commit adds them"
        }
        "obs.orphans" => {
            "{n} times files were written with no commit after them: orphans unless a later commit adds them"
        }
        "obs.retried" => "{n} request{s} repeated after a retryable failure",
        "obs.small" => {
            "{n} files under 5 MiB were uploaded as multipart uploads: 3 requests each where one PUT would do"
        }
        "obs.races" => "{n} conditional write{s} lost a race (412): another writer got there first",
        "obs.hot" => "{key} was read {n} times ({pct}% of all requests)",
        "obs.listed" => "{prefix} was listed {n} times",
        "obs.headget" => {
            "{n} objects were checked with HEAD and then read with GET: the GET alone answers both"
        }
        "obs.throttled" => {
            "{n} request{s} throttled (503 SlowDown or 429), each with {min} or more in flight: under {safe} at once would likely avoid it"
        }
        "obs.throttled.rate" => {
            "{n} request{s} throttled (503 SlowDown or 429): the store asked for fewer requests per second"
        }
        "obs.slow" => {
            "{n} request{s} took over {ms} ms against a median of {median} ms; the slowest, {op}, took {max} ms"
        }
        "obs.prefixrate.reads" => {
            "{prefix} took up to {n} reads a second, {pct}% of the {limit} S3 serves one prefix: spread its keys over more prefixes before it throttles"
        }
        "obs.prefixrate.writes" => {
            "{prefix} took up to {n} writes a second, {pct}% of the {limit} S3 serves one prefix: spread its keys over more prefixes before it throttles"
        }
        "obs.serial" => {
            "{n} requests ran one after another for {time}, each waiting for the one before: {k} at once would take about {fast}"
        }
        "obs.savesTime" => "about {time} sooner ({pct}% of the time waiting)",
        "obs.saves" => "{n} fewer request{s} ({pct}% of the session)",
        _ => "",
    }
}

impl Phrase {
    /// The phrase in English.
    pub fn en(&self) -> String {
        // one pass over the template: an argument's own text (a key named
        // "{n}.json") is never read as a placeholder
        let tpl = template(self.key);
        let mut out = String::with_capacity(tpl.len());
        let mut rest = tpl;
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

/// A text in English: its phrases one after the other.
pub fn en(text: &[Phrase]) -> String {
    text.iter().map(Phrase::en).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_argument_is_never_read_as_a_placeholder() {
        let x = p(
            "mpu",
            &[
                ("key", "b/{n}{s}.bin".into()),
                ("n", "2".into()),
                ("s", plural(2)),
                ("size", "200 B".into()),
            ],
        );
        assert_eq!(x.en(), "multipart upload b/{n}{s}.bin · 2 parts · 200 B");
    }

    #[test]
    fn every_key_has_an_english_template() {
        let x = p(
            "mpu",
            &[
                ("key", "b/k".into()),
                ("n", "2".into()),
                ("s", plural(2)),
                ("size", "200 B".into()),
            ],
        );
        assert_eq!(x.en(), "multipart upload b/k · 2 parts · 200 B");
        assert_eq!(
            en(&[x, p("mpu.open", &[])]),
            "multipart upload b/k · 2 parts · 200 B (not completed)"
        );
        assert_eq!(p("size", &[("size", "1 B".into())]).en(), " · 1 B");
    }
}
