# Changelog

All notable changes are listed here. The project follows [Semantic Versioning](https://semver.org).

## [Unreleased]

## [16.0.0] - 2026-11-11

A verdict to trust, sessions of any size, charts that can be read. The review this came from is
[docs/review-15.0.md](docs/review-15.0.md); every row of it is in this version.

### Added

- **Two trial scenarios.** `cut-body`: a download's headers and length go out and its body stops half way; the
  verdict says whether the program resumed from the cut, read the whole again, or took the half as the whole.
  `lost-race`: a conditional write (`If-None-Match: *`, `If-Match`) is answered 412 though the clean run won it;
  faults are counted by commit, so the next number is that commit's retry; a fenced SlateDB writer that stops is
  right. Both are chosen from what the clean run did. See [docs/trials.md](docs/trials.md).
- **A broke verdict says where**: the request, the line of code when callers are known, and the SDK's own setting
  to change, in the CLI, in `check` (every format) and in the app.
- **A table per operation** at the head of the Time pane and in `show`: requests, the share that failed, p50, p95
  and p99, percentiles of fewer than 20 requests marked. Error rate over time; a latency heat map; a trial drawn
  over its clean run on one axis, with a rule at each fault.
- `show --all`; `check -f json` has `noBaseline`; `summary.operations[]` has `errorShare`, `p95Ms`, `p99Ms`, `few`.
- A `.sonde` file opens from the system (a double click, Open With, a drop on the icon). Quitting the app ends
  the program being recorded and a running trial, and writes the session.
- **How hard a trial presses**: `--fault slowdown:3` or `error:3` fails a request's first three attempts in a row
  (1 to 5); the app's dialog has the same dial, and a verdict row reads `slowdown:3`.
- A live operations table follows its end. A refused field says why under itself, and says so to a screen reader.

### Changed

- **Sessions of any size.** A session is read as a stream and analysed in one pass: a million requests in `show`
  take 7 s and 1.8 GiB (45 s and 7.3 GB), `show -f json`, `share --json` and `export` are written as they are made
  (they could not run past 500 thousand), `share --html` keeps to 2000 operations and says so (328 MB), `sessions`
  reads what was counted beside each file (24 s every call), and the app's view of a session is its first 500
  operations and the totals, the rest read as the table scrolls, sorted and filtered in Rust (351 MB). Budgets are
  kept by a test in CI.
- **One time axis** on every chart over time, a value scale and a whole legend on every chart of counts, each kept
  by the chart lint. The histogram's bars are labelled by their counts. A card is as high as its content.
- **A run under a second, or of fewer than 20 requests, says totals**, not rates, and draws no sparkline.
- `show` says each fact once, in one form; a sparkline is as wide as its slices and says what a bar is.
- A whole-second log (`import`) is spread over its seconds and the session says so: no in-flight figure and no
  "one after another" finding are made of it.
- `check` with `baseline = "auto"` and no baseline yet checks the budget and says there is none; a missing baseline
  names the program and any baseline of the same executable under other arguments. `check --init` writes
  `[program] name`.
- 408 owes a retry, in trials, episodes and signals alike; 501 owes none.
- A trial's session is found by its name; a program's name titles `share` pages and is OTLP's `service.name`;
  `export --to` posts in parts.
- A trial says each fact once, in the reader's language. Capacity's sentences have their singulars and whole
  shares (`98%`). A request's time is written one way everywhere ("Nov 7, 2026, 00:22:25.074").
- Native Shell 0.5.0; the macOS icon is inset as the system's are.

### Fixed

- **With `{trial}` or `SONDE_TRIAL` in the key, a program that never retries was "survived"**: no request of the
  trial matched the clean run. The run's own name is taken out of a key before requests are matched.
- A request the clean run itself failed was owed a retry; a loop that polls for a fixed time was "degraded"; a
  wait was measured across senders; jitter was judged of waits too short to tell; "written twice" was said of
  deletes; `--fault x --fault x` ran twice into one file; an interrupt left the program's children running.
- **Two analyses were quadratic**: retries in an outage (374 s at 100 thousand requests, now 0.5 s) and the
  small-multipart finding (22 s, now 0.5 s).
- **The built-in store answered every body on a kept connection in 41–46 ms** (now 0.1 ms): the tour's first
  finding was that. It honours `If-None-Match`, `If-Match` and `DeleteObjects`.
- The gzip limit covered `import` only; one absurd line aborted `show`. Dot segments were refused in the key and
  not in the bucket's place. A replay could not remove what it wrote on a store without `DeleteObjects`, and
  counted what remained twice.
- On Windows a trial started from the app never began: the pseudo-console waits to be told where the cursor
  is, and a trial's output has no terminal to answer. sonde answers it.
- A retry in the signals was any later request of the same key, however much later. Requests sonde answered
  itself were priced.
- The app: a finding's advice opened under another finding; three time axes in one pane; tiles and tables cut
  figures and dates; Run's actions wrapped ragged; Compare said each fact twice; Chinese headings had no weight,
  and one word stood for Run, Runs and the button. In dark, tables that draw their own cells were black on
  dark. On Linux a window's first field was drawn focused and selected.

### Removed

- `Sessions.open` and `Sessions.analyse` in the bridge (the paged view replaces them); `.runpoints.json`.

## [15.0.0] - 2026-11-11

One reading of a run: four signals, one chart system, one way to draw each thing — and a verdict that is right.
No new place and no new command. The review this came from is [docs/review-14.0.md](docs/review-14.0.md); every
row of it is in this version.

### Added

- **Four signals.** A run's header is **rate, errors, latency, bytes** (and in flight): each a figure with its
  line over the session, and a press opens its chart. `sonde show` and a run's summary print the same lines.
- **What could not be seen**: bytes a second each way over time; p50, p95 and p99 over time; the first byte
  against the rest of the exchange; errors and retries by class over time, injected ones marked; request and
  response sizes, and MiB/s against size; cost by operation; a status-code table; the prefix nearest S3's limit.
  The Time pane groups them as Traffic, Latency, Errors and Where. A shared page and Compare have the signals too.
- **Trials in the app.** A trial's session opens, says it is one and whose, and marks the requests sonde
  answered itself (rows, the inspector, the lanes and the scatter). "Try under faults…" on Run and on the Runs
  pane lists the scenarios it will run before it runs them. The sessions list says when a run broke under faults.
- **A program has a name**: `[program] name` in `sonde.toml`, or `--name`. Its baseline and trend survive a
  changed argument or interpreter path.
- `sonde check -f json` has `trials`, `verdict` and `program`; a trial's row says `notRetryable`, `lostWrites`,
  `delayed`, and its wall time against the clean run's. `sonde sessions` shows requests, seconds, errors and trials.
- A second launch, a `sonde://` link or an opened file goes to the running app.

### Changed

- **One chart system.** Every chart, Capacity's, Compare's and the shared page's included, has one frame, one
  left edge, one row pitch, one legend, one tooltip and one empty state; `scripts/lint-charts.py` counts them.
- **One of each.** Two renderings of a figure, the kit's heading and row only, spacing on the scale, two control
  heights, three icon sizes, one empty state, one function for a duration; each kept by a lint. A pane that
  narrows drops its lowest-ranked figures and columns, and never clips one.
- **Chinese is whole.** Sentences written in Rust (capacity findings, the lifecycle verdict, layout facts, probe
  titles and summaries, plans, job progress, the credentials line) are phrases with a Chinese template each.
- The shared page has the app's layout.
- Native Shell moved to its newest commit. The focus ring shows for the keyboard only; a pressed chart scrolls
  its operation into view; the icon, the middle ellipsis, the options and the boxed pairs are Native Shell's own.
- `sonde export` writes OTLP unless told otherwise; "newest" is the newest recording, never a replay or an import;
  `compare`, `replay` and `diff` take a session's name.
- Unknown top-level keys in `sonde.toml` are an error (`[[scenarios]]` was ignored in silence), and a scenario's
  `on` must name operations that exist.

### Fixed

- **A program that never retries could be "survived".** A retry is now counted against the clean run: a request
  the clean run sent *n* times and the trial answered successfully fewer than *n* times was not retried.
- **Requests without a key shared one identity**: of two `DeleteObjects` batches only the first met the fault.
  They are told apart by their body and their paging.
- A lost answer on CompleteMultipartUpload was read as an upload left unfinished and "written twice". Jitter was
  judged across attempts ("jittered" of a fixed 1 s, 2 s, 4 s). A lost write that is not retried was counted among
  the reads; a 404 scenario was judged as if a retry were owed; `slow` had no figure.
- **Ctrl-C did not stop a trial** and the interrupted scenario was saved as "broke". It ends the trial and writes
  nothing. A trial's program now has the clean run's stdin.
- `sonde check` ignored a broken trial without a budget and said "over budget" of one; a run without `--trial`
  kept the old verdicts; trials of an earlier trial stayed for ever.
- Deleting a run deleted whatever path its `.trials.json` named. Trials are found by name, beside the run.
- `{trial}` was rewritten in runs that are not trials. The injected-fault finding is first, not last.
- **The proxy forwarded keys with `.` or `..` segments to another key** (14.0 refused them in sonde's own
  requests only). It answers 400.
- **A replay left what it wrote** on a versioned bucket. It removes the versions, says how many, and exits 70 if
  any remain.
- **"Acting on N findings"** was arithmetic that could not be. It subtracts the requests the findings name, and
  says nothing where that cannot be measured.
- The app: a trial's row did nothing, or opened another session; a bucket's sheet kept the old endpoint's figures
  when the endpoint changed beneath it; a live session's pane lost its sort, scroll and zoom each second; the
  sidebar toggle did nothing while a place held the sidebar open.
- Prefix folding counted a lone deep folder several times; a Delta table under a path with `/data/` lost its
  files; a folder import followed a symlink to itself; `--log-out` on `run` wrote an empty file (it is refused);
  `check --init` wrote a time limit an identical run would fail; `-f` help listed formats the command refuses.
- A gzip file that holds more than 2 GiB is refused rather than read into memory.
- A program fed by a pipe that reads slowly slows the pipe's writer, as it would without sonde.
- A session that cannot be read says so, and why (it stayed an outline for ever); a 404 for a bucket that is not
  there has the tone of a failure among the status codes, as the errors count it.
- The timeline's axis says the most in flight the session had (it said its nearest round tick: "up to 5" beside
  the tile's 8); a time tick reads `+200 ms` on every chart; bytes sent and received share one stem per slice.
- A toast of a window that closed was the main window's for a moment; two going together logged
  "a kept row is gone". The example session has its line in the sessions list.
- Under 600 points of height a session's signal tiles keep to a line each, so the operations keep their rows;
  a bucket's four tiles are two and two beside an open object, never one column. Compatibility says its title once.

### Removed

- The header's six loose figures (the four signals replace them), the pressure strip, 60 locale keys nothing used,
  four of five duration functions, three of five figure renderings, the second tooltip, the icon's canvas.

## [14.0.0] - 2026-11-10

Run your program against an S3 that misbehaves, and sonde says whether it survived. Thirteen versions observed;
this one makes the failures happen. The review this came from is [docs/review-13.0.md](docs/review-13.0.md).

### Added

- **Trials.** `sonde run --trial -- <command>` runs the command once clean, then once per fault scenario chosen
  from what it did, and says of each whether the program **survived**, was **degraded** or **broke**: the faults
  and how many were retried, the wait before a retry and whether the waits are jittered, how much the load grew,
  writes made twice, uploads left unfinished, the exit. See [docs/trials.md](docs/trials.md).
- **Faults in the proxy**: a status on each request's first attempt (`slowdown`, `error`), a write whose answer is
  lost after the store acted (`lost-answer`), a failed completion (`fail-complete`), a delay (`slow`), S3's
  per-prefix rate (`s3-limits`). Rules match by operation and key and count attempts at one request, never the
  order of arrival. `--fault NAME` picks scenarios; `[[scenario]]` in `sonde.toml` defines a project's own.
- `{trial}` in an argument, and `SONDE_TRIAL`, give each run of a trial a place of its own.
- A trial is a session: its first finding says how many requests met a fault sonde injected, records carry
  `injected`, and a replay skips them.
- **`sonde check`** prints a run's trials as rows and fails when the program broke (`-f markdown` and `-f github`
  too). **The app** shows them on a run's Runs pane, each opening its trial's session.
