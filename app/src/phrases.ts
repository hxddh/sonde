// What sonde says about a session, in the reader's language. Rust gives each
// title, note and observation as phrases (a key and its arguments) beside
// its English text (crates/proxy/src/phrase.rs); English shows that text,
// other languages say the phrases from their templates here. A phrase with
// no template in a language falls back to the English text, whole.

import { type CorePhrase, sayCoreText } from "./corephrases";
import { type Key, lang, t } from "./i18n";
import type { EpisodeRow, Observation, Phrase } from "./types";

const zh: Record<string, string> = {
  request: "{op} {target}",
  "request.prefix": "{prefix}",
  size: " · {size}",
  "cond.create": "条件创建（If-None-Match: *）",
  "cond.write": "条件写入（If-Match）",
  race: "竞争失败：412 Precondition Failed",
  probe: "在 {dir} 中查找 {n} 次，均未找到",
  listing: "分 {n} 页列出 {bucket}/{prefix}",
  mpu: "分片上传 {key} · {n} 个分片 · {size}",
  "mpu.aborted": "（已中止）",
  "mpu.open": "（未完成）",
  "mpu.billed": "这次上传既没有完成也没有中止：已上传的分片仍在计费",
  retry: "{op} {target} 在 {first} 后重试了 {n} 次",
  "retry.wait": "{ms} ms 后才成功或放弃",
  "step.deleted": "删除了 {file}",
  "slate.manifest": "写入 manifest {n}",
  "slate.wal.empty": "写入空的 WAL {n}（隔离标记）",
  "slate.wal": "刷写 WAL {n} · {size}",
  "slate.sst": "SST 刷写到 L0 · {size}",
  "slate.compactions": "写入 compaction 状态 {n}",
  "iceberg.metadata.v": "提交：表元数据 v{v}",
  "iceberg.metadata": "提交：表元数据",
  "delta.commit": "提交 {n}",
  "hudi.instant": "{action} {instant}",
  "hudi.metadata": "元数据表：{action} {instant}",
  "hudi.properties": "写入表属性（hoodie.properties）",
  "paimon.snapshot": "快照 {n}",
  "paimon.schema": "写入 schema {n}",
  "paimon.hint": "更新提示文件 {file}",
  "delta.checkpoint": "写入 checkpoint",
  "delta.last": "更新 _last_checkpoint",
  "lance.commit": "提交版本 {v}",
  "lance.data": " · {n} 个数据文件（{size}）",
  "lance.index": " · 索引（{size}）",
  "lance.deletions": " · 删除了行",
  "lance.read.v": "读取版本 {v}",
  "lance.read": "读取数据集",
  "lance.ranges": " · {files} 个文件的 {n} 个范围（{size}）",
  "step.other": "{op} {key}",
  "step.files": " · {n} 个文件（{size}）",
  "after.listing": "先列出了 {dir}/",
  "after.reads": "之前读取了 {n} 次",
  "after.misses": "之前查找了 {n} 次，均未找到",
  "after.mixed": "之前读取了 {n} 次（{m} 次未找到）",
  "loose.uncommitted": "写入了 {n} 个文件，之后没有提交（{size}）",
  "slate.walprobe": "查找最新的 WAL：探测了编号 {lo}…{hi}",
  "read.manifest": "读取 manifest {n}",
  "read.compactions": "读取 compaction 状态 {n}",
  "read.metadata": "读取表元数据 v{v}",
  "read.commit": "读取提交 {n}",
  "read.file": "读取 {file}",
  "read.count": "（{n} 个请求）",
  "loose.listed": "列出了 {dir}/",
  "loose.misses": "查找了 {n} 次，均未找到",
  "loose.reads": "读取了 {n} 次",
  "loose.deletes": "删除了 {n} 个文件",
  "obs.unfinished": "{n} 个分片上传既没有完成也没有中止：分片会一直计费，直到生命周期规则清理它们",
  "obs.orphans.one": "写入了文件但之后没有提交：除非后续提交引用它们，否则就是孤儿文件",
  "obs.orphans": "有 {n} 次写入了文件但之后没有提交：除非后续提交引用它们，否则就是孤儿文件",
  "obs.small": "{n} 个小于 5 MiB 的文件用分片上传：每个 3 个请求，一个 PUT 就够",
  "obs.rename": "{n} 个对象从 _temporary/ 复制出来再删除：靠重命名提交（Hadoop 的 FileOutputCommitter）在 S3 上把每个输出文件又写了一遍；S3A committer 只写一遍",
  "obs.markers": "写入了 {n} 个目录标记（名为 …/ 的空对象），删除标记的请求 {m} 个：s3a 设置 fs.s3a.directory.marker.retention=keep 就会保留它们",
  "obs.hot": "{key} 被读取了 {n} 次（占全部请求的 {pct}%）",
  "obs.listed": "{prefix} 被列出了 {n} 次",
  "obs.headget": "{n} 个对象先用 HEAD 查看、再用 GET 读取：单独一个 GET 就能同时回答两者",
  "obs.throttled": "{n} 个请求被限流（503 SlowDown 或 429），当时都有 {min} 个或更多请求在进行：同时少于 {safe} 个大概率可以避免",
  "obs.throttled.rate": "{n} 个请求被限流（503 SlowDown 或 429）：存储要求降低每秒请求数",
  "obs.saves": "可少发 {n} 个请求（占会话的 {pct}%）",
  "fix.fixed": "已修复",
  "fix.less": "减少",
  "fix.back": "又出现",
  "fix.new": "新出现",
  "fix.open": "未修复",
  "fix.saved": "少了 {n} 个请求（预计 {p}）",
  "fix.savedTime": "等待少了 {time}（预计 {p}）",
  "trial.advice.retries": "{sdk} 没有重发这个请求：打开它的重试（{setting}）",
  "trial.advice.retry": "失败的请求要重发：500、502、503、504、408 和 429 都应当重试，S3 的 SlowDown、RequestTimeout 和 RequestTimeTooSkewed 也一样",
  "trial.advice.skew": "403 RequestTimeTooSkewed 应当按响应 Date 给出的时间重新签名后重试（Java 和 JavaScript 的 AWS SDK 会这样做，boto3 不会）；并让机器的时钟保持准确",
  "trial.advice.timeout": "给每次尝试设一个远短于停顿的超时（首字节通常远不到一秒就到），这样停住的请求会被重发",
  "trial.advice.list": "刚写入之后的列表可能还不包含它：记下程序自己写入的键，或者在缺少某个键时先重新列一次再继续",
  "trial.advice.cut": "把读到的字节数与 Content-Length 核对，再读取其余部分（Range）或整个重读",
  "trial.advice.race": "收到 412 后重新读取日志，带着条件写入下一次提交；不要不带条件地写这个键",
  "trial.why.notMet": "这个场景没有遇到程序的任何请求",
  "trial.why.uploads": "留下了 {n} 个未完成的分片上传（分片在中止前一直计费）",
  "trial.why.reads": "放弃了 {n} 次失败的读取，没有重发",
  "trial.why.orphans": "比正常运行多留下 {n} 个对象：是丢失的那次提交写入的，除非随后的提交引用了它们",
  "trial.why.blind": "{n} 次竞争失败的写入用同一个 If-Match 重发：真正的写入者的 ETag 会再次拒绝它",
  "trial.why.lost": "{n} 次写入收到 500 后没有重试；存储里已经有了",
  "trial.why.writes": "{n} 次写入失败后再也没有重发",
  "trial.why.exit0": "，而程序以 0 退出",
  "trial.why.taken": "{n} 次下载被截断，其余部分再也没有读取",
  "trial.why.taken.whole": "：把一半当成了全部（退出码 {exit}，与正常运行相同）",
  "trial.why.holes": "{n} 次下载被截断后从截断点之后继续读：缺了字节",
  "trial.why.unrun": "{n} 次条件写入竞争失败，之后没有新的提交，也没有读回胜出的内容",
  "trial.why.unrun.exit": "，而程序以 {exit} 退出",
  "trial.why.overwrote": "{n} 次竞争失败后不带条件地写了这个键：另一个写入者的提交会丢失",
  "trial.why.waited": "等完了 {n} 次首字节前长达 {time} 的停顿：没有超时把它截断",
  "trial.why.reuploaded": "{n} 个上传从第一个分片起重做了一遍，而存储其实已经完成了：每个分片都发送了两次",
  "trial.why.rewrote": "{n} 个被列表漏掉的键被再次写入：程序以为它们不存在",
  "trial.why.missed": "本次运行写入、却被列表漏掉的 {n} 个键没有像正常运行那样被读取：程序在缺少它们的情况下继续了",
  "trial.why.staleUnused": "列表漏掉了本次运行写入的 {n} 个键，而程序之后的请求都与它们无关：从请求看不出它是否察觉",
  "trial.why.cleanExit": "正常运行自己就以 {exit} 退出",
  "trial.why.exit": "以 {exit} 退出（正常运行以 {clean} 退出）",
  "trial.why.refused.completed": "重试的 {op} 得到 {answer}：上传其实已经完成",
  "trial.why.refused.made": "重试的 {op} 得到 {answer}：存储已经完成了第一次尝试",
  "trial.why.policy": "被录制策略（{policy}）拒绝：比正常运行多 {n} 个请求，因此看不出程序在故障下会怎样",
  "trial.advice.policy": "让每次运行有自己的位置（把试验名 SONDE_TRIAL 放进键里），或者允许程序覆盖已有的对象（--allow-writes）",
  "trial.advice.policy.placed": "这次运行已经有自己的位置：允许程序删除或覆盖它在那里找到的对象（--allow-writes）",
  "obs.prefixrate.reads": "{prefix} 每秒最多读取 {n} 次，达到 S3 单个前缀上限 {limit} 的 {pct}%：在被限流前把键分散到更多前缀",
  "obs.prefixrate.writes": "{prefix} 每秒最多写入 {n} 次，达到 S3 单个前缀上限 {limit} 的 {pct}%：在被限流前把键分散到更多前缀",
  "obs.serial": "{n} 个请求一个接一个地运行了 {time}，每个都在等上一个：同时发 {k} 个大约只需 {fast}",
  "obs.savesTime": "大约快 {time}（占等待时间的 {pct}%）",
  "split.slow": "{pct}% 的慢请求（超过 {ms}）",
  "split.slow.most": "{pct}% 的慢请求（超过 {ms}）和 {all}% 的全部请求",
  "split.spread": "慢请求很分散：没有哪一组占到一半",
  "split.cost.first": "{pct}% 的费用",
  "split.cost": "；{pct}% 的费用",
  "split.and": "、",
  "split.end": "。",
  "split.operation": "是 {v}",
  "split.prefix": "在 {v} 下",
  "split.caller": "来自 {v}",
  "split.sdk": "来自 {v}",
  "split.status": "的应答是 {v}",
  "split.status.none": "没有应答",
  "split.size.none": "没有正文",
  "split.size.under": "小于 {hi}",
  "split.size.between": "在 {lo} 到 {hi} 之间",
  "split.size.over": "大于 {lo}",
};

