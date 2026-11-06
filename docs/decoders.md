# Layout decoders

`crates/decoders` reads layouts through a read-only `Source` (S3, a local directory, memory) and produces a
`Layout`: facts, issues (info, warning, error), a timeline, tables and snapshots of the logical state over time
(replay: `Layout::state_at`). Format versions are checked strictly: an unknown version is reported as such and
not decoded — sonde never guesses.

| Layout | Detected by | Decodes | Format versions |
|---|---|---|---|
| **SlateDB** | `<root>/manifest/<20 digits>.manifest` | Manifests (a big-endian u16 version, then FlatBuffers `ManifestV1` / `ManifestV2`, read with a bounds-checked FlatBuffers reader): writer and compactor epochs, fencing events (epoch changes between manifests), `replay_after_wal_id`, `wal_id_last_seen`, L0 SSTs and sorted runs (V2: views over `ssts`), checkpoints, external databases. WAL SSTs (`wal/<id>.sst`): timeline, gaps (an error above the replay point: entries lost), GC lag (WAL SSTs already in L0 and not collected, their age). Compacted SSTs (`compacted/<ULID>.sst`, creation time from the ULID): live, kept by a checkpoint, orphan, or missing. `compactions/` state files counted. One snapshot per manifest. | Manifest v1, v2 (SlateDB ≤ 0.18). Tested on databases written by SlateDB 0.18.0 (`tools/gen-slatedb`). |
| **Apache Iceberg** | `<root>/metadata/*.metadata.json` | The current metadata (`version-hint.text`, else the highest number), snapshots and refs, each snapshot's manifest list and manifests (Avro, with Snappy / zstd): live data and delete files, records, bytes; orphan data files (under `data/`, in no snapshot); snapshots eligible for expiry (`history.expire.max-snapshot-age-ms`, `min-snapshots-to-keep`, refs) and the bytes expiring them frees; missing files. One snapshot per Iceberg snapshot. | `format-version` 1, 2, 3. Tested on tables written by pyiceberg 0.9.1 (`tools/gen_iceberg.py`). |
| **lakeFS** | `<root>/_lakefs/dummy` | Graveler's Pebble SSTables in `_lakefs/` (a LevelDB/RocksDB table reader: footer, index, Snappy blocks): metaranges (range ids, key bounds, counts) and ranges (path → physical address, size, ETag, mtime); data objects referenced by no committed range. Commits, branches and tags live in lakeFS's KV store, not in object storage: each metarange is shown as one committed tree. | RocksDB-format table (magic `0x88e241b785f4cff7`) and LevelDB; block compression none or Snappy. Tested on a namespace written by lakeFS (quickstart). |
| **Milvus Woodpecker** | `<root>/<log>/<segment>/<block>.blk` | Per segment: blocks (`n.blk`), compacted blocks (`m_n.blk`), `footer.blk` (index records, footer: blocks, records, version, compacted flag, LAC), `write.lock`; record CRC32 checked; active segments read from their blocks' headers (a ranged GET of 128 bytes); block-id gaps. Timeline of segment starts and seals. | Record format versions 5 and 6. Tested on objects encoded with Woodpecker's own codec (`tools/gen-woodpecker`, module at commit 3a8ce69). |
| **turbopuffer**, **WarpStream** | Name patterns (`tpuf/`, `turbopuffer/`, `warpstream/`, `…/_segments/`) | **Not decoded: the formats are not published.** Prefix statistics and write cadence from the listing only. | — |
| **Generic** | Any prefix (on request) | Objects and bytes by sub-prefix, writes per interval, median and p99 intervals, longest pause. | — |

Plugins add lenses in TSX (see [plugins.md](plugins.md)); the example `plugins/delta-lens` reads Delta Lake's
`_delta_log`.
