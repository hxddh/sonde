//! What a run leaves in the store, read from its requests: the objects that
//! stand when it ends, and how two runs differ in them.
//!
//! Each key is followed to its last change the store made (a write, a copy
//! onto it, a completed upload; a delete takes it away), so a write sent
//! twice under a fault leaves what it leaves sent once. Keys are then read
//! without what changes from one run of a program to the next
//! ([`crate::profile::stable`]: a UUID, a date, a number) and counted: two
//! runs of a table writer leave the same objects though no key of one is a
//! key of the other.
//!
//! What it says is which objects stand, never what is in them: a size and an
//! ETag differ from run to run in programs that did the same (a table's
//! metadata by a few bytes, a file of generated rows by thousands), so a
//! wrong byte written to the right key is not seen here.

use std::collections::BTreeMap;

use reqlog::Record;

/// The objects a run leaves, each key read as [`crate::profile::stable`]
/// reads it and counted by bucket.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Left {
    objects: BTreeMap<(String, String), usize>,
    /// Objects the run took away that it had not written: they stood before
    /// it, and a run that leaves them is one that leaves more.
    removed: BTreeMap<(String, String), usize>,
    /// A delete of several keys whose document the run did not keep whole:
    /// what it took away is not known, and the run is compared with none.
    unknown: bool,
}

/// How what one run leaves differs from what another does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    /// Objects the other run leaves and this one does not, and one of them.
    pub missing: usize,
    pub a_missing: Option<String>,
    /// Objects this run leaves and the other does not, and one of them.
    pub extra: usize,
    pub an_extra: Option<String>,
    /// Of the missing, those of a kind this run leaves none of (the other
    /// run's `out/part-{n}.json`, where this one wrote no such key at all),
    /// and one of them: an output never written, not one more or fewer of
    /// many.
    pub absent: usize,
    pub an_absent: Option<String>,
}

/// Whether the store made what `r` asked: its own 2xx, or the 2xx it gave
/// a request whose answer a fault lost (the program was told it failed; the
/// object is there all the same).
pub fn made(r: &Record) -> bool {
    let ok = |s: u16| (200..300).contains(&s);
    if crate::trial::faulted(r) {
        crate::trial::store_status(r).is_some_and(ok)
    } else {
        r.status.is_some_and(ok)
    }
}

/// What `records` leave. `names`: the names of the runs it is held against
/// (a trial's scenario and `clean`; none for runs that are no trial's),
/// each read as one word wherever a key holds it. The same names for both
/// runs: a program puts its run's name where it likes (`t{trial}/x` is
/// `tclean/x` in one run and `tslowdown/x` in the other), and a word of its
/// own that holds one (`errors/x` under the scenario `error`) reads the
/// same in both.
pub fn left<'a>(records: impl IntoIterator<Item = &'a Record>, names: &[&str]) -> Left {
    left_where(records, names, made)
}

/// [`left`], with `made` saying which requests left their mark: the store's
/// own word ([`made`]), or more (a write a trial answered in the store's
/// place, which the program was right to take as there).
pub fn left_where<'a>(
    records: impl IntoIterator<Item = &'a Record>,
    names: &[&str],
    made: impl Fn(&Record) -> bool,
) -> Left {
    let mut in_time: Vec<&Record> = records.into_iter().collect();
    in_time.sort_by_key(|r| (r.time, r.id));
    let read = |key: &str| {
        let mut key = key.to_string();
        for name in names.iter().filter(|n| !n.is_empty()) {
            key = key.replace(name, "{trial}");
        }
        crate::profile::stable(&key)
    };
    // Each key the run changed, by a hash of its name (a run of a million
    // writes keeps sixteen bytes a key, not the keys): how it reads
    // ([`crate::profile::stable`], kept once per reading), whether it
    // stands, and whether its first change was a delete (it stood before).
    struct Key {
        reads: u32,
        stands: bool,
        before: bool,
    }
    let mut keys: std::collections::HashMap<u64, Key> = Default::default();
    let mut readings: Vec<(String, String)> = Vec::new();
    let mut numbered: std::collections::HashMap<(String, String), u32> = Default::default();
    let mut change = |bucket: &str, key: &str, stands: bool| {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (bucket, key).hash(&mut h);
        keys.entry(h.finish())
            .and_modify(|k| k.stands = stands)
            .or_insert_with(|| {
                let reading = (bucket.to_string(), read(key));
                let reads = *numbered.entry(reading.clone()).or_insert_with(|| {
                    readings.push(reading);
                    readings.len() as u32 - 1
                });
                Key {
                    reads,
                    stands,
                    before: !stands,
                }
            });
    };
    let mut unknown = false;
    for r in in_time {
        if !made(r) {
            continue;
        }
        let Some(bucket) = r.bucket.as_ref().map(|b| b.as_str()) else {
            continue;
        };
        match (r.kind.as_str(), r.key.as_deref()) {
            ("PutObject" | "CopyObject" | "CompleteMultipartUpload", Some(key)) => {
                change(bucket, key, true);
            }
            ("DeleteObject", Some(key)) => change(bucket, key, false),
            ("DeleteObjects", _) => {
                let rare = r.rare();
                match rare.request_body.as_deref() {
                    Some(doc) if !rare.request_body_truncated => {
                        for (key, _version) in reqlog::deleted_objects(doc) {
                            change(bucket, &key, false);
                        }
                    }
                    _ => unknown = true,
                }
            }
            _ => {}
        }
    }
    let mut objects: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut removed: BTreeMap<(String, String), usize> = BTreeMap::new();
    for k in keys.values() {
        let reading = &readings[k.reads as usize];
        if k.stands {
            *objects.entry(reading.clone()).or_default() += 1;
        } else if k.before {
            *removed.entry(reading.clone()).or_default() += 1;
        }
    }
    Left {
        objects,
        removed,
        unknown,
    }
}

