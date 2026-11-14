// What each chart of the Time pane answers, in a sentence computed from what
// it draws (over the stretch shown): its title asks, this says. How to read a
// chart is not said here: it is the frame's `how`, behind the info mark.

import { bytes, cost, count, duration, percent, rate } from "../../format";
import { t } from "../../i18n";
import { sizeLabel } from "../../signals";
import type { Profile, Signals } from "../../types";
import { groupName } from "./system";

/** A stretch of slices (a zoom), or every one: the indexes it covers. */
const within = (n: number, range?: [number, number] | null): [number, number] => (range ? [Math.max(0, range[0]), Math.min(n - 1, range[1])] : [0, n - 1]);
const requests = (b: Profile["rate"][number]) => b.ok + b.miss + b.client + b.server;

/** Whether any request began in slices `a` to `b`: a stretch of none is no stretch to reckon. */
export const began = (p: Profile, a: number, b: number) => p.rate.slice(a, b + 1).some((x) => requests(x) > 0);

/** When: how many requests, and the busiest slice. */
export function timelineSaid(p: Profile, range?: [number, number] | null): string {
  const [a, b] = within(p.rate.length, range);
  let n = 0;
  let peak = -1;
  for (let i = a; i <= b; i++) {
    const k = requests(p.rate[i]);
    n += k;
    if (peak < 0 || k > requests(p.rate[peak])) peak = i;
  }
  if (n === 0 || peak < 0) return t("timeline.none");
  return t("timeline.text", { n: count(n), peak: count(requests(p.rate[peak])), at: duration(p.rate[peak].tMs) });
}

/** What ran at once: the most in flight, and when (of the stretch shown). */
export function lanesSaid(p: Profile, domainMs?: [number, number] | null): string {
  let n = p.peakInFlight;
  let at = p.peakAtMs;
  if (domainMs) {
    n = 0;
    // the level in force at the stretch's start, then each change within it
    for (const x of p.inFlight) {
      if (x.tMs >= domainMs[1]) break;
      if (x.tMs < domainMs[0]) {
        n = x.n;
        at = domainMs[0];
        continue;
      }
      if (x.n > n) (n = x.n), (at = x.tMs);
    }
  }
  return n <= 1 ? t("lanes.text.one") : t("lanes.text", { n: count(n), at: duration(at) });
}

/** How much moved: the bytes each way over the stretch shown, and the busiest moment. */
export function bytesSaid(s: Signals, range?: [number, number] | null): string {
  const up = s.bytes.up;
  const down = s.bytes.down;
  const [a, b] = within(Math.max(up.length, down.length), range);
  let sent = 0;
  let got = 0;
  let peak = a;
  for (let i = a; i <= b; i++) {
    sent += up[i] ?? 0;
    got += down[i] ?? 0;
    if ((up[i] ?? 0) + (down[i] ?? 0) > (up[peak] ?? 0) + (down[peak] ?? 0)) peak = i;
  }
  // the series are rates (a second's worth): a slice moved its rate times its length
  const per = s.binMs / 1000;
  const total = range ? [sent * per, got * per] : [s.bytes.upBytes, s.bytes.downBytes];
  return t("bytesTime.text", { up: bytes(total[0]), down: bytes(total[1]), at: duration(peak * s.binMs) });
}

/** A group of requests a table or chart reads (a kind of request, a prefix…). */
export interface Said {
  name: string;
  requests: number;
  errorShare: number;
  p95Ms: number | null;
  few: boolean;
}

/** Which were slow or failed: the most of the requests, then the group that
 * failed most (where any did), else the slowest at p95 (of enough to tell). */
export function groupsSaid(rows: Said[]): string {
  const all = rows.reduce((n, r) => n + r.requests, 0);
  const most = [...rows].sort((a, b) => b.requests - a.requests)[0];
  if (!most || all === 0) return "";
  const share = percent(most.requests / all);
  const failed = [...rows].filter((r) => r.errorShare > 0).sort((a, b) => b.errorShare - a.errorShare)[0];
  if (failed) return t("byOp.textFailed", { kind: most.name, share, worst: failed.name, failed: percent(failed.errorShare) });
  const told = rows.filter((r) => !r.few && r.p95Ms != null);
  const slow = told.length > 1 ? [...told].sort((a, b) => b.p95Ms! - a.p95Ms!)[0] : null;
  if (slow) return t("byOp.textSlow", { kind: most.name, share, slow: slow.name, p95: duration(slow.p95Ms!) });
  return rows.length === 1 ? t("byOp.textAll", { n: count(all), kind: most.name }) : t("byOp.text", { kind: most.name, share });
}

