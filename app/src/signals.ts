// A run by its four signals — rate, errors, latency, bytes — and how many
// requests were in flight: what the session's header says of each (a figure,
// its line over the session, one line of detail), in words. Rust counts them
// (crates/proxy/src/signals.rs); a page shared before 15.0 has none, and
// its figures are read from the profile's slices instead.

import { bytes, count, duration, percent, rate, transfer } from "./format";
import { type Key, t } from "./i18n";
import type { Profile, SessionSummary, SessionView, Signals } from "./types";

export type SignalId = "rate" | "errors" | "latency" | "bytes" | "inFlight";

/** A signal as its tile says it: nothing here is a number any more. */
export interface SignalTile {
  id: SignalId;
  label: string;
  value: string;
  detail: string;
  tone: "plain" | "bad" | "quiet";
  /** Its line over the session, slice by slice (empty: nothing to draw). */
  series: number[];
  /** What the line is of: counts as bars, a level as steps, a measure as a line. */
  kind: "bars" | "steps" | "line";
}

/** Under this many requests a percentile is of too few to be told from its
 * neighbours: said beside it wherever one is shown. */
export const FEW = 20;

/** A run too short to have a pace: under a second, or under `FEW` requests.
 * Its header says totals ("9 requests", "3.0 MiB"), never rates, and draws
 * no line over a time that is a moment. Rust's own flag where the signals
 * carry it. */
export function brief(s: Signals): boolean {
  return s.short ?? (s.timed && (s.wallMs < 1000 || s.rate.requests < FEW));
}

const sum = (xs: number[]) => xs.reduce((a, b) => a + b, 0);

/** The signals of a session read from its profile: what a page shared before
 * 15.0 can still say. What the profile does not hold (transport failures,
 * injected faults, sizes, first bytes, statuses) is left empty, not guessed. */
export function signalsFromProfile(p: Profile, s: SessionSummary): Signals {
  const timed = p.timed !== false;
  const secs = p.wallMs / 1000;
  const per = (n: number) => (timed && secs > 0 ? n / secs : null);
  const bin = Math.max(p.binMs, 1e-6) / 1000;
  const n = (b: Profile["rate"][number]) => b.ok + b.miss + b.client + b.server;
  const requests = sum(p.rate.map(n));
  const client = p.rate.map((b) => b.client);
  const server = p.rate.map((b) => b.server);
  const up = p.rate.map((b) => (b.bytesUp ?? 0) / bin);
  const down = p.rate.map((b) => (b.bytesDown ?? 0) / bin);
  // the most in flight within each slice
  const flight = p.rate.map(() => 0);
  for (const x of p.inFlight) {
    const i = Math.min(flight.length - 1, Math.max(0, Math.floor(x.tMs / Math.max(p.binMs, 1e-6))));
    if (i >= 0) flight[i] = Math.max(flight[i], x.n);
  }
  const failed = sum(client) + sum(server);
  return {
    timed,
    wallMs: p.wallMs,
    binMs: p.binMs,
    bins: p.rate.length,
    rate: { value: per(requests), unit: "1/s", series: p.rate.map((b) => n(b) / bin), requests: requests || s.requests },
    errors: {
      value: requests ? failed / requests : null,
      series: p.rate.map((b) => b.client + b.server),
      count: failed,
      counts: { client: sum(client), server: sum(server), transport: 0, injected: 0 },
      client,
      server,
      transport: p.rate.map(() => 0),
      injected: p.rate.map(() => 0),
      retries: p.rate.map((b) => b.retried ?? 0),
      retriesTotal: sum(p.rate.map((b) => b.retried ?? 0)),
    },
    latency: { value: timed ? (p.p50Ms ?? null) : null, series: p.rate.map((b) => b.p50Ms), p50Ms: timed ? (p.p50Ms ?? null) : null, p95Ms: timed ? (p.p95Ms ?? null) : null, p99Ms: timed ? (p.p99Ms ?? null) : null, p95: p.rate.map((b) => b.p95Ms), p99: p.rate.map((b) => b.p99Ms) },
    bytes: { value: per(s.bytesUp + s.bytesDown), series: up.map((u, i) => u + down[i]), upPerSec: per(s.bytesUp), downPerSec: per(s.bytesDown), up, down, upBytes: s.bytesUp, downBytes: s.bytesDown },
    inFlight: { value: timed ? p.peakInFlight : null, series: flight, peak: p.peakInFlight, peakAtMs: p.peakAtMs },
    firstByte: null,
    sizes: { request: [], response: [], throughput: [], mibPerSec: null },
    statuses: [],
    cost: { amount: p.cost.amount, currency: p.cost.currency, operations: [] },
    prefixes: [],
  };
}

