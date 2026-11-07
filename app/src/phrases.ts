// What sonde says about a session, in the reader's language. Rust gives each
// title, note and observation as phrases (a key and its arguments) beside
// its English text (crates/proxy/src/phrase.rs); English shows that text,
// other languages say the phrases from their templates here. A phrase with
// no template in a language falls back to the English text, whole.

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
  "obs.unfinished": "{n} 个分片上传既没有完成也没有中止：分片会一直计费，直到生命周期规则清理它们",
  "obs.orphans.one": "写入了文件但之后没有提交：除非后续提交引用它们，否则就是孤儿文件",
  "obs.orphans": "有 {n} 次写入了文件但之后没有提交：除非后续提交引用它们，否则就是孤儿文件",
  "obs.retried": "{n} 个请求在可重试的失败后被重发",
  "obs.small": "{n} 个小于 5 MiB 的文件用分片上传：每个 3 个请求，一个 PUT 就够",
  "obs.races": "{n} 次条件写入竞争失败（412）：另一个写入者先到了",
  "obs.hot": "{key} 被读取了 {n} 次（占全部请求的 {pct}%）",
  "obs.listed": "{prefix} 被列出了 {n} 次",
};

const TEMPLATES: Record<string, Record<string, string>> = { zh };

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

/** Why an observation matters, for the card it shows on hover: the key of its first phrase. */
export const observationWhy = (o: Observation): string | null => {
  const key = o.phrases?.[0]?.key;
  if (!key) return null;
  const k = `why.${key === "obs.orphans.one" ? "obs.orphans" : key}`;
  const s = t(k as Key);
  return s === k ? null : s;
};
