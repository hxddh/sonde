# 17.0 M3: memory — notes for the merge

## Changelog

### Added

- `sessions` marks a session with a bad line in its middle (`!`, the line on stderr, `unreadable` in JSON).
- Exit 65 for a session that is not one (was 70).
- The budgets test compares two runs of a million.

### Changed

- **Memory, not only output.** About 1.1 KB a request (1.75; 1.4 with 1 000-character keys, was 4.5): `show` of a
  million 1.1 GiB (1.74), of two million 2.2 GiB (3.4). Budget: 1.5 GiB (2).
- **`compare` and `diff` read one session at a time.** Two runs of a million: 1.16 GiB (4.74); of two million
  2.25 GiB (9.4).
- **"Waiting" is one measure**: a kind's waiting in `compare` is its share, as in `show`.
- `compare` counts the kinds it leaves out.
- The summary cache is stamped before reading, in ns; a run's figures are keyed by the analysis version.

### Fixed

- A `.sonde` whose gzip stream is cut is read up to the cut, with a warning.
- A session rewritten while counted kept the old counts.
- Kept warnings outlived a change to the analysis.

## Decisions

| Topic | Options | Choice and why |
|---|---|---|
| Lean headers | drop list; keep list | **Keep list** (`session::read_by_analysis`): a drop list misses checksums and metadata. A test reads the analyses' sources for header names. |
| Repeated text in a record | `String`; `reqlog::Text` | **`Text`** for operation, method, endpoint, bucket, region, query, key; keys interned per read. File format unchanged. |
| An operation's English title | stored; derived | **Derived** (`Episode::title()`); phrase arguments are `Text`, sharing the request's target. |
| `compare` | both runs whole; one at a time | **One at a time** (`profile::Run`); `diff` keeps the replay as `replay::answers`. |
| Waiting by kind | in-flight time; share | **Share** (`waited_ms`). `groups[].busyMs` keeps its name until M1 renames it. |
| Cache key for figures | cache version; sonde version; analysis version | **`summary::ANALYSIS`** = sonde version + a number; a fingerprint test over the fixtures fails when figures change and the number does not. Counts keep their own version. |
| Cache stamp | after reading; before | **Before**, mtime in ns. |
| Bad line | skip; refuse | **Both commands tolerate a torn last line and flag a middle one**: `show` 65, `sessions` marks it. |
| Cut gzip | refuse; read to the cut | **Read to the cut, warn.** |

## Not done

- Ten million in 8 GB: 8 GB holds about seven million. The rest is `Record`'s shape (472 bytes, mostly empty
  fields); changing it belongs with M1's format version.
- Times were measured with other builds running (load 3–7); the budgets test alone: a million in 11 s.