/** A median of slices, each weighted by its requests: the median of the
 * requests as near as slices can tell it. */
function medianOf(rows: Profile["rate"]): number | null {
  const has = rows.filter((b) => requests(b) > 0).sort((a, b) => a.p50Ms - b.p50Ms);
  const all = has.reduce((n, b) => n + requests(b), 0);
  let run = 0;
  for (const b of has) {
    run += requests(b);
    if (run * 2 >= all) return b.p50Ms;
  }
  return null;
}

/** Whether requests slowed: the median of the first half against the second,
 * and the slowest slice's p95 (of the stretch shown). */
export function latencySaid(p: Profile, range?: [number, number] | null): string {
  const [a, b] = within(p.rate.length, range);
  const rows = p.rate.slice(a, b + 1);
  const has = rows.filter((x) => requests(x) > 0);
  if (has.length === 0) return "";
  const worst = has.reduce((m, x) => (x.p95Ms > m.p95Ms ? x : m), has[0]);
  const tail = { p95: duration(worst.p95Ms), at: duration(worst.tMs) };
  const mid = Math.floor(rows.length / 2);
  const first = medianOf(rows.slice(0, mid));
  const second = medianOf(rows.slice(mid));
  if (has.length < 4 || first == null || second == null) return t("latencyTime.textFew", { p50: duration(medianOf(rows) ?? 0), ...tail });
  // within a fifth of each other: held
  const held = Math.abs(second - first) <= 0.2 * Math.max(first, second);
  return held ? t("latencyTime.textHeld", { p50: duration(medianOf(rows) ?? 0), ...tail }) : t(second > first ? "latencyTime.textSlower" : "latencyTime.textFaster", { a: duration(first), b: duration(second), ...tail });
}

/** Which requests were slow: how many are drawn, and the slowest of them. */
export function scatterSaid(p: Profile, domainMs?: [number, number] | null): string {
  const rows = domainMs ? p.samples.filter((x) => x.tMs >= domainMs[0] && x.tMs < domainMs[1]) : p.samples;
  if (rows.length === 0) return "";
  const slow = rows.reduce((m, x) => (x.ms > m.ms ? x : m), rows[0]);
  const g = p.groups[slow.group];
  return t("scatter.text", { n: count(rows.length), kind: g ? groupName(g) : "—", ms: duration(slow.ms), at: duration(slow.tMs) });
}

/** The busiest kinds over time: the one whose p95 rose highest, and when. */
export function kindsSaid(p: Profile, domainMs?: [number, number] | null): string {
  let best: { group: number; p95Ms: number; tMs: number } | null = null;
  for (const g of p.groupRate ?? []) {
    const tMs = g.bin * p.binMs;
    if (domainMs && (tMs < domainMs[0] || tMs >= domainMs[1])) continue;
    if (!best || g.p95Ms > best.p95Ms) best = { group: g.group, p95Ms: g.p95Ms, tMs };
  }
  const g = best ? p.groups[best.group] : null;
  return best && g ? t("kinds.text", { kind: groupName(g), p95: duration(best.p95Ms), at: duration(best.tMs) }) : "";
}

/** Where the waiting went: the kind (or the line of the program) that waited most. */
export function spentSaid(p: Profile, by: "kind" | "code"): string {
  const top =
    by === "code"
      ? [...(p.callers ?? [])].sort((a, b) => b.waitedMs - a.waitedMs).map((c) => ({ name: c.place, share: c.waitedMs / Math.max(p.waitingMs, 1e-9), waitedMs: c.waitedMs }))[0]
      : p.groups.map((g) => ({ name: groupName(g), share: g.share, waitedMs: g.waitedMs }))[0];
  return top ? t("profile.spentText", { what: top.name, pct: percent(top.share), time: duration(top.waitedMs) }) : "";
}