/** A session's signals: Rust's, or (an older share) the profile's. */
export function signalsOf(v: Pick<SessionView, "signals" | "profile" | "summary">): Signals | null {
  return v.signals ?? (v.profile ? signalsFromProfile(v.profile, v.summary) : null);
}

/** Bytes a second: "575 KiB/s". */
export const bytesRate = (perSecond: number) => `${bytes(perSecond)}/s`;

/** The kinds of failure a session had, most telling first: "1 server · 2 client". */
function failures(s: Signals): string[] {
  const c = s.errors.counts;
  const parts: [number, Key][] = [
    [c.server, "signal.server"],
    [c.client, "signal.client"],
    [c.transport, "signal.transport"],
    [c.injected, "signal.injected"],
  ];
  return parts.filter(([n]) => n > 0).map(([n, key]) => t(key, { n: count(n) }));
}

/** The four signals and in flight, in rank order (a narrow header drops the
 * last first). A figure a log without times cannot give says so in words —
 * "not timed" — and never reads 0. */
export function signalTiles(s: Signals): SignalTile[] {
  const none = t("signal.notTimed");
  const retries = s.errors.retriesTotal > 0 ? [t("signal.retries", { n: count(s.errors.retriesTotal) })] : [];
  const wrong = [...failures(s), ...retries];
  const both = s.bytes.upBytes + s.bytes.downBytes;
  // a run of a moment: totals, and no line over it
  const few = brief(s);
  const over = (series: number[]) => (s.timed && !few ? series : []);
  // a percentile of a handful of requests is one of them: said
  const tails = s.latency.p95Ms == null || s.latency.p99Ms == null ? "" : t(s.rate.requests < FEW ? "signal.tailsFew" : "signal.tails", { p95: duration(s.latency.p95Ms), p99: duration(s.latency.p99Ms), n: count(s.rate.requests) });
  return [
    {
      id: "rate",
      label: few ? t("signal.requests") : t("signal.rate"),
      value: few ? t("run.bandRequests", { n: s.rate.requests }) : s.rate.value == null ? none : t("signal.perSecond", { n: rate(s.rate.value) }),
      detail: few ? t("signal.inTime", { t: duration(s.wallMs) }) : s.timed ? t("signal.requestsIn", { n: count(s.rate.requests), t: duration(s.wallMs) }) : t("run.bandRequests", { n: s.rate.requests }),
      tone: !few && s.rate.value == null ? "quiet" : "plain",
      series: over(s.rate.series),
      kind: "bars",
    },
    {
      id: "errors",
      label: t("signal.errors"),
      // of a handful, how many; of a run, the share
      value: few ? t("signal.failedOf", { n: count(s.errors.count), of: count(s.rate.requests) }) : s.errors.value == null ? "—" : percent(s.errors.value),
      detail: wrong.length ? wrong.join(" · ") : t("signal.noErrors"),
      tone: s.errors.count > 0 ? "bad" : "plain",
      series: few ? [] : s.errors.series,
      kind: "bars",
    },
    {
      id: "latency",
      label: t("signal.latency"),
      value: s.latency.p50Ms == null ? none : duration(s.latency.p50Ms),
      detail: tails,
      tone: s.latency.p50Ms == null ? "quiet" : "plain",
      series: over(s.latency.series),
      kind: "line",
    },
    {
      id: "bytes",
      label: t("signal.bytes"),
      // a log without times still knows how much moved, and a run of a moment has no pace: the amount
      value: few || s.bytes.value == null ? bytes(both) : bytesRate(s.bytes.value),
      detail: few || s.bytes.upPerSec == null || s.bytes.downPerSec == null ? transfer(s.bytes.upBytes, s.bytes.downBytes) : transfer(s.bytes.upPerSec, s.bytes.downPerSec, bytesRate),
      tone: "plain",
      series: over(s.bytes.series),
      kind: "bars",
    },
    {
      id: "inFlight",
      label: t("signal.inFlight"),
      value: s.inFlight.value == null ? none : count(s.inFlight.peak),
      detail: s.inFlight.value == null ? "" : few ? t("signal.ofRequests", { n: s.rate.requests }) : t("signal.peakAt", { t: duration(s.inFlight.peakAtMs) }),
      tone: s.inFlight.value == null ? "quiet" : "plain",
      series: over(s.inFlight.series),
      kind: "steps",
    },
  ];
}

