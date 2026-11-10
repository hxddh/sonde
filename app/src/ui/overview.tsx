// A bucket at a glance: above its objects, what sonde can tell without being
// asked — its size when it was measured, incomplete uploads and what they
// cost, versioning, the systems whose layouts it recognises. Four tiles,
// always there: each says what it knows (or that it is finding out), and
// opens its detail in a sheet. The objects under them never move.

import { batch, cleanup, effect, signal, untracked, View } from "nsh";
import { bytes, count, day, money } from "../format";
import { type Key, t } from "../i18n";
import { api, watch } from "../native";
import { bucket, endpoint, go, prefix } from "../store";
import type { Detection, DuReport, MpuReport, VersioningStatus } from "../types";
import { Stat } from "./kit";

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

const reason = (e: unknown) => (e instanceof Error ? e.message : typeof e === "object" && e && "message" in e ? String((e as { message: unknown }).message) : String(e));

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

/** Versioning is a bucket's, whatever prefix of it shows: asked once per bucket, not per prefix. */
const versioning = signal<Record<string, { status?: VersioningStatus; failed?: string; at: number }>>({});
const bucketKey = (ep: string, b: string) => `${ep}\n${b}`;
/** The buckets whose versioning is being asked for: one question each. */
const asking = new Set<string>();

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
    const { [bucketKey(ep, b)]: __, ...others } = versioning();
    versioning.set(others);
    reloads.update((n) => n + 1);
  });
}

/** Fills in what is known of the bucket and prefix shown: the cheap facts, as it opens. */
function useFacts() {
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
      // the bucket's: an answer is the bucket's whatever prefix shows by then
      const bk = bucketKey(ep, b);
      const v = versioning()[bk];
      if ((!v || v.failed !== undefined || now - v.at > FRESH_MS) && !asking.has(bk)) {
        asking.add(bk);
        api
          .versioning(ep, b)
          .then(
            (status) => versioning.set({ ...versioning(), [bk]: { status, at: Date.now() } }),
            (e) => versioning.set({ ...versioning(), [bk]: { failed: reason(e), at: Date.now() } }),
          )
          .finally(() => asking.delete(bk));
      }
    });
  });
}

/** S3's versioning status in the reader's language. */
export const versioningLabel = (s: string) => (s === "Enabled" ? t("versions.enabled") : s === "Suspended" ? t("versions.suspended") : s === "Unversioned" || s === "" ? t("common.off") : s);

/** The four tiles over a bucket's objects: Size, Incomplete uploads,
 * Versioning, Layouts. Always there, one height: a tile that is finding
 * out shows its place, one with nothing to report says so calmly. */
export function Overview() {
  useFacts();
  const mpu = () => here().mpu;
  const layouts = () => here().layouts;
  const size = () => here().size;
  const fail = (what: Scanned) => here().failed?.[what];
  const pending = (what: Scanned) => here()[what] === undefined && !fail(what);
  const v = () => {
    const ep = endpoint();
    const b = bucket();
    return ep && b ? versioning()[bucketKey(ep, b)] : undefined;
  };
  const month = (cost: number, currency: string) => `${money(cost, currency)}/${t("overview.month")}`;
  const TILE = "min-w-44 flex-1";
  return (
    <View testID="overview" class="flex-row flex-wrap gap-3">
      <Stat
        class={TILE}
        testID="overview-capacity"
        icon="chart-pie"
        label={t("overview.size")}
        // measured on request only: until then the tile is the way to ask
        value={size() ? bytes(size()!.totalBytes) : t("overview.measureAction")}
        detail={size() ? `${month(size()!.costPerMonth, size()!.currency)} · ${day(size()!.scannedAt)}` : t("overview.measureHint")}
        onPress={() => go("capacity")}
      />
      <Stat
        class={TILE}
        testID="overview-multipart"
        icon="layers"
        label={t("overview.uploads")}
        pending={pending("mpu")}
        tone={fail("mpu") || (mpu()?.staleUploads ?? 0) > 0 ? "warn" : "plain"}
        value={fail("mpu") ? t("overview.failed") : mpu() ? (mpu()!.totalUploads > 0 ? `${count(mpu()!.totalUploads)} · ${bytes(mpu()!.totalBytes)}` : t("overview.noUploads")) : ""}
        detail={
          fail("mpu") ??
          (mpu()
            ? mpu()!.totalUploads > 0
              ? `${t("mpu.stale", { n: mpu()!.staleUploads, age: t("common.days", { n: mpu()!.thresholdDays }) })} · ${month(mpu()!.costPerMonth, mpu()!.currency)}`
              : mpu()!.lifecycle.status === "missing"
                ? t("overview.noRule")
                : `${t("mpu.lifecycle")}: ${t(`mpu.lifecycle.${mpu()!.lifecycle.status}` as Key)}`
            : "")
        }
        onPress={() => go("multipart")}
      />
      <Stat
        class={TILE}
        testID="overview-versions"
        icon="file-clock"
        label={t("overview.versioning")}
        pending={!v()}
        tone={v()?.failed !== undefined ? "warn" : "plain"}
        value={v()?.failed !== undefined ? t("overview.failed") : v()?.status ? versioningLabel(v()!.status!.status) : ""}
        detail={v()?.failed ?? (v()?.status?.objectLock ? `${t("versions.lock")}: ${v()!.status!.objectLock}` : t("overview.versionsHint"))}
        onPress={() => go("versions")}
      />
      <Stat
        class={TILE}
        testID="overview-layouts"
        icon="boxes"
        label={t("overview.layouts")}
        pending={pending("layouts")}
        tone={fail("layouts") ? "warn" : "plain"}
        value={fail("layouts") ? t("overview.failed") : layouts()?.length ? [...new Set(layouts()!.map((d) => LAYOUT_LABEL[d.format] ?? d.format))].join(", ") : t("common.none")}
        detail={fail("layouts") ?? (layouts()?.length ? layouts()!.map((d) => d.root || "/").join(" · ") : t("layouts.noneFound"))}
        onPress={() => go("layouts")}
      />
    </View>
  );
}
