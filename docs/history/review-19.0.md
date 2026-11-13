# 19.0 reviewed, 20.0 proposed

A review of 19.0.0 after its release, by four readers who did not write it: one hunted bugs with the released binary
against moto 5.2.5 and `run --demo`; one walked the README as a newcomer, timed, and measured a million requests; one
judged all 194 pictures of CI's tour (wide, least, dark, Chinese) against `app/DESIGN.md`; one looked at the project's
health, its scope and 19.0's "Done when". The plan it is held to is [review-18.0.md](review-18.0.md#190).

**Confirmed** means run and seen; **read** means from the code only. The four marked ✓ were confirmed again by hand.

## In short

The loops close for a newcomer: install, `run --demo`, two planted wastes named with their line and fix, the fix
confirmed (`✓ fixed … saved 20 requests (predicted 20)`) in 44 seconds. Trials found real bugs (a body cut short, a
stale list, a stall with no timeout), and `--trial --again` re-ran only those in 9 s. Twelve parallel runs keep twelve
sessions. `show` of a million requests: 1.38 GiB, 7.5 s, under 17.0.

What did not hold — each a verdict sonde gives that is not true:

1. **"Fixed" and "new" for a job that did not change** when its keys carry an hour, a Hive partition
   (`hour=10`), a short run id or a counter ✓. Most scheduled jobs write such keys.
2. **`check` passes a failed program**: `check --program-exit 3` exits 0 "within budget" ✓; `check` alone checks the
   newest session of any program, so a run that exited 3 with 0 requests passes another program's budget.
3. **`compare` calls a regression "less"** (a prefix listed 92 → 216 times) and leaves out throttling 649 → 1,452.
4. **Trials blame the program for the SDK and the store**: boto3 aborting its own upload is `refused` under the
   default policy; the same program under the same faults breaks on `--demo` and survives on moto, unsaid.

And the surface 19.0 froze is larger and less settled than a contract can carry: 14 commands, ~158 flags, 18 schemas
with ~600 properties, display text among them, and "operation" meaning both a program's operation and an S3 call.

## What 19.0 got wrong

### Verdicts

| | Where | What |
|---|---|---|
| High | `profile.rs` `stable()` | Keys with `year=/month=/day=/hour=`, `dt=2026-11-13-21`, a time folder, `job-3fa2b1c9`, `run-42` are not normalised: an unchanged run reads `requests 21 (no change)` beside `✓ fixed … saved 19` and `! new`. Confirmed ✓. |
| High | `check` | `--program-exit 3` with a budget: exit 0, `"passed": true`, markdown "within budget ❌" ✓. The session's recorded exit is ignored. Without a file, `check` takes the newest session of any program, not `[program] name`. Confirmed. |
| High | `compare` | A finding listed 92 → 216 times reads `↓ less … saved 0 requests (predicted 91)`; throttling 649 → 1,452 is not among its findings; "4 still open" names none. Confirmed. |
| High | `no-delete`, program identity | A program is its full command line: `etl.py --date 2026-11-14` is refused writing what `--date 2026-11-13` wrote, and the program exits 1 only because it is recorded. Adding a `{trial}` argument loses "since the run before". Confirmed. |
| Medium | trials | boto3's own `AbortMultipartUpload` of an upload it created in the same recording is `refused`, with advice to add `{trial}` that is already there; with `--allow-writes` it is `broke`. "The program did not break" while two scenarios were refused. Confirmed. |
| Medium | trials, `--demo` | `lost-answer` and `complete-500` break on `--demo` (a retried Complete is 404) and survive on moto; `docs/ci.md` recommends `--demo` for CI. Confirmed. |
| Medium | `--set-baseline`, `check --init` | A failed run is accepted as the baseline and as `--init`'s source (`max-errors = 1`). Confirmed. |
| Medium | `fixloop.rs:405` `take()` | Runs finishing out of order lose findings: 6 at once kept 3 of 6. Confirmed. |
| Medium | `import s3://` | An upload started in one pass and completed in the next is warned unfinished; `--follow --alert` alerts on it. Confirmed. |
| Low | `obs.headget` | Its identity has no subject: every HEAD-then-GET is one finding; `expect` cannot name a place. Read and confirmed. |
| Low | words | A "less" finding back over its threshold is silent; imported logs show a waiting what-if they say is unknown; the serial-requests saving does not add up (32 ms vs 168 ms). Confirmed. |