- **Lance is read as versions.** A commit (`_versions/<n>.manifest`, written with `If-None-Match: *`) is one
  operation with the listings, the transaction file and what it wrote: `version 6 committed · 1 data file
  (439.4 KiB)`, `· an index`, `· rows deleted`; the ranged reads of a scan are one `read version 7 · 3 ranges of 1
  file`. A recorded pylance run of 63 requests is eight operations (`tools/gen_lance.py` makes the recording).

### Fixed

- **The app's own downloads were still cut at 60 s** (13.0 fixed the proxy only). The timeout is now for the
  headers and for each stretch of silence, never for the whole download.
- **`sonde probe -e A -e B` left the other endpoint's objects** when one run failed or was interrupted: it returned
  on the first. Every run is now waited to its end; an interrupt cancels them all and each cleans up.
- **A replay confined to a prefix still sent `PUT /bucket?inventory…`** from a session recorded before 13.0, which
  stored it as CreateBucket. What is skipped is decided by the request, not by the stored name.
- **After an outside edit of `config.toml`, the app sent to the old address until restart**: a save that took the
  edit in kept the cached clients.
- **Prefixes were cut short, twice.** The analysis kept three folder levels, so `db/wal/2026/11/` and
  `db/wal/2026/12/` were one row; the charts then clipped a label at its end, so `slate/db/compacted/` and
  `slate/db/compactions/` read the same. Prefixes are now whole folders, folded into the one above only when there
  are more than a chart can hold (and the row says how many it holds); a label says the shared start once and keeps
  its end; the tooltip has the whole prefix.
- **`sonde demo` shows the product**: 28 requests read as 11 operations (a multipart upload, a Delta commit, a
  retry after 503, a paged listing, lookups that find nothing) and four findings, where it printed 53 lines.
- **`sonde check`** names the session it judged; a failed row says by how much; the warnings row is "new
  warnings", which is what it counted; a 10% tolerance is 10% for a small cost too.
- **`sonde import <folder>`** reads every log file under a folder as one session.
- `sonde diff` on two ordinary runs says they are not a session and its replay (64); `sonde compare` says "no
  change" for a figure that did not move.
