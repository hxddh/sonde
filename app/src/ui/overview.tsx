// A bucket at a glance: above its objects, what sonde can tell without being
// asked — incomplete uploads and what they cost, versioning, the systems
// whose layouts it recognises, the size when it is known. Each card shows
// only when it has something to say, and opens its detail in a sheet.

import { batch, cleanup, createMotion, effect, signal, Text, untracked, View } from "nsh";
import { Button as BaseButton } from "nsh/base";
import { bytes, money } from "../format";
import { t } from "../i18n";
import { api, watch } from "../native";
import { bucket, endpoint, go, prefix, type StoreTab } from "../store";
import type { Detection, DuReport, MpuReport, VersioningStatus } from "../types";
import { Icon, type IconName } from "./icon";

export const LAYOUT_LABEL: Record<string, string> = { slatedb: "SlateDB", iceberg: "Apache Iceberg", lakefs: "lakeFS", woodpecker: "Milvus Woodpecker", turbopuffer: "turbopuffer", warpstream: "WarpStream", generic: "Write cadence" };

interface Known {
  mpu?: MpuReport | null;
  versioning?: VersioningStatus | null;
  layouts?: Detection[] | null;
  size?: DuReport | null;
}

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
  const stop = () => {
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
      const have = known()[knownKey(ep, b, p)] ?? {};
      if (have.size === undefined) remember(ep, b, p, { size: api.duCached(ep, b, p) });
      const job = async <R,>(start: Promise<string>, put: (r: R | null) => void) => {
        try {
          const id = await start;
          jobs.push(id);
          put(await watch<R>(id, () => {}));
        } catch {
          put(null);
        }
      };
      if (have.mpu === undefined) void job<MpuReport>(api.mpuScan(ep, b, p), (r) => remember(ep, b, p, { mpu: r }));
      if (have.layouts === undefined) void job<Detection[]>(api.detectLayouts(ep, b, p), (r) => remember(ep, b, p, { layouts: r }));
      if (have.versioning === undefined)
        api.versioning(ep, b).then(
          (v) => remember(ep, b, p, { versioning: v }),
          () => remember(ep, b, p, { versioning: null }),
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
      <BaseButton
        testID={`overview-${props.id}`}
        accessibilityLabel={`${props.title}: ${props.value}`}
        onPress={() => go(props.id)}
        class="gap-1 rounded-(--radius-card) border border-border bg-card px-3.5 py-2.5 hover:bg-row-hover active:bg-muted"
      >
        <View class="flex-row items-center gap-1.5">
          <Icon name={props.icon} size={13} tint={props.tone === "warn" ? "warn" : "muted-foreground"} />
          <Text class="flex-1 text-caption font-medium text-muted-foreground line-clamp-1">{props.title}</Text>
          <Icon name="chevron-right" size={12} />
        </View>
        <If when={!props.pending} else={<View class="my-1 h-4 w-24 rounded-(--radius-sm) bg-muted" />}>
          <Text class={`text-heading font-semibold tabular-nums line-clamp-1 ${props.tone === "warn" ? "text-warn" : "text-foreground"}`}>{props.value}</Text>
        </If>
        <If when={!props.pending} else={<View class="h-3 w-32 rounded-(--radius-sm) bg-muted/70" />}>
          <Text class="text-caption text-muted-foreground line-clamp-1">{props.detail ?? ""}</Text>
        </If>
      </BaseButton>
    </View>
  );
}

/** The cards over a bucket's objects. */
export function Overview() {
  useFacts();
  const k = here;
  const mpu = () => k().mpu;
  const layouts = () => k().layouts;
  const v = () => k().versioning;
  const size = () => k().size;
  return (
    <View testID="overview" class="flex-row flex-wrap gap-2.5">
      <Tile
        id="capacity"
        icon="chart-pie"
        title={t("overview.size")}
        value={size() ? bytes(size()!.totalBytes) : t("overview.measure")}
        detail={size() ? `${money(size()!.costPerMonth, size()!.currency)}/${t("overview.month")} · ${size()!.scannedAt.slice(0, 10)}` : t("overview.measureHint")}
      />
      <If when={mpu() === undefined || (mpu() && (mpu()!.totalUploads > 0 || mpu()!.lifecycle.status !== "covered"))}>
        <Tile
          id="multipart"
          icon="layers"
          title={t("overview.uploads")}
          pending={mpu() === undefined}
          tone={mpu() && mpu()!.staleUploads > 0 ? "warn" : "plain"}
          value={mpu() ? (mpu()!.totalUploads > 0 ? `${mpu()!.totalUploads} · ${bytes(mpu()!.totalBytes)}` : t("overview.noUploads")) : ""}
          detail={mpu() ? (mpu()!.totalUploads > 0 ? `${t("mpu.stale", { n: mpu()!.staleUploads, days: mpu()!.thresholdDays })} · ${money(mpu()!.costPerMonth, mpu()!.currency)}/${t("overview.month")}` : t("overview.noRule")) : ""}
        />
      </If>
      <If when={v() === undefined || (v() && v()!.status !== "Unversioned" && v()!.status !== "")}>
        <Tile id="versions" icon="file-clock" title={t("overview.versioning")} pending={v() === undefined} value={v()?.status ?? ""} detail={v() ? (v()!.objectLock ? `${t("versions.lock")}: ${v()!.objectLock}` : t("overview.versionsHint")) : ""} />
      </If>
      <If when={layouts() === undefined || (layouts() && layouts()!.length > 0)}>
        <Tile
          id="layouts"
          icon="boxes"
          title={t("overview.layouts")}
          pending={layouts() === undefined}
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
