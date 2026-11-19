// Browse: prefixes and objects of a bucket (ListObjectsV2, loaded page by
// page and kept in Rust), and an object's details.

import { batch, cleanup, computed, effect, resource, Scroll, signal, Text, untracked, View, type ViewRef } from "nsh";
import { DataTable, type DataTableRef, ResizablePanel, ResizablePanelGroup } from "nsh/base";
import { isAvailable } from "nsh/desktop";
import { bytes, count, date } from "../format";
import { t } from "../i18n";
import { core } from "../phrases";
import { api, Listing, watch } from "../native";
import { bucket, copy, endpoint, notify, offerCommands, type PlaceCommand, prefix, report } from "../store";
import { KeyVersions } from "./Versions";
import type { Entry, ObjectDetail } from "../types";
import { Icon, type IconName } from "../ui/icon";
import { titleCells, Await, Back, ProgressBar, Button, Card, Choice, Disclosure, Empty, fitColumns, ICON, IconButton, InWindowMenu, KeyValue, ListRow, More, PanelHeader, Path, popupMenu, Strong, Title, ROW, rowHeight, SearchField, SPLIT_HANDLE, BARE_TABLE, terminal, TYPE, WIDTH, Crumb, Row, COLUMN, PANE, TEXT } from "../ui/kit";
import { rememberedSize } from "../ui/sizes";
import { invalidate, useBucketFacts } from "../ui/overview";
import { ShowSidebar } from "../ui/shell";
import { BucketPicker, EndpointPicker } from "../ui/picker";

const WINDOW = 400;

/** The objects' columns by rank: the name (which takes the room the others
 * leave, `NAME` at least) and the size always; then Modified, Class and
 * Parts while they fit — Parts goes first, then Class, then Modified, each
 * whole, none clipped. */
const NAME = 240;
const OBJECT_COLUMNS = [
  // (a date in full, in every language: "Sep 30, 2026, 12:59 PM" is the widest)
  { key: "modified", width: COLUMN.date },
  { key: "class", width: COLUMN.storageClass },
  { key: "parts", width: COLUMN.objectParts },
] as const;
const SIZE = 96;

function Breadcrumbs() {
  const parts = () => {
    const p = prefix();
    // every segment, empty ones too (`a//b/` is its own place): each crumb's prefix is the key's own
    const segs = p.endsWith("/") ? p.slice(0, -1).split("/") : p.split("/");
    return p ? segs.map((s, i) => ({ name: s || "∅", prefix: `${segs.slice(0, i + 1).join("/")}/` })) : [];
  };
  return (
    <View class="min-w-0 flex-row flex-wrap items-center gap-1">
      {/* links of the path (the last one the place shown): screen readers hear them so */}
      {/* the scope beside the section's title: the bucket, then each folder of the prefix (the last the place shown) */}
      <Crumb testID="crumb-root" href={`s3://${bucket()}/`} onOpen={() => prefix.set("")} current={parts().length === 0} label={bucket() ?? ""}>
        <Text class={parts().length === 0 ? TEXT.body : TEXT.bodyQuiet}>{bucket()}</Text>
      </Crumb>
      <Each of={parts()}>
        {(p, i) => (
          <View class="flex-row items-center gap-1">
            <If when={!terminal()} else={<Text class="text-muted-foreground">/</Text>}>
              <Icon name="chevron-right" size={ICON.small} />
            </If>
            <Crumb href={`s3://${bucket()}/${p().prefix}`} onOpen={() => prefix.set(p().prefix)} current={i() === parts().length - 1} label={p().name}>
              <Text class={i() === parts().length - 1 ? `${TEXT.body}` : `${TEXT.bodyQuiet}`}>{p().name}</Text>
            </Crumb>
          </View>
        )}
      </Each>
    </View>
  );
}

/** An object in the inspector: its address, what can be done with it (Download, Versions, and behind ⋯ what
 * copies it), then one list of what it is — its ETag, checksums and encryption where it has them — and its
 * response headers as the one section that opens. Versions opens in its place, with the way back. */
