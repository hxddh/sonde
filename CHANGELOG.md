# Changelog

All notable changes are listed here. The project follows [Semantic Versioning](https://semver.org).

## [29.0.0] - 2026-11-22

sonde answers with one plan: where the run's time went, then what to change, each change with the time it saves; a run again says only what changed, and sonde is smaller — fewer commands, one place for runs, no terminal UI.

### Changed

- Findings are one plan of diagnoses, ranked by the run's time saved ("≈1.9 s of 6.1 s (31%)", money beside it from
  a cent): findings with one root cause (the same objects) are one diagnosis with one saving and one fix, and
  requests one after another join the diagnosis that owns them ("and send the other 62 requests 8 at once"),
  counting only what no other fix removes and what can go at once, its fix for the real operation. Small-object fixes
  pick JSONL, Parquet or tar by what the objects hold.
- A reading opens with where the time went: the program's own, the S3 round trips (count and median), the bytes
  when moving them took most; when most of it is the program's own, it says so. The demo store's times are marked
  indicative.
- What a fix saved is the run's time and money against the prediction ("1.2 s less (predicted 1.5 s)"), or "within
  noise" when the run moved less than the program's own earlier runs of the command vary. Findings are judged by
  their counts alone: `back` is gone (new or more).
- A fault scenario not survived is a finding (`trial`) with its line of code and the fix for the SDK, read fixed or
  open by the next trial; `run --again` is removed (a trial runs every scenario).
- `check --init` records the run's open findings by count (`[check.findings]`), writes the program's own command
  (`--demo` for a demo run) and honours `-f json`; `check` fails on a new finding or one that grew.
- A program run again says what changed since its run before: fixed with what it saved, each diagnosis still open on
  a line with its code line, new ones in full; `sonde show` has the whole plan. `run --trial` prints the clean run's
  head line, one verdict per scenario and what did not survive.
- `show -f json` is lean: each diagnosis carries its text, code lines (`at`), fix and saving, so `diagnoses[0]` is
  the next step; `findings` only with `--all`; no `phrases`, `impact`, `level` or `summary.kinds`; `since` is null on
  a first run and says each finding's change once (`since.findings`, its `measured` now `saved`); figures rounded.
- `show RUN --since OTHER` compares two runs (it replaces `compare`); `sonde replay RUN -e E` replays one;
  `sonde run --listen [ADDR]` serves an endpoint with no program (it replaces `proxy`); `sonde endpoints test E
  --probes BUCKET` runs the probes (it replaces `probe`). `endpoints add ID --profile P` with no URL takes the
  profile's endpoint and region; `check -f markdown` reads the report's link from `SONDE_REPORT_URL`.
- The app: Runs is where a program is run — with no run chosen it is the command bar, and a running program is the
  newest run, its terminal beside its operations; the places are Runs, Buckets and Settings, and Activity opens from
  the View menu, ⌘⇧A or the palette. A run's header is its command, where its time went and one line of figures;
  output stands beside the operations; Group by is on the operations' title; a chart's title says how to read it;
  endpoints are tested from their ⋯.
- The app: a run's findings are its plan, a diagnosis a line with the time it saves; choosing one narrows the
  operations and opens its fix in the inspector — the line of code, the change for the program's own SDK (others in a
  menu), Apply — and "Run again" steps back to outline. A trial's verdicts fill the run's History as they come, and
  what broke is among the run's diagnoses; "Run trials again" asks the plan anew.
### Removed

- The terminal UI (`sonde tui`) and its release build.
- `sonde compare` and `compare ORIGINAL REPLAY`, `sonde proxy`, `sonde probe` with `probe list/runs/diff` and saved
  probe runs, and exit code 3 (probes that could not run exit 70).
- `--rate`, `--concurrency`, `run --live`, `endpoints import`, `check --report-url`, `bucket size --cached` and
  `--no-multipart`, `show --by sdk|status`; config `concurrency`, `rate-limit`, `exports-dir`, `downloads-dir` and
  `[settings.prices.by-class]` (old files still load; the keys are ignored).