### Edges and safety

| | Where | What |
|---|---|---|
| Medium | `import`, `show` | **Key names reach the terminal raw**: ESC and BEL from a key planted in an access log set the window title. `share --html` escapes. Confirmed. |
| Medium | `compare --replay` | A recorded CreateBucket is replayed outside the prefix and left behind, after "nothing outside it is touched". Confirmed. |
| Medium | `run` | Unknown flags before `--` are taken as the program: `run --demo --typo -- true` exits 70 and writes `…---typo.sonde` ✓. No endpoint exits 70, not 64. |
| Medium | `run -o` | Two runs with one `-o`: the second prints the first's report, its own recording lost; an existing file is overwritten silently. Confirmed. |
| Low | | `show -f json` with no findings has `whatIf: null` against its schema; a truncated CloudTrail exits 64, not 65; SIGTERM says 137, exits 143; a command that cannot start leaves an empty session `show` then picks; `compare` alone refuses after a program's first run; keys with `..` refused by the proxy. Confirmed. |
| Low | `check --init` | Prints `uses: hxddh/sonde@v18` (`check.rs:207`) ✓. |

### The interface

| | Where | What |
|---|---|---|
| High | least window | The session pane breaks: "What / to / chan / ge", Group by off the pane, "Request / s", "Incompl / ete up…"; the header takes 40% of the height; header actions become unlabelled icons. Three tiles abreast get the one-line layout (`kit.tsx:553`). Seen. |
| High | Time charts | 12 of 15 sentences say how to read the chart, not what it shows (`profile.rs` hints, `ui/profile.tsx:296-479`); a picked stretch replaces the sentence with its range. Every chart stays; their sentences must answer. Seen and read. |
| High | trials | sonde's own fault answers are red on the timeline, the operation rows and "Errors 8 of 8"; the legend calls them teal. Red says the store failed, which is what did not happen. Seen. |
| Medium | session list | Every clean run wears a red "Broke": its trials' verdict on a run that exited 0. Rows read alike: `tellApart` keeps the end and the column cuts it. Seen and read. |
| Medium | compare | Figures marked too few are coloured (+43% red); equal figures read "a → a"; the title repeats one minute. Seen and read. |
| Medium | words | Session tiles say "No request failed" when 67% found nothing; a finding says "read 11 times" where the table says "12 lookups, none found"; one cost written "<0.01" and "0.0072" on one pane. Seen. |
| Medium | Buckets | "Abort uploads…" is the loudest control, "Add abort rule…" the fix is outline; the dialog is "Confirm … Run … 2 item(s)"; "Size: **Measure**". Seen. |
| Low | | An empty slice can become the stretch and dims every chart; the "since" tally counts new and less but lists fixed and back; Chinese splits "5.7 / ms"; middle cuts keep the wrong ends. Seen. |

### Health

