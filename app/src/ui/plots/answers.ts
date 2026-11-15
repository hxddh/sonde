// What each chart of the Time pane answers, in a sentence computed from what
// it draws (over the stretch shown): its title asks, this says. How to read a
// chart is not said here: it is the frame's `how`, behind the info mark.

import { bytes, change, cost, count, duration, percent, rate } from "../../format";
import { t } from "../../i18n";
import { FEW, sizeLabel } from "../../signals";
import type { Profile, RunChange, Signals } from "../../types";
import { groupName, sliceAt, slotMs } from "./system";

/** A stretch of slices (a zoom), or every one: the indexes it covers. */
const within = (n: number, range?: [number, number] | null): [number, number] => (range ? [Math.max(0, range[0]), Math.min(n - 1, range[1])] : [0, n - 1]);
const requests = (b: Profile["rate"][number]) => b.ok + b.miss + b.client + b.server;

/** The mark a sentence that states a percentile carries when it is of fewer than `FEW` requests ("· of only 9
 * requests"), as the tile beside it does: a p95 of eight is one of eight, not a tail. Nothing for enough. */
const few = (n: number): string => (n > 0 && n < FEW ? ` · ${t("plots.fewOf", { n: count(n) })}` : "");

/** The busiest slice's requests as a rate, with its unit: "15 in 5 ms" in slices under a second, "120/s" from
 * slices of a second up (a count of a slice whose length is not said is no rate). */
export const slicePeak = (n: number, binMs: number) => (binMs >= 1000 ? t("shape.peakRate", { n: rate((n * 1000) / binMs) }) : t("shape.peakIn", { n: count(n), t: duration(binMs) }));

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
  // no slice holds two: when they began, first to last
  if (requests(p.rate[peak]) === 1 && n > 1) {
    const at = p.rate.slice(a, b + 1).filter((x) => requests(x) > 0);
    return t("timeline.textApart", { n: count(n), a: duration(at[0].tMs), b: duration(at[at.length - 1].tMs) });
  }
  return t("timeline.text", { n: count(n), peak: slicePeak(requests(p.rate[peak]), p.binMs), at: duration(p.rate[peak].tMs) });
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
  return t("bytesTime.text", { up: bytes(total[0]), down: bytes(total[1]), at: duration(slotMs(s, peak)) });
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
  // the slowest p95 of a slice of enough requests to have one (else of any: the sentence is then marked few)
  const told = has.filter((x) => requests(x) >= FEW);
  const worst = (told.length ? told : has).reduce((m, x) => (x.p95Ms > m.p95Ms ? x : m), (told.length ? told : has)[0]);
  const tail = { p95: duration(worst.p95Ms), at: duration(worst.tMs) };
  const mid = Math.floor(rows.length / 2);
  const first = medianOf(rows.slice(0, mid));
  const second = medianOf(rows.slice(mid));
  const mark = few(rows.reduce((n, x) => n + requests(x), 0));
  if (has.length < 4 || first == null || second == null) return t("latencyTime.textFew", { p50: duration(medianOf(rows) ?? 0), ...tail }) + mark;
  // within a fifth of each other: held
  const held = Math.abs(second - first) <= 0.2 * Math.max(first, second);
  return (held ? t("latencyTime.textHeld", { p50: duration(medianOf(rows) ?? 0), ...tail }) : t(second > first ? "latencyTime.textSlower" : "latencyTime.textFaster", { a: duration(first), b: duration(second), ...tail })) + mark;
}

/** How the latencies spread over the stretch shown: its median and p99, of every request that ran in it. A
 * stretch reckoned on its own (the app's) is a profile of its own, with its own figures; a zoom into the run's
 * slices (a shared page's) reads them off the latencies' grid, to its buckets' edges ("up to"). */