- Schemas `compare`, `compare-answers`, `probe-list`, `probe-runs`, `probe-diff` (`compare-replay` is `replay`,
  `probe` is `endpoints-probes`).
- The app's Run place, Output tab, the run header's tiles and the chart cards' ⓘ.
- The app's trial results dialog and "Try again what broke"; a finding's inline advice and SDK tabs.
### Fixed

- A finding under its threshold in a large run and over it in a small one read "back" with the same count.
- A trial's clean run of another command was compared with the program's runs and became the run the next one was
  compared with.
- `show --by key` puts a listing under its prefix, not its bucket; "1 object"; no lines for amounts under a cent.

### 中文

sonde 现在给出一份改动计划：先说运行的时间花在哪里，再说改哪里、各省多少时间；再次运行只说变化；sonde 也更小了——命令更少、运行只有一个位置、去掉终端界面。

- 变更：发现合并为一份按节省的运行时间排序的诊断计划（"≈1.9 s / 6.1 s（31%）"，满一分钱才附金额）；同一根因（同一批对象）
  的发现合为一条诊断，只给一个节省值和一个修复；串行请求并入所属诊断，只计其他修复不会去掉、且能同时发出的请求，
  修复按实际操作给出；小对象修复按内容选 JSONL、Parquet 或 tar。
- 变更：读数先说明时间花在哪里（程序自身、S3 往返、传输）；大部分时间在程序自身时会直说；演示存储的耗时标为仅供参考。
- 变更：修复节省改为整次运行的耗时和费用（"少了 1.2 s（预计 1.5 s）"），变化小于该程序以往同一命令运行的波动时显示"在波动范围内"；
  发现只按计数判断，去掉"又出现"。
- 变更：未扛住的故障场景成为一条 `trial` 发现，带代码行和 SDK 修复，下次试验判定已修复或未修复；移除 `run --again`。
- 变更：`check --init` 记录当前未修复的发现及计数（`[check.findings]`）、写入程序自己的命令并支持 `-f json`；`check` 遇到新发现或计数增长即失败。
- 变更：再次运行只说变化（已修复及节省、未修复的每条一行、新出现的完整列出），完整计划见 `sonde show`；`run --trial` 只打印首行、每个场景一行结论和未通过项。
- 变更（不兼容）：`show -f json` 精简，每条诊断直接带文字、代码行（`at`）、修复和节省；`findings` 仅在 `--all` 下输出；
  去掉 phrases、`impact`、`level`、`summary.kinds`；首次运行 `since` 为 null；`measured` 改名 `saved`；数值取整。
- 变更：`show RUN --since OTHER` 取代 `compare`；`sonde replay RUN -e E` 重放；`sonde run --listen` 取代 `proxy`；
  `sonde endpoints test E --probes BUCKET` 取代 `probe`；`endpoints add --profile` 不带 URL 时取 profile 的端点与区域。
- 变更：应用中"运行记录"即运行程序之处，未选中运行时是命令栏，运行中的程序是最新一条运行；位置只有运行记录、桶和设置，
  "活动"从"显示"菜单、⌘⇧A 或命令面板打开；运行标题为命令、时间去向和一行数字；输出在操作旁；图表标题说明如何阅读。
- 变更：应用中运行的发现即计划，每个诊断一行并附可节省的时间；选中后筛选操作，并在检查器中打开修复（代码行、程序自身 SDK 的改法，
  其他 SDK 在菜单里、应用），"再次运行"改为次要按钮；试验结论实时填入运行的"历史"，失败项出现在诊断中；"再次试验"重新显示计划。