| | Where | What |
|---|---|---|
| High | `release.yml` | `publish` waits for nothing of CI: 19.0.0 started in the second CI did, reused nothing, rebuilt 10 targets in 1 h 23 min, and published while "Action live" was red. Confirmed. |
| High | Action | Its only live run failed: the 19.0 script ran against release 18.0.0 (`--program-exit` unknown) ✓. `@main` or a SHA installs `latest`. |
| Medium | contract | 18 schemas, ~600 properties, ~530 required; phrase arrays and `layout`'s display tables are under the promise. `operation` means two things (`operations[]` vs `requests[].operation`, `--by operation` vs `run`'s "kind"). `config.toml` is outside it though edited by hand. Confirmed by count. |
| Medium | CI | 28 of 194 pictures exempt as live; pictures approved for Linux only; macOS and Windows tours never run. Budgets are wall-clock, retried once. Confirmed. |
| Medium | signing | macOS unsigned and not notarized; Windows has no Authenticode and the notes do not say so. Needs the owner's secrets. |
| Low | docs | Memory for a million requests is said three ways (2 GB, 1.5 GB, 1.1 GiB); `compare` of two measured 1.8 GiB against 1.16 documented; binary 79 MB against 55.7. `config.toml` and `sonde.toml` overlap (`record-policy` vs `[record] policy`) with no page saying which wins. Confirmed. |

## What holds

- The newcomer path, end to end, without an account; moto works as documented.
- The fix loop on stable keys, failed runs not compared, fixed/back in sequence, savings against the prediction.
- `--trial --again`; trials finding real bugs; refusals told.
- Concurrent runs keep their sessions; closed stdout, Ctrl-C, SIGSEGV keep their codes; `endpoints add --replace`,
  `prune` and `layout --kind` dry runs; Unicode keys; `share --html` escapes.
- `unsafe` forbidden, 10 fuzz targets on every parser, actions pinned by SHA, provenance and `SHA256SUMS`, 650 tests.

## 19.0's "Done when"

| | Verdict |
|---|---|
| Install → `run --demo` → a fix confirmed in ten minutes, no account | Met (44 s). |
| Keys that change every run read "no change" | Met for ms timestamps and UUIDs; **not** for hours, partitions, short ids, counters. |
| `@v19` installs 19.x; PR CI under 30 min | Met (24–28 min); the live Action check failed. |
| No command, flag, file or word of an older version | Met; `run` lets any flag through. |
| Every picture approved, nothing overlaps at the least window | Partly: 28 exempt, Linux only, the least window breaks. |

## 19.1, then 20.0

The contract says a break comes in a major and is announced one version ahead. So:

**19.1 (now, no break):** the verdicts and safety above — they are bugs, not changes of contract — and the
announcement of what 20.0 breaks, said by the commands it touches.

**20.0: every verdict true, one way to name things, a promise sonde can keep.** No new command, format or screen.

### 19.1 — what sonde says is true (S)

- Identity: normalise hours, `key=value` partitions, short ids and counters, with tests from real lakehouse layouts
  (Hive, Delta, Iceberg, Spark `part-` files); a finding that only moved key is "no change". HEAD-then-GET carries its
  object's prefix.
- `check`: fails a program that failed (its recorded exit, or `--program-exit`); without a file, checks the newest
  run of `[program] name` (else of the program in this directory), never another's; refuses a failed run as baseline
  or as `--init`'s source; prints `@v19`.
- `compare` and `run`: direction from the counts (216 > 92 is "more"); throttling and every error class compared;
  "still open" names them; a "less" back over its threshold is "back".
- Trials: a program may abort, overwrite or delete what it wrote in the same recording; a verdict that depends on the
  store's retry semantics says so; "did not break" only when every scenario was judged; `--again` keeps the flags.
- Safety: control characters in keys escaped everywhere text reaches a terminal; replay never creates a bucket;
  `run` refuses unknown flags (64) and a missing endpoint is 64; `-o` refuses an existing file without `--force`; no
  empty session when the command cannot start; 137/143 one code.
- Fix loop under concurrency: `take()` merges runs that finish out of order.
- `import`: uploads across passes; a truncated CloudTrail exits 65.
- Release: `publish` waits for CI's green run of the same commit and reuses its builds; "Action live" runs after a
  release; the Action refuses `latest` for a branch or SHA ref.

### 20.0 — 1. One name for a kind of request, one for a program (M)