/** Rust's own English, template by template (crates/proxy/src/phrase.rs `template`): a view names an
 * operation by its phrases alone, so English is said from them too. check-locales fails a template that
 * differs from Rust's, and a key one side has and the other has not. */
const en: Record<string, string> = {
  request: "{op} {target}",
  "request.prefix": "{prefix}",
  size: " · {size}",
  "cond.create": "conditional create (If-None-Match: *)",
  "cond.write": "conditional write (If-Match)",
  race: "lost a race: 412 Precondition Failed",
  probe: "{n} lookups in {dir}, none found",
  listing: "listed {bucket}/{prefix} in {n} pages",
  mpu: "multipart upload {key} · {n} part{s} · {size}",
  "mpu.aborted": " (aborted)",
  "mpu.open": " (not completed)",
  "mpu.billed": "the upload was never completed or aborted: its parts are billed",
  retry: "{op} {target} retried {n}× after {first}",
  "retry.wait": "{ms} ms until it succeeded or gave up",
  "step.deleted": "deleted {file}",
  "slate.manifest": "manifest {n} written",
  "slate.wal.empty": "WAL {n} written empty (fencing marker)",
  "slate.wal": "WAL {n} flushed · {size}",
  "slate.sst": "SST flushed to L0 · {size}",
  "slate.compactions": "compaction state {n} written",
  "iceberg.metadata.v": "commit: table metadata v{v}",
  "iceberg.metadata": "commit: table metadata",
  "delta.commit": "commit {n}",
  "lance.commit": "version {v} committed",
  "lance.data": " · {n} data file{s} ({size})",
  "lance.index": " · an index ({size})",
  "lance.deletions": " · rows deleted",
  "lance.read.v": "read version {v}",
  "lance.read": "read the dataset",
  "lance.ranges": " · {n} ranges of {files} file{s} ({size})",
  "hudi.instant": "{action} {instant}",
  "hudi.metadata": "metadata table: {action} {instant}",
  "hudi.properties": "table properties written (hoodie.properties)",
  "paimon.snapshot": "snapshot {n}",
  "paimon.schema": "schema {n} written",
  "paimon.hint": "{file} hint updated",
  "delta.checkpoint": "checkpoint written",
  "delta.last": "_last_checkpoint updated",
  "step.other": "{op} {key}",
  "step.files": " · {n} file{s} ({size})",
  "after.listing": "after listing {dir}/",
  "after.reads": "after {n} read{s}",
  "after.misses": "after {n} lookup{s} that found nothing",
  "after.mixed": "after {n} reads ({m} found nothing)",
  "loose.uncommitted": "{n} file{s} written, no commit followed ({size})",
  "slate.walprobe": "looked for the newest WAL: probed ids {lo}…{hi}",
  "read.manifest": "read manifest {n}",
  "read.compactions": "read compaction state {n}",
  "read.metadata": "read table metadata v{v}",
  "read.commit": "read commit {n}",
  "read.file": "read {file}",
  "read.count": " ({n} requests)",
  "loose.listed": "listed {dir}/",
  "loose.misses": "{n} lookup{s} that found nothing",
  "loose.reads": "{n} reads",
  "loose.deletes": "{n} file{s} deleted",
  "obs.unfinished": "{n} multipart upload{s} never completed or aborted: the parts stay billed until a lifecycle rule removes them",
  "obs.orphans.one": "files were written with no commit after them: orphans unless a later commit adds them",
  "obs.orphans": "{n} times files were written with no commit after them: orphans unless a later commit adds them",
  "obs.small": "{n} files under 5 MiB were uploaded as multipart uploads: 3 requests each where one PUT would do",
  "obs.rename": "{n} object{s} copied out of _temporary/ and deleted: a commit by rename (Hadoop's FileOutputCommitter) writes each output file again on S3; an S3A committer writes it once",
  "obs.markers": "{n} directory marker{s} written (empty objects named …/), {m} delete requests of markers: s3a keeps them with fs.s3a.directory.marker.retention=keep",
  "obs.hot": "{key} was read {n} times ({pct}% of all requests)",
  "obs.listed": "{prefix} was listed {n} times",
  "obs.headget": "{n} objects were checked with HEAD and then read with GET: the GET alone answers both",
  "obs.throttled": "{n} request{s} throttled (503 SlowDown or 429), each with {min} or more in flight: under {safe} at once would likely avoid it",
  "obs.throttled.rate": "{n} request{s} throttled (503 SlowDown or 429): the store asked for fewer requests per second",
  "obs.prefixrate.reads": "{prefix} took up to {n} reads a second, {pct}% of the {limit} S3 serves one prefix: spread its keys over more prefixes before it throttles",
  "obs.prefixrate.writes": "{prefix} took up to {n} writes a second, {pct}% of the {limit} S3 serves one prefix: spread its keys over more prefixes before it throttles",
  "obs.serial": "{n} requests ran one after another for {time}, each waiting for the one before: {k} at once would take about {fast}",
  "obs.savesTime": "about {time} sooner ({pct}% of the time waiting)",
  "obs.saves": "{n} fewer request{s} ({pct}% of the session)",
  "split.slow": "{pct}% of the slow requests (over {ms}) are ",
  "split.slow.most": "{pct}% of the slow requests (over {ms}), and {all}% of all requests, are ",
  "split.spread": "the slow requests are spread: no one group holds half of them",
  "split.cost.first": "{pct}% of the cost is ",
  "split.cost": "; {pct}% of the cost is ",
  "split.and": ", ",
  "split.end": ".",
  "split.operation": "{v}",
  "split.prefix": "under {v}",
  "split.caller": "from {v}",
  "split.sdk": "from {v}",
  "split.status": "answered {v}",
  "split.status.none": "without an answer",
  "split.size.none": "without a body",
  "split.size.under": "under {hi}",
  "split.size.between": "of {lo} to {hi}",
  "split.size.over": "over {lo}",
  "fix.fixed": "fixed",
  "fix.less": "less",
  "fix.back": "back",
  "fix.new": "new",
  "fix.open": "open",
  "fix.saved": "saved {n} request{s} (predicted {p})",
  "fix.savedTime": "{time} less waiting (predicted {p})",
  "trial.advice.retries": "{sdk} did not send it again: turn its retries on ({setting})",
  "trial.advice.retry": "send a failed request again: a 500, 502, 503, 504, 408 or 429 is owed a retry, and so is S3's SlowDown, RequestTimeout or RequestTimeTooSkewed",
  "trial.advice.skew": "a 403 RequestTimeTooSkewed is owed a retry signed again by the clock the answer's Date gives (the AWS SDKs for Java and JavaScript do this; boto3 does not); and keep the machine's clock in time",
  "trial.advice.timeout": "give each attempt a timeout well under a stall (a first byte comes in well under a second), so that a stalled request is sent again",
  "trial.advice.list": "a listing just after a write may not show it yet: keep the keys the program wrote, or list again before going on without one",
  "trial.advice.cut": "check the bytes read against Content-Length, and read the rest (Range) or the whole again",
  "trial.advice.race": "on a 412, read the log again and write the next commit with its condition; never write the key without one",
  "trial.why.notMet": "the scenario met none of the program's requests",
  "trial.why.uploads": "left {n} multipart upload{s} unfinished (their parts are billed until aborted)",
  "trial.why.reads": "gave up on {n} failed read{s} without sending {it} again",
  "trial.why.orphans": "{n} more object{s} left than the clean run leaves: what the lost commit wrote, unless the commit that followed names {it}",
  "trial.why.blind": "{n} lost write{s} sent again with the same If-Match: a real writer's ETag would refuse it again",
  "trial.why.lost": "{n} write{s} answered 500 and not retried; the store has {it}",
  "trial.why.writes": "{n} write{s} failed and {was} never sent again",
  "trial.why.exit0": ", and the program exited 0",
  "trial.why.taken": "{n} download{s} cut short and the rest never read",
  "trial.why.taken.whole": ": the half was taken as the whole (exit {exit}, as the clean run)",
  "trial.why.holes": "{n} download{s} cut short and read on from past the cut: bytes are missing",
  "trial.why.unrun": "{n} conditional write{s} lost {its} race: no commit followed {it} and nothing read back what won",
  "trial.why.unrun.exit": ", and the program exited {exit}",
  "trial.why.overwrote": "{n} lost race{s} answered by writing the key without a condition: the other writer's commit would be gone",
  "trial.why.waited": "waited out {n} stall{s} of {time} before the first byte: no timeout cut {it} short",
  "trial.why.reuploaded": "{n} upload{s} made again from {its} first part though the store had completed {it}: every part sent twice",
  "trial.why.rewrote": "{n} key{s} a listing left out written again: the program took {it} for missing",
  "trial.why.missed": "{n} key{s} this run wrote and a listing left out {was} never read as the clean run read {it}: the program went on without {it}",
  "trial.why.staleUnused": "a listing left out {n} key{s} this run wrote, and nothing the program did after turned on {it}: the requests cannot tell whether it noticed",
  "trial.why.cleanExit": "the clean run exited {exit} itself",
  "trial.why.exit": "exited {exit} (the clean run exited {clean})",
  "trial.why.refused.completed": "the retried {op} was answered {answer}: the upload had completed",
  "trial.why.refused.made": "the retried {op} was answered {answer}: the store had made the first attempt",
  "trial.why.policy": "refused by the recording policy ({policy}): {n} request{s} more than in the clean run, so what the program does under the fault cannot be told",
  "trial.advice.policy": "give each run a place of its own (the trial's name, SONDE_TRIAL, in its keys), or let the program write over what it finds (--allow-writes)",
  "trial.advice.policy.placed": "the run has a place of its own already: let the program delete or write over what it finds there (--allow-writes)",
};

