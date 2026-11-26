//! What part of a table format a key names: a SlateDB manifest or WAL, an
//! Iceberg, Delta, Lance, Hudi or Paimon commit or data file. Faults aim at
//! a table's log by it; the analysis groups a run's requests by it.

use serde::Serialize;

/// A storage format sonde recognizes from keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Format {
    SlateDB,
    Iceberg,
    Delta,
    Lance,
    Hudi,
    Paimon,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::SlateDB => "SlateDB",
            Format::Iceberg => "Iceberg",
            Format::Delta => "Delta",
            Format::Lance => "Lance",
            Format::Hudi => "Hudi",
            Format::Paimon => "Paimon",
        }
    }
}

/// What a key is within its format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    SlateManifest(u64),
    SlateWal(u64),
    SlateSst,
    SlateCompactions(u64),
    SlateGcBoundary,
    IcebergMetadata(Option<u64>),
    IcebergVersionHint,
    IcebergManifestList,
    IcebergManifest,
    IcebergData,
    DeltaCommit(u64),
    DeltaCheckpoint,
    DeltaLastCheckpoint,
    DeltaData,
    /// A dataset's version: `_versions/<n>.manifest`, written if absent.
    LanceManifest(u64),
    /// What a commit did, written before its manifest: `_transactions/*.txn`.
    LanceTransaction,
    /// A fragment's file: `data/*.lance`.
    LanceData,
    /// An index's file: `_indices/<uuid>/…`.
    LanceIndex,
    /// Rows marked deleted in a fragment: `_deletions/*`.
    LanceDeletion,
    /// A Hudi instant on its timeline: `.hoodie/<instant>.<action>[.<state>]`
    /// (1.x: `.hoodie/timeline/<instant>_<completed>.<action>`).
    HudiInstant {
        instant: String,
        action: String,
        state: HudiState,
        /// Of the metadata table under `.hoodie/metadata/`.
        metadata: bool,
    },
    /// `.hoodie/hoodie.properties`: the table's settings.
    HudiProperties,
    /// What a write marks while it runs (`.hoodie/.temp/`), and `.hoodie/.aux/`.
    HudiMarker,
    /// A base file (`<file id>_<token>_<instant>.parquet`) or a log file
    /// (`.<file id>_<instant>.log.<n>_<token>`).
    HudiData,
    /// `snapshot/snapshot-<n>`: a Paimon commit.
    PaimonSnapshot(u64),
    /// `snapshot/LATEST` or `snapshot/EARLIEST`: a hint to the newest or oldest.
    PaimonHint,
    /// `manifest/manifest-*`, `manifest-list-*`, `index-manifest-*`.
    PaimonManifest,
    /// `schema/schema-<n>`.
    PaimonSchema(u64),
    /// A data or changelog file in a bucket: `bucket-<n>/data-*`.
    PaimonData,
}

/// Where a Hudi instant is: asked for, begun, or completed (the commit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudiState {
    Requested,
    Inflight,
    Completed,
}

impl Part {
    pub fn format(&self) -> Format {
        use Part::*;
        match self {
            SlateManifest(_) | SlateWal(_) | SlateSst | SlateCompactions(_) | SlateGcBoundary => {
                Format::SlateDB
            }
            IcebergMetadata(_) | IcebergVersionHint | IcebergManifestList | IcebergManifest
            | IcebergData => Format::Iceberg,
            DeltaCommit(_) | DeltaCheckpoint | DeltaLastCheckpoint | DeltaData => Format::Delta,
            LanceManifest(_) | LanceTransaction | LanceData | LanceIndex | LanceDeletion => {
                Format::Lance
            }
            HudiInstant { .. } | HudiProperties | HudiMarker | HudiData => Format::Hudi,
            PaimonSnapshot(_) | PaimonHint | PaimonManifest | PaimonSchema(_) | PaimonData => {
                Format::Paimon
            }
        }
    }

    /// A data file: said by its name alone ([`part_of`]), or by its table's
    /// log ([`data_of`]).
    pub fn is_data(&self) -> bool {
        use Part::*;
        matches!(self, IcebergData | DeltaData | HudiData | PaimonData)
    }

