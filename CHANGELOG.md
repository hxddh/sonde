# Changelog

All notable changes are listed here. The project follows [Semantic Versioning](https://semver.org).

## [27.0.0] - unreleased

### Changed

- Time: the pane opens at once. Its charts are built as they come into view, not all before the pane shows; every
  chart is still there.
- The app is built on Native Shell 0.6.1 (fixes from its 0.6 line).

### Fixed

- `sonde run` names the line that sent a request again with the AWS SDK for JavaScript v3's newer releases (the
  client moved to `@smithy/core`), and in a program written as ES modules (`import`), on any Node.js the SDK runs on.
- The README named the GitHub Action `@v25`; it names `@v26`, and a check keeps the major in step.

### 中文

- 时间页立即打开：图表在滚动到可见时才构建，不再在页面显示前一次性全部构建；所有图表仍然都在。
- 应用基于 Native Shell 0.6.1（包含其 0.6 系列的修复）。
- 修复：AWS SDK for JavaScript v3 的较新版本（客户端移到了 `@smithy/core`）下，`sonde run` 重新能指出发出请求的代码行；
  以 ES 模块（`import`）编写的程序同样可以，适用于该 SDK 支持的任何 Node.js 版本。
- 修复：README 中 GitHub Action 写的是 `@v25`，现改为 `@v26`，并新增检查保证主版本号一致。

## [26.0.0] - 2026-11-19

sonde now records what the program really got, guards deletes more strictly, and looks at home on each system.

### Changed

- **The app, refined on each system:**
  - macOS: the sidebar runs the window's height with the window's buttons in it, and the toolbar stands over the
    place only. Settings' tabs are centred. The palette's scrim is lighter in light mode.
  - Windows and Linux: shortcuts are written in the interface's face (Ctrl+K), not in monospace.
  - Time: the charts are no longer boxed. Each group (Traffic, Latency, Errors, Where) is a section with its name.
  - A run's row in the sidebar says its count first ("18 requests · curl · demo"), so what sent it and where give
    way first, with no gap.
  - Buckets › Objects: "Flat listing" moved into the bucket's ⋯ (still ⇧⌘F), so the row over the objects keeps to
    one line beside the inspector.
  - The endpoint form puts the CA file and the buckets side by side: its Save stays in view in a short window.
  - A fix's code sits closer in its well. A disabled field's text is easier to read in dark mode.

### Fixed

- A request the program stops waiting for (its timeout, a closed connection) while the store is still answering
  is recorded, with the transport error "the program stopped waiting for the store's answer". Before, it was lost.
- A download the program stops reading part way is recorded with the bytes it read (its transfer and egress too),
  not with the whole object's length. Stopping early is no error: a reader may want only a file's header.
- A request sonde sends itself (a probe, a listing, a download, a replay) is timed from when it went, as
  the proxy's are, not from when its answer was read: a slow answer no longer moved it later on the timeline.
- A run file whose disk filled for a moment mid-line stops there: only its last line is torn, and the records before
  it are read. Before, the next record was written after the half line and the file read as broken.
- `no-delete` reads a DeleteObjects body as XML: a batch with an object the old reader missed (`<Object >`, a
  second `<Key>`, a prefix, CDATA, a comment) or anything else it does not expect is refused, not forwarded.
- Restoring a key with another delete marker under its latest one says the key stays deleted. Before, the plan
  named an older version as becoming current.
- A lifecycle plan is refused when the bucket's lifecycle changed since it was made. Before, its PUT replaced a
  rule added meanwhile.

### 中文

sonde 现在如实记录程序真正拿到的内容，删除保护更严格，界面在各平台上更像原生应用。

- **各平台的界面更精致：**macOS 上侧边栏占满窗口高度，窗口按钮位于其中，工具栏只在内容区上方；设置的标签居中；
  浅色模式下命令面板的遮罩更淡。Windows 和 Linux 上的快捷键改用界面字体（Ctrl+K）。
