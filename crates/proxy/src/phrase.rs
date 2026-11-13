//! What sonde says about a session, as data: a phrase is a key and its
//! arguments, so the app and the shared page say it in the reader's language
//! (`app/src/phrases.ts`) while the CLI and the session's English text come
//! from the templates here. A text is phrases said one after the other.

use std::collections::BTreeMap;

use reqlog::Text;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Phrase {
    pub key: &'static str,
    pub args: Args,
}

/// A phrase's arguments by name, in the order of their names: a map as it is
/// written and read, kept as the two or three pairs it is. Every operation
/// of a session has a title, and a session may have a million: its values
/// are shared text (an operation's name, a key its requests name already).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Args(Vec<(&'static str, Text)>);

impl Args {
    pub fn get(&self, name: &str) -> Option<&Text> {
        self.0.iter().find(|(k, _)| *k == name).map(|(_, v)| v)
    }

    pub fn contains_key(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn values(&self) -> impl Iterator<Item = &Text> {
        self.0.iter().map(|(_, v)| v)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &Text)> {
        self.0.iter().map(|(k, v)| (*k, v))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::ops::Index<&str> for Args {
    type Output = Text;
    fn index(&self, name: &str) -> &Text {
        self.get(name).expect("an argument of that name")
    }
}

impl<V: Into<Text>> FromIterator<(&'static str, V)> for Args {
    /// A name given twice keeps its last value, as a map would.
    fn from_iter<I: IntoIterator<Item = (&'static str, V)>>(pairs: I) -> Self {
        let mut all: Vec<(&'static str, Text)> = Vec::new();
        for (k, v) in pairs {
            let v = v.into();
            match all.iter_mut().find(|(have, _)| *have == k) {
                Some(slot) => slot.1 = v,
                None => all.push((k, v)),
            }
        }
        all.sort_by(|a, b| a.0.cmp(b.0));
        all.shrink_to_fit();
        Args(all)
    }
}

impl Serialize for Args {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(self.0.iter().map(|(k, v)| (k, v)))
    }
}

/// A phrase: `key`'s template with `args`.
pub fn p(key: &'static str, args: &[(&'static str, String)]) -> Phrase {
    Phrase {
        key,
        args: args.iter().cloned().collect(),
    }
}

/// [`p`] of text shared already (an operation's name, a target its
/// requests' key is in): a million operations hold it once.
pub fn p_shared(key: &'static str, args: &[(&'static str, Text)]) -> Phrase {
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
        "lance.commit" => "version {v} committed",
        "lance.data" => " · {n} data file{s} ({size})",
        "lance.index" => " · an index ({size})",
        "lance.deletions" => " · rows deleted",
        "lance.read.v" => "read version {v}",
        "lance.read" => "read the dataset",
        "lance.ranges" => " · {n} ranges of {files} file{s} ({size})",
        "hudi.instant" => "{action} {instant}",
        "hudi.metadata" => "metadata table: {action} {instant}",
        "hudi.properties" => "table properties written (hoodie.properties)",
        "paimon.snapshot" => "snapshot {n}",
        "paimon.schema" => "schema {n} written",
        "paimon.hint" => "{file} hint updated",
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
        "loose.deletes" => "{n} file{s} deleted",
        "obs.unfinished" => {
            "{n} multipart upload{s} never completed or aborted: the parts stay billed until a lifecycle rule removes them"
        }
        "obs.orphans.one" => {
            "files were written with no commit after them: orphans unless a later commit adds them"
        }
        "obs.orphans" => {
            "{n} times files were written with no commit after them: orphans unless a later commit adds them"
        }
        "obs.small" => {
            "{n} files under 5 MiB were uploaded as multipart uploads: 3 requests each where one PUT would do"
        }
        "obs.rename" => {
            "{n} object{s} copied out of _temporary/ and deleted: a commit by rename (Hadoop's FileOutputCommitter) writes each output file again on S3; an S3A committer writes it once"
        }
        "obs.markers" => {
            "{n} directory marker{s} written (empty objects named …/), {m} delete requests of markers: s3a keeps them with fs.s3a.directory.marker.retention=keep"
        }
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
        "split.slow" => "{pct}% of the slow requests (over {ms}) are ",
        "split.slow.most" => {
            "{pct}% of the slow requests (over {ms}), and {all}% of all requests, are "
        }
        "split.spread" => "the slow requests are spread: no one group holds half of them",
        "split.cost.first" => "{pct}% of the cost is ",
        "split.cost" => "; {pct}% of the cost is ",
        "split.and" => ", ",
        "split.end" => ".",
        "split.operation" => "{v}",
        "split.prefix" => "under {v}",
        "split.caller" => "from {v}",
        "split.sdk" => "from {v}",
        "split.status" => "answered {v}",
        "split.status.none" => "without an answer",
        "split.size.none" => "without a body",
        "split.size.under" => "under {hi}",
        "split.size.between" => "of {lo} to {hi}",
        "split.size.over" => "over {lo}",
        "fix.fixed" => "fixed",
        "fix.less" => "less",
        "fix.back" => "back",
        "fix.new" => "new",
        "fix.open" => "open",
        "fix.saved" => "saved {n} request{s} (predicted {p})",
        "fix.savedTime" => "{time} less waiting (predicted {p})",
        "trial.advice.retries" => "{sdk} did not send it again: turn its retries on ({setting})",
        "trial.advice.retry" => {
            "send a failed request again: a 500, 502, 503, 504, 408 or 429 is owed a retry, and so is S3's SlowDown, RequestTimeout or RequestTimeTooSkewed"
        }
        "trial.advice.skew" => {
            "a 403 RequestTimeTooSkewed is owed a retry signed again by the clock the answer's Date gives (the AWS SDKs for Java and JavaScript do this; boto3 does not); and keep the machine's clock in time"
        }
        "trial.advice.timeout" => {
            "give each attempt a timeout well under a stall (a first byte comes in well under a second), so that a stalled request is sent again"
        }
        "trial.advice.list" => {
            "a listing just after a write may not show it yet: keep the keys the program wrote, or list again before going on without one"
        }
        "trial.advice.cut" => {
            "check the bytes read against Content-Length, and read the rest (Range) or the whole again"
        }
        "trial.advice.race" => {
            "on a 412, read the log again and write the next commit with its condition; never write the key without one"
        }
        "trial.why.notMet" => "the scenario met none of the program's requests",
        "trial.why.uploads" => {
            "left {n} multipart upload{s} unfinished (their parts are billed until aborted)"
        }
        "trial.why.reads" => "gave up on {n} failed read{s} without sending {it} again",
        "trial.why.orphans" => {
            "{n} more object{s} left than the clean run leaves: what the lost commit wrote, unless the commit that followed names {it}"
        }
        "trial.why.blind" => {
            "{n} lost write{s} sent again with the same If-Match: a real writer's ETag would refuse it again"
        }
        "trial.why.lost" => "{n} write{s} answered 500 and not retried; the store has {it}",
        "trial.why.writes" => "{n} write{s} failed and {was} never sent again",
        "trial.why.exit0" => ", and the program exited 0",
        "trial.why.taken" => "{n} download{s} cut short and the rest never read",
        "trial.why.taken.whole" => {
            ": the half was taken as the whole (exit {exit}, as the clean run)"
        }
        "trial.why.holes" => {
            "{n} download{s} cut short and read on from past the cut: bytes are missing"
        }
        "trial.why.unrun" => {
            "{n} conditional write{s} lost {its} race: no commit followed {it} and nothing read back what won"
        }
        "trial.why.unrun.exit" => ", and the program exited {exit}",
        "trial.why.overwrote" => {
            "{n} lost race{s} answered by writing the key without a condition: the other writer's commit would be gone"
        }
        "trial.why.waited" => {
            "waited out {n} stall{s} of {time} before the first byte: no timeout cut {it} short"
        }
        "trial.why.reuploaded" => {
            "{n} upload{s} made again from {its} first part though the store had completed {it}: every part sent twice"
        }
        "trial.why.rewrote" => {
            "{n} key{s} a listing left out written again: the program took {it} for missing"
        }
        "trial.why.missed" => {
            "{n} key{s} this run wrote and a listing left out {was} never read as the clean run read {it}: the program went on without {it}"
        }
        "trial.why.staleUnused" => {
            "a listing left out {n} key{s} this run wrote, and nothing the program did after turned on {it}: the requests cannot tell whether it noticed"
        }
        "trial.why.cleanExit" => "the clean run exited {exit} itself",
        "trial.why.exit" => "exited {exit} (the clean run exited {clean})",
        "trial.why.refused.completed" => {
            "the retried {op} was answered {answer}: the upload had completed"
        }
        "trial.why.policy" => {
            "refused by the recording policy ({policy}): {n} request{s} more than in the clean run, so what the program does under the fault cannot be told"
        }
        "trial.advice.policy" => {
            "give each run a place of its own (the trial's name, SONDE_TRIAL, in its keys), or let the program write over what it finds (--allow-writes)"
        }
        "trial.why.refused.made" => {
            "the retried {op} was answered {answer}: the store had made the first attempt"
        }
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

/// A phrase in English from a key and arguments kept as text (a verdict
/// read back from its file). A key without a template says nothing.
pub fn en_of(key: &str, args: &BTreeMap<String, String>) -> String {
    // one pass, as [`Phrase::en`]: an argument's own braces are not read
    let tpl = template(key);
    let mut out = String::with_capacity(tpl.len());
    let mut rest = tpl;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if args.contains_key(&after[..close]) => {
                out.push_str(&args[&after[..close]]);
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

    /// A trial's reasons and advice are said by the app in both languages:
    /// each key has its line in each table of `app/src/phrases.ts`, and the
    /// English one is this file's.
    #[test]
    fn a_trials_sentences_have_their_templates_in_the_app() {
        let ts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/src/phrases.ts");
        let ts = std::fs::read_to_string(ts).unwrap();
        for key in [
            "advice.retries",
            "advice.retry",
            "advice.cut",
            "advice.race",
            "advice.skew",
            "advice.timeout",
            "advice.list",
            "why.notMet",
            "why.uploads",
            "why.reads",
            "why.orphans",
            "why.blind",
            "why.lost",
            "why.writes",
            "why.exit0",
            "why.taken",
            "why.taken.whole",
            "why.holes",
            "why.unrun",
            "why.unrun.exit",
            "why.overwrote",
            "why.waited",
            "why.reuploaded",
            "why.rewrote",
            "why.missed",
            "why.staleUnused",
            "why.cleanExit",
            "why.exit",
            "why.refused.completed",
            "why.refused.made",
            "why.policy",
            "advice.policy",
        ] {
            let key = format!("trial.{key}");
            let en = template(&key);
            assert!(!en.is_empty(), "{key}: no template");
            let lines: Vec<&str> = ts
                .lines()
                .filter(|l| l.trim_start().starts_with(&format!("\"{key}\": ")))
                .collect();
            assert_eq!(lines.len(), 2, "{key}: a line in each language");
            let quoted = serde_json::to_string(en).unwrap();
            assert!(
                lines.iter().any(|l| l.contains(&quoted)),
                "{key}: the app's English is not this file's"
            );
        }
    }

    /// Group by's sentence is said by the app in both languages too.
    #[test]
    fn the_split_sentences_have_their_templates_in_the_app() {
        let ts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/src/phrases.ts");
        let ts = std::fs::read_to_string(ts).unwrap();
        let src = include_str!("phrase.rs");
        let keys: Vec<&str> = src
            .lines()
            .filter_map(|l| l.trim_start().strip_prefix("\"split."))
            .filter_map(|l| l.split('"').next())
            .collect();
        assert!(keys.len() >= 16, "{keys:?}");
        for k in keys {
            let key = format!("split.{k}");
            let quoted = serde_json::to_string(template(&key)).unwrap();
            let lines: Vec<&str> = ts
                .lines()
                .filter(|l| l.trim_start().starts_with(&format!("\"{key}\": ")))
                .collect();
            assert_eq!(lines.len(), 2, "{key}: a line in each language");
            assert!(lines.iter().any(|l| l.contains(&quoted)), "{key}");
        }
    }

    /// The fix loop's words, said by the app in both languages.
    #[test]
    fn the_fix_loops_words_have_their_templates_in_the_app() {
        let ts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../app/src/phrases.ts");
        let ts = std::fs::read_to_string(ts).unwrap();
        for state in [
            crate::fixloop::State::Fixed,
            crate::fixloop::State::Less,
            crate::fixloop::State::Back,
            crate::fixloop::State::New,
            crate::fixloop::State::Open,
        ] {
            assert_eq!(state.phrase().en(), state.label());
        }
        for key in [
            "fix.fixed",
            "fix.less",
            "fix.back",
            "fix.new",
            "fix.open",
            "fix.saved",
            "fix.savedTime",
        ] {
            let quoted = serde_json::to_string(template(key)).unwrap();
            let lines: Vec<&str> = ts
                .lines()
                .filter(|l| l.trim_start().starts_with(&format!("\"{key}\": ")))
                .collect();
            assert_eq!(lines.len(), 2, "{key}: a line in each language");
            assert!(lines.iter().any(|l| l.contains(&quoted)), "{key}");
        }
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