    /// Whether a write of this part ends a step of the format.
    pub fn commits(&self) -> bool {
        use Part::*;
        matches!(
            self,
            SlateManifest(_)
                | SlateWal(_)
                | SlateSst
                | SlateCompactions(_)
                | IcebergMetadata(_)
                | IcebergVersionHint
                | DeltaCommit(_)
                | DeltaCheckpoint
                | DeltaLastCheckpoint
                | LanceManifest(_)
                | HudiInstant {
                    state: HudiState::Completed,
                    ..
                }
                | HudiProperties
                | PaimonSnapshot(_)
                | PaimonHint
                | PaimonSchema(_)
        )
    }
}

fn digits(s: &str, n: usize) -> Option<u64> {
    (s.len() == n && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// A data file of a table of `format`, for the formats whose data files
/// are not known by their folder: what a Parquet file under a table's root
/// is once that table's log (`_delta_log/`, `metadata/`, …) was seen.
pub fn data_of(format: Format) -> Option<Part> {
    match format {
        Format::Iceberg => Some(Part::IcebergData),
        Format::Delta => Some(Part::DeltaData),
        Format::Hudi => Some(Part::HudiData),
        Format::Paimon => Some(Part::PaimonData),
        Format::SlateDB | Format::Lance => None,
    }
}

/// Whether a key may be a table's data file: one [`part_of`] knows by its
/// name, or a Parquet file under a `data/` folder or named `part-*`, as
/// Iceberg, Delta, Hive and Spark all write them. Whose it is, if any
/// table's, its path does not say: the log beside it does ([`data_of`]).
pub fn may_be_data(key: &str) -> bool {
    let name = key.rsplit('/').next().unwrap_or(key);
    (name.ends_with(".parquet") && (key.contains("/data/") || name.starts_with("part-")))
        || part_of(key).is_some_and(|p| p.is_data())
}

/// A data file as Iceberg's writers name it:
/// `<partition>-<task>-<uuid>[-<n>].parquet` (`00000-0-6c5e…-00001.parquet`).
fn iceberg_data_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".parquet") else {
        return false;
    };
    let mut parts = stem.split('-');
    let number =
        |s: Option<&str>| s.is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
    number(parts.next()) && number(parts.next()) && parts.next().is_some()
}

/// The version an Iceberg metadata file's name starts with: a run of at
/// least five digits and a dash (`00003-<uuid>`). A name that is a UUID
/// alone carries none, even when its first group is all digits.
fn iceberg_version(stem: &str) -> Option<u64> {
    let (n, rest) = stem.split_once('-')?;
    (n.len() >= 5 && n.bytes().all(|b| b.is_ascii_digit()) && !rest.is_empty() && !is_uuid(stem))
        .then(|| n.parse().ok())
        .flatten()
}