- 时间页的图表不再加框，按流量、延迟、错误、位置分节并显示节名。
- 侧边栏的运行记录先写请求数（"18 个请求 · curl · demo"），空间不够时先省略发出者和端点，不再留空隙。
- 桶 › 对象："平铺列出"移到桶的 ⋯ 菜单（仍可用 ⇧⌘F），对象上方的一行在检查器旁不再折行。
- 端点表单把 CA 文件和桶并排放置，窗口较矮时"保存"仍在视野内；修复代码块更紧凑；深色模式下禁用的输入框文字更易读。
- 修复：`no-delete` 现在按 XML 解析 DeleteObjects 请求体，含有旧解析器漏看的对象或结构不符合预期的批量删除一律拒绝，
  不再转发。
- 修复：最新删除标记下面还有删除标记时，恢复计划会说明对象仍处于删除状态，不再声称旧版本会成为当前版本。
- 修复：生命周期计划生成后若存储桶的生命周期配置已被改动，执行时会拒绝并要求重新生成计划，不再覆盖期间新增的规则。
- 修复：程序在存储还未应答时就不再等待（超时或关闭连接）的请求现在也会被记录，并标为传输错误；以前会丢失。
- 修复：程序读到一半就放弃的下载，现在按实际读到的字节数记录（传输量和出网费也按此计算），不再按整个对象的长度；
  提前停止读取不算错误，程序可能只需要文件头。
- 修复：sonde 自己发出的请求（探测、列举、下载、回放）的时间改为发出的时刻，与代理记录一致，不再取读完应答的
  时刻；慢应答不会再让它在时间线上后移。
- 修复：磁盘在写到一行中间时短暂写满，运行文件就停在那里，只有最后一行不完整，之前的记录照常读出；以前下一条记录会
  接在半行之后，整个文件读不出来。

## [25.0.0] - 2026-11-18

sonde now names the setting to change and what it saves, in money too.

### Added

- **Fixes in the engine's own words:**
  - A finding names the engine's setting, its value and where the program sets it: SlateDB
    (`manifest_poll_interval`, `flush_interval`), DuckDB, rclone, s5cmd, Trino and Hadoop s3a. Delta, Lance and
    Iceberg cleanups name the table's real path.
  - `show -f json` gives it as `fixes[].setting` (`engine`, `key`, `value`, `where`), an edit an agent can apply.
  - A program an engine drove gets no SDK code that does not fit it. `object_store` programs get Rust fixes.
  - The app's fix shows the setting first, with Copy and a press that opens the file that sets it.
  - Text, markdown and the TUI say the setting too.
- **Bytes and money, made whole:**
  - A run's cost is split into requests, transfer (egress) and the storage it leaves a month, with noncurrent
    versions in a versioned bucket.
  - Egress is priced per vendor (AWS, GCS, OSS; R2, Wasabi, B2, MinIO and Ceph at 0) and charged when the endpoint
    says `egress = true`. Until then it is said, not counted.
  - `check --max-transfer` (or `[check] max-transfer`) holds the bytes a run may send out.
  - Four findings:
    - `obs.rangeagain`: the same range read again;
    - `obs.wholeread`: large objects read whole and in ranges;
    - `obs.smallreads`: a storm of small reads;
    - `obs.crawl`: many prefixes listed once each.
  - Time: "What did the run cost, and when?", requests, transfer and storage stacked.
- **The first minute:**
  - The welcome's demo runs a small job and opens its run on the finding, with the line of code that sent it. The
    built-in store answers it as a nearby region would (20 ms for a HEAD or a LIST, 40 ms for a GET or a PUT), so
    its run reads the same on every machine.
  - "Apply and run again" applies the fix and says what it saved, measured beside the prediction. It works on every
    system with nothing installed.
  - Run proposes the commands a chosen folder runs: Makefile, package.json, pyproject, Python and Spark programs,
    `go run .`, `cargo run`.
  - rclone (`RCLONE_S3_*`) and the DuckDB CLI (`-init` with `SET s3_*`) go through sonde by themselves.
