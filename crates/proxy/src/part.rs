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

    /// A data file: only a guess from its name, taken beside its table's log.
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

/// The format part a key names, from its last two path segments. A data
/// file's name is only a guess: the analysis takes it when the run also
/// touched that table's log.
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
                    .or_else(|| s.split('-').next().and_then(|n| n.parse().ok()));
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
            // data files: under …/data/ next to metadata/ (Iceberg) or _delta_log/ (Delta)
            if name.ends_with(".parquet") && key.contains("/data/") {
                Some(Part::IcebergData)
            } else if name.ends_with(".parquet") && name.starts_with("part-") {
                Some(Part::DeltaData)
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