/** One operation of a run: how many, what share failed, how long. */
export interface OpStats {
  operation: string;
  count: number;
  errorShare: number;
  p50Ms: number | null;
  p95Ms: number | null;
  p99Ms: number | null;
  /** Under `FEW` requests: its percentiles are of too few to be told apart. */
  few: boolean;
}

/** A run by operation: Rust's own table (`summary.operations`), or — a share
 * made before 16.0 — what its profile's kinds hold of it (no p99, and a
 * lookup that found nothing counted with its operation). */
export function operationStats(v: Pick<SessionView, "summary" | "profile">): OpStats[] {
  const own = v.summary.operations;
  if (own?.length) return own.map((o) => ({ operation: o.operation, count: o.count, errorShare: o.errorShare, p50Ms: o.p50Ms, p95Ms: o.p95Ms, p99Ms: o.p99Ms, few: !!o.few || o.count < FEW }));
  const timed = v.profile.timed !== false;
  const by = new Map<string, OpStats & { top: number }>();
  for (const g of v.profile.groups) {
    const x = by.get(g.operation);
    if (!x) by.set(g.operation, { operation: g.operation, count: g.requests, errorShare: g.errors, p50Ms: timed ? g.p50Ms : null, p95Ms: timed ? g.p95Ms : null, p99Ms: null, few: false, top: g.requests });
    else {
      // the percentiles of the larger of its two kinds: the profile holds no others
      if (g.requests > x.top) (x.top = g.requests), (x.p50Ms = timed ? g.p50Ms : null), (x.p95Ms = timed ? g.p95Ms : null);
      x.count += g.requests;
      x.errorShare += g.errors;
    }
  }
  return [...by.values()].map(({ top: _, ...o }) => ({ ...o, errorShare: o.count ? o.errorShare / o.count : 0, few: o.count < FEW })).sort((a, b) => b.count - a.count);
}

/** A size bucket by its upper bound, short enough for an axis: "empty", "≤ 1.0 KiB". */
export function sizeLabel(leBytes: number): string {
  return leBytes <= 0 ? t("sizes.empty") : `≤ ${bytes(leBytes)}`;
}

/** A size bucket by both its bounds (each is twice the last): "512 B – 1.0 KiB". */
export function sizeRange(leBytes: number): string {
  if (leBytes <= 0) return t("sizes.empty");
  // the one up to 1 B has no lower bound to say
  return leBytes <= 1 ? "1 B" : `${bytes(leBytes / 2)} – ${bytes(leBytes)}`;
}

/** The share of S3's limit for a prefix a session's busiest second reached: the nearer of reads and writes. */
export function nearestLimit(x: { readShare: number; writeShare: number }): { share: number; kind: "reads" | "writes" } {
  return x.writeShare > x.readShare ? { share: x.writeShare, kind: "writes" } : { share: x.readShare, kind: "reads" };
}