function Detail(props: { entry: Entry; onBack?: () => void; /** open on its versions */ versions?: boolean }) {
  const ep = endpoint()!;
  const b = bucket()!;
  // a failure shows in the panel, with Try again: not a wait that never ends
  const detail = resource(
    () => props.entry.key,
    (key) => api.objectDetail(ep, b, key),
  );
  const versions = signal(props.versions ?? false);
  const progress = signal<string | null>(null);
  let downloadJob: string | null = null;
  let gone = false;
  cleanup(() => {
    gone = true;
    // another object chosen, or the panel closed: the download stops, and says so
    if (downloadJob) {
      api.cancelJob(downloadJob);
      notify(t("browse.downloadStopped"));
    }
  });
  const download = async () => {
    // busy from the press, not from the job's start: a second press starts nothing
    if (progress() !== null) return;
    progress.set(t("common.loading"));
    try {
      const job = await api.download(ep, b, props.entry.key);
      // started after the panel went: stopped at once, nothing left running
      if (gone) return void api.cancelJob(job);
      downloadJob = job;
      const r = await watch<{ path: string; bytes: number }>(job, (e) => {
        if (e.type === "Progress") progress.set(`${bytes(e.progress.done)}${e.progress.total ? ` / ${bytes(e.progress.total)}` : ""}`);
      });
      notify(t("common.exportedTo", { path: r.path }));
    } catch (e) {
      report(e);
    }
    // ended either way: nothing left to stop when the panel closes
    downloadJob = null;
    progress.set(null);
  };
  // a link anyone can open for an hour, copied
  const presign = async () => {
    try {
      copy(await api.presign(ep, b, props.entry.key, 3600), t("browse.presignCopied"));
    } catch (e) {
      report(e);
    }
  };
  const etagText = (d: ObjectDetail) => {
    const k = d.etag?.kind;
    if (!k) return null;
    if (k.kind === "multipart") return t("browse.multipartParts", { n: k.parts });
    if (k.kind === "md5") return t("browse.singlePart");
    return t("browse.opaqueEtag");
  };
  // said once: "512 B", "1.5 KiB (1,536 B)", "—" for a size not reported
  const sizeText = (n: number | null) => (n === null || n === undefined ? "—" : n < 1024 ? bytes(n) : `${bytes(n)} (${count(n)} B)`);
  const uri = () => `s3://${b}/${props.entry.key}`;
  // what it is, in one list: each row there only where the object has it
  const rows = (d: ObjectDetail): [string, string][] => {
    const has = (k: string, v: string | null | undefined): [string, string][] => (v ? [[k, v]] : []);
    const sse = d.sse.customerAlgorithm ? `SSE-C (${d.sse.customerAlgorithm})` : d.sse.algorithm;
    return [
      [t("browse.size"), sizeText(d.size)],
      ...has("Content-Type", d.contentType),
      [t("browse.modified"), date(d.lastModified)],
      [t("browse.storageClass"), d.storageClass],
      ...has(t("browse.version"), d.versionId),
      ...has(t("browse.etag"), d.etag?.value),
      ...has(t("browse.uploadKind"), etagText(d)),
      ...has(t("browse.partSizes"), d.partSizeGuesses.length > 0 ? d.partSizeGuesses.map((s) => bytes(s)).join(", ") : null),
      ...d.checksums.map((c): [string, string] => [c.algorithm, `${c.value} · ${c.kind === "composite" ? t("browse.composite", { n: c.parts ?? "?" }) : t("browse.fullObject")}${c.kindFromHeader ? "" : ` · ${t("browse.inferred")}`}`]),
      // what it lacks is said too: no checksum stored, no encryption reported (what a compatibility check asks)
      ...(d.checksums.length === 0 ? [[t("browse.checksum"), t("browse.noChecksum")] as [string, string]] : []),
      ["SSE", sse || t("browse.noSse")],
      ...has(t("browse.kmsKey"), d.sse.kmsKeyId),
      ...has(t("browse.bucketKey"), d.sse.bucketKeyEnabled === null || d.sse.bucketKeyEnabled === undefined ? null : d.sse.bucketKeyEnabled ? t("common.on") : t("common.off")),
      ...has(t("browse.objectLock"), d.objectLock),
      ...has(t("browse.restore"), d.restore),
      // the object's own metadata by its header's name, as it was set: never mistaken for one of the facts above
      ...d.metadata.map(([k, v]): [string, string] => [`x-amz-meta-${k}`, v]),
    ];
  };
  // what can be done with it: on its row, in the palette, and on their shortcuts
  const commands = (): PlaceCommand[] => [
    { id: "object-download", label: t("browse.download"), icon: "download", shortcut: "mod+s", disabled: progress() !== null, run: download },
    { id: "object-versions", label: t("browse.versionsOpen"), icon: "file-clock", shortcut: "mod+y", disabled: versions(), run: () => versions.set(true) },
    { id: "object-uri", label: t("store.copyUri"), icon: "copy", shortcut: "mod+shift+c", run: () => copy(uri()) },
    { id: "object-presign", label: t("browse.presignCopy"), icon: "link", run: () => void presign() },
  ];
  offerCommands(() => props.entry.name || props.entry.key, commands);
  return (
    <If when={!versions()} else={<KeyVersions objectKey={props.entry.key} onBack={() => versions.set(false)} />}>
      <View selectable class="gap-4" testID="object-detail">
        <View class="gap-1">
          {/* in the panel, its header names the object; on its own (a terminal), this does */}
          <If when={props.onBack}>
            <View class="flex-row items-center gap-2">
              <Back onPress={() => props.onBack?.()} testID="detail-back" />
              <Title lines={2} class="min-w-0 flex-1">{props.entry.name}</Title>
            </View>
          </If>
          <Text class={`${TEXT.codeQuiet} line-clamp-2`}>{uri()}</Text>
        </View>
        {/* what takes the object, what opens its history; behind ⋯ what copies it */}
        <View class="flex-row items-center gap-2">
          <Button small variant="outline" icon="download" label={t("browse.download")} shortcut="mod+s" onPress={download} disabled={progress() !== null} testID="download" />
          <Button small variant="outline" icon="file-clock" label={t("browse.versionsOpen")} shortcut="mod+y" onPress={() => versions.set(true)} testID="object-versions" />
          <More
            testID="object-more"
            actions={() => [
              { label: t("store.copyUri"), icon: "copy", run: () => copy(uri()) },
              { label: t("browse.presignCopy"), icon: "link", run: () => void presign() },
            ]}
          />
        </View>
        <If when={progress()}>{(p) => <ProgressBar value={null} label={p()} />}</If>
        <Await loading={detail.loading() || !detail()} error={detail.error()} retry={() => detail.refetch()} waiting={t("browse.readingObject")} testID="object">
          <If when={detail()}>
            {(d) => (
              <View class="gap-4">
                <KeyValue dense rows={rows(d())} onCopy={(v) => copy(v)} />
                <Disclosure title={t("browse.headers")} count={d().headers.length}>
                  <KeyValue dense mono rows={d().headers.map(([k, v]) => [k, v] as [string, string])} onCopy={(v) => copy(v)} />
                </Disclosure>
              </View>
            )}
          </If>
        </Await>
      </View>
    </If>
  );
}

