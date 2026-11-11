// A run by its four signals — rate, errors, latency, bytes — and how many
// requests were in flight: what the session's header says of each (a figure,
// its line over the session, one line of detail), in words. Rust counts them
// (crates/proxy/src/signals.rs); a page shared before 15.0 has none, and
// its figures are read from the profile's slices instead.

import { bytes, change, count, duration, percent, rate } from "./format";
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
  return [
    {
      id: "rate",
      label: t("signal.rate"),
      value: s.rate.value == null ? none : t("signal.perSecond", { n: rate(s.rate.value) }),
      detail: s.timed ? t("signal.requestsIn", { n: count(s.rate.requests), t: duration(s.wallMs) }) : t("run.bandRequests", { n: s.rate.requests }),
      tone: s.rate.value == null ? "quiet" : "plain",
      series: s.timed ? s.rate.series : [],
    },
    {
      id: "errors",
      label: t("signal.errors"),
      value: s.errors.value == null ? "—" : percent(s.errors.value),
      detail: wrong.length ? wrong.join(" · ") : t("signal.noErrors"),
      tone: s.errors.count > 0 ? "bad" : "plain",
      series: s.errors.series,
    },
    {
      id: "latency",
      label: t("signal.latency"),
      value: s.latency.p50Ms == null ? none : duration(s.latency.p50Ms),
      detail: s.latency.p95Ms == null || s.latency.p99Ms == null ? "" : t("signal.tails", { p95: duration(s.latency.p95Ms), p99: duration(s.latency.p99Ms) }),
      tone: s.latency.p50Ms == null ? "quiet" : "plain",
      series: s.timed ? s.latency.series : [],
    },
    {
      id: "bytes",
      label: t("signal.bytes"),
      // a log without times still knows how much moved: the amount, not a pace
      value: s.bytes.value == null ? bytes(both) : bytesRate(s.bytes.value),
      detail: s.bytes.upPerSec == null || s.bytes.downPerSec == null ? `↑ ${bytes(s.bytes.upBytes)} ↓ ${bytes(s.bytes.downBytes)}` : `↑ ${bytesRate(s.bytes.upPerSec)} ↓ ${bytesRate(s.bytes.downPerSec)}`,
      tone: "plain",
      series: s.timed ? s.bytes.series : [],
    },
    {
      id: "inFlight",
      label: t("signal.inFlight"),
      value: s.inFlight.value == null ? none : count(s.inFlight.peak),
      detail: s.inFlight.value == null ? "" : t("signal.peakAt", { t: duration(s.inFlight.peakAtMs) }),
      tone: s.inFlight.value == null ? "quiet" : "plain",
      series: s.timed ? s.inFlight.series : [],
    },
  ];
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

/** Two runs by their four signals, before and after: each tile says both
 * figures and by how much the second differs, where both can be said. */
export function signalChanges(before: Signals, after: Signals): SignalTile[] {
  const a = signalTiles(before);
  const b = signalTiles(after);
  const figure = (s: Signals, id: SignalId) => ({ rate: s.rate.value, errors: s.errors.value, latency: s.latency.p50Ms, bytes: s.bytes.value, inFlight: s.inFlight.value })[id];
  return b.slice(0, 4).map((x, i) => {
    const [u, v] = [figure(before, x.id), figure(after, x.id)];
    // more failures or a longer wait is worse; the rest only differs
    const worse = (x.id === "errors" || x.id === "latency") && u != null && v != null && v > u;
    return { ...x, value: `${a[i].value} → ${x.value}`, detail: u == null || v == null || u === v ? x.detail : `${change(u, v)} · ${x.detail}`, tone: worse ? "bad" : "plain" };
  });
}