- **A Parquet file is not called Delta or Iceberg by its name**: only when the session touched that table's log.
- **Keys with a `.` or `..` segment are refused** before anything is sent: the URL library collapses them, so the
  request went to another key. The special-keys probe says this is sonde's limit, not the store's.
- A response cut mid-body is recorded as cut (and counted as an error), not as a clean 200; a proxy stopped with
  requests in flight says the session lacks them.
- `watch --logs --once` exits 70 when none of the files listed could be read; the store's own read for a copy in
  an access log is part of the copy; `-f` is refused by every command for a format it does not write.
- Finished jobs are pruned by kind (100 downloads no longer blank a layout's time slider); a probe cleanup that
  could not list says so; a capacity scan that may not list versions says they were not counted, not 0; a probe
  whose setup write failed is an error across the suite; a lifecycle document sonde cannot add to is never planned
  over.
- **The interface, from the 13.0 review**: an empty run is no longer deleted under its reader, however it was
  opened; with the sidebar hidden a place that needs its list shows it, and a list that fails says so; Sessions
  keeps its session, search and Runs/Programs when left and come back to; a lane's cell shows the worst of the
  requests it holds and says how many, a sampled view says it is a sample, the legend has client errors; the heat
  map follows the chosen stretch and an empty stretch says so; flow labels no longer sit on their nodes; the rename
  dialog opens in the window that asked; "Copy table" copies plain numbers.

## [13.0.0] - 2026-11-10

Still and true. Every state says what it is, nothing moves under the hand, each thing is drawn one way — and a
session's time is seen request by request. The review this came from is [docs/review-12.0.md](docs/review-12.0.md).

### Added

- **What ran at once**: every request on its lane, from its start to its end in its outcome's color — stacked ran
  together, in a row waited their turn. A press opens the request.
- The Time pane is a board: each chart in one frame (its question, a sentence that answers it, the plot, its
  legend), two abreast where there is room, under one timeline whose chosen stretch every chart over time follows.
- The sidebar lists what the place has: the commands run before (Run), the sessions (Sessions), the buckets
  (Buckets). One left column in the whole window.
- A `config.toml` that cannot be read is said so, with Reload, in Settings and Endpoints; nothing is saved over it.
- `sonde import` reads gzip and says how many lines it skipped; CloudTrail keeps which upload, part and listing a
  request was; HAR keeps its request headers.
- Native Shell at `ec8430f`: buttons show their hints and shortcuts (the old commit dropped every tooltip).

### Changed

- A session's page: its command and figures, then tabs, then the pane, which takes all the room under them. The
  findings sit with the operations they narrow; the session's shape is a strip over them, full width.
- Choosing an operation opens the inspector and moves nothing else (the sessions list used to leave). A bucket's
  four tiles (size, incomplete uploads, versioning, layouts) are always there; the sessions' search field too.
- One of each: a figure, a section heading, a row, a disclosure, a banner (`Notice`), a dialog's foot (Cancel, then
  the commit, at the end), a way back, a state for what is fetched (on its way, failed with why and Try again,
  there). Two tab controls, not three. Settings' labels sit above their controls on one grid.
- One duration (0.4 ms … 2 h 05 min, the unit chosen after rounding), one percent, one clock per list; counts are
  grouped everywhere. Fewer is green and more is red in every chart that compares.
- Run's command field is a field like the others, with one row under it; the run's band is one card whose primary
  is Run again.
- The proxy cuts an exchange only when it stops moving for 60 s (it cut every exchange at 60 s).
- `--no-delete` goes by the method: every DELETE, `POST ?delete`, `PUT ?lifecycle`, and anything it cannot name
  that is not a read. A path that starts with `//` is refused.
- Compatibility probes are refused while read-only is on, and each run is in the audit log; a probe whose setup
  failed is an error, not a verdict.
- `-f` formats a command does not write, and `--tolerance` or `--no-new-warnings` without a baseline, are usage
  errors. `replay` to several stores and `watch --logs --once` exit by what happened.
- Words: `…` on every command that opens something; Delete for data, Remove for configuration; one name per
  place. Chinese uses one word per concept (桶, 键, 分片上传, 任务, 重放).

### Fixed

- A configuration that failed to load was replaced by an empty one and saved over; a save lost what the file had
  gained meanwhile; starting the demo dropped an endpoint named `demo`; `--url` and `--rate` were saved by the TUI.
- Cancelling a job left probe objects, uploads and buckets, and a download's partial file; a job that panicked
  stayed running; finished jobs were never dropped.
- A replay whose prefix could not be listed deleted what was under it afterwards; a prefix-confined replay
  replayed bucket inventory and analytics settings.
- `watch --logs` read two hours of logs again past 50,000 files; access-log imports counted lifecycle lines as
  requests and failed a file on one short line; `share --redact` kept the requester's ARN.
- The shared page showed a function's source for "Loading…", opened nothing on a request's dot, and chose the wrong
  row from an output line once sorted.
- A session in its own window never grew while live, had no toasts, and laid itself out by another window's size;
  toasts showed in every window.
- Failed loads read "Loading…" forever or as "nothing here" (a comparison, an object, buckets, versions, runs).
- The bucket tree showed one endpoint's folders under another's bucket; Refresh never read folders again; a folder
  with `#` in its name could not be chosen.
- A job's progress bar never passed 1%; Rename saved unchanged renamed the session; a run past its p95 budget read
  `verdict.p95`; an empty run opened within eight seconds was deleted; Run's clock read `-1:-1`; a second run's
  output was appended under the first; `:` then a place did nothing on the terminal's home list.
- `184532 ms`, `1000 ms`, `1024 KiB`, `+0%`; commands with `KEY=value` in screen-reader labels and the palette.
- And the rest of the review's list: the SlateDB WAL scan, the lakeFS name cut inside a character, the lifecycle
  rule's id, STS with a port, percentiles defined three ways, cost for requests that never left.

## [12.0.0] - 2026-11-09

Honesty and fit. What a log can and cannot say, the code that sent each request in Python, Node.js and Java, the
change between two runs down to the lines, and controls and tables that line up.

### Added

- The Runs pane says since which run (and commit) a program is past its baseline, and opens the code change
  between the last run within and the first past: file by file, the later run's findings on their lines.
- Naming the code that sent each request in Node.js (the AWS SDK for JavaScript v3, through `NODE_OPTIONS`) and
  Java (the AWS SDK for Java 2, a `-javaagent` that adds a global interceptor only when the SDK is on the class
  path), beside Python's.
- The time to the first byte: the proxy measures it and the whole exchange (latency now includes the body); the
  operation's requests show both shades. Access logs keep their turnaround, who sent each request and its user
  agent; CloudTrail keeps who and with what.
- `sonde check`: `--max-p95` / `max-p95`, and `expect = [...]` in `sonde.toml` for findings kept on purpose (never
  a new warning, listed as expected). A finding's "Expected" copies its line.
- The PR comment `sonde check -f markdown` would write, rendered in the Runs pane, with Copy.
- Where a run's requests went by prefix as one map (requests, cost, or the busiest second against S3's per-prefix
  limit), and in Compare where they changed, each prefix colored from fewer to more.