export function heatLatencySaid(p: Profile, range?: [number, number] | null): string {
  const [a, b] = within(p.rate.length, range);
  const n = p.rate.slice(a, b + 1).reduce((k, x) => k + requests(x), 0);
  if (!range) return p.p50Ms == null || n === 0 ? "" : t("latencyHeat.text", { p50: duration(p.p50Ms), p99: duration(p.p99Ms ?? p.p50Ms) }) + few(n);
  const cells = p.latencyHeat.filter((c) => c.bin >= a && c.bin <= b).sort((x, y) => x.bucket - y.bucket);
  const all = cells.reduce((k, c) => k + c.n, 0);
  // the upper edge of the bucket the request at share `q` is in (three buckets a decade, as Rust counts them)
  const upTo = (q: number) => {
    let run = 0;
    for (const c of cells) if ((run += c.n) >= q * all) return 10 ** (c.bucket / 3);
    return 0;
  };
  return all ? t("latencyHeat.textUpTo", { p50: duration(upTo(0.5)), p99: duration(upTo(0.99)) }) + few(all) : "";
}

/** Which requests were slow: how many are drawn, and the slowest of them. */
export function scatterSaid(p: Profile, domainMs?: [number, number] | null): string {
  const rows = domainMs ? p.samples.filter((x) => x.tMs >= domainMs[0] && x.tMs < domainMs[1]) : p.samples;
  if (rows.length === 0) return "";
  const slow = rows.reduce((m, x) => (x.ms > m.ms ? x : m), rows[0]);
  const g = p.groups[slow.group];
  return t("scatter.text", { n: count(rows.length), kind: g ? groupName(g) : "—", ms: duration(slow.ms), at: duration(slow.tMs) });
}

/** Whether larger requests took longer: how many are drawn, and how long the largest of them took. */
export function sizeSaid(p: Profile, domainMs?: [number, number] | null): string {
  const rows = (domainMs ? p.samples.filter((x) => x.tMs >= domainMs[0] && x.tMs < domainMs[1]) : p.samples).filter((x) => (x.bytes ?? 0) > 0);
  if (rows.length === 0) return "";
  const big = rows.reduce((m, x) => ((x.bytes ?? 0) > (m.bytes ?? 0) ? x : m), rows[0]);
  return rows.length === 1 ? t("scatterSize.text.one", { size: bytes(big.bytes ?? 0), ms: duration(big.ms) }) : t("scatterSize.text", { n: count(rows.length), size: bytes(big.bytes ?? 0), ms: duration(big.ms) });
}

/** Where the waiting went: the kind of request that waited most. */
export function spentSaid(p: Profile): string {
  const top = p.groups[0];
  return top ? t("profile.spentText", { what: groupName(top), pct: percent(top.share), time: duration(top.waitedMs) }) : "";
}

/** How long most took: the median, p95 and p99 as shares done by then, of the requests the profile holds (the
 * stretch's, when one is chosen), marked when they are few. */
export function distSaid(p: Profile): string {
  if (p.p50Ms == null) return "";
  const n = p.rate.reduce((k, b) => k + requests(b), 0);
  // a handful has no p95 or p99 to state: its median and its slowest
  if (n < FEW) return t("dist.textFew", { p50: duration(p.p50Ms), max: duration(Math.max(0, ...p.groups.map((g) => g.maxMs))) }) + few(n);
  return t("dist.text", { p50: duration(p.p50Ms), p95: duration(p.p95Ms ?? 0), p99: duration(p.p99Ms ?? 0) });
}

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
    const k = (e.series[i] ?? 0);
    retries += e.retries[i] ?? 0;
    if (k > 0) {
      n += k;
      if (first < 0) first = i;
      last = i;
    }
  }
  const again = retries > 0 ? t("failures.retried", { n: count(retries) }) : "";
  if (n === 0) return [t("signal.noErrors"), again].filter(Boolean).join("; ");
  const when = first === last ? t("failures.at", { t: duration(slotMs(s, first)) }) : t("failures.between", { a: duration(slotMs(s, first)), b: duration(slotMs(s, last + 1)) });
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
    const f = (e.series[i] ?? 0);
    const n = Math.max(Math.round(((s.rate.series[i] ?? 0) * s.binMs) / 1000), f);
    all += n;
    failed += f;
    if (n > 0 && f / n > worst.share) worst = { share: f / n, i };
  }
  if (failed === 0) return t("signal.noErrors");
  return t("errorRate.text", { pct: percent(failed / Math.max(all, 1)), peak: percent(worst.share), at: duration(slotMs(s, worst.i)) });
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
  const top = s.cost.kinds.filter((x) => x.cost > 0).sort((a, b) => b.cost - a.cost)[0];
  return top ? t("costBy.text", { op: top.kind, pct: percent(top.share), total: cost(s.cost.amount, s.cost.currency) }) : "";
}