/** How long most took: the median, p95 and p99 as shares done by then. */
export const distSaid = (p: Profile): string => (p.p50Ms == null ? "" : t("dist.text", { p50: duration(p.p50Ms), p95: duration(p.p95Ms ?? 0), p99: duration(p.p99Ms ?? 0) }));

/** What a status is called when S3 gave no code of its own for it: its HTTP name. */
export const REASON: Record<number, string> = { 200: "OK", 201: "Created", 204: "No Content", 206: "Partial Content", 301: "Moved Permanently", 304: "Not Modified", 307: "Temporary Redirect", 400: "Bad Request", 403: "Forbidden", 404: "Not Found", 405: "Method Not Allowed", 409: "Conflict", 411: "Length Required", 412: "Precondition Failed", 416: "Range Not Satisfiable", 429: "Too Many Requests", 500: "Internal Server Error", 501: "Not Implemented", 502: "Bad Gateway", 503: "Service Unavailable", 504: "Gateway Timeout" };

type Status = Signals["statuses"][number];
/** An answer as the store (or HTTP) names it: "503 SlowDown", "200 OK", "No answer". */
export const calledOf = (x: Status) => (x.status === null ? t("statuses.noAnswer") : `${x.status} ${x.code ?? REASON[x.status] ?? t("statuses.unnamed")}`);

/** When the requests failed: how many over the stretch shown, what they all
 * were where they were one thing (or sonde's own answers), and between when. */
export function failuresSaid(s: Signals, range?: [number, number] | null, of: Signals = s): string {
  const e = s.errors;
  const [a, b] = within(e.series.length, range);
  let n = 0;
  let retries = 0;
  let first = -1;
  let last = -1;
  for (let i = a; i <= b; i++) {
    const k = (e.server[i] ?? 0) + (e.client[i] ?? 0) + (e.transport[i] ?? 0) + (e.injected[i] ?? 0);
    retries += e.retries[i] ?? 0;
    if (k > 0) {
      n += k;
      if (first < 0) first = i;
      last = i;
    }
  }
  const again = retries > 0 ? t("failures.retried", { n: count(retries) }) : "";
  if (n === 0) return [t("signal.noErrors"), again].filter(Boolean).join("; ");
  const when = first === last ? t("failures.at", { t: duration(first * s.binMs) }) : t("failures.between", { a: duration(first * s.binMs), b: duration((last + 1) * s.binMs) });
  // what they were: one answer, or every one sonde's own (the statuses are of the whole, or of the stretch reckoned)
  const failing = of.statuses.filter((x) => x.status === null || (x.status >= 400 && x.status !== 404));
  const all = of.errors;
  const whole = failing.reduce((k, x) => k + x.count, 0) === all.count;
  const what = all.counts.injected === all.count ? t("failures.bySonde") : whole && failing.length === 1 ? calledOf(failing[0]) : null;
  const said = what ? t(n === 2 ? "failures.textBoth" : "failures.textAll", { n: count(n), what, when }) : t("failures.text", { n: count(n), when });
  return [said, again].filter(Boolean).join("; ");
}

/** What share failed: of the stretch shown, and its worst slice. */
export function errorRateSaid(s: Signals, range?: [number, number] | null): string {
  const e = s.errors;
  const [a, b] = within(e.series.length, range);
  let all = 0;
  let failed = 0;
  let worst = { share: 0, i: a };
  for (let i = a; i <= b; i++) {
    const f = (e.server[i] ?? 0) + (e.client[i] ?? 0) + (e.transport[i] ?? 0) + (e.injected[i] ?? 0);
    const n = Math.max(Math.round(((s.rate.series[i] ?? 0) * s.binMs) / 1000), f);
    all += n;
    failed += f;
    if (n > 0 && f / n > worst.share) worst = { share: f / n, i };
  }
  if (failed === 0) return t("signal.noErrors");
  return t("errorRate.text", { pct: percent(failed / Math.max(all, 1)), peak: percent(worst.share), at: duration(worst.i * s.binMs) });
}

