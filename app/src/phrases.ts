// What sonde says about a session, in the reader's language. Rust gives each
// title, note and observation as phrases (a key and its arguments) beside
// its English text (crates/proxy/src/phrase.rs); English shows that text,
// other languages say the phrases from their templates here. A phrase with
// no template in a language falls back to the English text, whole.

import { type CorePhrase, sayCoreText } from "./corephrases";
import { type Key, lang, t } from "./i18n";
import type { Episode, Observation, Phrase } from "./types";

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
  "obs.shared": "{n} 次 {format} 写入都是：",
  "obs.injected": "{n} 个请求遇到了 sonde 注入的故障（场景 {scenario}）：这里的错误和重试来自演练，不是存储本身",
  "obs.unfinished": "{n} 个分片上传既没有完成也没有中止：分片会一直计费，直到生命周期规则清理它们",
  "obs.orphans.one": "写入了文件但之后没有提交：除非后续提交引用它们，否则就是孤儿文件",
  "obs.orphans": "有 {n} 次写入了文件但之后没有提交：除非后续提交引用它们，否则就是孤儿文件",
  "obs.retried": "{n} 个请求在可重试的失败后被重发",
  "obs.small": "{n} 个小于 5 MiB 的文件用分片上传：每个 3 个请求，一个 PUT 就够",
  "obs.races": "{n} 次条件写入竞争失败（412）：另一个写入者先到了",
  "obs.hot": "{key} 被读取了 {n} 次（占全部请求的 {pct}%）",
  "obs.listed": "{prefix} 被列出了 {n} 次",
  "obs.headget": "{n} 个对象先用 HEAD 查看、再用 GET 读取：单独一个 GET 就能同时回答两者",
  "obs.throttled": "{n} 个请求被限流（503 SlowDown 或 429），当时都有 {min} 个或更多请求在进行：同时少于 {safe} 个大概率可以避免",
  "obs.throttled.rate": "{n} 个请求被限流（503 SlowDown 或 429）：存储要求降低每秒请求数",
  "obs.slow": "{n} 个请求耗时超过 {ms} ms，而中位数是 {median} ms；最慢的是 {op}，耗时 {max} ms",
  "obs.saves": "可少发 {n} 个请求（占会话的 {pct}%）",
  "obs.prefixrate.reads": "{prefix} 每秒最多读取 {n} 次，达到 S3 单个前缀上限 {limit} 的 {pct}%：在被限流前把键分散到更多前缀",
  "obs.prefixrate.writes": "{prefix} 每秒最多写入 {n} 次，达到 S3 单个前缀上限 {limit} 的 {pct}%：在被限流前把键分散到更多前缀",
  "obs.serial": "{n} 个请求一个接一个地运行了 {time}，每个都在等上一个：同时发 {k} 个大约只需 {fast}",
  "obs.savesTime": "大约快 {time}（占等待时间的 {pct}%）",
};

const TEMPLATES: Record<string, Record<string, string>> = { zh };

/** What sonde's services say (a finding, a verdict, a plan, a job, a probe's
 * sentence), in the reader's language: its phrases beside its English
 * (`corephrases.ts`). English shows the text; Chinese says the phrases, or
 * the English whole where one has no words yet. The one way a page shows
 * a sentence Rust wrote. */
export const core = (text: CorePhrase[] | undefined | null, en: string | null | undefined): string => sayCoreText(text ?? undefined, en ?? "", lang() === "zh");

/** An endpoint's name as the reader sees it: the built-in demo's is the app's own word, in its language. */
export const endpointName = (e: { id: string; name: string }): string => (e.id === "demo" ? t("endpoints.demoName") : e.name);

/** `text` in the reader's language; `en` (Rust's English) when there is no template. */
export function say(text: Phrase[] | undefined, en: string): string {
  const table = TEMPLATES[lang()];
  if (!table || !text?.length) return en;
  let out = "";
  for (const p of text) {
    const tpl = table[p.key];
    if (tpl === undefined) return en;
    out += tpl.replace(/\{(\w+)\}/g, (_, k: string) => (k === "s" ? "" : (p.args[k] ?? "")));
  }
  return out;
}

export const episodeTitle = (e: Episode) => say(e.titleText, e.title);

/** An episode's notes, those said once for the whole session left out. */
export function episodeNotes(e: Episode, shared: string[]): string[] {
  return e.notes.flatMap((n, i) => (shared.includes(n) ? [] : [say(e.notesText?.[i] ? [e.notesText[i]] : undefined, n)]));
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