- 移除：终端界面（`sonde tui`）；`compare`、`proxy`、`probe` 及其子命令和保存的探测记录、退出码 3；`--rate`、`--concurrency`、
  `run --live`、`endpoints import`、`check --report-url`、`bucket size --cached/--no-multipart`、`show --by sdk|status` 及相关配置项；
  应用的"运行"位置、"输出"标签页、标题指标卡和图表上的 ⓘ。
- 修复：大运行中低于阈值、小运行中超过阈值的发现被显示为"又出现"；命令不同的试验干净运行被当作程序的上一次运行；
  `show --by key` 把列举归到前缀下；"1 object" 单数；不足一分钱的金额不再单列。

## [28.0.0] - 2026-11-21

sonde's JSON always parses, its savings and imports count what really happened, a pasted command runs only what it says, and the app no longer stops while it opens a large run.

### Changed

- `show`, `check`, `compare` and `share` with no run named refuse (64) when this folder has none, instead of taking
  another program's newest run.
- `check --set-baseline` accepts a run written with `-o`; `--baseline none` means no baseline, sonde.toml's included.
  In the GitHub Action, `baseline: auto` that finds nothing leaves sonde.toml's baseline in place.
- `endpoints add` and `--url` take the region from an AWS URL, else `AWS_REGION`, before us-east-1.
- `import` prints its summary on stderr, as `run` does: stdout is only what was asked for.
- The app's settings apply silently, without a "Settings saved · Undo" toast after each change (`settings.saved`,
  `common.undo`).
- The app: Versions of an object is on ⇧⌘H (Ctrl+Y is Redo on Windows and Linux), and Buckets' sections (⌥⌘1–4) no
  longer fire while typing. File › Run is gone (⌘1 opens Run); the Window menu is macOS's alone; macOS's app menu says
  "Settings…", Linux's "Quit".
### Fixed

- `-f json` is always valid JSON: a key's DEL, C1 controls or direction marks are written `\u007f`, `‮`, never
  as the terminal escapes them.
- `check --baseline` naming a missing file or an `http://` URL is a usage error (64) that says what to do;
  `--tolerance` and `[check] tolerance` refuse a negative value or a non-number.
- What a fix saved is measured right when keys hold a run's own id (`jobs/{id}/state.json`): a drop from 40 reads to
  36 saves 4, not 39.
- A smaller workload is no longer read as a fix: with the same command, a run a third or more below what fixing the
  earlier run's findings would leave says "the workload shrank" and counts no saving.
- A listing retried after errors counts once in `obs.listed`, and a small upload whose part was retried is still a
  small upload. A trial's outcome is never lowered by a later reason: broke stays broke.
- Imports: a HAR's status 0 and an access log's `-` are no answer, not priced; CloudTrail's `NoSuch*` and `*NotFound`
  errors are 404, conflicts 409; buckets are read from R2, GCS and Spaces hosts and from names that hold `.s3`; a HAR
  entry with no time is skipped and counted instead of failing the import; a base64 error body is read.
- "Copy as curl" and "copy as aws cli" quote every value taken from a record, so a crafted method, region or
  operation name in a shared run cannot run commands where it is pasted; a store's region is taken only as letters,
  digits and dashes.
- A request signed with a non-ASCII UTF-8 header (`x-amz-meta-name: café`, as the Go SDK and minio-go send) is no
  longer refused; a value that is not UTF-8 is refused with the reason.
- An upload sent as unsigned aws-chunked counts its payload, not its framing. A virtual-hosted bucket in capitals or
  with a final dot (`b.LOCALHOST.`) is read as that bucket.
- Under `no-delete`, a rename can no longer pass as a PutObject through the SDKs' `x-id` marker.
- A store's error body that stops part way is a transport error, not forwarded with a wrong length; only its first
  64 KiB is held. The proxy raises its open-files limit and no longer spins when out of them. A transport error
  naming a presigned URL is redacted; two sondes renewing the certificate no longer remove each other's lock.
- Replay sends DeleteObjects and settings documents with the Content-MD5 of what it sends, keeps a `?` in a copy
  source's key, and leaves out pages that go on from the original store's version or upload markers.
