# 15.0 reviewed, 16.0 proposed

A review of 15.0.0 after its release, by three readers who did not write it: the Rust hunted for bugs with the
released binary against a fake store; the CLI used as a newcomer would use it, and sessions of ten thousand to a
million requests measured; every screen looked at (94 of 122 pictures, four variants) and the interface counted
against its own standard. Then what 16.0 should be. 15.0's own review is [review-14.0.md](review-14.0.md).

Marked **confirmed** where it was run and seen, **read** where it is from the code only.

## What 15.0 got wrong

### The verdict is still wrong in the form the docs recommend

| | Where | What |
|---|---|---|
| High | `proxy/trial.rs` `judge` | **With `{trial}` or `SONDE_TRIAL` in the key, a program that never retries is "survived".** No request of the trial has the clean run's identity (the key differs by the trial's name), so every one falls to the old rule: any later answer is the retry. A program that writes `state` twice and swallows errors: `broke` with a fixed key, `survived · 1 fault, retried · after 205 ms` with `etl-{trial}`, its own sleep printed as backoff. Confirmed twice. `docs/trials.md` recommends exactly this form. |
| Medium | `trial.rs` | A request the store itself fails in the clean run is counted as meant and can never be answered: boto3 with retries on, against a store that answers `DeleteObjects` 501, is "broke · 2 writes failed and were never sent again". 501 is owed a retry here and is not retryable in `episode.rs`. Confirmed. |
| Medium | `trial.rs` | A loop that polls for a fixed time, with the SDK retrying, is "degraded · gave up on 1 failed read": the backoff used the window. The reverse (a loop that polls more often in the trial reads as retried) is read. |
| Low | `trial.rs`, `record.rs` | The wait is measured to the next record of the identity whoever sent it (16 threads on one key: "after 4 ms to 901 ms (jittered)"); the same fixed 10 ms backoff read "jittered" once and "in step" once; "written twice" is said of deletes; `--fault x --fault x` runs it twice into one file; a scenario may be named `clean`; an interrupt kills the direct child only (a supervisor's SIGINT leaves a grandchild running). Confirmed, the deletes read. |

### Elsewhere in the Rust

| | Where | What |
|---|---|---|
| Medium | `proxy/session.rs` | **The 2 GiB gzip limit covers `import` only.** A 14 MB `.sonde` holding one 3 GiB line aborts `show` and `check` (`memory allocation of 4258004992 bytes failed`). Confirmed. |
| Medium | `proxy/lib.rs` | **Dot segments are refused in the key only.** `GET /%2e%2e/demo` is recorded as GetObject `../demo` while the store served a listing of `demo`; `/./demo/k` is recorded as bucket `.`. The record, the fault identity and the prefix rules name another place than the store saw. Confirmed. |
| Medium | `proxy/replay.rs` | Replay removes what it wrote with `DeleteObjects` only: on a store without it (the built-in one answers 501) every replay ends "Removed 0 objects", exit 70. What remains is counted twice (7 for 4 versions and a marker); `removed` adds the batch size whatever the body's per-key errors say. Confirmed, the last read. |
| Low | `proxy/signals.rs` | A retry is any later record of the identity after a retryable failure, with no time bound: a 503 and a read an hour later is "1 retry"; five polls of a key that always answers 503 are "4 retries"; a 408 is never one. One request reads "rate 500 req/s", "1 requests"; bytes read `585.9 PiB` on one line and `600000.0 TiB` on the next. Confirmed. |
| Low | `proxy/profile.rs` | Requests sonde answered itself are priced: a trial's six injected 503s are billed. Confirmed. |

### The CLI, used

| | Where | What |
|---|---|---|
| Medium | `sonde demo`, the built-in store | **Every answer with a body on a kept connection takes 41–46 ms** (1 ms on a fresh one): the store, not the proxy, by the look of it Nagle against delayed ACK. The tour's first finding, "19 requests one after another… 8 at once would take 44 ms", is that artifact. Confirmed. |
| Medium | `sonde import` | An access log's times are whole seconds, so a second's requests all start at once: the same traffic is "36 at once" recorded and "108 at once" imported, and the throttling finding gives advice built on it. Confirmed. |
| Low | `show` | The same fact two or three times, in different forms ("10004 requests" and "10,004 requests", "146.1 s" and "2m 26s"); two "received" figures two lines apart that differ; the sparkline stretches few bins unevenly, is 32 full blocks for any long steady run, and does not say which percentile it draws; "acting on 1 of 2 findings" with four listed. Confirmed. |
| Low | `check` | `baseline = "auto"` with no baseline yet is a usage error and the budget is not checked, so the documented `sonde.toml` fails a first CI run; a baseline failure closes "over budget"; `-f github` repeats the baseline sentence on every row; with trials only, `-f json` has an inner `passed: false` with nothing checked. Confirmed. |
| Low | several | A trial's session is not found by its name (it needs `trials/…`, which no page says); `docs/trials.md` says in one place that the app cannot start a trial; a program's name is not used by `share` or as OTLP's `service.name`; `show --errors -f json` ignores `--errors`; `sessions` prints the whole command (230 columns); a HAR entry for another host is dropped uncounted. Confirmed. |

## Large sessions, measured

Nobody had measured past a few thousand requests. Sessions of a 16-worker lake job, generated: 10 thousand, 100
thousand, 500 thousand and 1 million requests (1.2 to 124 MiB gzipped). Wall time and peak memory, 4 cores:

| | 10k | 100k | 500k | 1M | Unusable from |
|---|---|---|---|---|---|
| `show` | 0.4 s, 86 MiB | 4.0 s, 741 MiB | 20 s, 3.7 GB | 45 s, 7.3 GB | about 250k |
| `check --baseline` | 1.0 s | 12.4 s, 1.1 GB | 50 s, 5.5 GB | 99 s, 11 GB | about 85k |
| `show -f json`, `share --json`, `export` | 1 s, 0.3 GB | 8–11 s, 2.6–4 GB | cannot (over 12 GB) | cannot | 50–75k |
| `share --html` | 2.8 MB page | 32 MB page | 163 MB | 328 MB | the page, long before the time |
| The app's view of a session | 3.5 MB | 35 MB | 175 MB | 351 MB, 11 GB in Rust | about 300k, by these figures |
| `sessions`, one such file in the folder | | | | 24 s, 4.8 GB, every call | |

On an ordinary workload everything is linear: 45 µs and 7.3 KB of memory a request. Two shapes are not:

| | Where | What |
|---|---|---|
| High | `proxy/episode.rs` `retries` | **Quadratic in an outage.** Each retryable failure scans the next 60 s for its retry and, when the program gave up, never finds it. Half the requests answered 503 and none retried: `show --summary` takes 18.6 s at 30 thousand requests and 374 s at 100 thousand. An outage is when someone opens the session. Confirmed. |
| High | `proxy/episode.rs`, the small-multipart finding | **Quadratic in small uploads**: a search of all records for each id of each small upload. One-part multipart uploads, the README's own example: 22 s at 100 thousand, 207 s at 300 thousand. Confirmed. |
| High | `sonde-nsh/sessions.rs` `view_of` | The app's view carries every operation (460 bytes each, title twice) as a JSON tree that is cloned on every open. The 4000 of D-080 caps the dots only. Confirmed by rebuilding the view. |
| Medium | `session.rs`, `fixes.rs`, `record.rs`, `sessions.rs` | Open keeps every line as text while it parses (4.8 GB for a 124 MiB file); `what_if` clones every record to add up a time (a quarter of `show`); JSON outputs are built whole, then printed whole; `check` opens the run twice and up to thirty earlier runs in full; `sessions` parses every session on every call; `show` reads the operations twice. Confirmed. |

## The interface

The owner's standing complaint holds: "still rough and disordered in places". It is concentrated in three places —
the Time pane's charts, how a pane reflows at the least window or beside an inspector, and a few tables set by hand.

| | Where (picture) | What |
|---|---|---|
| High | `*-03d-filtered` | **A finding's advice is shown under another finding.** "What to change" is appended after all the rows and scrolled to: finding 2's advice stands under finding 3's title. At the least window the operations table under it is its header and no row. |
| High | `min-04c-object` | Bucket tiles beside the inspector cut their label and their figure ("Incomp…", "3 · 17…"), which the standard forbids. |
| High | `wide-03g`, `03h`, `03k`, `03m` | **Three time axes in one pane** (`+0/+230/+460`, `+0/+229/+462`, `+0/+200/+400`, and one without `+`); 15.0's changelog said one. The first chart has no value scale and no legend for its two colours; its three finding marks pile up as "2 3 1". Almost no chart has a value axis, so "how many" needs a hover. |
| Medium | `wide-02d` | The latency histogram labels each bar with the cumulative share: the tallest bar reads "67%". |
| Medium | `wide-02c`, `wide-03` | A run of nine requests in a third of a second is shown as rates ("27/s", "9.0 MiB/s") with sparklines of stray ticks; rate, bytes and in flight draw one shape three times; in dark the sparklines are navy on near-black. |
| Medium | `wide-03o`, `wide-02c` | Status codes is a table drawn by hand: no headings, an unlabelled column, rows that read only "404". A chart of two bars stands in a 300 pt card stretched to its neighbour's height. |
| Medium | `wide-03h-runs` | Compare says each fact twice (tiles, then a strip of figures), shows "1.4 ms → 1.4 ms −5%", and is green throughout though it says the two programs do not compare. |
| Medium | `wide-02b`, `min-02b` | Run's actions wrap ragged, right-aligned, over three rows. |
| Medium | `wide-04d`, `zh-04d`, `wide-05` | Table headers and dates are cut while room stands empty beside them ("Bytes h…", "2026年8月1…"); ages read "86.0 d". The layout lint says ok: it only adds up widths. |
| Medium | every `zh-` | Chinese headings are regular weight where the Latin in them is bold, so titles lose their level. Wrong words: "2 个客户端" (two clients, for two client errors); one word for Run, Runs and the button; one for Operations and Actions. |
| Medium | `trial.tsx` (read) | The dialog's strength and limit are fixed and unsaid; "Cancel" both dismisses and stops; a finished row does not lead to its session. **No picture covers any of 15.0's trial interface.** |

Counted again, against 15.0's claim that each is at its standard and kept by a lint:

| | 14.0 | 15.0 said | Today |
|---|---|---|---|
| Renderings of a figure | 5 | 2 | 2; Compare shows both for one fact; `Figures` is dead code |
| Headings by hand | 13 | 0 | 0 bold; 26 uses of a third weight (`font-medium`) the lint does not name |
| Rows by hand | 11 | 0 | 4, and one five-column table on `Row` |
| The chart frame, pitch, edge, legend, tooltip | Time only | one each | one each; not counted: 4 time-tick schemes, 5 bar thicknesses, 5 bar radii, 7 heights |
| Layout thresholds | 5 | 4 | 4 and three more under other names |
| Writers of a length of time | 5 | 1 | 4 (`duration`, `elapsed`, `days`, `ago`) |
| Unused locale keys | 48 | 0 | 0 the lint can see; 87 of 973 keys pass only by a prefix |
| Raw base controls in pages | | 0 | 5 |
| Files too large for one idea | | | `plots.tsx` 1,922 lines, `kit.tsx` 1,297, `sessionview.tsx` 1,076 |

What a reader used to Grafana, Datadog or Honeycomb still misses: a table per operation (count, error share, p50,
p95, p99); error *rate* over time; a latency heat map over time; a value scale and grid on every chart; a trial
drawn over its clean run; the moments of the faults marked on the timeline; percentiles flagged when there are too
few requests to have them.

## Native Shell 0.5.0

Read at the tag (81 commits past the pin), not built. Plot, the table, the sheet and the terminal are unchanged:
it lets sonde delete no chart. Worth taking:

| | Use |
|---|---|
| `fileTypes`, `Application.on("openFiles")` | a `.sonde` file opens from the system: a double click, Open With, a drop on the icon |
| `Application.on("quit")` | the program being recorded and a running trial are ended, the session written |
| `terminal:` as a class variant | about 25 to 90 of the 176 `terminal()` conditionals only choose class strings |
| `initialScrollAtEnd`, a list's viewport start | a live session opens at its end and follows it; a re-read keeps its place |
| `accessibilityState.invalid`, an element's description | a refused field, a confirmation's message (sonde sets no accessibility state today) |
| Per-platform icons; the unified log on macOS | the macOS icon inset; a packaged app's logs can be had |
| Views out of sight made over frames | the Time pane's 27 plots (one call into JS passed 2 s under load in 15.0's tests): to be measured |