/** What the store answered: the most frequent answer, and the next. */
export function statusesSaid(s: Signals): string {
  const all = [...s.statuses].sort((a, b) => b.count - a.count);
  const n = all.reduce((k, x) => k + x.count, 0);
  if (!all.length || n === 0) return "";
  if (all.length === 1) return t("statuses.textAll", { n: count(n), called: calledOf(all[0]) });
  const said = t("statuses.text", { called: calledOf(all[0]), share: percent(all[0].count / n) });
  return `${said}; ${t("statuses.then", { called: calledOf(all[1]), n: count(all[1].count) })}`;
}

/** What the requests cost: the dearest kind and its share of the whole. */
export function costSaid(s: Signals): string {
  const top = s.cost.operations.filter((x) => x.cost > 0).sort((a, b) => b.cost - a.cost)[0];
  return top ? t("costBy.text", { op: top.operation, pct: percent(top.share), total: cost(s.cost.amount, s.cost.currency) }) : "";
}

/** When each prefix was busy: the busiest prefix, in the session's busiest second or the stretch's busiest slice. */
export function heatSaid(p: Profile, range?: [number, number] | null): string {
  if (!range) {
    const top = [...p.prefixes].sort((a, b) => b.peakPerSecond - a.peakPerSecond)[0];
    return top ? t("heat.text", { prefix: top.prefix, n: count(top.peakPerSecond) }) : "";
  }
  const cell = p.heat.filter((c) => c.bin >= range[0] && c.bin <= range[1]).reduce<Profile["heat"][number] | null>((m, c) => (!m || c.n > m.n ? c : m), null);
  const prefix = cell ? p.prefixes[cell.prefix]?.prefix : null;
  return cell && prefix ? t("heat.textAt", { prefix, n: count(cell.n), at: duration(cell.bin * p.binMs) }) : "";
}

/** Where requests went: how many were answered, and the prefix most went to. */
export function flowsSaid(p: Profile): string {
  const ok = p.flows.filter((f) => f.target === "ok").reduce((s, f) => s + f.value, 0);
  const all = p.flows.filter((f) => ["ok", "not found", "client error", "server error"].includes(f.target)).reduce((s, f) => s + f.value, 0);
  const top = p.prefixes[0];
  return all ? t("flows.text", { n: all, ok: percent(ok / all), prefix: top?.prefix ?? "", share: percent((top?.requests ?? 0) / all) }) : "";
}

/** How large the bodies were: the size most of them were. */
export function sizesSaid(s: Signals): string {
  const les = [...new Set([...s.sizes.request, ...s.sizes.response].map((b) => b.leBytes))];
  const n = (le: number) => (s.sizes.request.find((b) => b.leBytes === le)?.count ?? 0) + (s.sizes.response.find((b) => b.leBytes === le)?.count ?? 0);
  const most = [...les].filter((le) => le > 0).sort((a, b) => n(b) - n(a))[0] ?? les[0];
  return most === undefined ? "" : t("sizes.text", { size: sizeLabel(most), n: count(n(most)) });
}

/** How fast bodies moved: the median speed. */
export const throughputSaid = (s: Signals): string => (s.sizes.mibPerSec == null ? "" : t("throughput.text", { speed: t("throughput.speed", { n: rate(s.sizes.mibPerSec) }) }));

/** Which requests waited on each other: the longest chain, and what eight at once would take. */
export function chainsSaid(p: Profile): string {
  const c = p.chains?.[0];
  return c ? t("chains.text", { n: count(c.requests), time: duration(c.endMs - c.startMs), fast: duration(c.atOnceMs) }) : "";
}

/** Which prefixes were busiest: the first by the measure chosen. */
export function prefixSaid(p: Profile, measure: "requests" | "cost" | "peak"): string {
  const value = (x: Profile["prefixes"][number]) => (measure === "cost" ? (x.cost ?? 0) : measure === "peak" ? x.peakPerSecond : x.requests);
  const top = p.prefixes.filter((x) => x.prefix !== "other").sort((a, b) => value(b) - value(a))[0];
  if (!top) return "";
  const v = value(top);
  const shown = measure === "cost" ? cost(v, p.cost.currency) : measure === "peak" ? t("prefixUse.perSecond", { n: count(Math.round(v)) }) : t("prefixUse.requests", { n: count(v) });
  return t("prefixUse.text", { prefix: top.prefix, value: shown });
}