- **S3 coverage:**
  - RenameObject (refused under `no-delete`, as it removes its source) and UpdateObjectEncryption are named.
  - The checksums SHA512, MD5, XXHASH64, XXHASH3 and XXHASH128 are read, verified and probed.
  - New probes: `cond.delete-if-match` and `cond.copy-dest-if-match`. There are 52 probes in all.
- `sonde completions <shell>` and `sonde man <dir>`. The terminal archives carry both.
- Settings › Updates: **Install when sonde quits**, offered first while a recording or a job runs.
- Release notes in Chinese, when a release writes them.

### Changed

- **Native Shell v0.6.0.** The apps require `>=0.6, <0.7`.
- Motion moves on one spring, and nothing moves under reduced motion.
- An object request sonde cannot name is `Unknown`, not GetObject, PutObject or DeleteObject (by `x-id` when it
  names one).
- An Iceberg manifest nested more than 32 deep is an error, not a crash (apache-avro 0.23).
- **Breaks** in `show -f json` and `check -f json`:
  - `cost` requires `requests`, `transfer` and `storage`; `cost.amount` (and each kind's, prefix's and caller's)
    includes egress when it is charged.
  - `whatIf` and `runPoint` carry `bytesOut`.
  - The verdict gains `transfer`.
  - A trial's `advice` also carries its English (`text`).
  - `fixes[]` puts an engine's setting first and drops SDK code when an engine sent the requests.
- CI checks Rust 1.97 (the declared minimum), runs MinIO, checks DuckDB through sonde, and checks that the README's
  pictures are the approved tour's.

### Fixed

- Share in the app opens the page in the browser. Before, it said so and opened nothing.
- `sonde share` works with Native Shell 0.6's page, whose stylesheet is linked.
- An update of the app around sonde no longer announces the running version as new.

### Removed

- The welcome's "The demo is ready: press Return" (`welcome.demoReady`): the demo runs, and its run opens on its
  finding.

### 中文

sonde 现在会指出该改哪个设置，以及能省多少，包括省多少钱。

- **用引擎自己的设置给出修复：**SlateDB、DuckDB、rclone、s5cmd、Trino、Hadoop s3a 的修复直接写出设置名、取值和程序里
  设置它的位置（`fixes[].setting`），应用里可一键复制；`object_store` 程序也有修复建议。
- **字节和费用算完整：**费用拆成请求费、出网费、每月存储费（含版本桶里的旧版本）；出网费按厂商定价，端点设置
  `egress = true` 后计入；`check --max-transfer` 限制出网流量；新增四类发现：重复读取同一范围、整读大对象、小对象读
  风暴、遍历大量前缀；时间页新增"这次运行花了多少钱、花在什么时候"。
- **首分钟：**欢迎页的演示跑一个小任务并停在发现上（内置存储按就近区域的延迟应答，每台机器上结果相同），
  "应用并重跑"后显示实测节省和预测；运行页根据文件夹推荐命令；rclone 和 DuckDB 自动连到 sonde。
- **S3 覆盖：**识别 RenameObject、UpdateObjectEncryption；支持 SHA512、MD5、XXHash 校验；新增条件删除、复制目标
  条件两项探测，共 52 项。
- 新增 `sonde completions`、`sonde man`；设置 › 更新新增"sonde 退出时安装"；Native Shell 升级到 v0.6.0，动效统一
  为一套弹簧。
- 修复：应用里的"分享"现在会真的打开页面。

## [24.0.2] - 2026-11-16

### Fixed

- **The docs match what sonde does:**
  - A run file is read in sonde's own format version only (contract.md said every older one).
  - `check`'s wording as each format writes it ("past the N it may use" in markdown), its closings, its exit codes
    and `--init` (refused when `sonde.toml` exists; errors as measured; the room is `--tolerance`).
  - The head line of a run, the variables removed from a program's environment, what a CloudTrail import gives.
  - What the app's update check does, and that `nsh.json` grants no network.
  - The commands that read `sonde.toml`, `tencent-cos`, the paths on Windows.
  - Every `x-sonde-*` header a run can hold; the retry rule; the Native Shell pin.
- `sonde help check`: `--report-url` names `sonde share`'s page; `--init` no longer says it writes `[program]`.
- `show -f json`: a trial's `where` and `advice` are in the schema (held by a test), and `agents.md` says how to
  read `advice`.

### Changed

- **Each thing is said once in the docs, and linked elsewhere:** policies, safety, credentials, CI, sharing, the tour
  and the version files. contract.md no longer lists each major's breaks (CHANGELOG does). Resolved Native Shell
  findings moved to `docs/history`.
- CI checks that `docs/probes.md` is what the probe catalog makes.

## [24.0.1] - 2026-11-16

### Fixed

- **The fix loop:**
  - A finding fixed in part reads "less", not "fixed" and "new". A read written back, deletes one at a time and small
    writes are now identified by their bucket (identity rules 5; findings kept under rules 4 are made again).
  - A finding that fell under its threshold and then went away reads "fixed", and its return "back".
  - A run where nothing reached sonde fixes nothing and is never "the run before" (`since.empty`).
  - `sh -c X` and `bash -c X` are the same command as `X`.
- `show`, `check`, `compare` and `share` with no run named take the runs written with `-o` too.
- **Findings:**
  - The what-if total keeps one read of a key a cache and GET-alone both act on.
  - Small writes count objects, not PUTs.
  - A saving's share rounds like the share beside it.
  - A trial's "at …" names its first reason's request.
  - "12+ callers" only when there were more.
  - The footnote of a star only with a star.
  - A newline in a key is written out.
  - `compare` says "N still open", not "as above".
  - "an Apache Iceberg".
- **The app:**
  - A run written with `-o` is not offered Rename, Delete or Set as baseline; they all failed.
  - Every count is grouped ("1,500 requests"), in Chinese too.
  - Money under a millionth is written out, never "2.6e-7 USD".
  - Scrolling Output folds the run header.
  - Run with no command says why.
  - A reason given on press no longer reads "[object Object]".
  - In Chinese: History's new warnings are translated, "AWS profile" reads 配置文件, and a run with no commit is named
    by its time.
- **install.sh and the Action:**
  - install.sh takes the static build where glibc is older than 2.35 (RHEL 9, Amazon Linux 2023, Ubuntu 20.04),
    refuses Linux aarch64 there and says why, and never says "installed" for a binary that cannot start. The README no
    longer calls the binary static.
  - The Action's `run` input is run by bash as typed: `&&`, `|`, `>` and `cd x &&` are all recorded.
  - The Action's report is written in `working-directory`.
  - The merge-base's run is kept apart and is never "the run before" the pull request's.
  - The Action at `@v24.0` and `version: 24.0` install the newest 24.0.x.
- One endpoint's missing `ca-file` no longer stops every command: it is said when that endpoint is used.
- Docs:
  - the merge-base;
  - an earlier major's baseline;
  - the newest 6 runs;
  - `check -f json`'s `ok`;
  - the findings table;
  - `$XDG_DATA_HOME`.

### Removed

- A chart's "Save its rows as CSV" button: a button on every chart cluttered the cards. A run's data is
  `sonde show -f json`'s; Capacity keeps its own export.

## [24.0.0] - 2026-11-15

24.0 gives back what 21–23 took from view, finds the S3 mistakes people really make, and gates CI from the first pull
request. Finding identities (rules 4) and `[check] expect` change shape ([contract](docs/contract.md)).

### Changed

- **Nothing you read is lost.**
  - The run header stays open on every pane and at every size, folding only as you scroll: endpoint, date, commit, SDK
    and the run's sentence. Folded, latency says "median".
  - "Since the run before" names the finding that changed.
  - A bucket's tabs carry what is known: "Size · 176 MiB", "Incomplete uploads · 3" (marked when any is stale),
    "Layouts · …".
  - A program past its baseline is marked on its newest run in the sidebar.
  - A trial's verdict says where it turned ("at PutObject …"), its reason before its history.
  - The Time chart "Which kind of request got slow, and when?" is back as its own card.
  - A chart's (i) and CSV are quiet, never hidden.
  - An object says "Checksum: none stored" and "SSE: none reported".
  - A release now names every phrase the app stopped using (CI checks it against the last tag).
- **The sidebar says what sent each run's requests:** "boto3 · demo · 100 requests", "Lance, object_store · …", read
  from the user agents (an application before its SDK). `sonde runs` has a client column.
- **Findings for the mistakes people make:**
  - many small writes (→ fewer, larger objects);
  - deletes one at a time (→ DeleteObjects);
  - read-modify-write with no If-Match (→ a conditional write);
  - a missing key polled (→ back off, or a waiter).

  Each finding has fixes for boto3, JS, Go and Java and its money a month, on runs and on imports. A key read again and
  again is judged between its writes, with a floor of 10 reads saving 1%. Throttling is one kind; `obs.small` covers
  small multipart uploads; a one-time orphan is `obs.orphans` (1 time). `docs/cli.md` lists every finding with its fix.
- **CI from the first pull request:** with no stored baseline the Action records the merge-base in the same job (a
  `setup` input prepares it). A `--demo` run is never held to its waiting.
- **`[check] expect` takes a reason** (`expect = { "<identity>" = "<reason>" }`), shown in text, JSON, the PR comment
  and to agents, who leave such findings alone.
- **stale-list** is tried by default only on stores that are not AWS (AWS is strongly consistent; `--fault stale-list`
  still runs it).
- Runs written with `-o` elsewhere are listed in `sonde runs` and the app.
- **Interface:**
  - one clock, in the locale's hour cycle;
  - the least useful words are cut first;
  - Run, Record and Save say why on press instead of sitting disabled;
  - the sidebar toggle always toggles;
  - first-run labels are whole;
  - Linux Chinese is set in an SC face;
  - Buckets' objects table fills its pane;
  - an import from several principals says "from N callers";
  - counts are grouped everywhere ("87,139"); "probed ids 1–128".
- Native Shell at its main (32f1635): Windows IME placement in Settings, terminal resizing, Linux focus.

### Removed

- `high-contrast` in config.toml (the system's contrast is followed).
- The sidebar's "stays open here" refusal (`sidebar.held`): the toggle acts.
- `obs.orphans.one` (folded into `obs.orphans`); the `[check] expect` list form.

## [23.0.1] - 2026-11-15

### Fixed

- A run in the sidebar says again where it went and how many requests it sent (`demo · 18 requests`), beside what
  tells it from its neighbours; 23.0.0 showed one or the other.
- Compare's cost tile reads "Request cost" again (23.0.0 showed the two-store sentence's placeholders).
- An amount under a cent is written in two figures (0.0040 USD), never "<0.01".

## [23.0.0] - 2026-11-15

23.0 makes sonde an agent's tool, answers "will it work on that store?" truthfully, says cost a month, and calms the
interface. JSON outputs change shape ([contract](docs/contract.md)).

### Changed

- **For coding agents.** `show -f json` is the compact answer by default (a 15,100-request run: 6.6 MB → 6.7 KB; a
  109,800-line access log: 70 MB → 9 KB); `--all` is the whole. A finding carries its `kind`, what it saves in money,
  its callers as file, line, function and the code line, and the fix for the run's own SDK only; the run carries the
  command that verifies a fix. `docs/agents.md`, `llms.txt` and a line in `sonde --help` say the loop.
- **"✓ fixed" is honest.** When a rerun did much more work (requests 1.5× or more), no saving is counted and that is said.
- **Will it work on another store?** Replay first writes stand-ins for what the run read but did not write, so a job
  that reads its input replays with no false differences. It ends with what the program relies on (checksums,
  conditional writes, ranges, multipart, …) and how the store answers each, and each store's request cost. The app
  asks it from a run: ⋯ › On another store….
- **Money a month.** An import projects a month from its own span; a run uses `[program] runs-per-month`. Findings are
  ordered by what they save, each with its saving a month; the list ends with their total.
- An imported request's caller is its requester (the IAM principal). `compare` names both endpoints when they differ.
- **The interface, calmer:**
  - one surface level: tables fill their pane, no heading over a titled card;
  - the run header folds to one line on short panes and on Time and History;
  - "Since the run before" is one sentence with a Compare link;
  - dialogs sit under the title bar and grow with their content;
  - the inspector stays beside the table down to 780 pt;
  - pressable tiles look pressable;
  - row actions are quiet, never hidden;
  - sidebar rows say two things;
  - long identifiers stay on one line and copy on press.
- **Controls that did nothing now act:**
  - a chart pressed with no pointer over a mark (or with Return);
  - the chart's (i);
  - the title bar's place;
  - the sidebar toggle;
  - the Run folder field's icon (a folder dialog).
- A plan's count is typed only above 100 items (or for a lifecycle); below that, a plain confirm.
- Native Shell at its main (e4749c9): a side panel is a named modal dialog; inline code in prose keeps proportion.
- The demo store answers CreateBucket. Probe text says no `Some(400)`, and its summary counts every probe.

### Removed

- `show --summary` (it is the default now) and its schema.
- The app's Settings compatibility dialog and its probe screens: compatibility is asked from a run; `sonde probe` stays
  the full catalogue.

## [22.0.0] - 2026-11-15

22.0 is the smaller sonde: fewer commands, flags, files, words and doors, each finished. It reads no data of earlier
versions and keeps no old names ([contract](docs/contract.md)).

### Changed

- A recording is a **run** everywhere: `sonde runs` (it lists by itself), the `runs/` folder, `"run"` in JSON, the
  file format `sonde-run`, the app's Runs area. `sonde layout` has no `inspect`.
- Plain `run` and `proxy` record under `allow`; `--trial` and `--again` under `no-delete`, which refuses only deletes,
  aborts and lifecycle, policy and versioning changes.
- With no endpoint configured, `run` goes where an AWS SDK would (the default chain and region). A bucket in another
  region is signed for it, cached per bucket.
- A program is `--name` / `[program] name`, else what its command runs: `sh -c`'s command, a container's image,
  `python -m`'s module.
- `sonde` alone says where to start; `run --demo` shows a finding, its line, its fix, and the job fixed.
- More clients reach sonde with nothing set: s5cmd (`S3_ENDPOINT_URL`), `docker run` / `podman run` (host network,
  variables by name). When nothing arrives, the program's own client is named first.
- `share` writes one HTML file holding the app's own views and Time pane; two runs open on Compare.
- Every command that reports writes `text` and `json` (`run` leaves stdout to its program); `check` adds `markdown` and `github`. Destructive commands are a dry run,
  then `--yes`. Costs under a cent are said as a share; probe failures give their reason.
- All state beside the runs is in the folder's one index, under one lock, replaced durably.
- Charts, every one kept and shown:
  - correct on large runs (the heat map counts every request);
  - zoom draws the stretch's own detail;
  - throttles have their own colour, and a failure bar opens its requests;
  - latency bands on a log scale, rates with units;
  - computed sentences on Compare, trends, Capacity, the live panel and trials;
  - new: Compare over time, errors and p95 trends, bytes by month written, Group by key;
  - colour-blind safe pairs; sparklines in the terminal.
- The app: a finished run opens on its own view; one door to each place; Buckets in four sections, all on the keyboard;
  compatibility is checked from an endpoint in Settings.
- A million requests: `show` in about 350 MiB (was 1.1 GiB), `compare` of two in about 0.5 GiB (was 1.36); a
  record is 96 bytes. Signed chunked uploads stream. `show -f json` lists requests only with `--requests`.
- Latest dependencies: reqwest 0.13 (the operating system verifies certificates), RustCrypto 0.11, rand 0.10,
  rcgen 0.14, toml 1; Rust 1.97; Native Shell at its main; macOS CI on macOS 26. The SDK smoke run uses the latest
  boto3, JS, Go and Java SDKs with nothing forced, plus the AWS CLI, its image through `docker run`, polars and `--tls`.

- Inside: phrases live once, in `app/locales`, which Rust reads too; the analysis is its own crate (`sonde-analysis`)
  beside the proxy; a trial runs through one flow for the CLI and the app.

### Removed

- `share --serve`, `--port`, `--html`, `--otlp`, `--to` and the static site; all OTLP.
- `proxy --rotate/--alert`, `import --follow/--every/--alert`; `import s3://` is one pass that carries on from the last.
- `--confirm`; `check --max-cost`, `--max-p95`, `--no-new-warnings` (a baseline always forbids new warnings).
- CSV and the other Markdown outputs; reading sessions of earlier formats; the no-delete ledger and injected headers.

### Fixed

- `check` fails a run of no request ("nothing reached sonde").
- A 503 retried away is a note, not a warning; `max-errors` counts requests that failed after their retries.
- A hot key keeps its finding when some reads were throttled.
- A recording that could not write says so.
- A chart sentence on few requests says so.
- `--tls` certificates are accepted by Python 3.13's strict default.
- While a program runs, Run shows its requests live.
- The toolbar's endpoint opens with a press anywhere on it, not only on its arrow.
- `show -f json` of a run with 20 or more requests of a kind holds to its schema (`few` is left out when false).
- `show` of a million is linear again (60 s → 8.5 s).
- The Action passes over a baseline an earlier sonde wrote (the first CI run after upgrading has none, not a
  failure).

### Not in this release

- The macOS app is not signed with a Developer ID or notarized, and Windows has no Authenticode: the steps are in CI,
  waiting for the keys.

## [21.0.0] - 2026-11-14

21.0 removes and renames without notice: a break comes in the major that makes it ([contract](docs/contract.md)).

### Changed

- The app, refined on Native Shell's main: every size with its line, Chinese at 12 pt, motion and press feedback,
  colour only on marks, no boxes inside cards, actions on hover, one header with one primary, dialogs of two widths
  that keep their size, a narrower sidebar at the least window.
- Charts: labels off the data, titles across the card, shorter legends of drawn marks only, failure charts fitted to
  their failures, chart icons on hover. Every chart is kept.
- A program's key leaves out only dates, times, UUIDs and ids; when its command changed, the fix loop says so and
  counts no saving.
- `show`, `compare`, `share`, `check` and `import s3://` choose the run and the budget by one rule; a `sonde.toml`
  applies to its own program only.
- `--again` implies `--trial`.

### Removed

- `operations[].kind` is `operations[].shape` (`kind` is only the S3 call); `check --program-exit`.

### Fixed

- A key rewritten between reads, or not found, gets no cache advice; a prefix written between listings is not
  "listed again"; a chain breaks at a request that needs the one before.
- A request the policy refused is named, with its policy and `--policy allow`; SIGTERM is said; `sessions` shows each
  run's exit.
- `-o` refuses an existing file in `import` and `share` too, unless `--force`; nothing to import is bad data (65).
- `compare` of two imports no longer contradicts itself or shows unknown waiting; `check --init` warns on a run of
  no requests.
- Bucket tools: costs under a cent, no empty uploads table, lifecycle rules as readable lines.
- Session files are named by their program; deleting a session no longer breaks the list; Open the demo is offered
  whenever the demo is not in use.
- The tour fails what it cannot reach and is approved on Linux, macOS and Windows; CI runs the tour's variants side
  by side and caches Native Shell's build.

## [20.0.0] - 2026-11-14

20.0 breaks without a 19.x announcing it, by the owner's decision ([contract](docs/contract.md)). It reads 19.0's
sessions. From here a break is announced one version ahead.

### Changed

- One name for each thing: an S3 call is a `kind` (`requests[].kind`, `summary.kinds`, `show --by kind`); sessions
  are written in format 2.
- A program is its name, else its command with what changes between runs (dates, numbers, ids) left out, so a
  job run with a new `--date` keeps its baseline.
- Findings keep their identity across runs when keys hold `key=value` parts, numbers or ids (identity rules v2);
  less, more and open are judged by a finding's count.
- `sonde demo` is `sonde run --demo` (without a command, the tour); `--allow-writes` is `--policy allow`; `prune`
  confirms with `--confirm COUNT`; every flag has help.
- `record-policy` and `name-callers` move to `sonde.toml` (`[record]`); `config.toml` keeps what sonde keeps.
- The contract promises figures, ids and exit codes, not display text.
- The interface: every Time chart says in a sentence what it shows, computed from what it draws; a clean run never
  shows "Broke"; sonde's own answers are one colour; the header folds at the least window.
- Releases publish CI's green build of the same commit; main builds and tours Linux, macOS and Windows; the Action
  at a branch or commit installs its own major, and is tried live after each release.

### Fixed

- `check` fails a run whose program failed, and never checks another program's session.
- `compare` reads more requests as "more", not "less"; after a program's first run it says there is none before.
- `run`: an unknown option or no endpoint exits 64; `-o` refuses an existing file without `--force`; a command
  that cannot start leaves no session; SIGTERM exits 143 and says so.
- Replay never creates or deletes a bucket, and refuses one that is not there before sending anything.
- Control characters in keys and program text are escaped wherever sonde prints.
- `import s3://`: an upload completed in a later pass is not called unfinished; a cut CloudTrail document is bad
  data (65).
- The proxy forwards `.` and `..` key segments as sent; no-delete lets a program remove what it made in the same
  recording.
- Trials say "did not break" only when every scenario was judged; `--again` keeps the trial's flags.
- The tour shows committed sessions where it draws measured figures, so no picture is exempt; budgets gate on
  peak memory.

## [19.0.0] - 2026-11-13

19.0 drops compatibility with every earlier version: no aliases, shims, migrations or old names. The contract
([docs/contract.md](docs/contract.md)) starts here. Releases 0.1 to 18.0: [docs/history/changelog-0-18.md](docs/history/changelog-0-18.md).

### Added

- `sonde run --demo -- <command>`: the program against the built-in store `sonde demo` uses; no account or secrets
  (its times are not meaningful, and it says so).
- `install.sh` takes a major (`SONDE_VERSION=19`: the newest 19.x); the Action's `@v19` installs the newest 19.x.

### Changed

- The fix loop: a finding keeps one identity across runs of a program, however its keys or paths are numbered;
  a failed run is never compared; "fixed" says in which run, and "back" when it returned; "less" is judged by
  requests, not time.
- Run: one band in two rows; "Since the run before" says what was fixed, back or less, with the requests saved
  against those predicted. Trials say each piece of advice once and their history as a sentence.
- One failure is one colour everywhere; tiles never cut a figure; compare keeps five tiles abreast when wide.
- CI runs once per change and reuses its builds at release; macOS and Windows pictures run nightly.

- JSON: `busyMs` is `waitingMs`, `episodes` is `operations` (`episodesTotal` is `operationsTotal`), everywhere;
  every schema in `docs/schemas` follows.
- `compare` writes counts with separators, as `show` does (`1,000,000`).
- A replay always writes under a prefix of its own and removes what it wrote.
- A release made without the signing keys says what it skipped on its page, not only in its log.

### Removed

- The commands folded in 18.0: `watch`, `export`, `replay`, `diff`, `mpu`, `du`.
- `--log-out`, `--read-only`, `--no-delete`, `share --json` (now unknown flags); `run --callers` (the default);
  `compare --replay --in-place` and `--keep`.
- Reading what versions before 18.0 kept: the files beside sessions (`.summary.json`, `.trials.json`, `.exit`),
  `config.toml`'s `read-only`, `reqlog-*` and `stale-upload-days`; the app's reading of pages and verdicts made
  before 15.0 and 16.0.
- Homebrew: the formula, the cask and the tap's update.

### Fixed

- Two runs at once no longer write over one session, index or findings file (each is held while written).
- `show` of a large session takes no more memory than 17.0 did.
- `replay` compares keys and bytes, not only counts; `check --program-exit` fails a job its program failed.
- Exit codes: a closed stdout, `layout --kind` of nothing, `prune` dry run (4); `endpoints add --replace`.
- Two programs of one name are told apart in Programs.

- `uses: hxddh/sonde@v18` found no tag: each release now moves its major tag (`action-tag.yml`).