- A layout whose listing stopped at its cap says so and no longer calls the files past it missing; a GET that fails
  no longer ends a SlateDB, lakeFS or Woodpecker analysis; a local folder gives nothing outside it.
- A listing ends with an error when a store hands back a page marker again, instead of running forever; probe
  cleanup aborts uploads on every page and removes a temporary bucket whose creation went unanswered.
- `consistency.list-after-write` no longer fails a store that kept a write the client saw fail.
- `run --demo -o x.sonde` no longer overwrites `x-fixed.sonde` without `--force`; `runs prune` keeps a run still being
  recorded.
- The app no longer stops while it opens, receives or is handed a large run (a drop, a double-click, `sonde://open`):
  the run is read off the window's thread, and a live run no longer reads the whole runs folder every second.
- Stop pressed just after Run stops the program as soon as it starts; "Stopped" is said only when it did.
- Counts, sizes, durations and costs use the region's separators alike, and one minus sign.
- Time no longer shows the previous stretch's figures under a new one; Load all waits for a page being loaded
  instead of doing nothing; a search or size scan replaced as it starts no longer runs on.
- Compare… names runs of other days by their date; an operation of more than about 100,000 requests opens.
### 中文

sonde 的 JSON 总能解析，修复节省和导入按真实发生的计算，粘贴的命令只执行它写的内容，应用打开大运行时不再卡住。

- 变更：未指定运行且本目录没有运行时，`show`、`check`、`compare`、`share` 直接拒绝（64），不再取别的程序的运行。
- 变更：`check --set-baseline` 接受用 `-o` 写出的运行；`--baseline none` 表示不用基线（包括 sonde.toml 里的）；
  Action 的 `baseline: auto` 找不到时保留 sonde.toml 的基线。
- 变更：`endpoints add` 与 `--url` 先从 AWS 地址、再从 `AWS_REGION` 取区域，最后才用 us-east-1。
- 变更：`import` 的摘要与 `run` 一样输出到 stderr。
- 变更：应用的设置改后静默生效，不再弹出“设置已保存 · 撤销”。
- 变更：对象的版本改为 ⇧⌘H（Ctrl+Y 在 Windows 和 Linux 上是重做）；输入时不再触发桶的分区快捷键（⌥⌘1–4）；
  移除“文件 › 运行”（⌘1 打开运行）；“窗口”菜单仅 macOS 有；macOS 应用菜单为“设置…”，Linux 为“Quit”。
- 修复：`-f json` 始终是合法 JSON：键里的 DEL、C1 和方向控制符写作 `\u007f`、`‮`。
- 修复：`check --baseline` 指向不存在的文件或 `http://` 地址时为用法错误（64）并说明下一步；`--tolerance` 拒绝负数和非数。
- 修复：键里带运行自己的 id 时，修复省下的量算对了（40→36 省 4，而不是 39）；工作量变小不再被当成修复。
- 修复：出错后重试的列举只算一次；分片被重试过的小上传仍算小上传；试验结果不会被后面的原因降级。
- 修复：导入：HAR 状态 0 和访问日志的 `-` 视为没有响应、不计费；CloudTrail 的 NoSuch*/NotFound 为 404、冲突为 409；
  能从 R2、GCS、Spaces 主机和含 `.s3` 的桶名读出桶；缺时间的 HAR 条目跳过并计数；base64 错误体可读。
- 修复："复制为 curl / aws cli"对取自记录的每个值加引号，分享的运行不能借此在粘贴处执行命令。
- 修复：带非 ASCII UTF-8 签名头的请求不再被拒；未签名 aws-chunked 上传按负载计字节；大写或末尾带点的虚拟主机桶名能被识别；
  no-delete 下重命名不能借 `x-id` 冒充 PutObject。
