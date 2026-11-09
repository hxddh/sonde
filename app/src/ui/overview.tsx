// A bucket at a glance: above its objects, what sonde can tell without being
// asked — incomplete uploads and what they cost, versioning, the systems
// whose layouts it recognises, the size when it is known. Each card shows
// only when it has something to say, and opens its detail in a sheet.

import { batch, cleanup, createMotion, effect, signal, Text, untracked, View } from "nsh";
import { bytes, day, money } from "../format";
import { t } from "../i18n";
import { api, watch } from "../native";
import { bucket, endpoint, go, prefix, type StoreTab } from "../store";
import type { Detection, DuReport, MpuReport, VersioningStatus } from "../types";
import { Icon, type IconName } from "./icon";
import { Card } from "./kit";

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

interface Known {
  mpu?: MpuReport | null;
  versioning?: VersioningStatus | null;
  layouts?: Detection[] | null;
  size?: DuReport | null;
  /** Why a fact could not be found out (a scan failed, access was denied). */
  failed?: Partial<Record<"mpu" | "versioning" | "layouts", string>>;
}

const reason = (e: unknown) => (e instanceof Error ? e.message : typeof e === "object" && e && "message" in e ? String((e as { message: unknown }).message) : String(e));

/** What is known of each endpoint/bucket/prefix, shared by the cards and their sheets. */
export const known = signal<Record<string, Known>>({});
export const knownKey = (ep: string, b: string, p: string) => `${ep}\n${b}\n${p}`;
export function remember(ep: string, b: string, p: string, k: Known) {
  const key = knownKey(ep, b, p);
  known.set({ ...known(), [key]: { ...known()[key], ...k } });
}
export const here = (): Known => {
  const ep = endpoint();
  const b = bucket();
  return ep && b ? (known()[knownKey(ep, b, prefix())] ?? {}) : {};
};

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
    untracked(() => {
      stop();
      if (!ep || !b) return;
      const mine = opening;
      const have = known()[knownKey(ep, b, p)] ?? {};
      const failed = (what: "mpu" | "versioning" | "layouts", e: unknown) => {
        if (mine !== opening) return; // cancelled, not failed: nothing learned
        const k = known()[knownKey(ep, b, p)] ?? {};
        remember(ep, b, p, { failed: { ...k.failed, [what]: reason(e) } });
      };
      if (have.size === undefined) remember(ep, b, p, { size: api.duCached(ep, b, p) });
      const job = async <R,>(what: "mpu" | "layouts", start: Promise<string>, put: (r: R) => void) => {
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
      const retry = (what: "mpu" | "versioning" | "layouts") => have[what] === undefined || have.failed?.[what] !== undefined;
      if (retry("mpu")) void job<MpuReport>("mpu", api.mpuScan(ep, b, p), (r) => remember(ep, b, p, { mpu: r, failed: { ...known()[knownKey(ep, b, p)]?.failed, mpu: undefined } }));
      if (retry("layouts")) void job<Detection[]>("layouts", api.detectLayouts(ep, b, p), (r) => remember(ep, b, p, { layouts: r, failed: { ...known()[knownKey(ep, b, p)]?.failed, layouts: undefined } }));
      if (retry("versioning"))
        api.versioning(ep, b).then(
          (v) => mine === opening && remember(ep, b, p, { versioning: v, failed: { ...known()[knownKey(ep, b, p)]?.failed, versioning: undefined } }),
          (e) => failed("versioning", e),
        );
    });
  });
}

type Tone = "plain" | "warn" | "ok";

function Tile(props: { id: StoreTab; icon: IconName; title: string; value: string; detail?: string; tone?: Tone; pending?: boolean }) {
  // a card rises into place as it appears (not with reduced motion)
  const motion = createMotion({ enter: "fadeInUp", duration: 220 });
  return (
    <View class="min-w-44 flex-1" style={motion.style}>
      <Card pad="tile" testID={`overview-${props.id}`} label={`${props.title}: ${props.value}`} onPress={() => go(props.id)} class="flex-1">
        <View class="flex-row items-center gap-1.5">
          <Icon name={props.icon} size={13} tint={props.tone === "warn" ? "warn" : "muted-foreground"} />
          <Text class="flex-1 text-caption text-muted-foreground line-clamp-1">{props.title}</Text>
          <Icon name="chevron-right" size={12} />
        </View>
        <If when={!props.pending} else={<View class="my-1 h-4 w-24 rounded-(--radius-sm) bg-muted" />}>
          <Text class={`text-heading font-semibold tabular-nums line-clamp-1 ${props.tone === "warn" ? "text-warn" : "text-foreground"}`}>{props.value}</Text>
        </If>
        <If when={!props.pending} else={<View class="h-3 w-32 rounded-(--radius-sm) bg-muted/70" />}>
          <Text class="text-caption text-muted-foreground line-clamp-2">{props.detail ?? ""}</Text>
        </If>
      </Card>
    </View>
  );
}