const TEMPLATES: Record<string, Record<string, string>> = { en, zh };

/** What sonde's services say (a finding, a verdict, a plan, a job, a probe's
 * sentence), in the reader's language: its phrases beside its English
 * (`corephrases.ts`). English shows the text; Chinese says the phrases, or
 * the English whole where one has no words yet. The one way a page shows
 * a sentence Rust wrote. */
export const core = (text: CorePhrase[] | undefined | null, en: string | null | undefined): string => sayCoreText(text ?? undefined, en ?? "", lang() === "zh");

/** An endpoint's name as the reader sees it: the built-in demo's is the app's own word, in its language. */
export const endpointName = (e: { id: string; name: string }): string => (e.id === "demo" ? t("endpoints.demoName") : e.name);

/** `text` in the reader's language, from its phrases; `fallback` (Rust's own English, where it was sent
 * beside them) when a phrase has no template. English keeps a phrase's plural mark (`{s}`); Chinese has none. */
export function say(text: Phrase[] | undefined, fallback: string | undefined): string {
  const language = lang();
  const table = TEMPLATES[language];
  if (!table || !text?.length) return fallback ?? "";
  let out = "";
  for (const p of text) {
    const tpl = table[p.key];
    if (tpl === undefined) return fallback ?? "";
    // one pass: an argument's own braces (a key named "{n}.json") are not read as a placeholder
    out += tpl.replace(/\{(\w+)\}/g, (whole, k: string) => (k === "s" && language !== "en" ? "" : (p.args[k] ?? (k === "s" ? "" : whole))));
  }
  return out;
}

export const episodeTitle = (e: Pick<EpisodeRow, "titleText" | "title">) => say(e.titleText, e.title);

/** An episode's notes, in the reader's language. Those most writes of a format share (`shared`, a
 * conditional create on every SlateDB write) were said once as a finding until 18.0; they are an
 * operation's own notes again, so `shared` no longer leaves them out. */
export function episodeNotes(e: Pick<EpisodeRow, "notes" | "notesText">, _shared: string[]): string[] {
  return e.notes.map((n, i) => say(e.notesText?.[i] ? [e.notesText[i]] : undefined, n));
}

export const observationText = (o: Observation) => say(o.phrases, o.text);

/** What acting on a finding would save, in the reader's language. */
export const observationSaving = (o: Observation): string | null => (o.saving ? say([o.saving], o.savingText ?? "") : null);

/** Why an observation matters, for the card it shows on hover: the key of its first phrase. */
export const observationWhy = (o: Observation): string | null => {
  const key = o.phrases?.[0]?.key;
  if (!key) return null;
  const k = `why.${key === "obs.orphans.one" ? "obs.orphans" : key.startsWith("obs.prefixrate") ? "obs.prefixrate" : key}`;
  const s = t(k as Key);
  return s === k ? null : s;
};
