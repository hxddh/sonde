# Decisions

Newest last.

| # | Decision | Options | Choice and reason |
|---|---|---|---|
| D-001 | Language of the core | Rust; TSX with Rust helpers | **Rust for everything but screens**: the core is testable without a UI and survives a UI change. |
| D-002 | Native Shell version | crates.io 0.4.0; a git commit | **`longbridge/nsh` tag `v0.5.0`, commit `f4d68f7`**, pinned by full hash in `Cargo.toml` (`rev`) and CI (`NSH_REV`): the CLI must match the crates it links. |
| D-003 | SigV4 | `aws-sigv4`; our own | **Our own (`s3raw::sigv4`)**, tested against AWS's suite: it exposes the canonical request and allows deliberate skew for probes. |
| D-004 | HTTP | `reqwest` + rustls; hyper | **reqwest, rustls, native roots**: per-endpoint CAs, no OpenSSL. Redirects are not followed: a 301/307 is a finding. |
| D-005 | Where the listing lives | rows in JS; rows in Rust | **Rust (`Listing`)**: 100 000 rows never cross into the VM. |
| D-006 | Rust ↔ TSX types | typed `JsObject` structs; JSON | **JSON, mirrored by `app/src/types.ts`**: NSH traits on core types would break the layering. |
| D-007 | Static keys in the UI | a form field; config file only | **Config file only** (`config.toml`, 0600): a typed key would pass through the JS VM. |
| D-008 | Destructive confirmation | a checkbox; typed text | **Typed text** (the item count, or the bucket name); plans expire in 10 minutes and run once. |
| D-009 | Hermetic UI tests | mock in TSX; a server | **A demo S3 server (`sonde-core::demo`)**: real HTTP through the real client; also `sonde tui --demo`. |
| D-010 | The terminal host | `nsh-tui-host`; our own loop | **Our own loop on `nsh-platform-tui`** (FINDINGS #2): one binary with the bundle and sonde's CLI. |
| D-011 | Plugin views in the macOS app | a custom AppKit host; degrade | *(Superseded by D-031.)* **Degrade**: views opened in `sonde tui`. |
| D-012 | Plugin capability policy | trust the package; check | *(Superseded by D-031.)* **Check at install and on open**: only `sonde.read.*`. |
| D-013 | Second S3 implementation for CI | Ceph RGW; Garage; SeaweedFS | **Ceph RGW and RustFS**: the spec asks for Ceph; RustFS starts in seconds. MinIO optional (D-023). |
| D-014 | Iceberg sample | hand-written; pyiceberg | **pyiceberg 0.9.1** (`tools/gen_iceberg.py`): a real table with every kind of change. |
| D-015 | Woodpecker sample | a running Woodpecker; its codec | **Its own codec** (`tools/gen-woodpecker`, commit 3a8ce69): running it needs etcd and MinIO. |
| D-016 | lakeFS commits | parse KV dumps; trees only | **Trees only**: commits live in lakeFS's KV store, not the bucket; the UI says so. |
| D-017 | Closed formats | heuristics; cadence only | **Cadence only** (turbopuffer, WarpStream): their formats are not published. |
| D-018 | Probe verdict "differs" vs "fail" | — | **fail** when a guarantee breaks; **differs** for other statuses or shapes. |
| D-019 | Exit codes | 0/1; graded | **Graded** ([cli](cli.md#exit-codes)): CI fails on broken guarantees (1) and still reports differences (2). |
| D-020 | Linux binary | musl; glibc on an old base | **glibc on Ubuntu 22.04** (2.35): only libc/libm/libgcc_s; runs on Debian 12 slim. (A musl build was added later: [packaging](packaging.md).) |
| D-021 | Config path on macOS | `~/.config/sonde`; Application Support | **`~/Library/Application Support/sonde`**, the platform's convention; `$SONDE_CONFIG_DIR` / `$XDG_CONFIG_HOME` override. |
| D-022 | App capabilities | `fs`; none | **`clipboard`, `notifications`, `windows`, `picker`**: Rust writes every file; the VM needs no `fs` or `network`. |
| D-023 | Integration targets in CI | MinIO + Ceph + RustFS; Ceph + RustFS | **Ceph RGW + RustFS**: MinIO's images are no longer public. `scripts/integration.sh minio` with `MINIO_IMAGE` still works. |
| D-024 | macOS runners | macos-13/14; macos-15/15-intel | **macos-15 and macos-15-intel**: the older images never got a runner. |
| D-025 | Direction for 1.0 | a findings dashboard; the traffic | **The traffic** (record, explain, replay): sonde's edge is protocol truth; bucket dashboards compete with the cloud's tools. |
| D-026 | How the recorder sees requests | HTTPS MITM; an endpoint that re-signs | **An endpoint that re-signs**: no CA to install, and the program never holds the real credentials. |
| D-027 | 404 on GET/HEAD | errors; their own count | **"not found"**: formats probe for keys that do not exist yet (116 of SlateDB's 172 requests). |
| D-028 | Replay | re-send bytes; re-enact | **Re-enact** (synthetic payloads, mapped ids, a fresh prefix): sessions hold no object data, and stores issue their own ids. |
| D-029 | Direction for 2.0 | more sections; one place to start | **Run is the home**: a command beside its operations is the shortest path to the point. |
| D-030 | Navigation | nine sections; a few places and a palette | **Four places and a command palette** (⌘K, `:` in the terminal). |
| D-031 | Plugins | keep; remove | **Remove**: their cost (signing, trust, sandbox) outweighed views that opened only in the terminal. Supersedes D-011, D-012. |
| D-032 | The program's output | a log file; in the session | **In the session** (`{"log": …}` lines): one file, one clock. |
| D-033 | Sharing a session | a hosted service; a static site | **A static site** (`sonde share`): nothing to run, any host. |
| D-034 | Desktop platforms and updates | macOS only; all three | **macOS, Linux (GTK 4), Windows** from one TSX app; updates from GitHub Releases, only when asked. |
| D-035 | The look of 2.1 | restyle screen by screen; a design system first | **A design system first**: tokens, one icon family, a styled layer on nsh/base, then pages. |
| D-036 | Where details go | a third column; pages; an inspector | **An inspector**, a Sheet in a narrow window: the table keeps its room. |
| D-037 | The window's chrome | the system's title bar; the app's own | **The app's own toolbar** (`titleBar: hidden`), with a menu button on Linux and Windows: no repeated header. |
| D-038 | Saving settings | a Save button; as they change | **As they change**, with Undo: a change is a decision made. |
| D-039 | A visual check | by hand; in CI | **In CI** (`scripts/shots.sh`): behaviour tests cannot see a clipped column. |
| D-040 | Direction for 3.0 | more features; fewer places | **Fewer places** (Run, Sessions, Buckets); no new features. |
| D-041 | Bucket tools | tabs with forms; an overview | **An overview**: cheap facts fetched on open, costly ones when asked; the terminal keeps tabs. |
| D-042 | sonde's own requests, jobs, audit | three pages; one sheet | **Activity, one sheet**: one question, rarely why the app is opened. |
| D-043 | Localising the analysis | translate strings; phrases | **Phrases** (key + arguments): the CLI's English comes from the same templates, so they cannot drift. |
| D-044 | Compatibility | a Buckets tab; from the endpoints | **Its own window, from the endpoints**: it runs on several endpoints for minutes. |
| D-045 | Materials | opaque; glass | **Glass on floating surfaces, macOS only**: elsewhere an unblurred translucent card showed text through. |
| D-046 | Direction for 4.0 | new features; the design guides | **The guides, throughout**, written down in `app/DESIGN.md`; no new feature. |
| D-047 | Type | sizes per page; a scale | **Five roles** (11, 12, 13, 15, 20), tabular digits, the platform's monospace (FINDINGS 15): a desktop scale around a 13 pt body. |
| D-048 | A session's findings | chips with hover cards; a list | **A list**: a core result is not a chip, and a hover never carries what a reader needs. |
| D-049 | Overlays | each page its own; one of each | **`SidePanel` and `POPUP`**: three widths, one header, one scrim, one popup surface. |
| D-050 | The platform | the app's surfaces; the system's | **The system's** (sidebar material, Dock progress, represented file, tabs), plus an error boundary per place. |
| D-051 | Direction for 5.0 | more surfaces; the core result right | **The core result right**: bugs first, then faster and ranked findings, sessions to look after. |
| D-052 | Ranking findings | by kind; by requests | **Warnings first, then by requests affected**, with a saving where countable: requests are what a session measures honestly. |
| D-053 | Throttling advice | a fixed concurrency; the session's | **The fewest in flight when throttled**; at 4 or fewer, the rate per prefix instead. |
| D-054 | Sharing from the app | copy the command; write and serve | **Write and serve the page** on 127.0.0.1; a build without the viewer copies the command. |
| D-055 | A session's titles | rewrite the header; a side file | **`titles.json` beside the sessions**: a session file stays as recorded, and renaming is free. |
| D-056 | Direction for 6.0 | more findings; time and cost | **Time and cost, seen** (`profile.rs`, compare, Plot charts); bugs first. |
| D-057 | A kind's time | latencies added; union | **The union of its intervals**: ten parallel 50 ms reads are 50 ms of waiting. |
| D-058 | Request cost | none; configurable prices | **S3 Standard us-east-1 by default** (`settings.prices.*`): an estimate for comparing runs, not a bill. |
| D-059 | Charts | own canvases; Plot | **Plot**: native marks, hover, keyboard and accessibility; the canvas redrew on every mouse move. |
| D-060 | Comparing two sessions | answers only; before and after | **Before and after**, unless one is a replay: most comparisons are of a change. |
| D-061 | Direction for 7.0 | one session deeper; a program over its runs | **A program over its runs**, with a baseline, in the app and CI: most questions are about a change. |
| D-062 | What a program is | a given name; the command | **The command, spaces collapsed** (named later: D-110): nothing to set up. Baselines in `baselines.json`. |
| D-063 | Gating CI | a new tool; `sonde check` | **`sonde check`**: budgets and a baseline (10% tolerance), exit 1 when over; the same check as the app. |
| D-064 | Prefix rates | documented; measured | **S3's 5,500 reads / 3,500 writes per second per prefix**: a finding at half, a warning past 80% or throttled; the finding names its figure. |
| D-065 | The flow chart | per request; per kind and prefix | **Operation → prefix → outcome**, top 6 and 8, the rest "other": a Sankey reads only with few nodes. |
| D-066 | A monospace font | bundle one; the platform's | **The platform's (`ui-monospace`)**: a bundled font is mismeasured on GTK (FINDINGS 15, 18). |
| D-067 | Production traffic | a log shipper; import logs | **Import** (access log, CloudTrail, HAR): the same findings with no proxy. |
| D-068 | The timeline | its own pane; a When column | **The When column**: a Gantt beside it said the same thing twice. |
| D-069 | Updating sonde's code | new apps only; signed packages too | **Signed packages too**, once the project has keys; skipped without them. |
| D-070 | The run history | Combobox; the system's menu | **The system's menu**: in a Combobox, Return would pick a row instead of running. |
| D-071 | Views following the engine | timers; events | **Engine events** (`Sonde.on`): nothing runs while nothing changes. Run's live view still polls its file. |
| D-072 | Split views | fixed; ResizablePanelGroup | **ResizablePanelGroup**: lists keep their width, the inspector shares the rest. |
| D-073 | Which code sent a request | profilers; a header | **`x-sonde-caller`**, read and never forwarded; opt-in hooks set it. Nothing injected unless asked. |
| D-074 | A project's budget | CI flags; a file | **`sonde.toml` `[check]`**, found up to the repository root; flags win. |
| D-075 | Sharing of the time | busy time per kind; split | **Split among requests in flight**: shares add up to the waiting. |
| D-076 | Activity's requests | an inspector; a session | **Both**: an inspector, and "Open as a session". |
| D-077 | The run history field | Combobox; the system's menu | Kept from D-070: Return runs what was typed. |
| D-078 | The look of the app | nsh's look; the app's kit | **The app's kit** (`ui/kit.tsx`): nsh base components have no look; `scripts/lint-kit.py` keeps pages from styling them. |
| D-079 | Disclosure | Button + Collapsible; Accordion | **Accordion**: a real heading and an expanded state a Button cannot say. |
| D-080 | Requests drawn one by one | the slowest 400; all up to 4000 | **All up to 4000, else the slowest 400 and a spread**: the tail alone looks like a slow program. |
| D-081 | A run and its commit | none; git, then CI | **`git`, else `GITHUB_SHA`**, with branch and dirty flag: ties a regression to a change. |
| D-082 | Production from logs | a daemon; `watch --logs` | **`sonde watch --logs`**: each pass a run with a baseline and `check`. |
| D-083 | Prices per view | the settings'; the endpoint's | **The session's endpoint's**: one price per session everywhere. |
| D-084 | Callers beyond Python | none; a hook per SDK | **A hook per SDK** via `NODE_OPTIONS`, `JAVA_TOOL_OPTIONS`; Java only when the SDK is on the class path, never breaking the program. |
| D-085 | Latency | to the headers; the whole exchange | **The whole exchange, first byte beside it**: a program waits for the body too. |
| D-086 | Logs without times | zeros; nothing said | **Nothing said**: zeros would read as fast (CloudTrail). |
| D-087 | Findings kept on purpose | none; `expect` | **`expect` in `sonde.toml`**: the project says it, with its code. |
| D-088 | Sizes | per control; tokens | **Tokens (`CONTROL`, `ROW`, `FORM`) and a lint**: 12.0 had five of each. |
| D-089 | The window's columns | four; two and an inspector | **Two columns and an inspector**: the sidebar holds places and the current list. |
| D-090 | A session's page | findings above tabs; tabs and the pane | **The pane takes the room under its tabs**: findings belong with operations. |
| D-091 | The Time pane | charts stacked; a board | **A board** under one timeline: nine stacked sections were a scroll, not a view. |
| D-092 | Requests in flight | a step line; lanes | **Both**: the line says how many, the lanes which (96 slices × up to 24 lanes). |
| D-093 | A config that does not load | start empty; refuse to save | **Refuse to save, and say so**; a save re-reads the file and applies only its change. |
| D-094 | Read-only and probes | exempt; refused | **Refused**: probes write, delete and create buckets. |
| D-095 | A proxied exchange's time limit | 60 s in all; 60 s idle | **60 s without a byte**: transfers of any length go through while they move. |
| D-096 | Durations | two formatters; one | **One (`duration`)**, unit chosen after rounding, up to hours. |
| D-097 | What a fault rule counts | requests as they arrive; attempts at one request | **Attempts at one request**: "the third" is a race under concurrency. |
| D-098 | What "survived" means | the data is right; the requests show it | **What the requests show**; sonde does not read data back, and says so. |
| D-099 | A trial's sessions | among the runs; their own folder | **`trials/` beside the run**: faulted runs would skew trend, baseline and "newest". |
| D-100 | Running a command several times | sonde picks prefixes; the command says | **The command says** (`{trial}`, `SONDE_TRIAL`): sonde cannot know which argument is a place. |
| D-101 | Prefixes in the analysis | three levels; whole folders, folded | **Whole folders, the quietest folded up** when there are too many: three levels merged `wal/2026/11/` and `wal/2026/12/`. |
| D-102 | More engines | a version's theme; when recorded | **Only from a real recording**, kept as a test. |
| D-103 | What counts as a retry | same method and key later; against the clean run | **Against the clean run**: only it knows what the program does unprovoked. |
| D-104 | A request without a key | one per method and bucket; paging and body | **Sorted paging parameters and the body's hash** (`x-sonde-body-sha256`): two batches are two requests. |
| D-105 | A run's header | six figures; four signals | **Rate, errors, latency, bytes** (and in flight), in one order everywhere. |
| D-106 | Where the signals are computed | the app; Rust | **Rust (`signals.rs`)**: one JSON for app, page and CLI. |
| D-107 | Charts | each its own; a registry and a lint | **One frame (`Chart`) and registries** checked by `scripts/lint-charts.py`. |
| D-108 | A narrow pane | wrap and clip; rank | **Rank**: the lowest figures and columns leave first; nothing clipped mid-word. |
| D-109 | Rust's sentences in Chinese | translate in Rust; phrases | **Phrases** (`decoders::phrase`, `corephrases.ts`); a test fails on a key without a template. The CLI stays English. |
| D-110 | A program's identity | the command line; a name | **`[program] name` or `--name`, else the command**: no existing baseline is lost. |
| D-111 | A trial in the app | a row of verdicts; a marked session | **A marked session**, in no list (D-099); refused on a read-only endpoint. |
| D-112 | "Acting on N findings" | an estimate; the requests named | **The named requests, subtracted**: the estimate overstated tiny HEADs. |
| D-113 | Moving the Native Shell pin | with UI work; last | **Last, alone**: a moved pixel has one cause. |
| D-114 | A trial's name in a key | as written; taken out | **Taken out as a whole word**: `etl-{trial}` differs per run. |
| D-115 | Which answers owe a retry | each its own list; one | **One function** (`status_owes_retry`: 408, 429, 500, 502, 503, 504); 501 owes none. |
| D-116 | A lost conditional write | sonde writes a competitor; 412 to the program's own | **The program's write goes through and is answered 412**: sonde writes nothing of its own into a user's bucket. |
| D-117 | What a conditional-write fault counts | attempts per key; per commit | **Per commit (`commit_family`)**: a retry of version 7 is version 8, a new key. |
| D-118 | A body cut short | Range in identity; out | **Out**: the resume is a ranged GET and must not be cut again. |
| D-119 | How a session is read | whole; a stream | **A stream, analysed once**, keeping only what analysis reads; format unchanged. Budgets are a test. |
| D-120 | The app's view of a session | every operation; a page | **First 500 and totals, the rest by page**: the view was 351 MB at a million requests. |
| D-121 | What is counted of a session | parsed per listing; kept beside it | **`<stem>.summary.json`**: listing took 24 s for one large file. |
| D-122 | A short run | rates; totals | **Totals under 1 s or 20 requests**: such a rate is not measured. |
| D-123 | Zooming a chart | wheel and drag; two presses | **Two presses**: a chart taking the wheel stops the page scrolling. |
| D-124 | Log times to the second | as written; spread | **Spread over their second, in order, and said so**: otherwise "108 at once" was said of 16 workers. |
| D-125 | A trial's key that holds a run's name | take out the run's own name; substitute where the template had `{trial}`; both names out to look up, then match | **Both names out to look up; as written, else the scenario's name in some of `clean`'s places**: a program builds keys from `SONDE_TRIAL` any way, so the template cannot say where the name went. |
| D-126 | Which failures owe a retry | by status in trials; by status and code elsewhere | **One function (`episode::retryable`), by status and S3 code** (SlowDown, RequestTimeout, RequestTimeTooSkewed): S3 sends RequestTimeout as 400. Supersedes D-115's status rule. |
| D-127 | Where `lost-race` is offered | every conditional write; a commit log | **A create in a table's log, or an `If-Match` write**; a create of a key of its own taken as made is owed nothing: the store holds it. |
| D-128 | `skew` | move the clock; answer 403 RequestTimeTooSkewed | **Answer it on a first attempt**, when the clean run signed its requests. |
| D-129 | `complete-500` | a fault of its own; the lost answer on CompleteMultipartUpload | **The lost answer**, judged by its rules; an upload started again is degraded, not broke: the object is right. |
| D-130 | `slow-first-byte` | every GET held; one | **One download held 10 s**: a trial costs 10 s at most. Waited out is degraded. |
| D-131 | `stale-list` | judge the listing's use; the key's requests | **The key's requests**: read less is broke, written again degraded, untouched untested. |
| D-132 | What a recorded program may do by default | allow; no-delete | **no-delete** for `run`, `proxy`, `watch` and the app's Run; the demo allows. |
| D-133 | How no-delete keeps objects | a HEAD before each write; `If-None-Match: *` | **`If-None-Match: *` on PutObject and CompleteMultipartUpload**, a HEAD for CopyObject: atomic, no extra request. A store that ignores it writes over (said; use read-only). |
| D-134 | What no-delete lets a run write again | nothing; what it wrote | **What it wrote (a 2xx)**; a trial's runs share the clean run's. |
| D-135 | A trial's policy | allow; the run's | **The run's**. |
| D-136 | Read-only mode and Run | coupled; separate | **Separate**: read-only covers sonde's own acts; coupling would make every first Run read-only. |
| D-137 | Where the policy is widened | flag, sonde.toml, setting | **CLI: flag, then `[record] policy`, then no-delete. App: the setting; a project only narrows it.** |
| D-138 | When the major changes | every release; when a stable thing breaks | **When a stable thing breaks** ([contract.md](contract.md)): session format, JSON outputs, `sonde.toml`, flags, exit codes. |
| D-139 | JSON schemas | closed; open; a `schema` field | **Open objects, no field**, checked by a small validator in a test: adding a field is a minor. |
| D-140 | Session format version | 2; 1 | **1**, with an optional `sonde` field; a newer format is refused by name. |
| D-141 | Files at rest | umask; 0600 | **0600 when made** (Unix): sessions, caches, verdicts, baselines, audit log. |
| D-142 | What a request costs in memory | keep everything; keep what analysis reads | **A keep-list of headers, shared text (`reqlog::Text`), keys interned**: 1.1 KB a request (1.75). |
| D-143 | `compare` | both runs whole; one at a time | **One at a time** (`profile::Run`); a kind's waiting is its share, as in `show`. |
| D-144 | The summary cache | stamp after reading; before | **Stamped before, in ns, keyed by the analysis version**. |
| D-145 | A damaged session | refuse; read what is whole | **A torn last line or cut gzip is read to the cut, with a warning; a bad middle line is refused (65)**. |
| D-146 | Waiting a baseline allows | its tolerance + 5 ms; + 50 ms | **+ 50 ms**: a short run on a CI runner varies by tens of ms. |
| D-147 | Pointing Spark and Flink at sonde | `-D` properties; `core-site.xml`; their command line | **Their command line, keys from the environment**: Hadoop reads no `-D` properties, and argv is readable by every user. |
| D-148 | TLS on the endpoint | a second port; one port | **One port, told by the first byte**; a leaf from an authority whose key is never kept. |
| D-149 | S3 Express One Zone | pass through; re-sign; refuse | **Refuse with the reason (501)** until it can be tested against AWS. |
| D-150 | Hudi and Paimon | layouts; operations | **Operations**; layouts later. |
| D-151 | A rename-based commit | a step; a finding | **A warning (`obs.rename`)** with the S3A committer as the fix. |
| D-152 | The Java hook's jars | built at release; committed | **Committed, rebuilt in CI and compared byte for byte**; packed with zip so any JDK makes the same bytes. |
| D-153 | Writing figures | per crate; one | **One crate (`sonde-fmt`), the app's rounding**, checked against the app's cases. |
| D-154 | Windows | app only; tests and a CLI | **Tests in CI and a `.zip` CLI**; shell-specific tests stay Unix. |
| D-155 | Timed tests | every push; nightly | **Nightly**; a missed memory budget runs once more. |
| D-156 | Supply chain | tags; commits | **Actions pinned by commit, Dependabot, cargo-deny (ignores with reasons), provenance attestations**; an unsigned release warns. |
| D-157 | Fuzzing | every push; weekly | **Weekly**, ten targets. |
| D-158 | Chinese for a broke trial | 失败; 未通过 | **未通过**: 失败 is also a failed request, job and budget. |
| D-159 | A short run's sparkline | under 20 requests; under 1 s or 20 | **Both**, in the app and in `show`. |
| D-160 | Findings in a short pane | a fixed height; whole findings | **Whole findings, with how many are in view**: no row cut. |
| D-161 | A budget on its chart | the check's verdicts; `sonde.toml` | **`sonde.toml`** (`max-p95`, `max-errors` as a share, `max-waiting`), drawn as a dashed line. |
| D-162 | A range in a link | slice indices; ms | **ms**: slices are the reader's, times the session's. |
| D-163 | A chart's CSV | clipboard; save dialog; exports folder | **Exports folder**: Native Shell has no save dialog. |
| D-164 | Widths in pages | constants per file; the kit's names | **The kit's `COLUMN` and `PANE`**, read by the layout lint. |
| D-165 | A dialog taller than the window | fixed height; its body scrolls | **`ModalBody` scrolls; the title and actions stay.** |
| D-166 | `record.rs` | one file; a module per command | **A module per command**; shared start code in `record.rs`. |
| D-167 | Read-only mode and the recording policy | both; one | **The recording policy only.** sonde's own destructive acts keep the dry run, typed confirmation and audit log. |
| D-168 | A folded command | remove; alias for one version | **An alias for 18.0** that says its new form; gone in 19.0. |
| D-169 | `watch --logs` | `proxy`; `import` | **`import s3://… [--follow]`**: one verb reads logs. |
| D-170 | `mpu` under `bucket` | nested; flat | **`size`, `uploads`, `abort-uploads`, `lifecycle`**, flat. |
| D-171 | What sonde keeps of each session | files beside it; one index per folder | **`.index.jsonl` per folder**, append-only, compacted when mostly stale. |
| D-172 | A setting gone from `config.toml` | an error; ignored | **Ignored, said once, dropped at the next save**: an older file always loads. |
| D-173 | What no-delete lets a program write over | its own run's writes; its earlier runs' writes | **What its earlier runs wrote** (per program and endpoint, `written.json`), and keys under a `{trial}` place: a re-run and a re-trial work. |
| D-174 | When `skew` is offered | always; by SDK | **When the SDK retries it** (Java, JavaScript), or when named: botocore cannot. |
| D-175 | Trying again | everything; what did not survive | **`--trial --again`: broke, degraded and refused**, with each scenario's history per program (`verdicts.json`). |
| D-176 | A refusal by the policy in a trial | broke; its own | **`refused`**: the program was not judged; `check` warns. |
| D-177 | A finding across runs | by id; by kind and what it is about | **Kind and what it is about**; states fixed, less, back, new, open per program (`findings.json`). |
| D-178 | Fixed or less | under the threshold; gone at any threshold | **Gone at any threshold**, else "less"; a fixed finding shows its measured saving beside the predicted. |
| D-179 | Callers | opt-in; on | **On by default** (`--no-callers`): the fix loop needs the line; the Python hook loads lazily. |
| D-180 | Default `show` | everything; the summary | **The summary, findings first, one table**; the rest behind `--all`. |
| D-181 | The CI baseline | a local cache; the base branch's artifact | **The newest push run's artifact on the base branch**; none is a warning. |
| D-182 | The PR comment | every row; what changed | **Verdict, failed rows, what changed (new findings first), one link.** |
| D-183 | The GitHub Action | a repository of its own; this one | **This repository's root (`hxddh/sonde@v19`)**, versioned with the binary. |
| D-184 | Why slow | a chart per dimension; one split table | **One table split by a dimension, one sentence** (`show --by`, Group by). |
| D-185 | Run after a run | operations; findings | **The top three findings and one what-if**, and what changed since the run before. |
| D-186 | The Time pane | fewer charts; all | **All, shown**, read Traffic → Latency → Errors → Where, each titled by its question, following the stretch. |
| D-187 | Store tools | sheets and windows; sections | **One row of sections in Buckets**, reached from findings. Supersedes D-044. |
| D-188 | Interaction | per screen; one grammar | **Select → inspect → act**; one inspector; every act on a key or in the palette. |
| D-189 | Approving a visual change | a person looks; a committed baseline | **`app/shots.approved.json`**: a change past 0.5% not approved fails CI. |
| D-190 | sonde's own requests | a list in Activity; nowhere | **Nowhere**: an error says its code, status and request id. |
| D-191 | Compare's order | as opened; earlier first | **Earlier first**, from every door. |
| D-192 | Compatibility | keep shims; none | **None from 19.0**: old names, flags, files and the folded commands are gone; the contract starts at 19.0. |
| D-193 | A finding across runs | its text; a normalised identity | **A normalised identity** (numbers, ids and dates stripped from keys and paths), so a fix is seen as one. |
| D-194 | Distribution | Homebrew too; release assets | **Release assets, `install.sh` and the Action** (`@v19`); Homebrew dropped. |
| D-195 | CI | every OS each change; Linux, others nightly | **Linux per change, macOS/Windows/Intel nightly and at release**; the release reuses CI's builds. |