/** How many search hits the list holds; past them, a line says how many more. */
const HITS = 50;

function Objects(props: { /** an object chosen: its inspector, on its versions with `versions` */ onSelect: (e: Entry, versions?: boolean) => void; selected: string | null }) {
  const listing = signal<Listing | null>(null);
  const version = signal(0);
  const start = signal(0);
  const loading = signal(false);
  // why the last page could not be read: nothing more is asked for until Refresh
  const failure = signal<string | null>(null);
  const filter = signal("");
  let filterField: ViewRef | undefined;
  const searchHits = signal<Entry[] | null>(null);
  const searchState = signal<string | null>(null);
  const flat = signal(false);
  const loadingAll = signal(false);
  // a press on Refresh lists the same place again
  const reload = signal(0);
  let table: DataTableRef | undefined;
  // the table's own width decides which columns it shows
  const width = signal(0);
  const ranked = () => fitColumns(terminal() ? 0 : width(), SIZE + 2, NAME, [...OBJECT_COLUMNS]);
  let searchJob: string | null = null;
  const stopSearch = () => {
    if (searchJob) api.cancelJob(searchJob);
    searchJob = null;
  };

  effect(() => {
    const ep = endpoint();
    const b = bucket();
    const p = prefix();
    reload();
    // a search of the place before ends with it
    untracked(stopSearch);
    if (!ep || !b) return;
    const l = new Listing(ep, b, p, flat() ? null : "/");
    batch(() => {
      listing.set(l);
      start.set(0);
      filter.set("");
      // a search belongs to its place: its state goes with it
      searchState.set(null);
      searchHits.set(null);
      loading.set(false);
      failure.set(null);
    });
    // the row chosen was the place before's
    table?.select(null);
    untracked(() => void load(l));
    cleanup(() => l.dispose());
  });
  cleanup(stopSearch);
  // the open object's row stays chosen while its panel is open, and no longer
  effect(() => {
    if (props.selected === null) table?.select(null);
  });

  // the listing a page is being read for: another place's never blocks this one
  let reading: Listing | null = null;
  async function load(l: Listing) {
    if (reading === l || l.done) return;
    reading = l;
    loading.set(true);
    let failed: string | null = null;
    try {
      await l.loadMore();
    } catch (e) {
      failed = report(e).message;
    }
    if (reading === l) {
      reading = null;
      // the place shown now is this listing's: its state, not another's
      if (listing() === l)
        batch(() => {
          loading.set(false);
          failure.set(failed);
        });
    }
    version.update((v) => v + 1);
  }

  // the rows held: read once per change, whatever asks (the table, its cells)
  const rows = computed(() => {
    version();
    const l = listing();
    if (!l) return [] as Entry[];
    return l.rows(start(), WINDOW) as Entry[];
  });
  const total = () => {
    version();
    return listing()?.count ?? 0;
  };
  const done = () => {
    version();
    return listing()?.done ?? true;
  };
  const tableData = () =>
    rows().map((e) => ({
      // a terminal has no icons: a folder says so with a mark of its own (what a row is, is the entry's `isPrefix`)
      name: terminal() && e.isPrefix ? `▸ ${e.name}` : e.name,
      size: e.isPrefix ? "" : bytes(e.size),
      modified: e.isPrefix ? "" : date(e.lastModified),
      class: e.storageClass ?? "",
      parts: e.parts ? count(e.parts) : "",
    }));

  const loadAll = async () => {
    const l = listing();
    if (!l || loadingAll()) return;
    loadingAll.set(true);
    try {
      await l.loadAll(1_000_000);
    } catch (e) {
      report(e);
    }
    loadingAll.set(false);
    version.update((v) => v + 1);
  };
  const refresh = () => {
    // the listing again, and what the tiles over it say
    invalidate();
    reload.update((n) => n + 1);
  };

  const open = (index: number) => {
    const e = listing()?.row(index) as Entry | null;
    if (!e) return;
    if (e.isPrefix) prefix.set(e.key);
    else props.onSelect(e);
  };

  const search = async () => {
    const ep = endpoint();
    const b = bucket();
    if (!ep || !b || !filter()) return;
    stopSearch();
    searchHits.set([]);
    const where = `${ep}|${b}|${prefix()}`;
    const here = () => `${endpoint()}|${bucket()}|${prefix()}` === where;
    try {
      const job = await api.search(ep, b, prefix(), filter());
      searchJob = job;
      const r = await watch<{ matches: Entry[]; scanned: number }>(job, (ev) => {
        if (!here() || searchJob !== job) return;
        if (ev.type === "Progress") searchState.set(core(ev.progress.messageText, ev.progress.message));
        if (ev.type === "Partial") searchHits.update((h) => [...(h ?? []), ...(ev.data as Entry[])]);
      });
      // what another place's search found is not shown here
      if (!here() || searchJob !== job) return;
      batch(() => {
        searchHits.set(r.matches);
        searchState.set(t("browse.scanned", { n: count(r.scanned) }));
      });
      searchJob = null;
    } catch (e) {
      if (here()) report(e);
    }
  };

  const applyFilter = (v: string) => {
    filter.set(v);
    listing()?.setFilter(v);
    batch(() => {
      start.set(0);
      version.update((x) => x + 1);
    });
  };
  const entryAt = (row: number) => listing()?.row(row) as Entry | null;
  type Action = { label: string; icon: IconName; run: () => void };
  /** What a row offers: in the system's menu, or Native Shell's own where there is none. */
  const actions = (row: number | null): Action[] => {
    const e = row === null ? null : entryAt(row);
    const b = bucket();
    if (!e || !b) return [];
    const copies: Action[] = [
      { label: t("store.copyKey"), icon: "copy", run: () => copy(e.key) },
      { label: t("store.copyUri"), icon: "link", run: () => copy(`s3://${b}/${e.key}`) },
    ];
    return e.isPrefix
      ? [{ label: t("browse.open"), icon: "folder-open", run: () => prefix.set(e.key) }, ...copies]
      : [{ label: t("browse.detail"), icon: "info", run: () => props.onSelect(e) }, ...copies, { label: t("browse.versionsOpen"), icon: "file-clock", run: () => props.onSelect(e, true) }];
  };
  const menuRow = signal<number | null>(null);
  // the system's menus, a framed box in a terminal; the in-window one on the web
  const systemMenus = isAvailable;
  const menu = (row: number) => {
    menuRow.set(row);
    if (!systemMenus) return;
    void popupMenu(actions(row));
  };

  // the section's commands: in the palette and on their shortcuts, as on its row
  const up = () => {
    const p = prefix().replace(/[^/]*\/$/, "");
    prefix.set(p);
  };
  offerCommands(
    () => t("nav.objects"),
    () => [
      { id: "objects-filter", label: t("browse.filter"), icon: "search", shortcut: "mod+f", run: () => filterField?.focus() },
      { id: "objects-search", label: t("browse.searchAll"), icon: "search", shortcut: "mod+enter", disabled: !filter(), run: () => void search() },
      { id: "objects-up", label: t("browse.up"), icon: "chevron-left", shortcut: "mod+up", disabled: !prefix(), run: up },
      { id: "objects-flat", label: flat() ? t("browse.byFolder") : t("browse.flatListing"), icon: "list-filter", shortcut: "mod+shift+f", run: () => flat.set(!flat()) },
      { id: "objects-load-all", label: t("browse.loadAll"), icon: "download", shortcut: "mod+shift+l", disabled: done() || loadingAll(), run: () => void loadAll() },
      { id: "objects-refresh", label: t("common.refresh"), icon: "refresh-cw", shortcut: "mod+shift+r", run: refresh },
      { id: "bucket-copy-name", label: t("store.copyName"), icon: "copy", run: () => copy(bucket() ?? "") },
      { id: "bucket-copy-uri", label: t("store.copyUri"), icon: "link", run: () => copy(`s3://${bucket() ?? ""}/${prefix()}`) },
    ],
  );
  // the listing's columns: the name takes what the others leave
  const objectColumns = () => [
    { key: "name", title: t("browse.name"), width: terminal() ? 44 * 8 : width() > 0 ? Math.max(NAME, width() - SIZE - 2 - ranked().reduce((n, k) => n + (OBJECT_COLUMNS.find((c) => c.key === k)?.width ?? 0), 0)) : COLUMN.name, minWidth: COLUMN.keyLeast },
    { key: "size", title: t("browse.size"), width: SIZE, align: "end" as const },
    ...(ranked().includes("modified") ? [{ key: "modified", title: t("browse.modified"), width: COLUMN.date }] : []),
    ...(ranked().includes("class") ? [{ key: "class", title: t("browse.class"), width: COLUMN.storageClass }] : []),
    ...(ranked().includes("parts") ? [{ key: "parts", title: t("mpu.parts"), width: COLUMN.objectParts, align: "end" as const }] : []),
    ];
  return (
    <View class="min-h-0 flex-1 gap-2">
      {/* one row, one middle: where this is, then how much of it, finding in it, how it lists, and again.
          A pane too narrow for both breaks between them, never inside either. */}
      <View class="flex-row flex-wrap items-center gap-2">
        <View class="min-w-48 flex-1 flex-row items-center gap-2">
          {/* the head every section has: what it looks at (its tab above names it), then its actions at the end */}
          <Breadcrumbs />
        </View>
        {/* (too narrow for both, beside the inspector: they go to a line of their own, where the filter gives way;
            how it lists is behind ⋯, as on its shortcut, so the line never wraps) */}
        <View class="ms-auto min-w-0 shrink flex-row items-center justify-end gap-2">
          <Text class="shrink-0 text-small tabular-nums text-muted-foreground" testID="listing-count">
            {t("browse.objects", { n: count(total()) })}
            {done() ? "" : ` · ${t("browse.more")}`}
          </Text>
          {/* one mode: it narrows what is listed (every key under the prefix is searched from the empty list's row) */}
          <SearchField ref={(r) => (filterField = r)} small class={`${WIDTH.long} min-w-0 shrink`} testID="listing-filter" value={filter()} placeholder={t("browse.filter")} label={t("browse.filterLabel")} onChange={applyFilter} />
          {/* nothing to load once every row is here */}
          <If when={!done()}>
            <Button small variant="outline" label={t("browse.loadAll")} onPress={loadAll} disabled={loadingAll()} testID="load-all" />
          </If>
          <IconButton icon="refresh-cw" label={t("common.refresh")} shortcut="mod+shift+r" onPress={refresh} testID="listing-refresh" />
          {/* the bucket's own commands, as its row in the sidebar has them on a right press: reached by the keyboard here */}
          <More
            testID="bucket-more"
            actions={() => [
              { label: flat() ? t("browse.byFolder") : t("browse.flatListing"), icon: "list-filter", run: () => flat.set(!flat()) },
              { label: t("store.copyName"), icon: "copy", run: () => copy(bucket() ?? "") },
              { label: t("store.copyUri"), icon: "link", run: () => copy(`s3://${bucket() ?? ""}/${prefix()}`) },
            ]}
          />
        </View>
      </View>
      <View class="min-h-0 flex-1" onLayout={({ bounds }) => width.set(Math.round(bounds.size.width))}>
      {/* every key under the prefix searched: its matches in the listing's place, until closed */}
      <If when={searchHits()}>
        {(hits) => (
          <Card pad="none" testID="search-hits" class="min-h-0 flex-1">
            <Row first>
              <Text class={TEXT.body}>{t("browse.matches", { n: count(hits().length) })}</Text>
              <Text class={`min-w-0 flex-1 ${TEXT.note} line-clamp-1`}>{searchState() ?? ""}</Text>
              <IconButton icon="x" label={t("common.close")} onPress={() => (stopSearch(), searchHits.set(null))} testID="search-close" />
            </Row>
            <If when={hits().length > 0}>
              <Scroll class="min-h-0 flex-1" contentClass="p-1">
                <Each of={hits().slice(0, HITS)} key={(h) => h.key}>
                  {(h) => (
                    <ListRow label={h().key} selected={props.selected === h().key} onPress={() => (h().isPrefix ? prefix.set(h().key) : props.onSelect(h()))}>
                      <View class="shrink-0">
                        <Icon name={h().isPrefix ? "folder" : "file"} tint="muted-foreground" />
                      </View>
                      <Path path={h().key} max={96} class="min-w-0 flex-1 text-foreground" />
                    </ListRow>
                  )}
                </Each>
                <If when={hits().length > HITS}>
                  <Empty compact text={t("browse.moreMatches", { n: count(hits().length - HITS) })} />
                </If>
              </Scroll>
            </If>
          </Card>
        )}
      </If>
      <If when={!searchHits()}>
      {/* bare, as Runs' tables: the table fills its pane, its titles' hairline the one line over its rows */}
      <View class="min-h-0 flex-1">
        <InWindowMenu class="min-h-0 flex-1" enabled={!systemMenus} actions={() => actions(menuRow())}>
          <DataTable
            ref={(r) => (table = r)}
            testID="objects"
            accessibilityLabel={t("nav.objects")}
            class="min-h-0 flex-1"
            columns={objectColumns()}
            renderHeaderCell={terminal() ? undefined : titleCells(objectColumns)}
            data={tableData()}
            dataStart={start()}
            rowCount={total()}
            // a page that failed is not asked for again and again: Refresh asks
            hasMore={!done() && failure() === null}
            loading={loading()}
            rowSelectable
            resizableColumns
            fontSize={TYPE.body}
            rowHeight={rowHeight(ROW.table)}
            renderCell={
              terminal()
                ? undefined
                : (c) => {
                    if (c.key === "name") {
                      // what the row is comes from its entry, not from its text
                      const folder = rows()[c.row - start()]?.isPrefix ?? false;
                      return (
                        <View class="flex-1 flex-row items-center gap-2">
                          <Icon name={folder ? "folder" : "file"} tint={folder ? "foreground" : "muted-foreground"} />
                          {/* a name is cut in its middle, never at its end: both ends tell two apart */}
                          <Text class={`min-w-0 flex-1 ${TEXT.body} line-clamp-1`} style={{ textOverflow: "ellipsis-middle" }}>{c.text}</Text>
                        </View>
                      );
                    }
                    // sizes as figures at the end, under their title (no tabular digits in a cell: GTK cuts the last one); the rest quiet
                    return c.key === "size" || c.key === "parts" ? (
                      <View class="flex-1 items-end justify-center">
                        <Text class={TEXT.small}>{c.text}</Text>
                      </View>
                    ) : (
                      <Text class={`${TEXT.note} line-clamp-1`}>{c.text}</Text>
                    );
                  }
            }
            onContextMenu={(e) => {
              if (e.row !== undefined && e.row !== null) menu(e.row);
            }}
            onLoadMore={() => {
              const l = listing();
              if (l && failure() === null) void load(l);
            }}
            onVisibleRowsChange={({ range }) => {
              const want = Math.max(0, range.start - 100);
              if (want < start() || range.end > start() + WINDOW - 50) start.set(want);
            }}
            onRowPress={({ row }) => open(row)}
            classes={terminal() ? undefined : BARE_TABLE}
            empty={
              // each of its three states in its own words: on its way, failed, or truly nothing
              <Await loading={loading()} error={failure()} retry={refresh} waiting={t("browse.listing")} testID="listing">
                <If when={filter()} else={<Empty icon="folder" text={t("browse.emptyPrefix")} />}>
                  {/* nothing listed matches: the one way on, every key under the prefix searched */}
                  <Empty icon="search" text={t("browse.noMatch", { text: filter() })}>
                    <Button small variant="outline" icon="search" label={t("browse.searchAll")} shortcut="mod+enter" onPress={() => void search()} testID="search" />
                    {/* a key deleted (its latest version a delete marker) lists nowhere: its versions, by its name */}
                    <Button small variant="ghost" icon="file-clock" label={t("browse.versionsOf", { key: filter() })} onPress={() => props.onSelect({ key: `${prefix()}${filter()}`, name: filter(), isPrefix: false, size: 0, lastModified: null, etag: null, storageClass: null, parts: null }, true)} testID="versions-of" />
                  </Empty>
                </If>
              </Await>
            }
          />
        </InWindowMenu>
      </View>
      </If>
      </View>
    </View>
  );
}