- Fixes for slow requests, lost races and orphaned files.

### Changed

- A CloudTrail file, which has no request times, shows no latency charts, waiting figures or latency findings.
- Two control heights (h-8, h-7 with h-6 segments), three row heights, one settings width, one search field, one
  figure tile (`Stat`) and a middle-cut `Path`; Segmented and Options no longer place themselves; tables fit their
  own width (the operation's name takes the room, notes and bars go from the end). The lint fails on each drawn by
  hand.
- The endpoint form sits in place under its own header in the Settings window; the Sessions inspector has the
  panels' header. Chart modes are toggles, not tabs; the busiest kinds over time are a mode of the requests' chart.
  Charts with a table in the terminal show it to screen readers.

### Fixed

- A botocore client without signing (UNSIGNED) no longer fails under the Python hook; code places are read the same
  way everywhere (paths with spaces, drive letters) and open only plain files of the run's folder.
- `watch --logs` keeps a watermark and the keys it has seen, so a log written late under an earlier name is read.
- Windows paths in commands keep their backslashes; `--password=…`-style options and commands given to a shell are
  scrubbed. Imported and `logs of` sessions get no `$` and no Runs pane.
- Settings saves only what changed there; an endpoint's test result is forgotten when it changes; its URL and
  region follow the vendor while untouched; the Time pane's charts follow the zoom; a monthly cost reads as money;
  "one request at a time"; gzipped imports; and more (see the 12.0 milestones in the commit log).

## [11.0.0] - 2026-11-09

Order and precision. One kit drawn on Native Shell, one way to show each thing, charts down to every request, and
the loop from a run to its baseline, its commit and the code that sent its requests.

### Added

- Every request as a dot: when it ran (or how large it was) across, how long it took on a log scale, in its
  outcome's color, the p95 dashed; a press opens the request. Each busy kind of request over time, as small charts
  on one time axis. Time spent by kind or by the code that sent it. Two runs' latency as cumulative curves in
  Compare.
- Runs record the folder they ran in and the commit their code was at (or CI's `GITHUB_SHA`); Run again runs there.
- `sonde check` says since which run a program went past its baseline, and the commits around it.
- `sonde check -f github`: failed verdicts as errors, findings as annotations on the line of code that sent their
  requests. `sonde check --init` writes a `sonde.toml` the run passes, with room, and prints the CI steps.
- `sonde watch --logs s3://bucket/prefix`: production seen through its access logs or CloudTrail files, read as they
  arrive (`--every`, `--once`), each pass a run of `logs of s3://…` with findings, a budget and alerts.
- What a month of a program costs at the pace it runs, in Programs.
- Run's ended band: the run against its baseline, Run again, and after the demo a way to one's own store. A
  finding's code places open the file in the editor. Open (⌘O) uses the system's dialog and reads the file in sonde,
  however large; a file opened twice is the same session. Welcome opens a log of production.

### Changed

- One kit: one card (panel, tile, list), one table look (cells line up with their titles, figures at the end, no
  stripes), two heading levels, four buttons, a focus ring on every control, selection in one color, spacing on
  4/8/12/16/24; dialogs, disclosures and switches from Native Shell (Dialog, Accordion, Switch with its label);
  a lint keeps pages from drawing their own.
- One rendering per concept: an HTTP status is a dot and its code (success quiet), times are clock times, durations
  have one format, counts are grouped.
- The session header's figures open their breakdown with a press, and carry waiting and cost; the Time pane's
  duplicate figures are gone. The findings' heading says what acting on them saves.
- Charts share one gutter, tick style and color roles: tails in a quiet tone, retries in info, 404s readable in both
  themes, heat by its square root; the waterfall names its rows.
- Menus pop up in the terminal too; the toolbar keeps one control shape and folds search into an icon when narrow.

### Fixed

- The Python caller hook: paths and names outside ASCII, presigned URLs, requests to other endpoints, Debian's
  `dist-packages`. HAR imports keep no presigned credentials; shared pages and reports show no variable's value
  set before the command; commands read as shell words (quoted values, `env` options, compound commands).
- Costs agree in every view (and B2 prices lists as class C). `sonde check` resolves a `sonde.toml` baseline
  against the file, fetches https only, within a minute, never printing a signature; Markdown comments escape what
  the program wrote. `watch --alert` checks the last file and waits for its alerts.
- Adding an endpoint never replaces another; an edit keeps its CA file and settings. An object's detail is its own;
  Refresh lists again; a search stays with its place; the bucket tree reads past its first page.
- A run with no requests neither opens a comparison nor leaves a button to another session; a session deleted
  goes to the trash, or says why not. Settings follow changes made elsewhere; a confirmation stays while it runs;
  a replay to several stores keeps the ones that worked. Zoom lets go on a second press, and the last instant's
  findings are marked. Sorting reads numbers and durations in the app's language. Strings and plurals in Chinese.

## [10.0.0] - 2026-11-09

A program over its runs, and from a finding to the code that caused it. Fewer places, one thing at a time; the
Time pane answers in the order an S3 question runs.

### Added

- Programs: the sessions list by program, each with its runs' trend and a mark when its newest run is past its
  baseline; one opens on its runs. "Run again" runs a session's command once more and opens it compared with the
  run before.
- Which code sent a request: a finding names its top callers (`x-sonde-caller: file:line function`), its SDK and
  version (from the user agent) and trace (`traceparent`). For Python, `sonde run --callers` or the "Name the code"
  setting adds the header itself (an opt-in `sitecustomize`); the header never reaches the store.
- `sonde.toml`: a project's `[check]` budget and baseline kept with its code; a baseline may be an https URL;
  `sonde check -f markdown` writes a pull request's comment.
- `sonde watch --alert URL`: each new warning, and each finished file over budget, posted to a webhook.
- Time: the session's timeline full width with its axis, the findings marked and two presses to zoom (the charts
  below follow); what the store pushed back (throttles, failures, retries) over time; how long every request took
  (a distribution with the median, p95 and p99); each prefix against S3's 5,500 reads and 3,500 writes a second;
  what the requests cost, by kind and by prefix; throughput.
- Cost by prefix; request prices by the store's vendor (R2, B2, Wasabi, Tigris, a store of one's own).
- Activity's requests open as a session; the operations copy as a table; the fix's code has a copy button; a
  session deleted goes to the trash; a shared page's folder shows in the file manager; the palette searches every
  session; a file dragged over the window says what dropping it does.

### Changed

- One way in at first launch: the demo, landing on Run with a command ready.
- The findings take a line each while a pane of its own is open; the sessions list steps aside while an inspector
  is open; long keys are cut in the middle; Run, once ended, shows the run in a sentence and what stands out.
- Compare is one view: a replay's answers, before and after, and the transcripts on demand. Two different programs
  compare by their figures, and say so.
- Before and after, and two stores' p95, as dumbbells; a run's trend shows its baseline's tolerance band; the live
  view is the last 30 seconds.
- Costs of thousandths of a cent read per 1,000 runs; the run policy reads "Allow all / Block deletes / Block
  writes"; the read-only state is words in the toolbar; Settings puts safety first; number fields look like fields.
- The sidebar is a split like the others; tables size their columns to what they hold; the operations' column
  widths are kept.
