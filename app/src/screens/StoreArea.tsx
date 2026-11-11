// Buckets: what is in a bucket. The endpoint's buckets in the sidebar, as a
// tree whose folders are their prefixes (loaded as they open); beside it
// the chosen prefix's objects, under cards that say what sonde noticed —
// each opens its detail (incomplete uploads, versions, size, layouts) in a
// sheet. A terminal keeps them as tabs.

import { batch, cleanup, effect, resource, signal, Text, untracked, View } from "nsh";
import { Tree, type TreeItem, type TreeRef } from "nsh/base";
import { isAvailable, Menu } from "nsh/desktop";
import { t } from "../i18n";
import { sectionLabel } from "../labels";
import { api, asError, Listing } from "../native";
import { bucket, chooseBucket, copy, endpoint, go, prefix, report, showInSidebar, STORE_TABS, type StoreTab, storeSheet, storeTab } from "../store";
import type { Entry } from "../types";
import { Icon, type IconName, iconSvg } from "../ui/icon";
import { Button, IconButton, Loading, Notice, PanelHeader, ROWS, Segmented, SideHeader, SidePanel, terminal } from "../ui/kit";
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

/** A folder not read yet holds one row that says so (a Tree item is a folder only while it has children). */
const WAITING = "\u0000…";
const waiting = (id: string) => id.endsWith(WAITING);

/** The endpoint's buckets as a tree: a bucket's folders are its prefixes,
 * fetched when it opens. The state lives with the place; the tree itself is
 * drawn in the sidebar, under the places. */
function useBuckets() {
  const buckets = resource(endpoint, (ep) => api.buckets(ep));
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
    const list = buckets.error() ? null : buckets();
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
      // the endpoint changed while this was read: its tree is another's
      if (endpoint() !== ep) return;
      children.set({ ...children(), [id]: list });
    } catch (e) {
      if (endpoint() !== ep) return;
      report(e);
      const { [id]: _, ...rest } = children();
      children.set(rest);
    }
  };
  /** The buckets again, and every folder read again as it is opened. */
  const refresh = () => {
    const open = expanded();
    batch(() => children.set({}));
    buckets.refetch();
    for (const id of open) void load(id);
  };

  const kidsOf = (b: string, id: string): TreeItem[] | undefined => {
    const kids = children()[id];
    if (kids === undefined || kids === LOADING) return [{ id: `${id}${WAITING}`, label: t("common.loading"), disabled: true }];
    return kids.length ? kids.map((k) => node(b, k)) : undefined;
  };
  const node = (b: string, e: Entry): TreeItem => {
    const id = prefixId(b, e.key);
    return { id, label: e.name.replace(/\/$/, ""), children: kidsOf(b, id) };
  };
  const items = (): TreeItem[] => (buckets.error() ? [] : (buckets() ?? [])).map((bk) => ({ id: bucketId(bk.name), label: bk.name, children: kidsOf(bk.name, bucketId(bk.name)) }));

  const selected = () => {
    const b = bucket();
    if (!b) return null;
    return prefix() ? prefixId(b, prefix()) : bucketId(b);
  };
  const choose = (id: string) => {
    if (waiting(id)) return;
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
      { label: t("store.copyName"), icon: iconSvg("copy"), onSelect: () => copy(b) },
      { label: t("store.copyUri"), icon: iconSvg("link"), onSelect: () => copy(`s3://${b}/`) },
      { type: "separator" },
      ...(["multipart", "capacity", "layouts"] as StoreTab[]).map((s) => ({ label: sectionLabel(s), onSelect: () => (chooseBucket(b), go(s)) })),
    ]);
  };

  const list = () => (
    <View class="flex-1">
      <SideHeader title={t("browse.buckets")}>
        <IconButton icon="refresh-cw" label={t("common.refresh")} onPress={refresh} testID="buckets-refresh" />
      </SideHeader>
      {/* on their way, failed (why, and again), none, or the tree: each said truly */}
      <If when={buckets.error()}>
        <View class="gap-2 px-3">
          <Notice tone="error" text={asError(buckets.error()).message} testID="buckets-error" />
          <Button small variant="outline" icon="refresh-cw" label={t("error.retry")} onPress={refresh} class="self-start" />
        </View>
      </If>
      <If when={!buckets.error() && buckets.state() === "pending"}>
        <Loading class="px-4" />
      </If>
      <If when={!buckets.error() && buckets.state() !== "pending" && (buckets() ?? []).length === 0}>
        <Text class="px-4 text-small text-muted-foreground">{t("store.noBuckets")}</Text>
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
        listClass="px-2 pb-2"
        // the tree marks its focused row itself; a ring would be cut by the rows around it
        itemClass={ROWS.tree}
        renderItem={(entry) => {
          const isBucket = entry.id.startsWith("b:");
          const name = entry.label;
          return (
            <View
              testID={isBucket ? `bucket-${name}` : undefined}
              onContextMenu={isBucket ? () => menu(name) : undefined}
              class="h-7 flex-row items-center gap-2 pr-2"
              style={{ paddingLeft: 4 + entry.depth * 14 }}
            >
              <View class="w-3.5 items-center">
                <If when={entry.folder}>
                  <Icon name={expanded().includes(entry.id) ? "chevron-down" : "chevron-right"} size={12} />
                </If>
              </View>
              <Icon name={isBucket ? "database" : expanded().includes(entry.id) ? "folder-open" : "folder"} tint={isBucket ? "foreground" : "muted-foreground"} />
              <Text class={`min-w-0 flex-1 text-body line-clamp-1 ${waiting(entry.id) ? "text-muted-foreground" : "text-foreground"}`}>{name}</Text>
            </View>
          );
        }}
      />
    </View>
  );
  // the list is held open only for what is said only there: a listing that failed, an endpoint
  // with no bucket. Not while the buckets are on their way (an endpoint switch leaves no bucket
  // chosen for a moment: the sidebar would flash open and shut)
  const needed = () => !!endpoint() && (!!buckets.error() || (buckets.state() !== "pending" && !bucket() && (buckets() ?? []).length === 0));
  return { list, needed };
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
            {/* rebuilt per detail and per place (endpoint, bucket, prefix): each owns its jobs, and
                another bucket's report is never left under this one's name */}
            <Each of={[{ tab: s(), at: `${s()}\n${endpoint()}\n${bucket()}\n${prefix()}` }]} key={(x) => x.at}>
              {(x) => <View class="min-h-0 flex-1">{screen(x().tab)}</View>}
            </Each>
          </View>
        )}
      </If>
    </SidePanel>
  );
}

export function StoreArea() {
  if (terminal()) {
    return (
      <View class="flex-1" testID="screen-store">
        <View class="flex-row px-1">
          <Segmented testID="tab" value={storeTab()} items={STORE_TABS.map((s) => ({ value: s, title: s === "browse" ? t("nav.objects") : sectionLabel(s) }))} onChange={(s) => go(s as StoreTab)} />
        </View>
        <View class="flex-1">{screen(storeTab())}</View>
      </View>
    );
  }
  // the buckets are the sidebar's list while this place shows
  const { list, needed } = useBuckets();
  cleanup(showInSidebar("store", list, needed));
  return (
    <View class="flex-1" testID="screen-store">
      <Browse />
      <DetailSheet />
    </View>
  );
}