- 修复：中途中断的存储错误体记为传输错误；代理提高文件句柄上限且耗尽时不再空转；含预签名 URL 的传输错误被脱敏；
  两个 sonde 同时续期证书不再删掉对方的锁。
- 修复：回放附上 Content-MD5、保留复制源键里的 `?`、略过沿用原存储标记的翻页；布局列举截断时会说明且不再误报缺失；
  一次 GET 失败不再中止 SlateDB、lakeFS、Woodpecker 分析；本地目录不会读出其外的文件。
- 修复：存储重复返回翻页标记时列举报错结束；探测清理处理每一页上传和未应答的临时桶；
  `consistency.list-after-write` 不再误判。
- 修复：`run --demo -o x.sonde` 不再未经 `--force` 覆盖 `x-fixed.sonde`；`runs prune` 保留正在录制的运行。
- 修复：打开、拖入、双击或经 `sonde://open` 打开大运行时，应用不再卡住；录制中不再每秒读取整个运行目录。
- 修复：点击运行后立即停止，程序一启动就会停止；只有真正停止时才显示“已停止”。
- 修复：计数、大小、时长和费用统一使用地区的分隔符和同一个负号。
- 修复：时间页不再在新时段下显示上一时段的数字；“全部加载”会等正在加载的一页；被取代的搜索或容量扫描不再继续运行。
- 修复：“比较…”中不同日期的运行显示日期；超过约十万个请求的操作能打开。

## [27.0.0] - 2026-11-20

sonde's findings and costs now follow the order requests were sent, nothing secret leaves in a share, and the Time pane opens at once.

### Changed

- Time: the pane opens at once. Its charts are built as they come into view, not all before the pane shows; every
  chart is still there.
- The app is built on Native Shell 0.6.1 (fixes from its 0.6 line).
- The GitHub Action's `run-args` and `check-args` are words as written: a `$VAR` in them is no longer expanded (use
  `${{ vars.X }}` or `${{ env.X }}`).

### Fixed

- A run is read in the order its requests were sent. The recorder writes each request when its answer is in, so a
  run file is in the order they finished; read so, a range read again after a write looked unchanged
  (`obs.rangeagain`), a HEAD was paired with a GET sent before it (`obs.headget`), a write sent before a read was a
  read-modify-write (`obs.rmw`), and what a run leaves stored counted a delete before the write it followed.
- A DeleteObjects body is read by one XML reader everywhere: `a&amp;b` is the key `a&b`, and a prefixed `<s3:Key>`
  is a key. Before, each place had its own text search.
- What a run leaves stored counts the keys a DeleteObjects deletes (on a versioned bucket, a marker each, or the
  version named), and a RenameObject's object under its new name; a replay seeds a rename's source and sends it.
- `obs.markers` saves a DeleteObjects only when it deletes directory markers alone, and `obs.wholeread` sets a whole
  read against ranges of the same object, nothing written to it between.
- An AWS CLI run is offered no boto3 code as a fix.
- Redaction masks S3's `<CanonicalRequest>` and `<CanonicalRequestBytes>` (SignatureDoesNotMatch echoes every signed
  header's value there: a session token, an SSE-C key), finds presign parameters in any text, in any case and after
  any separator (`&amp;`, a canonical query line), and masks an element an error body was cut inside to its end.
- `sonde share` hides the word after an option that names a secret (`--token …`) and a URL's password
  (`https://me:…@host`) in the command, and escapes every `<` in the run it carries, so no text in it opens a
  comment or closes the page's script.
- `no-delete` reads a PutBucketVersioning document as XML: anything but plain `Enabled` is refused (before,
  `Suspen&#100;ed`, a comment or a UTF-16 body went through).
- An injected fault's error document escapes its code and message; a fault's code is letters and digits.
- The GitHub Action no longer `eval`s `run-args`, `check-args` or a plain `run`: they are split into words, quotes
  kept, and nothing in them is run or expanded.
- `--tls`: two sondes renewing the certificate at once no longer mix their files (`tls/make.lock`), and
  `bundle.pem` is rewritten only when it changes.
