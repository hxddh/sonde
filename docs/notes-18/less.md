# 18.0 stream "less": notes for the merge

## Changelog lines

### Changed (breaking: a major, D-138)

- **Fewer commands: 19 → 14 in `sonde --help`.** `diff` and `replay` are `compare` (`compare A B` sees a session
  and its replay; `compare FILE --replay E` replays); `watch` is `proxy --rotate/--alert`, and `watch --logs` is
  `import s3://bucket/prefix [--follow]`; `export` is `share --otlp`; `du` and `mpu` are `bucket size`,
  `bucket uploads`, `bucket abort-uploads`, `bucket lifecycle`. Each old command still runs in 18.0 and says its new
  form on stderr; they go in 19.0.
- **One safety model.** Read-only mode is gone: the setting, the toolbar lock and "Writes enabled" badge, the
  palette's toggle, the terminal header's chip, and its refusal of trials, replays and probes. What a recorded
  program may do is the recording policy alone; sonde's own destructive acts stay a dry run, a typed confirmation
  and an audit-log entry. A `config.toml` with `read-only` still loads and says it is ignored.
- One way to set the recording policy: `--policy`, with `--allow-writes` its one short form. `--no-delete` and
  `--read-only` are refused, naming `--policy …`.
- Gone: `share --json` (`show -f json`), `--log-out`, the request log's SQLite persistence (and rusqlite with its
  advisory). `--rate` and `--concurrency` are no longer listed (still taken; `config.toml` has them).
- Settings 15 → 7: recording policy, theme, language, contrast, name the code, price, currency. Requests in flight,
  requests per second and prices by class are `config.toml`'s; `reqlog-capacity`, `reqlog-persist` and
  `stale-upload-days` (now 7, as the lifecycle rule) are read and ignored, said on stderr.
- Findings 16 → 11: `obs.retried` (retrying is right; throttling and the errors chart say the rest), `obs.slow`
  (the latency charts), `obs.races` (the operation says it), `obs.injected` (a trial's run now says so first, as
  the app's trial notice does), `obs.shared` (said once among the operations' notes) are no longer findings;
  `obs.prefixrate` is a warning from 80% of S3's rate (or past half of it once throttled), no 50% note.
- One index per sessions folder (`.index.jsonl`) holds each session's counts, trial verdicts and exit code, in
  place of `<name>.summary.json`, `<name>.trials.json` and `<name>.exit` beside each; those are read once and moved in.
- One set of words: `show -f json` writes `operations`, `findings`, `requests` (were `episodes`, `observations`,
  `records`); `show --summary` writes `findings` and `waiting.waitingMs`. The app's "What stands out" is "Findings"
  (发现); the PR comment's details too.

## Decision rows

| | Question | Options | Chosen |
|---|---|---|---|
| D-167 | Read-only mode and the recording policy | both; one | **The recording policy only.** sonde's own destructive acts keep the dry run, typed confirmation and audit log; probes and replays run on what the user names. |
| D-168 | A folded command | remove; alias for one version | **An alias for 18.0** that says its new form on stderr; gone in 19.0. |
| D-169 | `watch --logs` | `proxy`; `import` | **`import s3://… [--follow]`**: one verb reads logs; one pass without `--follow`. |
| D-170 | `mpu` under `bucket` | `uploads [abort\|lifecycle]`; four siblings | **`size`, `uploads`, `abort-uploads`, `lifecycle`**, flat; `scan` and `abort` kept as aliases. |
| D-171 | What sonde keeps of each session | files beside it; one index per folder | **`.index.jsonl` per folder**: an append-only log under a lock, compacted when mostly stale; hidden, so a listing is its sessions. |
| D-172 | A setting gone from `config.toml` | an error; ignored | **Read, ignored, said on stderr**, dropped at the next save: an older file always loads. |

## Not done here, and why

- **Activity › Requests** (sonde's own requests) is still there: removing it touches `activity.tsx`, `store.ts`,
  the palette and the "show the failing request" path of every error toast, all the interface stream's. Its
  settings are gone; the in-memory log stays for the CLI's error lines.
- JSON fields inside a finding or an operation (`episodes`, `records`, `busyMs`) keep their names: the app, the
  viewer and the bridge share those types.
- Internal names (`Episode`, `Observation`, `busy_ms`) stay.

## For the interface stream (UI)

- Remove Activity › Requests (`activity.tsx` `Requests`, `screens/Requests.tsx` once `RecordDetail` moves,
  `ACTIVITY_TABS`, `focusRecord`/`showRequest` to the session inspector or a toast), then `api.requests`,
  `exportRequests`, `clearRequests` in `sonde-nsh`, and their locale keys.
- The Sessions place lists runs: "Runs" is its word for a recording, "session" the file (plan §6).
- The Run screen's policy picker is now the one safety control; its hint can say sonde's own deletes are
  confirmed by typing.
- `obs.unfinished` can link to Buckets › Incomplete uploads (`sonde bucket uploads`).