const detailWidth = rememberedSize("browse.detail", 400);

export function Browse() {
  // what the sections say of this bucket and prefix, found out as it opens (the section row is the way to them)
  if (!terminal()) useBucketFacts();
  // the object in the inspector, and whether it opened on its versions
  const selected = signal<Entry | null>(null);
  const onVersions = signal(false);
  const choose = (e: Entry | null, versions = false) => batch(() => (selected.set(e), onVersions.set(versions)));
  effect(() => {
    bucket();
    prefix();
    choose(null);
  });
  return (
    <View class="flex-1" testID="screen-browse">
      <If when={terminal()}>
        <View class="flex-row gap-2 px-1 pb-1">
          <EndpointPicker />
          <BucketPicker />
        </View>
      </If>
      <If when={endpoint() && bucket()} else={<Empty icon="database" title={endpoint() ? t("store.pickTitle") : undefined} text={endpoint() ? t("store.pickText") : t("common.chooseEndpoint")}><If when={endpoint()}><ShowSidebar /></If></Empty>}>
        <If
          when={terminal()}
          else={
            <ResizablePanelGroup class="flex-1" handleClass={SPLIT_HANDLE} handleAccessibilityLabel={t("browse.resizeDetail")}>
              <ResizablePanel minSize={PANE.objectsLeast} class="min-w-0 gap-3 px-4 py-4">
                <Objects onSelect={choose} selected={selected()?.key ?? null} />
              </ResizablePanel>
              <ResizablePanel visible={!!selected()} size={detailWidth.size()} minSize={PANE.detailLeast} maxSize={PANE.detailMost} onResize={detailWidth.onResize} class="bg-inspector">
              <If when={selected()}>
                {(e) => (
                  <View class="flex-1">
                    {/* the panel is named for what it shows: the object */}
                    <PanelHeader title={e().name || e().key} icon="file" onClose={() => choose(null)} closeTestID="detail-close" />
                    <Scroll class="flex-1" contentClass="p-4">
                      {/* one object's: what was asked for another never shows here */}
                      <Each of={[{ e: e(), versions: onVersions() }]} key={(x) => `${x.e.key}\n${x.versions}`}>
                        {(x) => <Detail entry={x().e} versions={x().versions} />}
                      </Each>
                    </Scroll>
                  </View>
                )}
              </If>
              </ResizablePanel>
            </ResizablePanelGroup>
          }
        >
          <If when={selected()} else={<Objects onSelect={choose} selected={null} />}>
            {(e) => (
              <Scroll class="flex-1" contentClass="p-1">
                <Detail entry={e()} versions={onVersions()} onBack={() => choose(null)} />
              </Scroll>
            )}
          </If>
        </If>
      </If>
    </View>
  );
}