/** S3's versioning status in the reader's language. */
export const versioningLabel = (s: string) => (s === "Enabled" ? t("versions.enabled") : s === "Suspended" ? t("versions.suspended") : s);

/** The cards over a bucket's objects. */
export function Overview() {
  useFacts();
  const k = here;
  const mpu = () => k().mpu;
  const layouts = () => k().layouts;
  const v = () => k().versioning;
  const size = () => k().size;
  const fail = (what: "mpu" | "versioning" | "layouts") => k().failed?.[what];
  const pending = (what: "mpu" | "versioning" | "layouts") => k()[what] === undefined && !fail(what);
  return (
    <View testID="overview" class="flex-row flex-wrap gap-3">
      <Tile
        id="capacity"
        icon="chart-pie"
        title={t("overview.size")}
        // not measured yet: no figure, and the card says how it is measured
        value={size() ? bytes(size()!.totalBytes) : "—"}
        detail={size() ? `${money(size()!.costPerMonth, size()!.currency)}/${t("overview.month")} · ${day(size()!.scannedAt)}` : `${t("overview.measure")}: ${t("overview.measureHint")}`}
      />
      <If when={fail("mpu")}>{(e) => <Tile id="multipart" icon="layers" title={t("overview.uploads")} tone="warn" value={t("overview.failed")} detail={e()} />}</If>
      <If when={!fail("mpu") && (pending("mpu") || (mpu() && (mpu()!.totalUploads > 0 || mpu()!.lifecycle.status !== "covered")))}>
        <Tile
          id="multipart"
          icon="layers"
          title={t("overview.uploads")}
          pending={pending("mpu")}
          tone={mpu() && mpu()!.staleUploads > 0 ? "warn" : "plain"}
          value={mpu() ? (mpu()!.totalUploads > 0 ? `${mpu()!.totalUploads} · ${bytes(mpu()!.totalBytes)}` : t("overview.noUploads")) : ""}
          detail={mpu() ? (mpu()!.totalUploads > 0 ? `${t("mpu.stale", { n: mpu()!.staleUploads, age: t("common.days", { n: mpu()!.thresholdDays }) })} · ${money(mpu()!.costPerMonth, mpu()!.currency)}/${t("overview.month")}` : t("overview.noRule")) : ""}
        />
      </If>
      <If when={fail("versioning")}>{(e) => <Tile id="versions" icon="file-clock" title={t("overview.versioning")} tone="warn" value={t("overview.failed")} detail={e()} />}</If>
      <If when={!fail("versioning") && (pending("versioning") || (v() && v()!.status !== "Unversioned" && v()!.status !== ""))}>
        <Tile id="versions" icon="file-clock" title={t("overview.versioning")} pending={pending("versioning")} value={v() ? versioningLabel(v()!.status) : ""} detail={v() ? (v()!.objectLock ? `${t("versions.lock")}: ${v()!.objectLock}` : t("overview.versionsHint")) : ""} />
      </If>
      <If when={fail("layouts")}>{(e) => <Tile id="layouts" icon="boxes" title={t("overview.layouts")} tone="warn" value={t("overview.failed")} detail={e()} />}</If>
      <If when={!fail("layouts") && (pending("layouts") || (layouts() && layouts()!.length > 0))}>
        <Tile
          id="layouts"
          icon="boxes"
          title={t("overview.layouts")}
          pending={pending("layouts")}
          value={layouts() ? [...new Set(layouts()!.map((d) => LAYOUT_LABEL[d.format] ?? d.format))].join(", ") : ""}
          detail={layouts() ? layouts()!.map((d) => d.root || "/").join(" · ") : ""}
        />
      </If>
    </View>
  );
}

/** Forgets what is known of a bucket (after a change sonde made to it). */
export function forget(ep: string, b: string, p: string, what: keyof Known) {
  const key = knownKey(ep, b, p);
  const { [what]: _, ...rest } = known()[key] ?? {};
  batch(() => known.set({ ...known(), [key]: rest }));
}
