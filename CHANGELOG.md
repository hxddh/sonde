# Changelog

All notable changes are listed here. The project follows [Semantic Versioning](https://semver.org).

## [Unreleased]

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
