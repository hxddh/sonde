# Decisions

Newest last; the decisions in force. Those a later one replaced: [history](../history/decisions-superseded.md).

| # | Decision | Options | Choice and reason |
|---|---|---|---|
| D-001 | Language of the core | Rust; TSX with Rust helpers | **Rust for everything but screens**: the core is testable without a UI and survives a UI change. |
| D-003 | SigV4 | `aws-sigv4`; our own | **Our own (`s3raw::sigv4`)**, tested against AWS's suite: it exposes the canonical request and allows deliberate skew for probes. |
| D-005 | Where the listing lives | rows in JS; rows in Rust | **Rust (`Listing`)**: 100 000 rows never cross into the VM. |
| D-006 | Rust ↔ TSX types | typed `JsObject` structs; JSON | **JSON, mirrored by `app/src/types.ts`**: NSH traits on core types would break the layering. |
| D-007 | Static keys in the UI | a form field; config file only | **Config file only** (`config.toml`, 0600): a typed key would pass through the JS VM. |
| D-009 | Hermetic UI tests | mock in TSX; a server | **A demo S3 server (`sonde-core::demo`)**: real HTTP through the real client; also `sonde tui --demo`. |
| D-010 | The terminal host | `nsh-tui-host`; our own loop | ~~Our own loop on `nsh-platform-tui`~~ — superseded by D-291: no terminal UI. |
| D-013 | Second S3 implementation for CI | Ceph RGW; Garage; SeaweedFS | **Ceph RGW and RustFS**: the spec asks for Ceph; RustFS starts in seconds. MinIO optional (D-023). |
| D-014 | Iceberg sample | hand-written; pyiceberg | **pyiceberg 0.9.1** (`tools/gen_iceberg.py`): a real table with every kind of change. |
| D-015 | Woodpecker sample | a running Woodpecker; its codec | **Its own codec** (`tools/gen-woodpecker`, commit 3a8ce69): running it needs etcd and MinIO. |
| D-016 | lakeFS commits | parse KV dumps; trees only | **Trees only**: commits live in lakeFS's KV store, not the bucket; the UI says so. |
| D-017 | Closed formats | heuristics; cadence only | **Cadence only** (turbopuffer, WarpStream): their formats are not published. |
| D-018 | Probe verdict "differs" vs "fail" | — | **fail** when a guarantee breaks; **differs** for other statuses or shapes. |
| D-019 | Exit codes | 0/1; graded | **Graded** ([cli](../cli.md#exit-codes)): CI fails on broken guarantees (1) and still reports differences (2). |
| D-020 | Linux binary | musl; glibc on an old base | **glibc on Ubuntu 22.04** (2.35): only libc/libm/libgcc_s; runs on Debian 12 slim. A static musl build beside it serves musl systems and older glibc ([packaging](packaging.md)). |
| D-021 | Config path on macOS | `~/.config/sonde`; Application Support | **`~/Library/Application Support/sonde`**, the platform's convention; `$SONDE_CONFIG_DIR` / `$XDG_CONFIG_HOME` override. |
| D-022 | App capabilities | `fs`; none | **`application`, `clipboard`, `notifications`, `windows`, `statusBar`, `picker`, `openUrl` (http, https, file)**: Rust writes every file; the VM needs no `fs` or `network`. |
| D-023 | Integration targets in CI | MinIO + Ceph + RustFS; Ceph + RustFS | **Ceph RGW + RustFS**: MinIO's images are no longer public. `scripts/integration.sh minio` with `MINIO_IMAGE` still works. |
| D-024 | macOS runners | macos-13/14; macos-15/15-intel | **macos-26 and macos-26-intel**: the older images never got a runner. |
| D-025 | Direction for 1.0 | a findings dashboard; the traffic | **The traffic** (record, explain, replay): sonde's edge is protocol truth; bucket dashboards compete with the cloud's tools. |
| D-026 | How the recorder sees requests | HTTPS MITM; an endpoint that re-signs | **An endpoint that re-signs**: no CA to install, and the program never holds the real credentials. |
| D-027 | 404 on GET/HEAD | errors; their own count | **"not found"**: formats probe for keys that do not exist yet (116 of SlateDB's 172 requests). |
| D-028 | Replay | re-send bytes; re-enact | **Re-enact** (synthetic payloads, mapped ids, a fresh prefix): sessions hold no object data, and stores issue their own ids. |
| D-029 | Direction for 2.0 | more sections; one place to start | ~~Run is the home~~ — superseded by D-292: Runs is the one place for runs. |
| D-031 | Plugins | keep; remove | **Remove**: their cost (signing, trust, sandbox) outweighed views that opened only in the terminal. Supersedes D-011, D-012. |
| D-032 | The program's output | a log file; in the session | **In the session** (`{"log": …}` lines): one file, one clock. |
| D-033 | Sharing a session | a hosted service; a static site | **A static site** (`sonde share`): nothing to run, any host. |
| D-034 | Desktop platforms and updates | macOS only; all three | **macOS, Linux (GTK 4), Windows** from one TSX app; updates from GitHub Releases, only when asked. |
| D-035 | The look of 2.1 | restyle screen by screen; a design system first | **A design system first**: tokens, one icon family, a styled layer on nsh/base, then pages. |
| D-036 | Where details go | a third column; pages; an inspector | **An inspector**, a Sheet in a narrow window: the table keeps its room. |
| D-037 | The window's chrome | the system's title bar; the app's own | **The app's own toolbar** (`titleBar: hidden`), with a menu button on Linux and Windows: no repeated header. |
| D-038 | Saving settings | a Save button; as they change | **As they change, silently**: a change is a decision made, and a system's settings apply without a word. |
| D-039 | A visual check | by hand; in CI | **In CI** (`scripts/shots.sh`): behaviour tests cannot see a clipped column. |
| D-040 | Direction for 3.0 | more features; fewer places | **Fewer places** (Run, Sessions, Buckets); no new features. |
| D-041 | Bucket tools | tabs with forms; an overview | **An overview**: cheap facts fetched on open, costly ones when asked; the terminal keeps tabs. |
| D-042 | sonde's own requests, jobs, audit | three pages; one sheet | **Activity, one sheet**: one question, rarely why the app is opened. |
| D-043 | Localising the analysis | translate strings; phrases | **Phrases** (key + arguments): the CLI's English comes from the same templates, so they cannot drift. |
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
| D-055 | A run's title | rewrite the header; a side file | **In the runs folder's index (`.index.jsonl`)**: a run's file stays as recorded, and renaming is free. |
| D-056 | Direction for 6.0 | more findings; time and cost | **Time and cost, seen** (`profile.rs`, compare, Plot charts); bugs first. |
| D-057 | A kind's time | latencies added; union | **The union of its intervals**: ten parallel 50 ms reads are 50 ms of waiting. |
| D-058 | Request cost | none; configurable prices | **S3 Standard us-east-1 by default** (`settings.prices.*`): an estimate for comparing runs, not a bill. |
| D-059 | Charts | own canvases; Plot | **Plot**: native marks, hover, keyboard and accessibility; the canvas redrew on every mouse move. |
| D-060 | Comparing two sessions | answers only; before and after | **Before and after**, unless one is a replay: most comparisons are of a change. |
| D-061 | Direction for 7.0 | one session deeper; a program over its runs | **A program over its runs**, with a baseline, in the app and CI: most questions are about a change. |
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
| D-077 | The run history field | Combobox; the system's menu | Kept from D-070: Return runs what was typed. |
| D-078 | The look of the app | nsh's look; the app's kit | **The app's kit** (`ui/kit.tsx`): nsh base components have no look; `scripts/lint-kit.py` keeps pages from styling them. |
| D-079 | Disclosure | Button + Collapsible; Accordion | **Accordion**: a real heading and an expanded state a Button cannot say. |
| D-080 | Requests drawn one by one | the slowest 400; all up to 4000 | **All up to 4000, else the slowest 400 and a spread**: the tail alone looks like a slow program. |
| D-081 | A run and its commit | none; git, then CI | **`git`, else `GITHUB_SHA`**, with branch and dirty flag: ties a regression to a change. |
| D-083 | Prices per view | the settings'; the endpoint's | **The session's endpoint's**: one price per session everywhere. |
| D-084 | Callers beyond Python | none; a hook per SDK | **A hook per SDK** via `NODE_OPTIONS`, `JAVA_TOOL_OPTIONS`; Java only when the SDK is on the class path, never breaking the program. |
| D-085 | Latency | to the headers; the whole exchange | **The whole exchange, first byte beside it**: a program waits for the body too. |
| D-086 | Logs without times | zeros; nothing said | **Nothing said**: zeros would read as fast (CloudTrail). |
| D-087 | Findings kept on purpose | none; `expect` | **`expect` in `sonde.toml`**: the project says it, with its code. |
| D-088 | Sizes | per control; tokens | **Tokens (`CONTROL`, `ROW`, `FORM`) and a lint**: 12.0 had five of each. |
| D-089 | The window's columns | four; two and an inspector | **Two columns and an inspector**: the sidebar holds places and the current list. |
| D-090 | A session's page | findings above tabs; tabs and the pane | **The pane takes the room under its tabs**: findings belong with operations. |
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
| D-109 | Rust's sentences in Chinese | translate in Rust; phrases | **Phrases** (`decoders::phrase`; their words are `phrase.*` in `app/locales`); a test fails on a key without a template. The CLI stays English. |
| D-111 | A trial in the app | a row of verdicts; a marked session | **A marked session**, in no list (D-099), and a run's row says nothing of its trials; refused on a read-only endpoint. |
| D-112 | "Acting on N findings" | an estimate; the requests named | **The named requests, subtracted**: the estimate overstated tiny HEADs. |
| D-113 | Moving the Native Shell pin | with UI work; last | **Last, alone**: a moved pixel has one cause. |
| D-114 | A trial's name in a key | as written; taken out | **Taken out as a whole word**: `etl-{trial}` differs per run. |
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
| D-127 | Where `lost-race` is offered | every conditional write; a commit log | **A create in a table's log, or an `If-Match` write**; what the lost write does to the store is D-323. |
| D-128 | `skew` | move the clock; answer 403 RequestTimeTooSkewed | **Answer it on a first attempt**, when the clean run signed its requests. |
| D-129 | `complete-500` | a fault of its own; the lost answer on CompleteMultipartUpload | **The lost answer**, judged by its rules; an upload started again is degraded, not broke: the object is right. |
| D-130 | `slow-first-byte` | every GET held; one | **One download held 10 s**: a trial costs 10 s at most. Waited out is degraded. |
| D-131 | `stale-list` | judge the listing's use; the key's requests | **The key's requests**: read less is broke, written again degraded, untouched untested. |
| D-132 | What a recorded program may do by default | allow; no-delete | **no-delete** for `run`, `proxy`, `watch` and the app's Run; the demo allows. |
| D-134 | What no-delete lets a run write again | nothing; what it wrote | **What it wrote (a 2xx)**; a trial's runs share the clean run's. |
| D-135 | A trial's policy | allow; the run's | **The run's**. |
| D-136 | Read-only mode and Run | coupled; separate | **Separate**: read-only covers sonde's own acts; coupling would make every first Run read-only. |
| D-137 | Where the policy is widened | flag, sonde.toml, setting | **CLI: flag, then `[record] policy`, then no-delete. App: the setting; a project only narrows it.** |
| D-138 | When the major changes | every release; when a stable thing breaks | **When a stable thing breaks** ([contract.md](../contract.md)): session format, JSON outputs, `sonde.toml`, flags, exit codes. |
| D-139 | JSON schemas | closed; open; a `schema` field | **Open objects, no field**, checked by a small validator in a test: adding a field is a minor. |
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
| D-156 | Supply chain | tags; commits | **Actions pinned by commit, Dependabot, cargo-deny (ignores with reasons), provenance attestations**; an unsigned release warns. |
| D-157 | Fuzzing | every push; weekly | **Weekly**, ten targets. |
| D-158 | Chinese for a broke trial | 失败; 未通过 | **未通过**: 失败 is also a failed request, job and budget. |
| D-159 | A short run's sparkline | under 20 requests; under 1 s or 20 | **Both**, in the app and in `show`. |
| D-160 | Findings in a short pane | a fixed height; whole findings | **Whole findings, with how many are in view**: no row cut. |
| D-161 | A budget on its chart | the check's verdicts; `sonde.toml` | **`sonde.toml`** (`max-p95`, `max-errors` as a share, `max-waiting`), drawn as a dashed line. |
| D-162 | A range in a link | slice indices; ms | **ms**: slices are the reader's, times the session's. |
| D-164 | Widths in pages | constants per file; the kit's names | **The kit's `COLUMN` and `PANE`**, read by the layout lint. |
| D-165 | A dialog taller than the window | fixed height; its body scrolls | **`ModalBody` scrolls; the title and actions stay.** |
| D-166 | `record.rs` | one file; a module per command | **A module per command**; shared start code in `record.rs`. |
| D-167 | Read-only mode and the recording policy | both; one | **The recording policy only.** sonde's own destructive acts keep the dry run, typed confirmation and audit log. |
| D-169 | `watch --logs` | `proxy`; `import` | **`import s3://… [--follow]`**: one verb reads logs. |
| D-170 | `mpu` under `bucket` | nested; flat | **`size`, `uploads`, `abort-uploads`, `lifecycle`**, flat. |
| D-171 | What sonde keeps of each session | files beside it; one index per folder | **`.index.jsonl` per folder**, append-only, compacted when mostly stale. |
| D-172 | A setting gone from `config.toml` | an error; ignored | **Ignored, said once, dropped at the next save**: an older file always loads. |
| D-174 | When `skew` is offered | always; by SDK | **When the SDK retries it** (Java, JavaScript), or when named: botocore cannot. |
| D-175 | Trying again | everything; what did not survive | **`--trial --again`: broke, degraded and refused**, with each scenario's history per program (in the runs folder's `.index.jsonl`). |
| D-176 | A refusal by the policy in a trial | broke; its own | **`refused`**: the program was not judged; `check` warns. |
| D-177 | A finding across runs | by id; by kind and what it is about | **Kind and what it is about**; states fixed, less, back, new, open per program (kept in `.index.jsonl`). |
| D-178 | Fixed or less | under the threshold; gone at any threshold | **Gone at any threshold**, else "less"; a fixed finding shows its measured saving beside the predicted. |
| D-179 | Callers | opt-in; on | **On by default** (`--no-callers`): the fix loop needs the line; the Python hook loads lazily. |
| D-180 | Default `show` | everything; the summary | **The summary, findings first, one table**; the rest behind `--all`. |
| D-181 | The CI baseline | a local cache; the base branch's artifact | **The newest push run's artifact on the base branch**; none is a warning. |
| D-182 | The PR comment | every row; what changed | **Verdict, failed rows, what changed (new findings first), one link.** |
| D-183 | The GitHub Action | a repository of its own; this one | **This repository's root (`hxddh/sonde@vN`)**, versioned with the binary. |
| D-184 | Why slow | a chart per dimension; one split table | **One table split by a dimension, one sentence** (`show --by`, Group by). |
| D-185 | Run after a run | operations; findings | **The top three findings and one what-if**, and what changed since the run before. |
| D-186 | The Time pane | fewer charts; all | **All, shown**, read Traffic → Latency → Errors → Where, each titled by its question, following the stretch. |
| D-187 | Store tools | sheets and windows; sections | **One row of sections in Buckets**, reached from findings. Supersedes D-044. |
| D-188 | Interaction | per screen; one grammar | **Select → inspect → act**; one inspector; every act on a key or in the palette. |
| D-189 | Approving a visual change | a person looks; a committed baseline | **`app/shots.approved.json`**: a change past 0.5% not approved fails CI. |
| D-190 | sonde's own requests | a list in Activity; nowhere | **Nowhere**: an error says its code, status and request id. |
| D-191 | Compare's order | as opened; earlier first | **Earlier first**, from every door. |
| D-192 | Compatibility | keep shims; none | **None from 19.0**: old names, flags, files and the folded commands are gone; the contract starts at 19.0. |
| D-194 | Distribution | Homebrew too; release assets | **Release assets, `install.sh` and the Action** (`@v19`); Homebrew dropped. |
| D-196 | tui, bucket, probe | remove; keep | **Kept** as they are. |
| D-198 | A finding's identity | 19.0's rules; v2 | **Rules v2** (`key=value` values, digit runs, hex ids), versioned; where two findings collide, they stay apart. |
| D-199 | A program | its command; its command less what changes | **Its name, else its command with whole-word values left out**: `python3` and `job1.py` stay. |
| D-200 | An S3 call's name | operation; kind | **`kind`** everywhere; session format 2. |
| D-201 | Settings | one file; two | **`config.toml` what sonde keeps, `sonde.toml` what a project says**; the recording policy is the project's. |
| D-202 | Display text | in the contract; not | **Not**: figures, ids, fields and exit codes are promised. |
| D-203 | A release | builds its own; CI's | **CI's green build of the same commit**; main tours every platform. Supersedes D-195. |
| D-204 | The tour's measured screens | exempt; fixtures | **Committed sessions**: no picture is exempt. |
| D-205 | Timed tests | nightly with a retry; every change, held | **Every change, held at peak memory with no retry** (each test optimised, in a process of its own); time is said, and held only past several times what it takes ([performance](performance.md)). Supersedes D-155. |
| D-206 | Breaking a stable thing | announce one version ahead; break in the major that makes it | **In the major that makes it**, without notice, by the owner's decision (21.0): no aliases, shims or "deprecated". Supersedes D-197. |
| D-207 | Native Shell's pin | a tag; its main | **Its tag v0.7.0 (3f8ecd8)**: Window Model v2, TitleBar, DataTable `grow`/`autoFit`/`onRowActivate`, Button `busy`; the apps require `>=0.7, <0.8`. Supersedes D-002. |
| D-208 | Type | sizes; sizes with their lines | **Each size with its line** (11/16 … 20/28); Chinese captions 12 pt; pages write `TEXT` roles. |
| D-209 | Motion and depth | per component; tokens; lengths and ease; one spring | **One spring, `SPRING` 620/45**, three shadows: no lengths; a press sinks; nothing moves under reduce-motion; chart draws keep Plot's own lengths. |
| D-210 | Status colour | paints text and fills; marks | **Marks only**: a dot, an icon or a rule; one `TONE` table; verdicts are quiet words. |
| D-211 | Surfaces | boxes in cards; bare | **No box inside a card**; row actions and sort arrows on hover or focus; one header (one primary and ⋯); dialogs 480 or 640, one size across states. |
| D-212 | Charts | labels on the data; beside it | **Off the data** (headroom, scale gutters); legends list drawn marks only; chart icons on hover. |
| D-213 | A hot key | many reads; reads a kept copy answers alike | **Every read found it whole, one ETag, nothing written between**: a rewritten or missing key gets no cache advice. |
| D-214 | A program's key | every number out; dates and ids out | **Dates, times, UUIDs and ids only**: other numbers are arguments; a changed command is said and saves nothing. |
| D-215 | Which run, which budget | per command; one rule | **`[program] name`, else this folder's, else the newest**; a `sonde.toml` applies to its own program only. |
| D-216 | An operation's type | `kind`; `shape` | **`operations[].shape`**: `kind` is only the S3 call. `check --program-exit` removed (the session keeps the exit). |
| D-217 | The tour | skips what it cannot reach; fails | **Fails**: every Time chart at every size; Linux, macOS and Windows approved. |
| D-218 | Policy of a plain run | no-delete; allow | **allow** (`--trial` and `--again`: no-delete, which refuses deletes, aborts and lifecycle, policy and versioning changes only): sonde does not change what a program may do with its own data. Supersedes D-133, D-173. |
| D-219 | One word | session; run | **run** in the CLI, JSON, folders, app and docs; the proxy's file format id follows. |
| D-220 | Formats | five, per command | **text and json**; `check` adds markdown and github. |
| D-221 | Confirmation | typed text; dry run then `--yes` | **A dry run, then `--yes`**. Supersedes D-008. |
| D-222 | State beside the runs | a file each; one index | **The folder's one `.index.jsonl`**, under one lock, replaced durably. |
| D-223 | share | a site, a server, OTLP; one file | **One HTML file holding the viewer and the run**: nothing fetched or served; a second run or baseline opens on Compare. No OTLP: "observable" means the product's own charts. |
| D-224 | A program's name | rules on its arguments; its script | **`--name`, else what the command runs**: `sh -c`'s command, a container's image, `python -m`'s module. |
| D-225 | Containers | documented; rewritten | **`docker run` / `podman run` get the host's network and the variables by name** (no key on a command line). |
| D-226 | Another region | an error; re-signed | **Re-signed for the region the store names**, cached per bucket; a request with no body is sent again. |
| D-227 | HTTP | reqwest 0.12, native roots; 0.13 | **reqwest 0.13, rustls with ring; the operating system verifies**; `--ca-file` adds roots. Supersedes D-004. |
| D-228 | A record in memory | 472 bytes; lean | **96 bytes**: interned names, shared query and headers, the rare part behind one pointer; the file format is unchanged. The analysis runs one part after another: a million in about 350 MiB. |
| D-229 | A stretch of a run | sliced as the whole; its own | **Computed on the run's axis with its own slices**: zoomed charts draw real detail. |
| D-230 | Throttles in charts | a 5xx; their own | **Their own class and colour**; a failure bar opens its requests. |
| D-231 | Colour pairs | by eye; linted | **lint-charts checks distance under deuteranopia and protanopia**: chart-4 orange, fewer/more blue/orange. |
| D-232 | Dependencies | as they come; monthly | **Dependabot monthly for every ecosystem**, RustCrypto as one group; its PRs run the whole CI. Rust 1.97 for every crate. |
| D-233 | Phrases | a table per crate; one place | **`app/locales` only** (`phrase.*`, `core.*`): Rust reads `en.json` built in (`sonde-fmt::words`). |
| D-234 | The proxy's crate | one; two | **`sonde-proxy` is the endpoint; `sonde-analysis` reads runs**, depending on the proxy, never the reverse. |
| D-235 | A trial's flow | the CLI's and the bridge's; one | **`trial::Trying`** for both. |
| D-236 | Generated TS types and schemas | generated; hand-written | **Hand-written**: the bridge's and CLI's answers are mostly `json!`; generating them would take more code, not less. |
| D-237 | `show -f json` | everything; compact | **Compact by default** (a finding's first 20 operations and a total, the run's SDK's fix only); `--all` the whole; `--summary` goes. |
| D-238 | What an agent needs | patches; places | **`kind`, callers as file/line/function/code, `verify`**: sonde names the line and checks the fix; it generates no patches. No MCP server. |
| D-239 | A fix after more or less work | counted; not | **Not counted** when requests grew 1.5× (and by 10 or more) with the same command, or shrank by a third (and by 10 or more) below what fixing the earlier run's findings would leave (`workloadChanged`): measured from that floor, a real fix, which also cuts requests, still counts. |
| D-240 | Findings' order | severity; saving | **What they save**: money, then time, then requests. |
| D-241 | A month | none; per source | **An import's own span; a run's `[program] runs-per-month`**. |
| D-242 | Replay of a program that reads input | differences; stand-ins | **Stand-ins of the recorded size** for what it read first, written outside the replay's run and removed after. |
| D-243 | Compatibility | every probe in Settings; what the program relies on | **Only the probes matching what the run used**, asked from a run (⋯ › On another store…); `probe` stays the catalogue. |
| D-244 | Typed confirmation | always; above 100 | **Above 100 items, or a lifecycle**; below, a plain confirm. |
| D-245 | Surfaces | cards in cards; one level | **One surface level**: tables fill their pane; a folded one-line run header on short panes and on Time and History. |
| D-246 | A chart pressed with no mark under the pointer | nothing; its weightiest row | **The row that weighs most** (`pressedRow`); a chart's how-to-read is a popover from its title, no mark beside it. |
| D-247 | What simplifying may take | anything quiet; nothing read | **Nothing a user reads**: the run header is one line of figures at every size (their detail in each one's tooltip); check-locales fails a release whose dropped phrases its changelog does not name. |
| D-248 | A run's line in the sidebar | its argument or its requests; both, and what sent them | **Client · endpoint · requests**, the client read from user agents while counting (an application before its SDK; two when both send 10%). |
| D-249 | Common mistakes | the kinds there were; four more, merged | **Small writes, single deletes, unconditional read-modify-write, a polled missing key**; throttling one kind, orphans one kind (identity rules 4). |
| D-250 | Same-key reads | 10% of the run; a floor | **Per stretch between writes, at least 10 reads saving 1%** of requests or cost. |
| D-251 | A PR with no stored baseline | the budget alone; its merge-base | **Its merge-base, recorded in the same job** (a `setup` input); a `--demo` run never held to its waiting. |
| D-252 | Expected findings | a list; with reasons | **identity → reason**, said wherever the finding is; agents leave them. |
| D-253 | stale-list by default | every store; not AWS | **Only off AWS** (strongly consistent since 2020). |
| D-254 | Runs written elsewhere | unlisted; listed | **Listed by path** from the runs index, never pruned while the file is there. |
| D-255 | Clock | 24-hour; the locale's | **The locale's hour cycle everywhere**, a day's group to the second when any row needs it. |
| D-256 | A chart's CSV | a button on each chart; none | **None**: a button on every chart cluttered the cards; a run's data is `sonde show -f json`'s, and Capacity keeps its own export. Supersedes D-163. |
| D-257 | Chinese release notes | a second file; a CHANGELOG part | **A `### 中文` part of the version's section**: one file to edit; the English shows when there is none. |
| D-258 | A fix for an engine's requests | SDK code; the engine's setting | **The engine's setting first** (`fixes[].setting`), SDK code dropped when an engine sent the requests: a wrong fix is worse than none. |
| D-259 | Spark or Hadoop key | both; one | **One Hadoop s3a key**, the Spark form in the code: the user agent cannot tell them apart. |
| D-260 | Where a setting is set | engine parsers; a text search | **A bounded text search of the run's folder** at analysis time: no parser per engine; it may find a comment. |
| D-261 | Trial advice's English | stored; added when written | **Added when written** (`advice.text`): right when the phrases change. |
| D-262 | Egress by default | always charged; never; unknown | **Unknown: said, not counted**, charged when the endpoint says `egress = true`: sonde cannot see where the client runs. |
| D-263 | Storage in a run's cost | in `amount`; apart | **Apart, a month**: it recurs, unlike a run's requests. |
| D-264 | `max-transfer` | budget and baseline; budget | **A budget only**: data volume varies from run to run. |
| D-265 | `obs.wholeread`'s evidence | the format; ranges of the same object | **Range reads of the same object in the run**: no guessing which formats are read in part. |
| D-266 | The welcome's demo | a curl or python command; a job sonde plays | **A job the app plays** behind `sonde demo-job export.py`, which the CLI runs too: every system, nothing installed. |
| D-267 | Where the demo lands | Run; Runs | **Runs, on its first diagnosis** (D-292). |
| D-268 | rclone and DuckDB | by hand; flags; variables; `-init` | **`RCLONE_S3_*` on every run; DuckDB's `-init` with `SET s3_*`** (no keys in it: DuckDB reads `AWS_*`). |
| D-269 | An object request sonde cannot name | the method's operation; `Unknown`; `x-id` | **`x-id` when it names one, else `Unknown`**: an unknown write must not pass `no-delete` as PutObject. |
| D-270 | RenameObject under `no-delete` | allow; refuse | **Refuse**: it removes its source key. |
| D-271 | MinIO in CI | an image; built from source | **Built from source**: no image can be pulled. |
| D-272 | Completions and man pages | visible commands; hidden; a parser apart | **A parser apart** (`sonde completions`, `sonde man`): clap_complete lists hidden subcommands. |
| D-273 | The window's chrome on macOS | the toolbar across; the sidebar the window's height | **The sidebar the window's height**, the window's buttons in it and the toolbar over the place: as the system's own apps. The same tree, the toolbar placed by platform. |
| D-274 | The Time pane's charts | a card each; sections | **Sections**: a hairline and the group's name, each chart on the card's fill without its edge; thirty boxed charts read as a wall of frames. Every chart stays. |
| D-275 | Linux's interface face | name GNOME's faces first (Adwaita Sans, Cantarell, Inter); `system-ui` | **`system-ui`**: GTK's is the desktop's own face (Ubuntu's, KDE's, GNOME's); a named face would set GNOME's on every desktop, and a `font-*` token keeps only its first family (FINDINGS 18). |
| D-276 | The demo's pictures, timed live | fake the run's times; a wider tolerance for its two pictures | **A wider tolerance** (12 cells, `shots-diff.py`): the run is a real one; its requests follow their answers (D-310), and a loaded runner still moves a sum's last digit. |
| D-277 | A chain's gap (`CHAIN_GAP_US`, `obs.serial`) | 10 ms; wider (100 ms, a second) | **10 ms**: a wider gap chains requests that wait on what an answer held (a Parquet footer read, then the ranges it names), which `Depends` cannot see, as it sees keys and prefixes and not content: a false `obs.serial`, the parallel fix wrong. |
| D-278 | The Time pane's charts, built | all at once; in slices on timers; a worker; as they come into view | **As they come into view**: the board is Native Shell's virtualized `List` in the pane's scroll (`viewport="parent"`), a row a group's heading or a line of its charts; opening it took 1.6 s, mostly JavaScript building charts not yet in view; now 0.3 s ([performance](performance.md#opening-the-time-pane)). Every chart stays. |
| D-279 | Naming callers in a Node.js ES module | `--import` an ESM hook (`module.register`, Node.js 20.6+) when the version allows; the `--require` hook alone | **The `--require` hook alone**, at `Module.prototype.load`: Node.js loads the SDK as CommonJS for an `import` too (its `node` export, or `main`), and that path skips `Module._load`; no flag that an older Node.js would refuse, no version to detect, no loader thread. |
| D-280 | `-f json` on a terminal | through the terminal escaper; raw | **Raw, with DEL, C1 and direction marks as `\uXXXX`**: one output that is valid JSON and inert on a terminal; the escaper's `\x7f` broke parsers. |
| D-281 | `show` or `check` with no run named, and none of this folder's | the newest run of any program; refuse | **Refuse (64)**, naming the newest run's program: a wrong run judged in silence is worse than a refusal. |
| D-282 | A signed header value that is not ASCII | forward raw bytes; UTF-8 text, others refused | **UTF-8 text, others refused** with a reason: raw bytes would turn the whole signing path into bytes, for clients AWS rejects anyway. |
| D-283 | A request naming an object subresource and an `x-id` | `x-id`; the subresource | **The subresource**: `x-id` is the client's to set; the subresource decides what the store does (and what no-delete guards). |
| D-284 | A layout listing that reached its cap | list on without a cap; say so and stop judging | **Say so and stop judging** missing and orphaned files: past the cut is unknown, and the cap keeps the window responsive. |
| D-285 | What a finding is worth | money, then time, then requests; the run's wall time saved | **The run's wall time saved**, each finding's from the what-if without its requests, money beside it from a cent: a request count said "saved 20" while the run took 30% longer, and money ranked a $0.00001 poll over 96% of the waiting. |
| D-286 | Findings with one root cause | each its own; one diagnosis | **One diagnosis** when half of each one's objects are the other's (objects, not requests or callers: the same objects written, read and HEADed are one cause; a key read again among them is its own), one saving from the what-if over the union, one fix; requests one after another are an attribute of the diagnosis that owns them. |
| D-287 | A fix measured | the requests it removed; the run's time and money against the program's spread | ~~The run's time and money~~ — superseded by D-299. |
| D-288 | A fault not survived | a verdict with advice; a finding | **A `trial` finding** with its line and the SDK's fix, in the same list and fix loop; `--again` went: every trial runs every scenario and the loop says what got fixed. |
| D-289 | What a run again prints | the plan; what changed | **What changed**: fixed with what it saved, open a line each, new in full; the plan was read on the first run and `show` keeps it. In agent JSON a diagnosis is whole (text, `at`, fix, saving): `diagnoses[0]` is the next step. |
| D-290 | One command per question | overlaps kept; folded | **Folded**: comparing runs is `show --since`, probing is `endpoints test --probes`, a standing endpoint is `run --listen`; each overlap had two code paths and two sets of docs. |
| D-291 | A terminal UI | kept; none | **None**: the desktop app, the CLI's JSON and a share's page cover its users; one form halves the app's code paths (180 terminal branches). |
| D-292 | Where a program is run in the app | Run; Runs | **Runs**: with no run chosen it is the command bar, and a run in progress is the newest run, its terminal beside its operations; Run duplicated Runs' view. Places: Runs, Buckets, Settings; Activity a panel. A new run: Runs pressed in Runs, ⌘N or File › New run; the sidebar has no heading, "+" or search of its own (⌘K finds a run). |
| D-293 | `check` without a baseline | a request budget; the open findings too | **The open findings too** (`[check.findings]`, by counts; requests one after another past a fifth more, as timing moves them): CI catches a new finding with no baseline. |
| D-294 | Where a fix opens | inside the list; the inspector | **The inspector**: one inspector, one primary per screen; the list stays a line per diagnosis, and choosing one narrows the table to its headline finding's operations. |
| D-295 | What follows a decision in a dialog | the dialog; the page it is about | **The page it is about**: a dialog is the decision; a trial's verdicts fill the run's History as they come. Trial verdicts are one row per scenario there; what to change is the trial finding's fix in the inspector. |
| D-296 | TCP_NODELAY on the program's sockets | the OS default; on | **On**, as on the store's: a record cannot see a wait the kernel adds (Nagle against a delayed ACK cost ~40 ms on answers of a few pieces, counted as the program's own), so sonde must not cause one. |
| D-297 | A change's time saved | the drop in the union of requests in flight; its requests' share of the waiting | **Their share**: each moment's waiting split among the requests in flight then; the shares add up to the waiting, equal the union's drop one at a time, and a tenth of a busy pool's requests saves about a tenth of its time, not nothing. |
| D-298 | Requests one after another beside other findings | only those no other finding claims; all of them | **All of them**, weighed on their own: a request that depends on an earlier one stays in order inside the chain instead of ending it, and sending at once that saves more than a diagnosis's headline is its own change, first (a format change does not outrank a bigger, simpler win). |
| D-299 | A fix measured | the run's time per finding; its requests beside its prediction | **Its own requests, beside the time its plan predicted**; the run's time is said once, against the largest move between earlier runs with the same findings (counts that move with timing, by identity): per-finding run time double counted, and a band over earlier fixes hid real ones. The workload is judged by objects asked of, less the writes fixed findings did away with. |
| D-300 | The fix code sonde prints | written by hand, read; run | **Run**: every boto3 fix for a fault or a common finding is pasted into a program exactly as `show -f json` gives it and run under `sonde run --demo` in CI, so a fix cannot claim what it does not do. |
| D-301 | A run's output in the app | beside the operations on Operations; a tab | **A tab (Output)**: beside the operations it pushed out the time strip (every Time chart is kept) and was crushed under the inspector; a live program opens on it. Supersedes 29.0's no tab. |
| D-302 | Requests that need an earlier one | ended the chain; held in order; a wave after it | **A wave after it**: a dependent waits only for what it depends on, and dependents go at once among themselves (a listing before a loop of uploads hid the whole loop); one alone at its level is not counted as one that could go at once. |
| D-303 | Telling a finding's identity apart | among findings; among the run's objects; among the run's findings of its kind | **Among the run's findings of its kind**, their traces counted by hash in one pass: among all the run's objects, yesterday's `_SUCCESS` or the next metadata file gave a finding a date and failed a daily `check`; a second findings pass cost about 60% more time at a million requests. |
| D-304 | Objects a fix freed | its writes; every object only its saved requests asked of | **Every object only its saved requests asked of**, whatever the operation: packing small reads or one listing for a crawl is a fix, not a smaller workload. |
| D-305 | Work in progress in the app | a signal each (program, trial, demo, recording); one | **One**: four signals let three slip past every guard (Run again, Updates' restart, the tray's Stop); a trial, the demo and Record… are at work like a program, and nothing starts beside them. |
| D-306 | Run and Run again's policy | the command bar's choice; the default, narrowed by the project | **The default, narrowed by `[record] policy`**: a run's header does not record a policy, and a choice kept in the bar made Run again quietly read-only; trials and Record… keep their own choice. No system-wide run shortcut: AltGr+R types ® on Windows layouts, and the tray has Stop. |
| D-307 | The shell's shape per platform | each its own; macOS's on Linux too | **macOS's on Linux too** (`tallSidebar`): libadwaita's split view is the sidebar at the window's height with its own top strip; Windows keeps the full-width toolbar and requests no backdrop (opaque fills, and Mica under translucent ones would leave Windows 10 bare). |
| D-308 | A table's column widths | guessed from characters; measured | **Measured** by nsh's `fitColumn`, in the face the cells are drawn in; the fill column takes the rest. Shortcuts are in the interface face on every platform. |
| D-309 | Objects a fix freed, when the finding is still there | counted; not counted | **Not counted**: a finding that is only smaller cannot tell a fix from a smaller workload; only a Fixed finding frees objects. |
| D-310 | The tour job's pace | beats fixed from the recording's first request; each request 1 ms after the previous answer | **After the previous answer**, 100 ms between parts: fixed beats kept the time of the requests a fix took out, so the saving did not match the prediction. |
| D-311 | Closing the main window | hides on macOS, quits elsewhere; quits everywhere | **Quits everywhere** (Native Shell's Window Model v2), Settings and run windows with it, asking first only while something is at work (one `atWork`), once; the Dock and a second launch reopen it. A main window that closed to nothing on macOS left a tray-less app with no way back but the Dock. |
| D-312 | The demo's name | each place checks for the demo; `programOf` names it | **`programOf` names it** ("Demo"): every list, toast and dialog names it alike. |
| D-313 | Menu shortcuts on Windows and Linux | Native Shell's application menu; bound by the app | **Bound by the app** (`keymap.bind`, scope app): Native Shell 0.7.0 puts the application menu on a hidden host there, where its shortcuts reach no window. |
| D-314 | The release after 32.0 | 32.1.0, every surface kept; 33.0.0 | **33.0.0**: a trial that refuses a shared place, a `--yes` that wants its dry run, `no-delete` refusing more, figures redefined in `-f json` and identity rules 6 each redefine a stable thing ([contract](../contract.md#versions)); opt-ins would have left the unsafe behaviour the default. |
| D-315 | What makes a run sonde's own | path containment alone; a mark in every run; containment and the index's imports | **In the runs folder and not listed as an import**, decided in Rust from where the file is, never from what it says: imports are copied into that folder, and a mark would make every 32.0 run ask. An import is marked before it is written; a run written with `-o` is from elsewhere. |
| D-316 | Where the Run again gate is | a flag from TSX on `Runs.run`; a bridge call that takes the run | **`Runs.runAgain(path, confirmed)`**: Rust reads the command itself, and for a run from elsewhere starts only when the call echoes the command and folder to the character: a yes to a scrubbed or earlier command is no yes. |
| D-317 | A command shown beside what runs it | scrubbed as a shared page's; whole | **Whole**, each character that would not show as itself written `⟨U+XXXX⟩`; a value the shell runs a command to make (`X="$(…)"`) is never folded anywhere: it is code, not a secret. |
| D-318 | Which files `show` reads a caller's line from | any readable file; the app's open list (source and text); source only | **Program source inside the run's folder** (canonical path, no link out, no device or network path; one `sonde_analysis::code` for app and CLI): the run names its own folder, so settings and text files under it are where a crafted run would look. |
| D-319 | How a copied command is made | quote strings harder; an argv written per shell | **An argv first**, written for sh, PowerShell or cmd.exe: aws values as `--name=value` (s3api has no positional values for `--`), one aws would read from elsewhere (`file://`, `http://`) in `--cli-input-json`, curl's URL after `--` with `-g`. Hidden characters are removed and said, not escaped: no escape is portable. PowerShell arguments no spelling passes in both modes run only under `$PSNativeCommandArgumentPassing`. |
| D-320 | Where a run's redaction is enforced | each importer; the writer | **The writer (`RunWriter::write`)**, idempotent, with a check by names so a recording pays almost nothing: no importer or future writer can skip it. |
| D-321 | A credentials failure told to a recorded program | the source's error; a fixed sentence and the exit status | **A fixed sentence**: what a `credential_process` printed, a token's path or an STS answer is not the program's to read, nor the run's to keep; `sonde endpoints test` says why. |
| D-322 | Numbers in keys under `share --redact` | kept (structure); all tokens; short numbers, dates and zero-padded counters kept | **The third**: layouts stay readable while account ids, IP addresses and grouped numbers go. The compared run is named by its time, not its file. |
| D-323 | What a lost race does to the write | forward then 412; 412 unsent; sonde writes a winner | **412 unsent**: a write the program is told it lost is in no store, so no writer that reads its log again finds its own commit as the winner and commits twice. sonde writing a winner would write into the user's table. Cost: no winner exists, so a writer that must read it reads `broke`. In a numbered log a later number is held until the lost one stands (no gap); not for SlateDB's WAL or Hudi's instants, whose writers step on by design. |
| D-324 | A trial's verdict on writes | as many answers as the clean run; what was made | **What was made**: more commits in a log than the clean run makes is `broke`; a failed write is made good only by a write that succeeded after it, never by an error after it. |
| D-325 | A trial whose runs share a place | warn; refuse; confine every trial | **Refused after the clean run (exit 77) unless `--same-place`**: the clean run is where writes are first known, and no faulted run precedes the check. A program whose every write names `clean` is taken to read `SONDE_TRIAL`, and each faulted run is then held to keys naming it: evidence from one run, enforced in the others. The demo store is exempt. |
| D-326 | How `no-delete` knows a key stood before the run | a HEAD before each first write; refuse unknown overwrites; proof from the store's own answers | **Proof only** (a 2xx to `If-None-Match: *`, a 404 seen first, a version id): no request or wait of sonde's own (recording adds none: D-296). Cost: a key written blind and then deleted is refused outside the `{trial}` place. What guards data (ACLs, the public access block, ownership controls, a legal hold off, a governance bypass) is refused with it. |
| D-327 | Cleanup's listings | one page; every page, ended by a marker handed back | **Every page**; a store that hands a marker back ends the listing (replay: stop and count; probes: ask again from the start while a round removes something; `list_all`: an error). Probe cleanup lists objects after versions always: an empty 200 is not "no objects". |
| D-328 | A 501 beside a broken guarantee | unsupported wins; fail wins | **Fail wins** when a write, copy or delete went through against its condition: data was lost whatever else was answered. |
| D-329 | A version id's shape | differs unless AWS's; a note | **A note**: AWS promises only opaque, URL-ready ids of at most 1,024 bytes; `differs` (exit 2) only for one that is not. |
| D-330 | Naming a store | its URL's name and port; its name and its answers | **Names and answers** (`Server`, an error document's marks), never a port: many stores listen on 9000, and a wrong guess priced a store as free. A probe run names the store that answered. |
| D-331 | How fix code is held to work | read by hand; boto3 under `run --demo` (D-300); every snippet parsed and compiled | **Every snippet, from one catalogue made of the phrases**: syntax wherever tests run, the SDK's compiler where its pinned packages are (`tools/fix-check`), a skip said aloud, and CI's `fixes` job where nothing may be skipped (`SONDE_FIXES_STRICT=1`). The AWS SDK for Rust has its own tag (`rust-aws`): object_store's calls are another API. |
| D-332 | A key read again that the program writes | split the finding per stretch; drop the advice; a cache dropped on write | **A cache dropped on each write**: the count was already one read a stretch, which only such a cache delivers; identities and savings unchanged. |
| D-333 | A body held to what it said | check at the end; keep back the last piece | **The last piece kept back until the body ends as said**: a store given every declared byte commits, whatever follows. `IncompleteBody` (short), `InvalidRequest` (longer, framing), `RequestTimeout` (stopped): S3's own, and SDKs retry only the last. |
| D-334 | A forwarded key | decoded and encoded again; the program's bytes | **The program's bytes on the wire**, signed decoded-then-encoded-once as a store reads it; a key that is not UTF-8 keeps `%XX` in the record (U+FFFD names another key). |
| D-335 | What a run says of how it was recorded | nothing; every endpoint's settings; a `--url`'s | **`pathStyle` and `caFile` for a `--url`, `tls`, and `policy` always**, optional header fields: a configured endpoint keeps its own settings. `verify` says the policy only when it differs from what `sonde run` would choose in the run's folder, so default runs keep their line; no secret is kept, a CA by its path. |
| D-336 | Time saved by requests whose bytes are still read | their whole part of the waiting; the part up to the first byte | **Up to the first byte**: the shards read the same bytes. The time account's bytes are parts of the waiting, never latencies added up; an at-once saving is of the requests its diagnosis leaves. |
| D-337 | A program's noise | largest adjacent move; MAD; range with a small-sample correction | **Range / d2(n) × 1.96√2** over up to five runs alike: a drift counts in full, and it is the band two runs differ within. Two runs never under `check`'s room (a tenth and 50 ms), three never under half; `check` has no estimator of its own, so its allowance is shared, its verdicts unchanged. |
| D-338 | A count's tolerance | exact; relative everywhere; relative from a floor | **Exact under 100, a hundredth from there**, one rule for the fix loop and `[check.findings]`. |
| D-339 | A bucket-level finding across buckets | `/`; a finding per bucket; the buckets listed | **Its buckets, sorted (`a,b`), the same finding while one is shared** (rules 6): `/` changed run to run and read as fixed and new. Single-bucket names are untouched; `…|/` in `[check]` still matches. |
| D-340 | An operation's buffer | one a format; one a table | **One a table** (bucket, root, format). A data file is a table's by the log seen for its root, or by its own name: a `data/` folder is no evidence. |
| D-341 | Iceberg v3 and old SlateDB checkpoints | read as v2, the newest manifests only; read what is referenced | **Read what is referenced**: a Puffin file once, its vectors' rows off the live records, statistics files not orphans (the sample built in the test: pyiceberg 0.9.1 writes no v3); a checkpoint's manifest read though older than the newest 500, orphans said unknown when it cannot be. |
| D-342 | A large import | the file as a DOM; a stream | **A stream, spilled through `RunWriter` beside the output** (0600, 24 bytes a request held): byte-identical to the import in memory. A lean reading drops the program's output (`Keep::AnalysisAndLogs` for the two readers that show it). |
| D-343 | How a terminal's `--yes` finds its plan | rescan and confirm its own; a printed id; kept by the command | **Kept by endpoint, bucket and options in `<data>/plans/` (0600), ten minutes, taken under a claim only one taker can make (a file created new; a rename is not exclusive on Windows) and gone before anything is sent**; `--plan ID` for one dry run. No flag skips the dry run: it would be the hole itself. Only what was listed is touched; a listed upload gone or written to refuses all. S3 has no conditional abort, so a change between the re-read and the request is not seen. |
| D-344 | An outcome the audit log cannot take | logged; failing the run; said | **Said** (stderr and exit 70, a notice in the app): the change is made and cannot be unmade. |
| D-345 | Keeping schemas honest | closed schemas; a strict reading in the test | **In the test, both ways, with listed exemptions**: readers keep open objects (the contract), while no field ships undescribed and none described is fiction. |
| D-346 | The Action's baseline and words | any completed run, `eval` for globs; a run that succeeded, no `eval` | **A run that succeeded only** (a failed program's run is not kept either), outputs in GitHub's delimiter form, a glob expanded with `compgen` and braces left to bash: no word of an input is read as shell twice. |
| D-347 | Stripping the packaged app | a profile in Native Shell's generated workspace; cargo's environment; a step after packaging | **`CARGO_PROFILE_RELEASE_STRIP=symbols` for `nsh package`**, checked on Linux by `scripts/check-stripped.sh`: the generated workspace is not ours to edit, and stripping after would be after the archive is made. |
| D-348 | What a run did to the store | sizes and ETags of what it wrote; the objects it leaves | **The objects it leaves** (`left.rs`): each key to its last change, read as `stable` reads it, counted. Ten real programs recorded three times each: the same every time by this reading, 8 of 10 with sizes, 7 with ETags (a table's metadata differs by bytes). It says which objects stand, not what is in them. |
| D-349 | What a trial holds a run to in what it leaves | every difference; more or fewer; a kind wholly missing | **A kind the clean run leaves and the run wrote none of: broke**, said only where the exit is the clean run's and nothing else broke; more than the clean run leaves stays the lost race's (degraded). One fewer of many is not said: writers that flush by the clock leave more or fewer. Replaces the count the lost race kept of its own; both runs' names are read out of keys wherever they stand (`t{trial}/x`), the same way in both. |
| D-350 | What a run leaves, in the fix loop | a verdict (fixed only if the same); always said; said when it differs | **Said when it differs** (`since.change.left`, one line): a fix that packs files leaves fewer on purpose, so it cannot gate "fixed"; said always it would read as "nothing changed", which a wrong byte in the right key is not. Eighteen real programs recorded five times each left the same every time; under faults one that flushes by the clock left 4 to 12 fewer of a kind, which is why a trial holds a run only to a kind wholly missing (D-349). The demo's fixed job now writes where the job always wrote: its fix changes how it reads. |
| D-351 | The release that reads what a run leaves | 33.1.0; 34.0.0 | **34.0.0**: a trial that was degraded can be broke (an output never written), which redefines a verdict, and the demo's fixed job writes elsewhere; nothing was added to the surface but one optional JSON field. Kept to that: the fix loop only says a difference, and a count of more or fewer is held against no run. |
| D-352 | A trailer of an aws-chunked body read whole | forwarded as headers; held to the policy; only the checksums named | **Only the checksums `x-amz-trailer` names**, a named one missing answered `InvalidRequest`: any other trailer went on as a header the policy never saw (`x-amz-bypass-governance-retention` past `no-delete`), signed with the real key. Checksums are the only trailers S3 defines, and the streamed path forwarded nothing else. Cost: an unnamed checksum trailer is dropped. |
| D-353 | A signed aws-chunked body that says 0 bytes | sent with `Content-Length: 0`; read whole first | **Read whole (at most 64 KiB) and its framing checked before it is sent**: told 0, the store never asks for the body, so a body cut short or longer than said made an empty object (D-333). A few framing lines are no request and no wait of sonde's own (D-296). |
| D-354 | A key that is not UTF-8 under `no-delete` | the record's `%XX` text; its bytes; never made | **Never made**: its record keeps `%FF` (D-334), which no string tells from a text key `%FF`, so neither spelling's create or 404 may let the other be deleted. Refuses more, never less; the record is unchanged. |
| D-355 | An Iceberg metadata file's commit number | the name's first group; `v<n>` or five or more digits and a dash | **`v<n>`, or five or more digits and a dash; a UUID alone none**: one UUID in forty begins with eight digits, and held a commit slot no writer had taken. |
| D-356 | A mask already in a value | proof it was redacted; redacted again | **Redacted again, each mask unchanged by it**: `…` is in metadata people write, and `«redacted»` beside a real signature kept the signature; a value is taken as redacted only when redacting it changes nothing. |
| D-357 | A copied command cmd.exe would cut | cut; a script file; not copied | **Not copied, said in its place** (copy it for PowerShell): cmd.exe reads 8191 characters a line and runs what is left of a longer paste. Nothing new to run or clean up. |
| D-358 | Where a run's folder may be | any path; not a disk's root; a folder on this machine | **A folder on this machine, through one check (`code::local_dir`)** for every use (settings, engines, git, budget, code change): on Windows a drive's path, plain or `\\?\X:\`, still one once resolved; never a share, device or NT path, so a file's own `cwd` cannot make sonde reach a host. A mapped drive is resolved before it is refused. |
| D-359 | Whether a run was recorded here | in the runs folder and no import; a ledger of recordings; and the index there to say so | **The index there to say it is no import** (`index::recorded_in`, the app and `show` alike): a lost or unreadable index made every import a recording. A ledger would be one more file to keep; this fails closed with the one there is. `show` reads a caller's line only for a run recorded here (D-318). |
| D-360 | A terminal's checks and the app's | each its own; in what both call | **In what both call**: uploads that changed since the plan are not aborted (`exec::run`, as the terminal's `--yes` was), and the app's import streams through the CLI's `Spool` (D-342). |
| D-361 | A run's name in what it leaves | replaced in order; each run its own name; the longest name at each place, however spelled | **The longest name at each place, in any case, with `-`, `_` or nothing between its words**: a program that writes `lost_answer` left nothing of `lost-answer`'s, and `clean` read as `lean`'s; reading each run with its own name only would make `errors/` under `error` missing (D-349). A project scenario that is part of `clean` is refused. |
| D-362 | A delete that leaves the object | taken as removing it; not | **Not**: a delete sonde refused on a lost race, or of one version, leaves the key standing; the clean run's output read missing and the run broke. |
| D-363 | A program's noise past three runs | the floor gone at four; half the room at three and more | **Half the room at three and more** (D-337): a fourth run narrowed a 10 s program's band from 525 to 40 ms, and runs alike gave 0, so any move read as time saved. |
| D-364 | The Action with no baseline found | nothing (exit 64 with no budget); `auto` always; `auto` unless sonde.toml names one | **`auto` unless sonde.toml's `[check]` names one**: a first run (a new repository, a tag, a new branch) failed before it could keep the artifact that would be the next run's baseline; `auto` over sonde.toml's would drop the project's. |
| D-365 | How strictly JavaScript fixes are typed | `strict`; as JavaScript | **As JavaScript** (`strict: false`): they are JavaScript, and `strict` asks for TypeScript (`obj.Body!`, typed parameters) in code a JavaScript program pastes. |
| D-366 | What 34.1.0 leaves as it is | each fixed; said | **Said, not built**: a 200 whose body is an `<Error>` (the proxy reads no answer's body), an upload's time (no field for when its body was sent), lost data a kind still has some of (D-349), a `{trial}` program's writes elsewhere (D-325), equality deletes' rows. Each needs a new field, request or verdict. |
| D-367 | The release of these fixes | 34.0.1; 34.1.0; 35.0.0 | **34.1.0**: documented behaviour that did not hold, and new words (a command not copied for cmd.exe, a scenario name refused, a run from elsewhere's lines not read); no field, flag or exit code redefined. |