/** When each prefix was busy: the busiest prefix, in the run's busiest second or the stretch's busiest slice. */
export function heatSaid(p: Profile, range?: [number, number] | null): string {
  if (!range) {
    const top = [...p.prefixes].sort((a, b) => b.peakPerSecond - a.peakPerSecond)[0];
    return top ? t("heat.text", { prefix: top.prefix, n: count(top.peakPerSecond) }) : "";
  }
  const cell = p.heat.filter((c) => c.bin >= range[0] && c.bin <= range[1]).reduce<Profile["heat"][number] | null>((m, c) => (!m || c.n > m.n ? c : m), null);
  const prefix = cell ? p.prefixes[cell.prefix]?.prefix : null;
  return cell && prefix ? t("heat.textAt", { prefix, n: count(cell.n), at: duration(slotMs(p, cell.bin)) }) : "";
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

/** Which prefixes were busiest: the first by requests, and its busiest second. */
export function prefixSaid(p: Profile): string {
  const top = p.prefixes.filter((x) => x.prefix !== "other").sort((a, b) => b.requests - a.requests)[0];
  return top ? t("prefixUse.text", { prefix: top.prefix, value: t("prefixUse.requests", { n: count(top.requests) }), peak: count(top.peakPerSecond) }) : "";
}

/** What a fault trial did: how many requests sonde answered in the store's place, how many the program sent again
 * and how many it gave up on, and how long the trial took against the clean run. */
export const trialSaid = (s: Signals, answered: number, trialMs: number, cleanMs: number): string =>
  t("trialOver.text", { n: count(answered), again: count(s.errors.retriesTotal), gaveUp: count(s.errors.gaveUp), trial: duration(trialMs), clean: duration(cleanMs) });

// --- two runs compared -------------------------------------------------------------

/** Which kinds of request changed: the one whose requests moved most, before → after, with how much; or that none
 * did. */
export function kindChangeSaid(c: RunChange): string {
  const moved = [...c.groups].filter((g) => g.requests[0] !== g.requests[1]).sort((a, b) => Math.abs(b.requests[1] - b.requests[0]) - Math.abs(a.requests[1] - a.requests[0]))[0];
  if (!moved) return t("compare.byKindSame");
  const how = moved.requests[0] === 0 ? t("trend.fromNone") : change(moved.requests[0], moved.requests[1]);
  return t("compare.byKindText", { kind: groupName(moved), a: count(moved.requests[0]), b: count(moved.requests[1]), change: how });
}

/** Where it changed: the prefix whose requests moved most. */
export function prefixChangeSaid(c: RunChange): string {
  const top = [...(c.prefixes ?? [])].sort((a, b) => Math.abs(b.requests[1] - b.requests[0]) - Math.abs(a.requests[1] - a.requests[0]))[0];
  if (!top || top.requests[0] === top.requests[1]) return t("prefixChange.none");
  return t("prefixChange.text", { prefix: top.prefix, line: t("prefixChange.line", { a: count(top.requests[0]), b: count(top.requests[1]) }) });
}

/** The latency a share of the requests was done within, read off buckets (its bucket's upper edge). */
function doneWithin(buckets: { leMs: number | null; n: number }[], q: number): number | null {
  const all = buckets.reduce((k, b) => k + b.n, 0);
  let run = 0;
  for (const b of buckets) if ((run += b.n) >= q * all) return b.leMs;
  return null;
}

/** How long requests took in each run: the median and p95 of each, to their buckets' edges ("up to"). */
export function latencyChangeSaid(c: RunChange): string {
  if (!c.latency) return "";
  const say = (v: number | null) => (v === null ? t("cdf.slower") : duration(v));
  const [a, b] = c.latency;
  return t("cdf.textChange", { a50: say(doneWithin(a, 0.5)), b50: say(doneWithin(b, 0.5)), a95: say(doneWithin(a, 0.95)), b95: say(doneWithin(b, 0.95)) });
}

/** Two runs over time: how long each took, and how many requests each sent. */
export const overTimeSaid = (a: Profile, b: Profile): string => t("compare.overTimeText", { a: duration(a.wallMs), b: duration(b.wallMs), na: count(a.rate.reduce((k, x) => k + requests(x), 0)), nb: count(b.rate.reduce((k, x) => k + requests(x), 0)) });

/** The run as it goes: the requests of its last half minute, their p95 and how many failed (its pace is a figure
 * beside it). */
export function liveSaid(s: Signals): string {
  const n = s.rate.requests;
  if (n === 0) return t("live.none");
  return [t("live.text", { n: count(n) }), s.latency.p95Ms != null ? `p95 ${duration(s.latency.p95Ms)}${few(n)}` : "", s.errors.count ? t("timeline.errors", { n: count(s.errors.count) }) : ""].filter(Boolean).join(" · ");
}

/** A busy kind's latency, slice by slice: its median and p95 in each slice it had requests. */
export interface KindRate {
  group: number;
  name: string;
  rows: { tMs: number; n: number; p50: number; p95: number }[];
}

/** The busiest kinds (at most `max`), each its median and p95 slice by slice, from the run's requests as the
 * profile keeps them (`samples`: every request up to 4,000; past that the slowest 400 and an even spread of
 * the rest, each of the spread weighed for the requests it stands for). Within `domainMs` when given; a kind
 * seen in one slice alone draws no line and is left out. */
export function kindRates(p: Profile, domainMs?: [number, number] | null, max = 5): KindRate[] {
  const all = p.samples;
  const total = p.rate.reduce((n, b) => n + requests(b), 0);
  // past what is kept, the slowest are each themselves and the rest each stand for several
  const SLOWEST = 400;
  const sampled = total > all.length && all.length > SLOWEST;
  const floor = sampled ? [...all].sort((a, b) => b.ms - a.ms)[SLOWEST - 1].ms : Infinity;
  const spread = sampled ? (total - SLOWEST) / (all.length - SLOWEST) : 1;
  const by = new Map<number, Map<number, { ms: number; w: number }[]>>();
  for (const x of all) {
    if (x.group >= max) continue;
    if (domainMs && (x.tMs < domainMs[0] || x.tMs >= domainMs[1])) continue;
    const bin = sliceAt(p, x.tMs);
    const slices = by.get(x.group) ?? new Map<number, { ms: number; w: number }[]>();
    by.set(x.group, slices);
    slices.set(bin, [...(slices.get(bin) ?? []), { ms: Math.max(0, x.ms), w: x.ms >= floor ? 1 : spread }]);
  }
  // a weighed percentile: the first latency by which `q` of the weight is done
  const at = (xs: { ms: number; w: number }[], q: number) => {
    const sum = xs.reduce((n, x) => n + x.w, 0);
    let done = 0;
    for (const x of xs) if ((done += x.w) >= q * sum) return x.ms;
    return xs[xs.length - 1].ms;
  };
  return [...by.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([group, slices]) => ({
      group,
      name: groupName(p.groups[group]),
      rows: [...slices.entries()]
        .sort((a, b) => a[0] - b[0])
        .map(([bin, xs]) => {
          xs.sort((a, b) => a.ms - b.ms);
          return { tMs: slotMs(p, bin), n: Math.round(xs.reduce((n, x) => n + x.w, 0)), p50: at(xs, 0.5), p95: at(xs, 0.95) };
        }),
    }))
    .filter((k) => k.rows.length > 1 && p.groups[k.group]);
}

/** Which kind got slow, and when: the slowest p95 of a busy kind, and its moment. */
export function kindsSaid(p: Profile, domainMs?: [number, number] | null): string {
  let best: { name: string; p95: number; tMs: number } | null = null;
  for (const k of kindRates(p, domainMs)) for (const r of k.rows) if (!best || r.p95 > best.p95) best = { name: k.name, p95: r.p95, tMs: r.tMs };
  return best ? t("kinds.text", { kind: best.name, p95: duration(best.p95), at: duration(best.tMs) }) : "";
}
