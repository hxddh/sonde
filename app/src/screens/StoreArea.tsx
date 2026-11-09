// Buckets: what is in a bucket. The endpoint's buckets on the left, as a
// tree whose folders are their prefixes (loaded as they open); on the right
// the chosen prefix's objects, under cards that say what sonde noticed —
// each opens its detail (incomplete uploads, versions, size, layouts) in a
// sheet. A terminal keeps them as tabs.

import { batch, Clipboard, effect, resource, signal, Text, untracked, View } from "nsh";
import { ResizablePanel, ResizablePanelGroup, Tree, type TreeItem, type TreeRef } from "nsh/base";
import { isAvailable, Menu } from "nsh/desktop";
import { t } from "../i18n";
import { sectionLabel } from "../labels";
import { api, Listing } from "../native";
import { bucket, chooseBucket, endpoint, go, notify, prefix, report, STORE_TABS, type StoreTab, storeSheet, storeTab } from "../store";
import type { BucketEntry, Entry } from "../types";
import { Icon, type IconName, iconSvg } from "../ui/icon";
import { IconButton, Loading, PanelHeader, SidePanel, SPLIT_HANDLE, terminal } from "../ui/kit";
import { rememberedSize } from "../ui/sizes";
import { TabBar } from "../ui/tabs";
import { Browse } from "./Browse";
import { Capacity } from "./Capacity";
import { Layouts } from "./Layouts";
import { Multipart } from "./Multipart";
import { Probes } from "./Probes";
import { Versions } from "./Versions";

const TAB_ICON: Record<StoreTab, IconName> = { browse: "folder", multipart: "layers", versions: "file-clock", capacity: "chart-pie", layouts: "boxes", probes: "flask-conical" };

function screen(s: StoreTab) {
  switch (s) {
    case "browse":
      return <Browse />;
    case "multipart":
      return <Multipart />;
    case "versions":
      return <Versions />;
    case "capacity":
      return <Capacity />;
    case "layouts":
      return <Layouts />;
    case "probes":
      return <Probes />;
  }
}

const bucketId = (b: string) => `b:${b}`;
const prefixId = (b: string, p: string) => `p:${b}/${p}`;
const LOADING = "loading";

/** The prefixes directly under `p` in a bucket, from its delimited listing:
 * past the first page too (folders sort among the keys), five pages at most. */
async function prefixesOf(ep: string, b: string, p: string): Promise<Entry[]> {
  const l = new Listing(ep, b, p, "/");
  try {
    for (let page = 0; page < 5 && !l.done; page++) await l.loadMore();
    return (l.rows(0, l.count) as Entry[]).filter((e) => e.isPrefix);
  } finally {
    l.dispose();
  }
}