- Outcome colors are for outcomes only; answered and not found differ in lightness, not only hue.

### Fixed

- `--redact`, OTLP, the tray, notifications and window titles could show a variable set before the command (a key,
  say): they show the program's name, and the command with values hidden.
- A SigV2 presigned URL could last forever; it may last a week, as SigV4's.
- "Where the time went" could add up past 100%; the live rate extrapolated a few milliseconds to "200 a second";
  the latency band curved through values no request had and bridged empty slices; the Sankey's widths were not
  requests.
- Chinese lacked the singular forms and three words; a CI check now holds every language to English's keys.
- Tables' sort arrows did nothing; uploads sorted by their age's text.
- A browser's HAR imported every page and script as S3 requests.
- The jobs badge could stay at "1 running"; a stopped listener's task waited for the next change; a setting
  changed just before Settings closed was lost; the run toast opened the wrong run.
- A shared session's fallback command broke on a path with spaces; Windows paths in window titles; breadcrumbs
  of keys with `//`; English-only words in Multipart, Layouts, Browse, Probes and the terminal's home.

### Removed

- The floating monitor, the sidebar's Recent, the outcome ring, the failure gauge and the latency spread chart
  (the distribution answers it); a run that made no request is not kept unless asked.

## [9.1.0] - 2026-11-08

Native Shell where sonde still had its own: fewer parts of sonde's, and a few slips put right.

### Fixed

- "Share as a web page" opened nothing in a packaged app: opening URLs needs the `openUrl` capability (http and
  https only).
- Fields named themselves to screen readers only by their placeholder; each now says its label.
- "1 requests", "1 incomplete uploads" and the like: a count of one reads in the singular.
- Ages ("3 h", "2 d") and an upload's age were English in Chinese.
- A run's notification opened the last run, not the one it told of, when pressed after sonde was quit; the
  permission is asked once, as a run starts, not as it ends.

### Changed

- Numbers in Settings and the multipart threshold are steppers (arrows and buttons; screen readers hear a spin
  button).
- The sessions list, the buckets list and an object's details are split views: drag the line between them; the
  width is kept.
- The findings open and close with a springing height; breadcrumbs are links (the last one the place shown).
- An operation's requests, a key's versions and a layout's timeline draw only the rows in view (thousands stay
  quick); the compatibility matrix is a table whose probe column stays while the stores scroll.
- Charts: the keys step the hover with a spring; the live view's failed share is a gauge; the latency band's
  labels sit inside it; the store matrix colors a p95 well past the recording's; every chart tells a screen
  reader the point the keys are on; the busiest prefixes read as a table in a terminal.
- Activity follows the engine (records and jobs as they come) instead of asking it on a timer.
- In a terminal, a session opens over the list, which is as it was on going back.
- The window opens where it was left (Native Shell's `restoreBounds`).
- A session's shape no longer prints its busiest moment over the findings' numbers, nor two numbers over each
  other.

### Removed

- sonde's own shortcut labels, window placement, legends (one for every chart now) and a chart nobody used.

## [9.0.0] - 2026-11-08

From diagnosis to observability. A run is watched as it goes, production traffic reads like a recording, every
finding says what to change and what acting on all of them would leave, and the charts of a session read as one.
Fewer of sonde's own parts: tables, toolbars, toggles, menus and words are Native Shell's.

### Added

- Live: Run shows requests a second, failures, in flight and the p95 as they happen, and how long requests take as
  a band (median to p95, p99 dashed) beside the session's shape; a small monitor floats above other windows (from
  Run or the tray); ⌘⌥R / Ctrl+Alt+R runs the last command again, or stops it, from any app.
- `sonde import`: S3 server access logs, CloudTrail data events and HAR files as sessions, also opened or dropped in
  the app.
- What to change: each finding with its fix in Python, Java, Go and JavaScript (or the bucket rule), shown in the app
  with an SDK switch; what acting on every finding would leave (requests, waiting, cost) in the app, `show` and JSON.