/// `8-4-4-4-12` hex digits, the way a UUID is written.
fn is_uuid(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// A data file as Delta's writers (and Spark's) name it:
/// `part-<n>-<uuid>[-c000 | .c000][.snappy].parquet`.
fn delta_data_name(name: &str) -> bool {
    name.starts_with("part-") && name.ends_with(".parquet")
}

/// The format part a key names, from its last two path segments. A data
/// file is known by its own name only (`part-…​.parquet`: Delta's;
/// `00000-0-<uuid>.parquet`: Iceberg's), which is a guess the analysis
/// takes beside that table's log; a folder's name (`data/`) says nothing,
/// a Delta, Hudi or Hive table has one too, so any other file is none.
pub fn part_of(key: &str) -> Option<Part> {
    let mut segs = key.rsplit('/');
    let name = segs.next()?;
    let dir = segs.next().unwrap_or("");
    let above = segs.next();
    // an index's files are a folder deeper: _indices/<uuid>/<file>
    if above == Some("_indices") {
        return Some(Part::LanceIndex);
    }
    // Hudi's markers and its auxiliary files, any depth under .hoodie/
    if [".hoodie/.temp/", ".hoodie/.aux/"]
        .iter()
        .any(|d| key.starts_with(d) || key.contains(&format!("/{d}")))
    {
        return Some(Part::HudiMarker);
    }
    if dir == ".hoodie" || (dir == "timeline" && above == Some(".hoodie")) {
        if name == "hoodie.properties" {
            return Some(Part::HudiProperties);
        }
        let metadata = key.contains(".hoodie/metadata/.hoodie/");
        return hudi_instant(name, metadata);
    }
    if let Some(n) = dir.strip_prefix("bucket-")
        && n.parse::<u32>().is_ok()
        && (name.starts_with("data-") || name.starts_with("changelog-"))
    {
        return Some(Part::PaimonData);
    }
    if is_hudi_file(name) {
        return Some(Part::HudiData);
    }
    let stem = |ext: &str| name.strip_suffix(ext);
    match dir {
        "snapshot" => match name {
            "LATEST" | "EARLIEST" => Some(Part::PaimonHint),
            _ => name
                .strip_prefix("snapshot-")
                .and_then(|n| n.parse().ok())
                .map(Part::PaimonSnapshot),
        },
        "schema" => name
            .strip_prefix("schema-")
            .and_then(|n| n.parse().ok())
            .map(Part::PaimonSchema),
        "manifest" if name.starts_with("manifest-") || name.starts_with("index-manifest-") => {
            Some(Part::PaimonManifest)
        }
        "manifest" => stem(".manifest")
            .and_then(|s| digits(s, 20))
            .map(Part::SlateManifest),
        "wal" => stem(".sst").and_then(|s| digits(s, 20)).map(Part::SlateWal),
        "compacted" if name.ends_with(".sst") => Some(Part::SlateSst),
        "compactions" => stem(".compactions")
            .and_then(|s| digits(s, 20))
            .map(Part::SlateCompactions),
        "gc" if name.ends_with(".boundary") => Some(Part::SlateGcBoundary),
        // newer datasets name a version by what is left to u64's end, so the
        // newest lists first; older ones by the version itself
        "_versions" => stem(".manifest").and_then(|s| {
            digits(s, 20)
                .map(|n| u64::MAX - n)
                .or_else(|| s.parse().ok())
                .map(Part::LanceManifest)
        }),
        "_transactions" if name.ends_with(".txn") => Some(Part::LanceTransaction),
        "_deletions" if name.ends_with(".arrow") || name.ends_with(".bin") => {
            Some(Part::LanceDeletion)
        }
        "data" if name.ends_with(".lance") => Some(Part::LanceData),
        "_delta_log" => {
            if let Some(n) = stem(".json").and_then(|s| digits(s, 20)) {
                Some(Part::DeltaCommit(n))
            } else if name == "_last_checkpoint" {
                Some(Part::DeltaLastCheckpoint)
            } else if name.contains(".checkpoint") {
                Some(Part::DeltaCheckpoint)
            } else {
                None
            }
        }
        "metadata" => {
            if let Some(s) = stem(".metadata.json") {
                // v3.metadata.json or 00003-<uuid>.metadata.json
                let v = s
                    .strip_prefix('v')
                    .and_then(|n| n.parse().ok())
                    .or_else(|| iceberg_version(s));
                Some(Part::IcebergMetadata(v))
            } else if name == "version-hint.text" {
                Some(Part::IcebergVersionHint)
            } else if name.starts_with("snap-") && name.ends_with(".avro") {
                Some(Part::IcebergManifestList)
            } else if name.ends_with(".avro") {
                Some(Part::IcebergManifest)
            } else {
                None
            }
        }
        _ => {
            // data files, by their names: the folder they are in is no evidence
            if delta_data_name(name) {
                Some(Part::DeltaData)
            } else if iceberg_data_name(name) {
                Some(Part::IcebergData)
            } else {
                None
            }
        }
    }
}

/// A Hudi instant's file: `<instant>[_<completed>].<action>[.requested|.inflight]`,
/// or 0.x's `<instant>.inflight` for a commit. The instant is a time,
/// 14 digits or more.
fn hudi_instant(name: &str, metadata: bool) -> Option<Part> {
    let mut parts = name.split('.');
    let time = parts.next()?;
    let instant = time.split('_').next()?;
    if instant.len() < 14 || !instant.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let rest: Vec<&str> = parts.collect();
    let (action, state) = match rest.as_slice() {
        ["inflight"] => ("commit", HudiState::Inflight),
        [a] => (*a, HudiState::Completed),
        [a, "requested"] => (*a, HudiState::Requested),
        [a, "inflight"] => (*a, HudiState::Inflight),
        _ => return None,
    };
    const ACTIONS: &[&str] = &[
        "commit",
        "deltacommit",
        "replacecommit",
        "clean",
        "compaction",
        "logcompaction",
        "clustering",
        "rollback",
        "savepoint",
        "restore",
        "indexing",
        "schemacommit",
    ];
    ACTIONS.contains(&action).then(|| Part::HudiInstant {
        instant: instant.to_string(),
        action: action.to_string(),
        state,
        metadata,
    })
}

/// A Hudi file slice's file: a base file `<file id>_<token>_<instant>.parquet`
/// (or .orc, .hfile) or a log file `.<file id>_<instant>.log.<version>_<token>`.
fn is_hudi_file(name: &str) -> bool {
    if name.starts_with('.') && name.contains(".log.") {
        return name[1..].split('_').count() >= 2;
    }
    let Some(stem) = [".parquet", ".orc", ".hfile"]
        .iter()
        .find_map(|e| name.strip_suffix(e))
    else {
        return false;
    };
    let parts: Vec<&str> = stem.split('_').collect();
    parts.len() == 3 && parts[2].len() >= 14 && parts[2].bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `data/` folder in a path is no evidence of Iceberg: a Delta table
    /// under one keeps its files, and a file no format names is unknown.
    #[test]
    fn a_data_folder_does_not_make_a_file_icebergs() {
        for key in [
            "lake/data/events/part-00000-6c5e0a1b-1f2e-4d3c-8b7a-0123456789ab.c000.snappy.parquet",
            "lake/data/events/day=1/part-00001-6c5e0a1b-1f2e-4d3c-8b7a-0123456789ab-c000.snappy.parquet",
            "events/part-00000-6c5e0a1b.c000.snappy.parquet",
        ] {
            assert_eq!(part_of(key), Some(Part::DeltaData), "{key}");
        }
        for key in [
            "wh/t/data/x=1/00000-0-6c5e0a1b-1f2e-4d3c-8b7a-0123456789ab.parquet",
            "wh/t/data/00003-12-6c5e0a1b-1f2e-4d3c-8b7a-0123456789ab-00001.parquet",
        ] {
            assert_eq!(part_of(key), Some(Part::IcebergData), "{key}");
        }
        // Hive's, DuckDB's, anyone's: unknown, not Iceberg's
        for key in [
            "wh/data/x=1/a.parquet",
            "wh/data/data_0.parquet",
            "lake/data/events/day=1/f1.parquet",
        ] {
            assert_eq!(part_of(key), None, "{key}");
            // (still a file a table's log can claim)
            assert!(may_be_data(key), "{key}");
        }
        // Hudi's and Paimon's under a data/ folder are their own
        assert_eq!(
            part_of("lake/data/t/dt=1/3b2a-0_0-21-23_20261112093000123.parquet"),
            Some(Part::HudiData)
        );
        assert_eq!(
            part_of("lake/data/p/dt=1/bucket-0/data-6f1a-0.parquet"),
            Some(Part::PaimonData)
        );
        assert!(!may_be_data("logs/2026/a.json") && !may_be_data("out/a.parquet"));
        assert_eq!(data_of(Format::Delta), Some(Part::DeltaData));
        assert_eq!(data_of(Format::Lance), None);
    }

    /// An Iceberg metadata file's version is the number its name starts
    /// with (`00003-<uuid>`) or `v<n>`; a name that is a UUID alone has
    /// none, even when the UUID's first group is all digits.
    #[test]
    fn an_iceberg_metadata_files_version_is_its_number_not_its_uuid() {
        let v = |name: &str| match part_of(&format!("t/metadata/{name}")) {
            Some(Part::IcebergMetadata(v)) => v,
            other => panic!("{name}: {other:?}"),
        };
        assert_eq!(
            v("00003-6c5e0a1b-1f2e-4d3c-8b7a-0123456789ab.metadata.json"),
            Some(3)
        );
        assert_eq!(
            v("00000123-6c5e0a1b-1f2e-4d3c-8b7a-0123456789ab.metadata.json"),
            Some(123)
        );
        assert_eq!(v("v7.metadata.json"), Some(7));
        assert_eq!(
            v("12345678-aaaa-bbbb-cccc-0123456789ab.metadata.json"),
            None
        );
        assert_eq!(
            v("6c5e0a1b-1f2e-4d3c-8b7a-0123456789ab.metadata.json"),
            None
        );
        assert_eq!(v("0003-x.metadata.json"), None);
        assert_eq!(
            crate::fault::commit_slot(Some(
                "t/metadata/12345678-aaaa-bbbb-cccc-0123456789ab.metadata.json"
            )),
            None
        );
    }
}