impl Left {
    /// How many objects stand.
    pub fn objects(&self) -> usize {
        self.objects.values().sum()
    }

    /// Whether what this run leaves can be held against another's.
    pub fn known(&self) -> bool {
        !self.unknown
    }

    /// How this run differs from `other` in what it leaves: none when they
    /// leave the same, or when either cannot be compared ([`Left::known`]).
    pub fn against(&self, other: &Left) -> Option<Difference> {
        if !self.known()
            || !other.known()
            || (self.objects == other.objects && self.removed == other.removed)
        {
            return None;
        }
        let name = |(bucket, key): &(String, String)| format!("{bucket}/{key}");
        let (mut missing, mut extra, mut absent) = (0, 0, 0);
        let (mut a_missing, mut an_extra, mut an_absent) = (None, None, None);
        for (k, n) in &other.objects {
            let mine = self.objects.get(k).copied().unwrap_or(0);
            if *n > mine {
                missing += n - mine;
                a_missing.get_or_insert_with(|| name(k));
                if mine == 0 {
                    absent += n;
                    an_absent.get_or_insert_with(|| name(k));
                }
            }
        }
        for (k, n) in &self.objects {
            let theirs = other.objects.get(k).copied().unwrap_or(0);
            if *n > theirs {
                extra += n - theirs;
                an_extra.get_or_insert_with(|| name(k));
            }
        }
        // what the other run took away and this one left where it stood
        for (k, n) in &other.removed {
            let mine = self.removed.get(k).copied().unwrap_or(0);
            if *n > mine {
                extra += n - mine;
                an_extra.get_or_insert_with(|| name(k));
            }
        }
        Some(Difference {
            missing,
            a_missing,
            extra,
            an_extra,
            absent,
            an_absent,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn rec(id: u64, method: &str, kind: &str, key: Option<&str>, status: u16) -> Record {
        Record {
            id,
            time: chrono::Utc
                .timestamp_opt(1_800_000_000 + id as i64, 0)
                .unwrap(),
            status: Some(status),
            kind: kind.into(),
            method: method.into(),
            bucket: Some("b".into()),
            key: key.map(Into::into),
            ..Default::default()
        }
    }

    fn put(id: u64, key: &str) -> Record {
        rec(id, "PUT", "PutObject", Some(key), 200)
    }

    #[test]
    fn a_key_is_followed_to_its_last_change() {
        let mut batch = rec(6, "POST", "DeleteObjects", None, 200);
        batch.rare_mut().request_body =
            Some("<Delete><Object><Key>t/c</Key></Object></Delete>".into());
        let run = [
            put(1, "t/a"),
            put(2, "t/a"), // written twice: there once
            put(3, "t/b"),
            rec(4, "DELETE", "DeleteObject", Some("t/b"), 204),
            put(5, "t/c"),
            batch,
            rec(7, "PUT", "PutObject", Some("t/d"), 503), // not made
            rec(8, "POST", "CompleteMultipartUpload", Some("t/e"), 200),
            rec(9, "PUT", "CopyObject", Some("t/f"), 200),
            rec(10, "GET", "GetObject", Some("t/g"), 200),
        ];
        let l = left(&run, &[]);
        assert_eq!(l.objects(), 3, "{l:?}");
        assert!(l.known());
        // (a key deleted and written again stands)
        let again = [
            put(1, "t/a"),
            rec(2, "DELETE", "DeleteObject", Some("t/a"), 204),
            put(3, "t/a"),
        ];
        assert_eq!(left(&again, &[]).objects(), 1);
    }

    #[test]
    fn two_runs_of_a_table_writer_leave_the_same_though_no_key_is_the_same() {
        let run = |uuid: &str, n: u32| {
            vec![
                put(1, &format!("t/data/part-00000-{uuid}-c000.snappy.parquet")),
                put(2, &format!("t/_delta_log/{n:020}.json")),
            ]
        };
        let a = left(&run("3f2504e0-4f89-11d3-9a0c-0305e82c3301", 1), &[]);
        let b = left(&run("9b2c1d7e-0a3f-4c55-8e21-77aa01bc9f10", 1), &[]);
        assert_eq!(a, b);
        assert_eq!(a.against(&b), None);
        // one commit more is one object more, whatever its number
        let mut twice = run("3f2504e0-4f89-11d3-9a0c-0305e82c3301", 1);
        twice.push(put(3, &format!("t/_delta_log/{:020}.json", 2)));
        let d = left(&twice, &[]).against(&a).unwrap();
        assert_eq!((d.missing, d.extra), (0, 1));
        assert_eq!(d.an_extra.as_deref(), Some("b/t/_delta_log/{n}.json"));
        // and one fewer is one missing
        let d = a.against(&left(&twice, &[])).unwrap();
        assert_eq!((d.missing, d.extra), (1, 0));
    }

    #[test]
    fn an_output_never_written_is_absent_and_one_fewer_of_many_is_only_missing() {
        let whole = left(
            &[
                put(1, "out/part-1.json"),
                put(2, "out/part-2.json"),
                put(3, "out/_SUCCESS"),
            ],
            &[],
        );
        // one part fewer: missing, and its kind is still there
        let fewer = left(&[put(1, "out/part-1.json"), put(3, "out/_SUCCESS")], &[]);
        let d = fewer.against(&whole).unwrap();
        assert_eq!((d.missing, d.absent), (1, 0));
        // no marker at all: absent
        let unmarked = left(&[put(1, "out/part-1.json"), put(2, "out/part-2.json")], &[]);
        let d = unmarked.against(&whole).unwrap();
        assert_eq!((d.missing, d.absent), (1, 1));
        assert_eq!(d.an_absent.as_deref(), Some("b/out/_SUCCESS"));
    }

    #[test]
    fn what_one_run_took_away_and_another_left_standing_is_left_over() {
        let del = |id| rec(id, "DELETE", "DeleteObject", Some("tmp/lock"), 204);
        // the lock stood before both runs: one removes it, one does not
        let tidy = left(&[put(1, "out/a"), del(2)], &[]);
        let untidy = left(&[put(1, "out/a")], &[]);
        let d = untidy.against(&tidy).unwrap();
        assert_eq!((d.missing, d.extra), (0, 1));
        assert_eq!(d.an_extra.as_deref(), Some("b/tmp/lock"));
        // a key the run itself wrote and removed was never left by either
        let own = left(&[put(1, "out/a"), put(2, "tmp/lock"), del(3)], &[]);
        assert_eq!(own.against(&untidy), None);
    }

    #[test]
    fn a_runs_own_name_in_a_key_is_no_difference() {
        // where the program likes: a word of its own, or part of one
        let names = ["error", "clean"];
        let clean = left(
            &[
                put(1, "etl-clean/state"),
                put(2, "tclean/x"),
                put(3, "errors/log"),
            ],
            &names,
        );
        let tried = left(
            &[
                put(1, "etl-error/state"),
                put(2, "terror/x"),
                put(3, "errors/log"),
            ],
            &names,
        );
        assert_eq!(tried.against(&clean), None);
        // and a key that is another key is still another
        let other = left(&[put(1, "etl-error/state"), put(2, "terror/y")], &names);
        assert_eq!(other.against(&clean).map(|d| d.absent), Some(2));
    }

    #[test]
    fn a_write_whose_answer_was_lost_stands_and_one_never_sent_on_does_not() {
        let fault = |mut r: Record, store: Option<u16>| {
            r.rare_mut().injected = Some("lost-answer".into());
            let hs = &mut r.lists_mut().response_headers;
            hs.push(reqlog::Header::new("x-sonde-fault", "lost-answer"));
            if let Some(s) = store {
                hs.push(reqlog::Header::new("x-sonde-store-status", s.to_string()));
            }
            r
        };
        let lost = fault(rec(1, "PUT", "PutObject", Some("t/a"), 500), Some(200));
        let unsent = fault(rec(2, "PUT", "PutObject", Some("t/b"), 503), None);
        assert_eq!(left(&[lost, unsent], &[]).objects(), 1);
    }

    #[test]
    fn a_batch_of_deletes_not_kept_whole_is_compared_with_none() {
        let mut batch = rec(2, "POST", "DeleteObjects", None, 200);
        batch.rare_mut().request_body = Some("<Delete><Object><Key>t/a</Key>".into());
        batch.rare_mut().request_body_truncated = true;
        let cut = left(&[put(1, "t/a"), batch], &[]);
        assert!(!cut.known());
        assert_eq!(cut.against(&left(&[], &[])), None);
    }
}