/** The endpoint's buckets as a tree: a bucket's folders are its prefixes, fetched when it opens. */
function Buckets() {
  const buckets = resource(endpoint, async (ep) => {
    try {
      return await api.buckets(ep);
    } catch (e) {
      report(e);
      return [] as BucketEntry[];
    }
  });
  const children = signal<Record<string, Entry[] | typeof LOADING>>({});
  const expanded = signal<string[]>([]);
  let tree: TreeRef | undefined;

  // another endpoint: another tree
  effect(() => {
    endpoint();
    untracked(() => batch(() => (children.set({}), expanded.set([]))));
  });
  // no bucket open yet: the first, rather than a page asking for one
  effect(() => {
    const list = buckets();
    if (!list?.length) return;
    untracked(() => {
      if (!bucket() || !list.some((b) => b.name === bucket())) chooseBucket(list[0].name);
    });
  });

  const load = async (id: string) => {
    const ep = endpoint();
    if (!ep || children()[id]) return;
    const [b, p] = id.startsWith("b:") ? [id.slice(2), ""] : [id.slice(2, id.indexOf("/")), id.slice(id.indexOf("/") + 1)];
    children.set({ ...children(), [id]: LOADING });
    try {
      const list = await prefixesOf(ep, b, p);
      children.set({ ...children(), [id]: list });
    } catch (e) {
      report(e);
      const { [id]: _, ...rest } = children();
      children.set(rest);
    }
  };

  const node = (b: string, e: Entry): TreeItem => {
    const id = prefixId(b, e.key);
    const kids = children()[id];
    return {
      id,
      label: e.name.replace(/\/$/, ""),
      children: kids === undefined || kids === LOADING ? [{ id: `${id}#…`, label: t("common.loading"), disabled: true }] : kids.length ? kids.map((k) => node(b, k)) : undefined,
    };
  };
  const items = (): TreeItem[] =>
    (buckets() ?? []).map((bk) => {
      const id = bucketId(bk.name);
      const kids = children()[id];
      return {
        id,
        label: bk.name,
        children: kids === undefined || kids === LOADING ? [{ id: `${id}#…`, label: t("common.loading"), disabled: true }] : kids.length ? kids.map((k) => node(bk.name, k)) : undefined,
      };
    });

  const selected = () => {
    const b = bucket();
    if (!b) return null;
    return prefix() ? prefixId(b, prefix()) : bucketId(b);
  };
  const choose = (id: string) => {
    if (id.includes("#")) return;
    if (id.startsWith("b:")) {
      chooseBucket(id.slice(2));
    } else {
      const b = id.slice(2, id.indexOf("/"));
      batch(() => {
        if (bucket() !== b) chooseBucket(b);
        prefix.set(id.slice(id.indexOf("/") + 1));
      });
    }
  };
  // a prefix opened from the objects shows in the tree
  effect(() => {
    const id = selected();
    if (id && id.startsWith("p:")) untracked(() => tree?.revealItem(id));
  });

  const menu = (b: string) => {
    if (!isAvailable) return;
    void Menu.popup([
      { label: t("store.copyName"), icon: iconSvg("copy"), onSelect: () => void Clipboard.writeText(b).then(() => notify(t("common.copied")), report) },
      { label: t("store.copyUri"), icon: iconSvg("link"), onSelect: () => void Clipboard.writeText(`s3://${b}/`).then(() => notify(t("common.copied")), report) },
      { type: "separator" },
      ...(["multipart", "capacity", "layouts"] as StoreTab[]).map((s) => ({ label: sectionLabel(s), onSelect: () => (chooseBucket(b), go(s)) })),
    ]);
  };

  return (
    <View class="flex-1 bg-sidebar/40">
      <View class="h-10 flex-row items-center gap-2 px-3">
        <Text role="header" class="flex-1 text-small font-semibold text-muted-foreground">
          {t("browse.buckets")}
        </Text>
        <IconButton icon="refresh-cw" label={t("common.refresh")} onPress={() => buckets.refetch()} />
      </View>
      <If when={buckets.state() === "pending"}>
        <Loading class="px-3" />
      </If>
      <If when={buckets.state() !== "pending" && (buckets() ?? []).length === 0}>
        <Text class="px-3 text-small text-muted-foreground">{t("store.noBuckets")}</Text>
      </If>
      <Tree
        ref={(r) => (tree = r)}
        testID="bucket-tree"
        accessibilityLabel={t("browse.buckets")}
        items={items()}
        expanded={expanded()}
        selected={selected()}
        reveal
        onExpandedChange={({ expanded: next }) => expanded.set(next)}
        onExpand={({ id }) => void load(id)}
        onSelectedChange={({ selected: id }) => choose(id)}
        class="min-h-0 flex-1"
        listClass="px-1.5 pb-2"
        itemClass="group h-7 rounded-(--radius-sm) hover:bg-row-hover selected:bg-selection disabled:opacity-60"
        renderItem={(entry) => {
          const isBucket = entry.id.startsWith("b:");
          const name = entry.label;
          return (
            <View
              testID={isBucket ? `bucket-${name}` : undefined}
              onContextMenu={isBucket ? () => menu(name) : undefined}
              class="h-7 flex-row items-center gap-1.5 pr-2"
              style={{ paddingLeft: 4 + entry.depth * 14 }}
            >
              <View class="w-3.5 items-center">
                <If when={entry.folder}>
                  <Icon name={expanded().includes(entry.id) ? "chevron-down" : "chevron-right"} size={12} />
                </If>
              </View>
              <Icon name={isBucket ? "database" : expanded().includes(entry.id) ? "folder-open" : "folder"} size={14} tint={isBucket ? "foreground" : "muted-foreground"} />
              <Text class={`min-w-0 flex-1 text-body line-clamp-1 ${entry.id.includes("#") ? "text-muted-foreground" : "text-foreground"}`}>{name}</Text>
            </View>
          );
        }}
      />
    </View>
  );
}

/** A detail of the bucket, from the edge of the window. */
function DetailSheet() {
  const close = () => storeSheet.set(null);
  return (
    <SidePanel open={storeSheet() !== null} onClose={close} size="wide">
      <If when={storeSheet()}>
        {(s) => (
          <View class="flex-1" testID="store-sheet">
            <PanelHeader icon={TAB_ICON[s()]} title={sectionLabel(s())} subtitle={`${bucket() ?? ""}/${prefix()}`} onClose={close} closeTestID="sheet-close" />
            {/* rebuilt per detail: each owns its jobs */}
            <Each of={[s()]} key={(x) => x}>
              {(x) => <View class="min-h-0 flex-1">{screen(x())}</View>}
            </Each>
          </View>
        )}
      </If>
    </SidePanel>
  );
}

const bucketsWidth = rememberedSize("store.buckets", 240);

export function StoreArea() {
  const tabs = () => (
    <TabBar tabs={STORE_TABS} value={storeTab()} icon={(s) => TAB_ICON[s]} label={(s) => (s === "browse" ? t("nav.objects") : sectionLabel(s))} onChange={(s) => go(s)} />
  );
  return (
    <If
      when={terminal()}
      else={
        <View class="flex-1" testID="screen-store">
          <ResizablePanelGroup class="flex-1" handleClass={SPLIT_HANDLE} handleAccessibilityLabel={t("store.resizeBuckets")}>
            <ResizablePanel fixed size={bucketsWidth.size()} minSize={180} maxSize={400} onResize={bucketsWidth.onResize}>
              <Buckets />
            </ResizablePanel>
            <ResizablePanel minSize={560} class="min-w-0">
              <Browse />
            </ResizablePanel>
          </ResizablePanelGroup>
          <DetailSheet />
        </View>
      }
    >
      <View class="flex-1" testID="screen-store">
        {tabs()}
        <View class="flex-1">{screen(storeTab())}</View>
      </View>
    </If>
  );
}