- `sonde replay FILE -e a -e b …`: one session on several stores at once, side by side (answers that differ, each
  kind's p95); in the app, the replay dialog's "Also on".
- `sonde export --otlp [--to URL]`: a session as OpenTelemetry traces, each operation a span with its requests.
- Charts: a moment rested on in one time chart is marked in the other; each finding's number stands over where it
  begins; how requests were answered as a ring; a sparkline per session in the list; Capacity's prefixes nested by
  folder; a replay's differences marked in its transcript; a program's history over a range of days; the latency
  band in Time.
- Signed packages of the app's code, offered first by Updates once the project's keys are set (packaging.md).
- CI: tables in the detail panel must fit it (`scripts/lint-layout.py`); each run's screens are compared with the
  last green run's in the job summary, with before | after | difference images.

### Changed

- The words are Native Shell's i18n (`app/locales/en.json`, `zh.json`).
- Tables are Native Shell's DataTable everywhere (every row, sortable, drawn by the terminal too); values of a few
  are ToggleGroups; the toolbar's commands and the sidebar's places are Toolbars (one Tab stop, arrows between);
  in a terminal a session's commands are a ContextMenu; a finding's why reads as text above its fixes.
- The Timeline pane is gone: the operations' When column places each in time, the shape says when.
- One loading state for every wait; dashed lines drawn dashed.
- `rust-version` 1.95.

### Fixed

- `share --redact` let names through: transport errors (the URL), job tags, query values such as
  `response-content-disposition`, and any "extension" after a dot (`john.smith`).
- The proxy took an expired presigned URL and a signature of any age; it now refuses them as S3 does (15 minutes).
- SigV2 requests to replication, object-lock and other subresources were refused; `localhost.` and the long form
  of `::1` were taken for another machine.
- `check --set-baseline` alone exited 64, refused a relative path; `--baseline` took no session name.
- `show` and `demo` wrote colour codes into a file; an exit status past 255 could read as success.
- A live session could lose a record written after a newer one, or read another file written in its place.
- A chain's saving counted the program's own work between requests.
- `share --serve` wrote to a temporary directory another user could claim first.
- Run: a stopped run or a command not found got SDK hints; the ended toast could say "ended" for a stopped run;
  output could be cut inside a character.
- Incomplete uploads lost Bytes and Cost off the panel's edge; the chains chart's legend sat on its axis; macOS:
  a scheme chosen in Settings left a light sidebar; `font-mono` text was proportional on Linux and Windows; the
  lifecycle rule broke inside its tags; a long command was cut off; a 404 `NoSuchBucket` read as "not found";
  concurrent large uploads each held up to 1 GiB.


## [8.0.0] - 2026-11-08

Safer, larger, and to the point. The recording endpoint serves only the program sonde gave its keys; sessions of a
million requests are compressed and read in steps; a session names the requests that waited on each other and what
running them together saves, and a replay compares the same workload's latency on two stores. New: `sonde demo`,
`sonde watch`, `sonde sessions`, one-file shares with redaction, a static Linux build.

### Security

- The recording proxy verifies every request's SigV4 signature (header or presigned) against keys made for that
  recording, and takes requests only for a loopback Host: an unsigned request, other keys or a web page rebinding a
  name to 127.0.0.1 get 403 and never reach the store. `sonde proxy` refuses a non-loopback `--listen` without
  `--allow-remote`.

### Added

- `sonde demo`: a small program's requests to the built-in store, recorded and explained, with no account.
- `sonde watch`: a proxy left running, with a status line for the last minute, each new warning once, and a new
  session file every `--rotate` minutes.
- `sonde sessions` (list, `prune --keep N --yes`, baselines kept); sessions are named by file name anywhere a path
  is taken.
- `sonde check --baseline auto` and `--set-baseline`: the baseline the app sets, shared with the CLI; a baseline of
  another program is named.
- `sonde share --html` (one self-contained page, each operation at `#op-N`) and `--redact` (bucket and key names
  replaced by stable tokens; headers, bodies, URLs, output and arguments dropped).
- `sonde endpoints add ID URL [--profile P]`.
- A finding for requests that each waited for the one before, with how much sooner eight at once would finish; Time
  shows each chain as it ran beside the same requests at once.
- Replay and diff compare each kind's p50 and p95 on both stores (CLI table, JSON, and a chart in the app).
- A run that sends sonde nothing says how each common SDK is pointed at it (CLI and app).
- Higher contrast (Settings › Appearance), and Updates names the installed version and its Native Shell runtime.
- The terminal UI tells the session in a sentence, its findings narrow the operations, and its columns sort.
- `sonde-linux-x86_64-musl.tar.gz`: a static binary for Alpine, distroless and old distributions; `install.sh`
  takes it on musl systems.
- Chinese for the web viewer; `sonde://` links are tested.

### Changed

- Finished sessions are gzip-compressed in place (35× on a large session); every reader opens both forms.
- A live recording is read in steps in the app; each finished run's history figures are kept on disk.
- `sonde show` answers on stdout; Markdown and CSV are real tables; a mistake in the command exits 64.
- Native Shell 1662115 (was beeb9fc).
- Fewer components: one figure, one section, one name/value list, one popup surface.

### Fixed

- The prefix nearest its rate limit was not always named first.
- A tolerance of 57% allowed 156 of 100 requests, and a free baseline allowed no cost at all.
- A warning grown from a note was not counted as new against the baseline.
- Formatting an XML document dropped its declaration and split empty elements.
- `sonde://open?path=` with `+` or `%2B` in the path, and bucket links with empty segments.
- A killed run asked for attention; the run badge was set after the permission prompt.
- The kinds preview's bars were shares of a whole instead of relative to the largest.
- A session read while recording and then compressed could read as empty.
- `sonde endpoints add` with a URL also registered an ad hoc endpoint.


## [7.0.0] - 2026-11-08

A program over its runs. A session says in a sentence where its time went and what to do first; its requests are
seen flowing from operation to prefix to outcome; runs of one command are tracked against a baseline, in the app
and in CI.

### Added

- Runs: a session's Runs pane plots every run of its command (requests, waiting, cost, warnings), the baseline
  dashed; make a run the baseline, and every other run is checked against it (10% to grow, no new warning).
- `sonde check [SESSION] --baseline FILE --max-requests N --max-waiting MS --max-cost X --max-errors N --tolerance
  PCT --no-new-warnings`: a run against a budget and a baseline, exit 1 when over, for CI.
- Where requests went: a Sankey from each kind of operation to the prefixes it touched to how they were answered.
- When and where: a heat map of requests by prefix over the session.
- A finding for a prefix near the request rate S3 serves one prefix (5,500 reads or 3,500 writes a second).
- The session in a sentence: where its time went and what acting on its first finding saves.
- The shape names its busiest moment; resting on the requests or took figure shows them by kind.
- `sonde://` links (`open?path=`, `session/<name>`, `bucket/<endpoint>/<bucket>/<prefix>`) and `sonde show --app`.
- A pressed run notification opens the run; warnings wait on the Dock's icon or the taskbar button; a failed run asks
  for attention while the window is away.
- A request's document and an error body read in a read-only code editor, the S3 error code marked.
- Session rows offer their commands to screen readers.

### Changed

- How long requests took is each kind's median to p95, the median a dot and the slowest a ring, instead of a dot per
  request.
- Compare loads off the interface's thread.

### Fixed

- A finding whose details changed between two runs (which request was the slowest, throttling either side of its
  threshold) was reported as both gone and new; a format's shared habits were merged into one.
- A line of output chose the wrong row when the operations were sorted.
- Concurrent multipart uploads to one key were credited with each other's create, and the attribution scanned the
  session once per upload.
- Compare opened from a replay compared it the wrong way round.
- Latency included fetching credentials; a body cut to the 256 KiB kept was guessed rather than flagged; latencies
  past a day overflowed; a session written over another kept its old count; shared pages lost their folder across
  versions; amounts just under a power of ten rounded to three digits; equal counts showed as fewer; operations with
  more requests than a call takes arguments failed to draw; a stray `%` in a dropped file's name left its other
  escapes undecoded.

## [6.0.0] - 2026-11-08

Time and cost, seen. A session says where its time went and what its requests cost, two runs of a program say what a
change did, and every chart is drawn by Native Shell's Plot: clear, quiet, interactive and accessible.

### Added

- Where the time went: the time a program waited on its store by kind of request (an operation, its lookups that
  found nothing apart), counted as wall-clock time so requests at once count once; the peak in flight; the slowest
  requests; and the requests' cost at S3's prices (`settings.prices.per-1000-writes`, `per-1000-reads`). In
  `sonde show`, in the app's new Time pane and on the shared page.
- `sonde compare BEFORE AFTER` and Compare › Before and after in the app: two runs of a program, requests, waiting,
  time, errors and cost with their change, the requests by kind before and after, and the findings gone and new.
- A session's shape beside its figures: requests by outcome over time and requests in flight. A press on a slice
  narrows the operations and the timeline to that moment. It grows live under Run.
- An operation's requests as a waterfall, a press away from each request.
- Capacity's prefixes as a treemap.
- Operations sort by time, requests or duration.

### Changed

- Every chart is a Native Shell Plot: the pointer, tooltips, fading and the keyboard handled natively, motion that
  respects reduced motion, and a sentence for screen readers saying what stands out. The timeline no longer redraws
  on every mouse move; zoom is a slice of the session's shape.
- Compare opens on Before and after, unless one session is a replay.
- `app/DESIGN.md` has the rules the charts follow.

### Fixed

- The last refresh of a live session was dropped when it came while the session was being read: Run and Sessions
  could keep showing a run without its last operations.
- Records joined from two recordings repeated ids, so findings, the inspector and copy could name another request;
  a session now numbers them afresh.
- "Small files uploaded in parts" left out each upload's CreateMultipartUpload: 3 requests each, as it says, and the
  saving with them.
- A DeleteObjects (or another control document) sent with plain chunked transfer was not kept, and its replay was
  skipped as from "an older session".
- A CopyObject of a version, and a document longer than the 256 KiB kept, were replayed with another meaning; each is
  now skipped with its reason.
- HEAD before a ranged or part GET (reading a large object in parallel) was reported as a request too many.
- The in-flight count behind the throttling finding could underflow; latency now counts from the request's arrival,
  its body's upload included.
- A search left over after deleting sessions down to four could hide them with no field to clear it; a deleted
  session's neighbour now takes its place.
- Dropped files: UNC paths, a stray `%`, `.SONDE` in capitals, a session already in the folder (on Windows), and a
  file torn mid-character all open.
- A session written by another tool with its keys in another order counted no requests; two shared sessions of one
  name shared a page.
- (Found reviewing 6.0 before its release.) A request that took no time (a refused one) left the requests in flight
  one too high for the rest of the session; a burst's peak in flight ran on through the idle time after it in long
  sessions; live charts drew themselves in again on every refresh; a slice picked on a live session's shape pointed
  at another time once the session grew.

## [5.0.0] - 2026-11-07

What a session finds, made more correct, faster and more useful: findings ranked by what they cost with the requests
acting on them would save, sessions you can look after and share from the app, and the bugs a review of 4.0 found.

### Added

- Three findings: objects checked with HEAD and then read with GET (the GET alone answers both); requests the store
  throttled (503 SlowDown, 429), with how many were in flight and a limit under which it likely would not have; and
  requests far slower than the session's median, the slowest named.
- Findings are ranked: warnings first, then by the requests they bear on; where it can be counted, each says what
  acting on it would save ("45 fewer requests (26% of the session)").
- Sessions can be renamed (a title of your own), deleted after asking, searched, and shared as a web page from the
  app: Share writes `sonde share`'s page, serves it on 127.0.0.1 and opens it in the browser. All from a session's
  context menu or its ⋯ menu.
- Drop a `.sonde` file on the window to open it; File › Open Recent lists the newest sessions.
- A 50,000-request session is a test (`crates/proxy/tests/perf.rs`): about 170 ms to read and analyse in a release
  build.

### Changed

- A session is read and analysed off the interface's thread, once per version of its file; a live one again only
  after the last reading has ended. Run and Sessions show the session's outline while it is read.
- The session list is a virtual list whose days stay in view while their sessions scroll.
- Dates follow the app's language and the user's 12- or 24-hour clock.
- Recent sessions in the sidebar have a second line — their arguments, else their endpoint — so two runs of one
  program can be told apart.
- Overview cards split their row into equal columns.
- Polls (a live session, Activity, the jobs badge) rest while the window is out of sight; a closed side panel
  returns the screen reader to what opened it.
- The viewer `sonde share` writes is embedded once, in `sonde-proxy`, for the CLI and the app alike.

### Fixed

- A replay refused nothing and cleaned up everything: its cleanup removed every object under the prefix it was
  given, including objects it had not written, and two replays in the same second shared a default prefix. Default
  prefixes now carry a random suffix, a prefix that already holds objects is refused, and a prefix is always a
  folder of its own (`taken` no longer matches `taken-not/`).
- A commit retried after a 503 was reported as "files written, no commit followed": the retried write is now kept
  with the step it retried.
- A CreateMultipartUpload retried after a failure left a false "multipart upload never completed".
- A session cut off in the middle of a character (a recorder killed mid-write) failed to open; it opens with the
  torn line skipped, as a torn line always was.
- A replay's root listing was compared with its prefix left on the keys, so every key differed.
- The overview kept saying "…" for a scan that was cancelled or failed; a failure now says so with its reason, and
  is tried again when the bucket reopens.
- A session's request count included the program's output lines; it counts requests only, and reads only what was
  appended since.
- Two recordings, replays or imports in the same second overwrote each other's file.
- Two recordings started at once could both start.
- Request ids could be assigned out of order under concurrent requests.
- An argument of a phrase holding `{n}` (a key named so) was substituted again in the English text.
- Versioning reads "On" / "Suspended" (已开启 / 已暂停) instead of S3's `Enabled`.
- A shared page's server refuses paths that are not plain names inside the site (on Windows a drive prefix such as
  `/C:/…` would have replaced the site's folder).

## [4.0.0] - 2026-11-07

The same app, made with the rigor of Native Shell's design guides: one design system that every page follows,
what a session found given the room it deserves, and the platform's own materials, progress and windows.

### Added

- A session's findings are its core result: each says what happened, why it matters and how many operations it is
  about, and narrows the operations when pressed. No more chips with the explanation behind a hover.
- macOS: the sidebar sits on the window's own sidebar material; session windows name their file in the title bar
  and gather as tabs.
- A program or a job at work shows as progress on the Dock's icon and the taskbar button.
- The Run split and the inspector's width are kept across launches.
- Each place draws inside an error boundary: a failure says so and offers Try again; the rest of the window keeps
  working.
- `app/DESIGN.md`: the app's design standards.

### Changed

- One type scale (caption 11, small 12, body 13, heading 15, title 20) replaces eight sizes written inline; live
  figures use tabular digits.
- One popup surface for menus, selects and the endpoint switcher; one side panel (three widths, one header) for
  Activity, a bucket's detail, the endpoint form and a narrow window's inspector; one scrim, a theme token.
- Primary buttons only for an area's default commit; commands that open a panel, dialog or window end with "…".
- Table rows grow with the user's text size.

### Fixed

- Windows: a program started from Run never ended in the app — ConPTY keeps its output open until the
  pseudo-console closes, so the app waited for output that would not end. The pseudo-console now closes when the
  program exits. (Found by the first screenshot tour on Windows in CI.)

## [3.0.0] - 2026-11-07

Fewer places, each doing more by itself. The app has three places — Run, Sessions, Buckets — and says what it
noticed without being asked; Settings, sonde's own activity and compatibility move out of the way; and what sonde
says about a session reads in Chinese as well as English.

### Added

- Buckets open on an overview above the objects: incomplete uploads and what they cost, versioning, the layouts
  sonde recognises, the size once measured — each card shown only when it has something to say, filled in as the
  bucket opens, and opening its detail (the uploads and the lifecycle verdict, a key's versions, the size by class
  and prefix, a decoded layout) in a sheet for the prefix on screen. No forms to fill first.
- Activity (toolbar, ⌘⇧A): sonde's own requests, its jobs and the changes it made, in a sheet from the window's edge;
  the toolbar button counts the jobs running.
- Settings in a window of their own (⌘, / Ctrl+,): General and Endpoints.
- Compatibility opens from Settings › Endpoints in its own window.
- Open a `.sonde` file recorded elsewhere or sent to you: ⌘O, the palette, or Sessions.
- Resting on an observation shows why it matters (a card on hover).
- Operations, notes and observations are localised: the analysis is structured phrases (a key and its arguments)
  rendered in the app and the shared web page in English or Chinese. The CLI prints the same English as before.
- Screenshots in CI on macOS and Windows as well as Linux (`sonde-shots-<platform>` artifacts).
- A run's end is announced to screen readers.

### Changed

- The sidebar holds three places: Run, Sessions, Buckets (Store renamed). Activity and Settings left it.
- On macOS, floating surfaces (the palette, sheets, cards, menus) use a material over what is behind them: Liquid
  Glass on macOS 26, a blur before. Overview cards rise into place (not with reduced motion).
- On Run, the status band says a run ended; the toast is for when you are elsewhere.
- Run's introduction is shorter; "Multipart" and "Capacity" read "Incomplete uploads" and "Size".
- The terminal UI keeps every place on its home list (Run, Sessions, Buckets, Activity, Settings) and the bucket
  tools as tabs.

### Removed

- The Requests, Jobs and Audit pages (now Activity), and the Multipart, Versions, Capacity, Layouts and
  Compatibility tabs in a window (now the overview's sheets and the Compatibility window).

## [2.1.0] - 2026-11-07

The desktop app, made with care: a design system before the pages, Native Shell's components used to the full, and
every place checked on a real window, light and dark, English and Chinese, wide and at the least size.

### Added

- A first launch welcomes: the built-in demo, the AWS profiles it finds, or an endpoint by hand, and an example
  session, instead of a Run page with nothing to run against.
- A title bar of the app's own: where you are, the program running, a searchable endpoint switcher, the read-only
  lock, search; the menus behind a button on Linux and Windows.
- Sessions: the figures at the top; what stands out as chips that narrow the operations to those it is about; the
  operations with an outcome dot, family colors and a bar of their place in time; an inspector from an operation to
  its requests to one request (headers and error bodies that fold, the XML highlighted); native context menus.
- A timeline drawn on a Canvas: a time axis, outcome colors, a card on hover, drag to zoom.
- Run: one command bar with recent commands, the working folder and the policy; a band of live figures; a toast when
  it ends; the program in the tray while it runs.
- Store: buckets and their prefixes as a tree; one field that filters and searches; an object's details as an
  inspector with sections that fold; context menus.
- Settings saved as they change, with Undo; endpoints edited in a side sheet, removed after a confirmation.
- The palette: icons, platform shortcuts, recent commands and sessions, endpoints and buckets.
- Icons (Lucide) throughout; toasts for notices and errors; tooltips with shortcuts.
- `scripts/shots.sh` and CI's `shots` job: screenshots of every place as a build artifact.

### Changed

- Shortcuts read as the platform writes them (Ctrl+K on Linux and Windows).
- A narrow window folds the sidebar away and shows the inspector over the content.
- Observations name the operations they are about; operations count their lookups that found nothing.
- The example session is named as a user would have run it.
- The web viewer draws the new session views.

### Fixed

- A remembered window smaller than the least size (a phone-sized first window on Linux) is no longer restored.
- The palette's highlighted row shows.

## [2.0.0] - 2026-11-07

sonde is now one gesture: type a command and watch what it does to your object store. Run is the home, the app has
four places and a command palette, and it runs on macOS, Linux and Windows.

### Added

- **Run**: a command runs in a real terminal inside the app (a PTY; ConPTY on Windows) with its S3 traffic recorded;
  its requests appear beside it as operations while it runs, and a notification says when it ends. It works in
  the terminal UI too.
- The program's output is kept in the session with the time it arrived, and Sessions line it up with the
  operations: choose an operation and the lines it printed light up, choose a line and the operations running then do.
- Sessions: a waterfall of operations; **Replay** on another endpoint from the app (the plan in a dialog, refused
  while read-only); **Compare** two sessions — the differences grouped, and both as transcripts side by side; open a
  session in a window of its own.
- `sonde share`: a session as a static web page anyone can open (operations, observations, output, every request),
  written to a folder or served with `--serve`; `--json` for the computed document alone.
- A command palette (⌘K; `:` in the terminal UI) with every place, tab and action.
- The desktop app for Linux (GTK 4) and Windows, built and released by CI.
- Updates in the desktop app (Settings › Updates): check, download, install and restart, from GitHub Releases; only
  when asked. Releases publish `latest.json`.

### Changed

- Four places — Run, Sessions, Store (the bucket tools as tabs) and Settings (configuration, endpoints, requests,
  jobs, audit) — replace the nine sections; ⌘1–⌘3 and ⌘, reach them, `1`–`4` in the terminal's home list.
- The Timeline became Sessions.
- Native Shell updated to `beeb9fc`.
- The first window opens at 1280×820, big enough for Run's two panes.

### Removed

- The plugin system: signed packages, trust roots, revocation lists, `sonde plugin`, the Plugins screen and the
  Delta lens example. Layout decoders stay built in.

### Fixed

- The demo server outlived only the runtime that started it first; it now runs on a thread of its own.

## [1.0.0] - 2026-11-07

sonde now looks at traffic, not only at buckets: record what an application does to its object store, read it as
operations, and replay it on another endpoint to see where they disagree. The Timeline is the app's home.

### Added

- `sonde run -- <command>`: runs a command against a local S3 endpoint that re-signs every request with sonde's
  credentials, forwards it and records it; `--live` prints requests as they happen.
- `sonde proxy`: the same endpoint, standing, for an application started elsewhere.
- `--no-delete` and `--read-only` for both: refused operations never leave the machine.
- Session files (`.sonde`, redacted JSON lines) and `sonde show`: requests grouped into operations — multipart
  uploads, retries, bursts of lookups, paged listings, and SlateDB, Iceberg and Delta steps (a commit with the reads
  and files that led to it) — followed by observations; `--requests` for one line per request, JSON with all of it.
- `sonde replay <session> -e <endpoint>`: re-enacts a session on another endpoint under a fresh key prefix (upload
  ids, ETags and continuation tokens mapped; payloads synthetic; bucket changes left out), cleans up, and compares
  every answer — exit 2 when anything differs. `sonde diff` compares two session files.
- The Timeline (macOS app and terminal UI): sessions, operations, their requests and the inspector; Record starts a
  local endpoint for the current endpoint and shows what to point an application at; an example session.
- Records keep control documents (DeleteObjects, tagging, lifecycle XML), for replay.
- `sonde-proxy` crate; SDK smoke tests (boto3, JS v3, Go v2, Java v2) run through `sonde run` in CI.

### Changed

- The app opens on the Timeline; in the terminal's home list it is `1`, so the other sections moved down by one.
- Integration tests run on Ceph RGW and RustFS; MinIO is optional (`MINIO_IMAGE`), its public images are gone.

### Fixed

- `install.sh` checks a `.sha256` per file: releases now publish them.
- The IMDSv2 token request sends `Content-Length: 0` (some metadata servers refused it with 411).
- The macOS disk image is sized explicitly (hdiutil sometimes ran out of room).

## [0.1.0] - 2026-11-06

### Added

- Endpoints: configuration (`config.toml`), import from AWS profiles, connectivity test with STS identity.
- Credentials: environment, profiles (static, `credential_process`, SSO, `role_arn`, web identity), IRSA,
  container credentials, IMDSv2; never exposed to the UI.
- `s3raw`: SigV4 (AWS test vectors), path- and virtual-hosted-style, custom CAs, retries with backoff, throttles.
- Request inspector: ring buffer and SQLite, redaction, filters, copy as aws cli / curl, JSON and HAR export.
- Browse: buckets, prefixes, 100 000+ objects, HEAD details (ETag and checksum analysis, SSE), downloads,
  presigned URLs, search.
- Multipart health, versions and delete markers, capacity breakdown.
- 40 compatibility probes, a vendor × probe matrix, saved runs and diffs, JSON and Markdown reports.
- Layout lens: SlateDB, Apache Iceberg, lakeFS, Milvus Woodpecker; cadence for turbopuffer and WarpStream; replay.
- Signed, sandboxed plugins; the Delta Lake lens example.
- Read-only by default; dry runs, typed confirmations and an audit log for destructive operations.
- The macOS app and the terminal UI from one TSX codebase; English and Chinese.
- `sonde` CLI: `probe`, `mpu`, `du`, `layout`, `endpoints`, `plugin`, `tui`.
