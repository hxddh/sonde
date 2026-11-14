// What sonde finds out of a bucket without being asked, as its objects open:
// its size when it was measured, its incomplete uploads, the systems whose
// layouts it holds. Kept per endpoint, bucket and prefix, so the sections
// that say them (Size, Incomplete uploads, Layouts) open on what is known.
// The section row is the one way to each: no tiles repeat it.

import { batch, cleanup, effect, signal, untracked } from "nsh";
import { t } from "../i18n";
import { api, watch } from "../native";
import { bucket, endpoint, prefix } from "../store";
import type { Detection, DuReport, MpuReport } from "../types";

export const LAYOUT_LABEL: Record<string, string> = {
  slatedb: "SlateDB",
  iceberg: "Apache Iceberg",
  lakefs: "lakeFS",
  woodpecker: "Milvus Woodpecker",
  turbopuffer: "turbopuffer",
  warpstream: "WarpStream",
  get generic() {
    return t("layouts.generic");
  },
};

type Scanned = "mpu" | "layouts";

interface Known {
  mpu?: MpuReport | null;
  layouts?: Detection[] | null;
  size?: DuReport | null;
  /** Why a fact could not be found out (a scan failed, access was denied). */
  failed?: Partial<Record<Scanned, string>>;
  /** When each scanned fact was found out: an old one is asked for again. */
  asked?: Partial<Record<Scanned, number>>;
}

/** How long a fact is taken as it was: a place opened again after this asks again. */
const FRESH_MS = 5 * 60_000;

const reason = (e: unknown) => (e instanceof Error ? e.message : typeof e === "object" && e && "message" in e ? `${(e as { message: unknown }).message}` : `${e}`);

/** What is known of each endpoint/bucket/prefix, shared by the tiles and their sheets. */
export const known = signal<Record<string, Known>>({});
export const knownKey = (ep: string, b: string, p: string) => `${ep}\n${b}\n${p}`;
export function remember(ep: string, b: string, p: string, k: Known) {
  const key = knownKey(ep, b, p);
  const was = known()[key];
  const now = Date.now();
  const asked = { ...was?.asked, ...(k.mpu !== undefined ? { mpu: now } : {}), ...(k.layouts !== undefined ? { layouts: now } : {}) };
  known.set({ ...known(), [key]: { ...was, ...k, asked } });
}
export const here = (): Known => {
  const ep = endpoint();
  const b = bucket();
  return ep && b ? (known()[knownKey(ep, b, prefix())] ?? {}) : {};
};

/** Bumped by `invalidate`: the place shown is asked about again. */
const reloads = signal(0);

/** Forgets what is known of the bucket and prefix shown and finds it out
 * again (Refresh: what changed elsewhere — uploads aborted, versioning
 * turned on — shows). */
export function invalidate() {
  const ep = endpoint();
  const b = bucket();
  if (!ep || !b) return;
  batch(() => {
    const { [knownKey(ep, b, prefix())]: _, ...rest } = known();
    known.set(rest);
    reloads.update((n) => n + 1);
  });
}

/** Fills in what is known of the bucket and prefix shown: the cheap facts, as it opens. */
export function useBucketFacts() {
  let jobs: string[] = [];
  // each opening of a bucket or prefix: an answer for an earlier one is dropped
  let opening = 0;
  const stop = () => {
    opening++;
    jobs.forEach((j) => api.cancelJob(j));
    jobs = [];
  };
  cleanup(stop);
  effect(() => {
    const ep = endpoint();
    const b = bucket();
    const p = prefix();
    reloads();
    untracked(() => {
      stop();
      if (!ep || !b) return;
      const mine = opening;
      const now = Date.now();
      const have = known()[knownKey(ep, b, p)] ?? {};
      const failed = (what: Scanned, e: unknown) => {
        if (mine !== opening) return; // cancelled, not failed: nothing learned
        const k = known()[knownKey(ep, b, p)] ?? {};
        remember(ep, b, p, { failed: { ...k.failed, [what]: reason(e) } });
      };
      if (have.size === undefined) remember(ep, b, p, { size: api.duCached(ep, b, p) });
      const job = async <R,>(what: Scanned, start: Promise<string>, put: (r: R) => void) => {
        try {
          const id = await start;
          if (mine !== opening) return void api.cancelJob(id);
          jobs.push(id);
          const r = await watch<R>(id, () => {});
          if (mine === opening) put(r);
        } catch (e) {
          failed(what, e);
        }
      };
      // not known yet, not found out last time, or found out long ago (what is shown stays while it is asked again)
      const due = (what: Scanned) => have[what] === undefined || have.failed?.[what] !== undefined || now - (have.asked?.[what] ?? 0) > FRESH_MS;
      if (due("mpu")) void job<MpuReport>("mpu", api.mpuScan(ep, b, p), (r) => remember(ep, b, p, { mpu: r, failed: { ...known()[knownKey(ep, b, p)]?.failed, mpu: undefined } }));
      if (due("layouts")) void job<Detection[]>("layouts", api.detectLayouts(ep, b, p), (r) => remember(ep, b, p, { layouts: r, failed: { ...known()[knownKey(ep, b, p)]?.failed, layouts: undefined } }));
    });
  });
}

/** S3's versioning status in the reader's language. */
export const versioningLabel = (s: string) => (s === "Enabled" ? t("versions.enabled") : s === "Suspended" ? t("versions.suspended") : s === "Unversioned" || s === "" ? t("common.off") : s);