- `operation` is a program's operation; an S3 call is a **kind** everywhere: `requests[].kind`, `show --by kind`,
  session format 2 (19.x reads 1 and 2; 20.0 writes 2).
- A program is its name, else its script and subcommand, not its full command line: arguments that change each run
  (`--date`) keep one program, one history, one `no-delete` ledger.
- One configuration story: what sonde keeps (`config.toml`: endpoints, prices) and what a project says
  (`sonde.toml`: program, check, record, scenarios); a setting lives in one of them; `config.toml` joins the contract.

### 2. A smaller promise (S)

- Out of the contract, said in `--help` and the schemas: display text (`*Text` phrases, `headline`, `facts`),
  `layout`, `probe`, `bucket`, `compare --replay`, `tui`'s developer flags.
- `sonde demo` folds into `run --demo` and `tui --demo`.
- One confirmation idiom (`--yes`, or `--confirm` the count where data is lost); `--allow-writes` is
  `--policy allow`; every flag has help; `-f` lists only what a command writes.
- The finding identity's normalisation is versioned, so `expect` and kept findings survive its improvements.

### 3. Run, Time and trials say what they show (M)

- Every Time chart keeps its place, shown; each sentence is computed from the data ("2 failed, both NoSuchBucket, in
  the last 10 ms"); how to read it moves to the legend or tooltip; a picked stretch keeps the sentence and says its
  range beside. A slice with no requests is not a stretch.
- sonde's own answers are one teal everywhere: timeline, rows, Errors tile.
- A run's outcome and its trials' verdict apart: a clean run is never "Broke"; trials read "2 of 5 broke" quietly.
  Runs told apart by what differs, cut from its start.
- "Few" and "same" are honest: no colour for figures too few to tell; one figure, not "a → a".
- Misses are said beside errors in the session header; one writing for one cost on a pane.

### 4. The least window (S)

The one-line tile layout only one per row; no heading breaks inside a word; toolbar extras behind ⋯; the session
header folds to one line as the pane scrolls; Run's output pane yields to its result. Chinese keeps a number with its
unit.

### 5. CI that proves it (S)

- The live screens held by a tolerant mask (the measured figures masked), not exempt; macOS and Windows tours run
  before a release, and their pictures are approved.
- Budgets measured by instructions or a cgroup's peak, not wall-clock with a retry.
- One memory figure, measured, in one place; `performance.md` remeasured at 20.0.

### Not in 20.0

New commands, decoders or screens; a sidecar, `/metrics`, a query language, an MCP server; splitting `replay` out of
`compare`; renames beyond kind and program; custom rules; in-test fault toggles; SDK callers beyond boto3, JS v3 and
Java 2; S3 Express, SigV4a, HTTP/2. Signing needs the owner's secrets (Developer ID, notarization, the package key, a
Windows certificate); the release says what it skipped, Windows included.

## To decide

- **`tui`**: a fourth interface beside the app, the share site and `--html`. Keep it outside the contract, or remove it.
- **`bucket`** (sizes, uploads, lifecycle, aborting uploads): store hygiene that can change data, not program
  behaviour. Keep it outside the contract, or remove it.
- **`probe`** (S3-compatibility tests, 5.7k lines): a separate product. Keep it outside the contract, or move it out.

## Done when

- An hourly job writing `hour=` partitions, run twice unchanged, reads "no change"; run with `--date` changed, one
  program.
- `check` of a run that exited 3 fails; of another program's run, refuses; `compare` of 92 → 216 reads "more".
- A boto3 upload under `lost-answer` is judged, on `--demo` and on moto alike, or says why they differ.
- `run --typo` exits 64; a key with ESC prints escaped.
- `requests[].kind` and `show --by kind`; `operation` means only a program's operation.
- The release waits for CI; every picture approved on Linux, macOS and Windows, live ones masked not exempt; nothing
  breaks at the least window; every Time chart shown, each sentence an answer.