`onWheel` and `onMouseUp` would make drag-to-zoom possible. **Not taken**: the Time pane is a page that scrolls, so
a chart that takes the wheel stops the page; sessions fit one screen; and it is code around Plot that upstream's own
selection would make void.

Risks of the move: a column's height in a row that does not stretch it is computed anew (every picture to compare);
malformed theme values are refused; local images are checked against grants; Windows painting changed again.

## 16.0

**A verdict to trust, sessions of any size, charts that can be read.**

| | What | Size |
|---|---|---|
| **M0: 15.0.1, now** | `{trial}` and `SONDE_TRIAL` are taken out of a key before requests are matched to the clean run, with a test that runs the docs' own example. What the clean run failed is not owed. The gzip limit on every reader. Dot segments refused in the whole path. Replay falls back to single deletes and counts what remains once. | S |
| **M1: the verdict, deeper** | Two scenarios. **A body cut short**: the answer's headers and length go out, the body stops part way; recovered is a later GET that covers from the cut to the end (a resume or a whole re-read), and taking the half as whole is broke. Range stays out of a request's identity. **A lost conditional write**: `If-None-Match: *` or `If-Match` answered 412 though the clean run won; faults and the clean run are counted by commit, not by key (the next number after a 412 is its retry); for SlateDB a 412 is fencing, and stopping is right. The built-in store honours conditions and `DeleteObjects` first, or none of it can be tested. A wait is measured within one sender; a broke verdict names the request, the line of code and the SDK's setting. | L |
| **M2: sessions of any size** | The two quadratic loops made linear. A session is read as a stream, never whole as text; `what_if` adds without cloning; JSON outputs are written as they are made. The app's view pages its operations (the first screen and the totals at once, the rest on scroll), and is not cloned. `sessions` keeps what it counted beside each file. `share --html` caps its rows and says so. Budgets, kept by a test: a million requests in `show` under 15 s and 2 GB; the app opens 100 thousand under 2 s. | L |
| **M3: charts that can be read** | One time axis from one function, with a lint that fails a second. A value scale (zero and the peak) and a whole legend on every chart of counts. The histogram labelled by its counts. A card as high as its content. A short run says totals, not rates. A table per operation: count, error share, p50, p95, p99. A trial over its clean run on one axis, the faults' moments marked. | M |
| **M4: the rough places** | A finding's advice opens inside its row. Tiles and tables never cut a figure or a date (the lint measures headers and cells, not sums). Run's actions in one row. Compare says each fact once. The trial dialog says its strength, stops with a word of its own, and leads to its sessions; pictures of every trial screen. Chinese: a face with a bold weight, and four words for Run, Runs, Operations, Actions. The lints learn what they missed: the third weight, named thresholds, arbitrary sizes, keys hidden by a prefix. `plots.tsx` split by chart family. | M |
| **M5: Native Shell 0.5.0** | The pin moved to the tag, last and alone; the adoptions above. | S |
| **M6: what 15.0 left** | The built-in store answers at once (the tour's first finding is real or absent). An import from a log with whole seconds spreads a second's requests, and says so. `check` with `baseline = "auto"` and no baseline checks the budget and says there is none yet; a missing baseline names the program and, when another baseline of the same executable exists, says so. `show` says each fact once. A trial's piped input is kept on disk past a size. | M |

**Not in 16.0**: drag or wheel zoom; a new place or dashboard; a daemon; an engine without a recording; a competitor
written by sonde into the user's bucket (for a lost race, the program's own write is let through and answered 412).

The risk, named: M2 changes how every command reads a session. It is done behind the budgets' test and a check
that each command's output on the fixtures is byte for byte what 15.0 printed.