- `sonde run` names the line that sent a request again with the AWS SDK for JavaScript v3's newer releases (the
  client moved to `@smithy/core`), and in a program written as ES modules (`import`), on any Node.js the SDK runs on.
- The README named the GitHub Action `@v25`; it names the current major, and a check keeps it in step.

### 中文

sonde 的发现和费用现在按请求的发出顺序计算，分享不再带出任何密钥，时间页立即打开。

- 时间页立即打开：图表在滚动到可见时才构建，不再在页面显示前一次性全部构建；所有图表仍然都在。
- 应用基于 Native Shell 0.6.1（包含其 0.6 系列的修复）。
- GitHub Action 的 `run-args` 和 `check-args` 按原文分词，其中的 `$VAR` 不再展开（请改用 `${{ vars.X }}` 或 `${{ env.X }}`）。
- 修复：运行按请求的发出顺序读取。录制器在应答到达时才写入请求，运行文件因此按完成顺序排列；以前按此顺序分析时，
  写入之后再次读取同一范围被当作内容未变（`obs.rangeagain`），HEAD 会与在它之前发出的 GET 配对（`obs.headget`），
  先发出的写入被当作“读后写”（`obs.rmw`），存储留存也会把删除算在它之后才发出的写入之前。
- 修复：所有地方都用同一个 XML 解析器读取 DeleteObjects 请求体：`a&amp;b` 就是键 `a&b`，带前缀的 `<s3:Key>`
  也算键。以前各处各自按文本查找。
- 修复：运行留存的存储会计入 DeleteObjects 删除的键（开启版本控制的桶上，每个键留下删除标记，或删除指定的版本），
  RenameObject 的对象按新名称计；回放会为重命名的源对象预置数据，并发送重命名的源。
- 修复：`obs.markers` 只在 DeleteObjects 全部删除目录标记时才计为可省；`obs.wholeread` 只把整读与同一对象、
  中间没有写入的范围读取相比。
- 修复：AWS CLI 的运行不再给出 boto3 代码作为修复。
- 修复：脱敏现在会遮盖 S3 返回的 `<CanonicalRequest>` 和 `<CanonicalRequestBytes>`（SignatureDoesNotMatch 会在其中回显
  所有签名头的值，包括会话令牌和 SSE-C 密钥），能在任意文本中识别预签名参数（不区分大小写，`&amp;` 或规范查询行也可），
  错误正文在某个元素中间被截断时，该元素一直遮盖到末尾。
- 修复：`sonde share` 会隐藏命令中密钥类选项后面的那个词（`--token …`）和 URL 中的密码（`https://me:…@host`），
  并转义所携带运行数据中的每个 `<`，任何文本都无法开启注释或提前结束页面脚本。
- 修复：`no-delete` 现在按 XML 解析 PutBucketVersioning 请求体，只放行明确的 `Enabled`，其余一律拒绝（以前
  `Suspen&#100;ed`、注释或 UTF-16 编码的请求体可以通过）。
- 修复：注入故障的错误文档会转义错误码和消息；故障的错误码只能由字母和数字组成。
- 修复：GitHub Action 不再对 `run-args`、`check-args` 和普通的 `run` 使用 `eval`，而是按词拆分并保留引号，其中的内容
  不会被执行或展开。
- 修复：`--tls`：两个 sonde 同时续期证书时不会再混用彼此的文件（`tls/make.lock`），`bundle.pem` 仅在内容变化时重写。
- 修复：AWS SDK for JavaScript v3 的较新版本（客户端移到了 `@smithy/core`）下，`sonde run` 重新能指出发出请求的代码行；
  以 ES 模块（`import`）编写的程序同样可以，适用于该 SDK 支持的任何 Node.js 版本。
- 修复：README 中 GitHub Action 写的是 `@v25`，现改为当前主版本，并新增检查保证主版本号一致。

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
