// Browse: prefixes and objects of a bucket (ListObjectsV2, loaded page by
// page and kept in Rust), and an object's details.

import { batch, cleanup, Clipboard, effect, resource, Scroll, signal, Text, untracked, View } from "nsh";
import { Button as BaseButton, DataTable, Input, Link, ResizablePanel, ResizablePanelGroup } from "nsh/base";
import { isAvailable, Menu } from "nsh/desktop";
import { bytes, count, date } from "../format";
import { t } from "../i18n";
import { api, Listing, watch } from "../native";
import { bucket, endpoint, go, notify, prefix, report, showRequest, versionKey } from "../store";
import type { Entry, ObjectDetail } from "../types";
import { Icon, type IconName, iconSvg } from "../ui/icon";
import { Badge, Bar, Button, CARD, Card, Choice, Empty, Field, IconButton, InWindowMenu, KeyValue, Loading, Muted, terminal, rowHeight, SPLIT_HANDLE, SwitchRow, TYPE, TABLE } from "../ui/kit";
import { Fold } from "../ui/record";
import { rememberedSize } from "../ui/sizes";
import { Overview } from "../ui/overview";
import { BucketPicker, EndpointPicker } from "../ui/picker";

const WINDOW = 400;

function Breadcrumbs() {
  const parts = () => {
    const p = prefix();
    // every segment, empty ones too (`a//b/` is its own place): each crumb's prefix is the key's own
    const segs = p.endsWith("/") ? p.slice(0, -1).split("/") : p.split("/");
    return p ? segs.map((s, i) => ({ name: s || "∅", prefix: `${segs.slice(0, i + 1).join("/")}/` })) : [];
  };
  return (
    <View class="min-w-0 flex-row flex-wrap items-center gap-0.5">
      {/* links of the path (the last one the place shown): screen readers hear them so */}
      <Link testID="crumb-root" href={`s3://${bucket()}/`} openWith="app" onOpen={() => prefix.set("")} current={parts().length === 0} accessibilityLabel={bucket() ?? ""} class="flex-row items-center gap-1.5 rounded-(--radius-sm) px-1.5 py-0.5 hover:bg-muted">
        <If when={!terminal()}>
          <Icon name="database" size={14} tint="foreground" />
        </If>
        <Text class="text-body font-semibold text-foreground">{bucket()}</Text>
      </Link>
      <Each of={parts()}>
        {(p, i) => (
          <View class="flex-row items-center gap-0.5">
            <If when={!terminal()} else={<Text class="text-muted-foreground">/</Text>}>
              <Icon name="chevron-right" size={12} />
            </If>
            <Link href={`s3://${bucket()}/${p().prefix}`} openWith="app" onOpen={() => prefix.set(p().prefix)} current={i() === parts().length - 1} accessibilityLabel={p().name} class="rounded-(--radius-sm) px-1.5 py-0.5 hover:bg-muted">
              <Text class={i() === parts().length - 1 ? "text-body font-medium text-foreground" : "text-body text-muted-foreground"}>{p().name}</Text>
            </Link>
          </View>
        )}
      </Each>
    </View>
  );
}

function Detail(props: { entry: Entry; onBack?: () => void }) {
  const ep = endpoint()!;
  const b = bucket()!;
  const detail = resource(
    () => props.entry.key,
    async (key) => {
      try {
        return await api.objectDetail(ep, b, key);
      } catch (e) {
        report(e);
        throw e;
      }
    },
  );
  const presignSecs = signal("3600");
  const url = signal<string | null>(null);
  const progress = signal<string | null>(null);
  let downloadJob: string | null = null;
  cleanup(() => {
    // another object chosen, or the panel closed: the download stops, and says so
    if (downloadJob) {
      api.cancelJob(downloadJob);
      notify(t("browse.downloadStopped"));
    }
  });
  const download = async () => {
    try {
      downloadJob = await api.download(ep, b, props.entry.key);
      progress.set(t("common.loading"));
      const r = await watch<{ path: string; bytes: number }>(downloadJob, (e) => {
        if (e.type === "Progress") progress.set(`${bytes(e.progress.done)}${e.progress.total ? ` / ${bytes(e.progress.total)}` : ""}`);
      });
      progress.set(null);
      downloadJob = null;
      notify(t("common.exportedTo", { path: r.path }));
    } catch (e) {
      progress.set(null);
      report(e);
    }
  };
  const presign = async () => {
    try {
      url.set(await api.presign(ep, b, props.entry.key, Number(presignSecs())));
    } catch (e) {
      report(e);
    }
  };
  const etagText = (d: ObjectDetail) => {
    const k = d.etag?.kind;
    if (!k) return "—";
    if (k.kind === "multipart") return t("browse.multipartParts", { n: k.parts });
    if (k.kind === "md5") return t("browse.singlePart");
    return t("browse.opaqueEtag");
  };
  return (
    <View selectable class="gap-4" testID="object-detail">
      <View class="gap-1.5">
        <View class="flex-row items-center gap-2">
          <If when={props.onBack}>
            <Button small variant="ghost" icon="chevron-left" label={t("common.back")} onPress={() => props.onBack?.()} testID="detail-back" />
          </If>
          <If when={!terminal()}>
            <Icon name="file" size={16} tint="foreground" />
          </If>
          <Text class="min-w-0 flex-1 text-heading font-semibold text-foreground line-clamp-2">{props.entry.name}</Text>
        </View>
        <Text class="font-mono text-caption text-muted-foreground line-clamp-2">{`s3://${b}/${props.entry.key}`}</Text>
      </View>
      <If when={detail()} else={<Loading />}>
        {(d) => (
          <View class="gap-4">
            <View class="flex-row flex-wrap gap-1.5">
              <Button small variant="outline" icon="download" label={t("browse.download")} onPress={download} disabled={!!progress()} testID="download" />
              <Button small variant="outline" icon="file-clock" label={t("browse.versions")} onPress={() => batch(() => (versionKey.set(d().key), go("versions")))} />
              <Button small variant="ghost" label={t("common.showRequest")} onPress={() => showRequest(d().recordId)} />
            </View>
            <If when={progress()}>{(p) => <Bar value={null} label={p()} />}</If>
            <KeyValue boxed
              rows={[
                [t("browse.size"), `${bytes(d().size)} (${count(d().size ?? 0)} B)`],
                ["Content-Type", d().contentType ?? "—"],
                [t("browse.modified"), date(d().lastModified)],
                ["Version", d().versionId ?? "null"],
                [t("browse.storageClass"), d().storageClass],
              ]}
            />
            <Fold title={t("browse.etag")}>
              <View class="gap-1.5">
                <Text class="font-mono text-caption text-foreground">{d().etag?.value ?? "—"}</Text>
                <Text class="text-small text-muted-foreground">{etagText(d())}</Text>
                <If when={d().partSizeGuesses.length > 0}>
                  <Text class="text-small text-muted-foreground">
                    {t("browse.partSizes")}: {d().partSizeGuesses.map((s) => bytes(s)).join(", ")}
                  </Text>
                </If>
              </View>
            </Fold>
            <Fold title={t("browse.checksums")} count={d().checksums.length}>
              <If when={d().checksums.length > 0} else={<Text class="text-small text-muted-foreground">{t("browse.noChecksum")}</Text>}>
                <View class="gap-1.5">
                  <Each of={d().checksums}>
                    {(c) => (
                      <View class="gap-0.5 rounded-(--radius) border border-border bg-card px-2.5 py-1.5">
                        <View class="flex-row items-center gap-2">
                          <Badge label={c().algorithm} tone="info" />
                          <Text class="text-caption text-muted-foreground">
                            {c().kind === "composite" ? t("browse.composite", { n: c().parts ?? "?" }) : t("browse.fullObject")}
                            {c().kindFromHeader ? "" : ` · ${t("browse.inferred")}`}
                          </Text>
                        </View>
                        <Text class="font-mono text-caption text-foreground">{c().value}</Text>
                      </View>
                    )}
                  </Each>
                </View>
              </If>
            </Fold>
            <Fold title={t("browse.encryption")}>
              <KeyValue boxed
                rows={[
                  ["SSE", d().sse.customerAlgorithm ? `SSE-C (${d().sse.customerAlgorithm})` : (d().sse.algorithm ?? t("browse.noEncryption"))],
                  ...(d().sse.kmsKeyId ? ([[t("browse.kmsKey"), d().sse.kmsKeyId]] as [string, string][]) : []),
                  ...(d().sse.bucketKeyEnabled !== null && d().sse.bucketKeyEnabled !== undefined ? ([[t("browse.bucketKey"), d().sse.bucketKeyEnabled ? t("common.on") : t("common.off")]] as [string, string][]) : []),
                  ...(d().objectLock ? ([[t("browse.objectLock"), d().objectLock!]] as [string, string][]) : []),
                  ...(d().restore ? ([[t("browse.restore"), d().restore!]] as [string, string][]) : []),
                ]}
              />
            </Fold>
            <Fold title={t("browse.metadata")} count={d().metadata.length} open={d().metadata.length > 0}>
              <If when={d().metadata.length > 0} else={<Text class="text-small text-muted-foreground">{t("common.none")}</Text>}>
                <KeyValue boxed rows={d().metadata} />
              </If>
            </Fold>
            <Fold title={t("browse.presign")} open={false}>
              <View class="gap-2">
                <View class="flex-row items-end gap-2">
                  <Choice
                    class="w-36"
                    label={t("browse.expires")}
                    value={presignSecs()}
                    items={[
                      { value: "300", title: t("browse.expires5m") },
                      { value: "3600", title: t("browse.expires1h") },
                      { value: "86400", title: t("browse.expires1d") },
                      { value: "604800", title: t("browse.expires7d") },
                    ]}
                    onChange={(v) => presignSecs.set(v)}
                  />
                  <Button small variant="outline" icon="link" label={t("browse.presign")} onPress={presign} testID="presign" />
                </View>
                <If when={url()}>
                  {(u) => (
                    <View class="gap-1.5 rounded-(--radius) border border-border bg-card px-2.5 py-2">
                      <Text class="font-mono text-caption text-foreground">{u()}</Text>
                      <View class="flex-row">
                        <Button small variant="ghost" icon="copy" label={t("common.copy")} onPress={() => Clipboard.writeText(u()).then(() => notify(t("common.copied")), report)} />
                      </View>
                    </View>
                  )}
                </If>
              </View>
            </Fold>
            <Fold title={t("browse.headers")} count={d().headers.length}>
              <KeyValue boxed rows={d().headers.map(([k, v]) => [k, v] as [string, string])} />
            </Fold>
          </View>
        )}
      </If>
    </View>
  );
}

function Objects(props: { onSelect: (e: Entry) => void; selected: string | null }) {
  const listing = signal<Listing | null>(null);
  const version = signal(0);
  const start = signal(0);
  const loading = signal(false);
  const filter = signal("");
  const searchText = signal("");
  const searchHits = signal<Entry[] | null>(null);
  const searchState = signal<string | null>(null);
  const flat = signal(false);
  const loadingAll = signal(false);
  // a press on Refresh lists the same place again
  const reload = signal(0);
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
      searchHits.set(null);
    });
    untracked(() => void load(l));
    cleanup(() => l.dispose());
  });
  cleanup(stopSearch);

  async function load(l: Listing) {
    if (loading() || l.done) return;
    loading.set(true);
    try {
      await l.loadMore();
    } catch (e) {
      report(e);
    }
    loading.set(false);
    version.update((v) => v + 1);
  }

  const rows = () => {
    version();
    const l = listing();
    if (!l) return [] as Entry[];
    return l.rows(start(), WINDOW) as Entry[];
  };
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
      name: e.isPrefix ? `▸ ${e.name}` : e.name,
      size: e.isPrefix ? "" : bytes(e.size),
      modified: e.isPrefix ? "" : date(e.lastModified),
      class: e.storageClass ?? "",
      parts: e.parts ? String(e.parts) : "",
    }));

  const loadAll = async () => {
    const l = listing();
    if (!l) return;
    loadingAll.set(true);
    try {
      await l.loadAll(1_000_000);
    } catch (e) {
      report(e);
    }
    loadingAll.set(false);
    version.update((v) => v + 1);
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
    if (!ep || !b || !searchText()) return;
    stopSearch();
    searchHits.set([]);
    const where = `${ep}|${b}|${prefix()}`;
    const here = () => `${endpoint()}|${bucket()}|${prefix()}` === where;
    try {
      const job = await api.search(ep, b, prefix(), searchText());
      searchJob = job;
      const r = await watch<{ matches: Entry[]; scanned: number }>(job, (ev) => {
        if (!here() || searchJob !== job) return;
        if (ev.type === "Progress") searchState.set(ev.progress.message);
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
    searchText.set(v);
    listing()?.setFilter(v);
    batch(() => {
      start.set(0);
      version.update((x) => x + 1);
    });
  };
  const entryAt = (row: number) => listing()?.row(row) as Entry | null;
  const copy = (text: string) => void Clipboard.writeText(text).then(() => notify(t("common.copied")), report);
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
      : [{ label: t("browse.detail"), icon: "info", run: () => props.onSelect(e) }, ...copies, { label: t("browse.versions"), icon: "file-clock", run: () => batch(() => (versionKey.set(e.key), go("versions"))) }];
  };
  const menuRow = signal<number | null>(null);
  // the system's menus, a framed box in a terminal; the in-window one on the web
  const systemMenus = isAvailable;
  const menu = (row: number) => {
    menuRow.set(row);
    if (!systemMenus) return;
    void Menu.popup(actions(row).map((a) => ({ label: a.label, icon: iconSvg(a.icon), onSelect: a.run })));
  };

  return (
    <View class="min-h-0 flex-1 gap-2">
      <View class="flex-row flex-wrap items-center gap-2">
        <Breadcrumbs />
        <View class="flex-1" />
        <Text class="font-mono text-small text-muted-foreground" testID="listing-count">
          {t("browse.objects", { n: count(total()) })}
          {done() ? "" : ` · ${t("browse.more")}`}
        </Text>
        <IconButton icon="refresh-cw" label={t("common.refresh")} onPress={() => reload.update((n) => n + 1)} testID="listing-refresh" />
      </View>
      <View class="flex-row flex-wrap items-center gap-2">
        <View class="h-8 min-w-[240px] flex-1 flex-row items-center gap-2 rounded-(--radius) border border-input bg-background px-2.5 focus-within:border-ring">
          <If when={!terminal()}>
            <Icon name="search" size={14} />
          </If>
          <Input
            testID="listing-filter"
            value={filter()}
            placeholder={t("browse.filterOrSearch")}
            autoCapitalize="none"
            autoCorrect={false}
            onChange={({ value }) => applyFilter(value)}
            onSubmit={search}
            submitBehavior="submit"
            accessibilityLabel={t("common.filter")}
            class="min-w-0 flex-1 text-body text-foreground placeholder:text-muted-foreground"
          />
          <If when={filter()}>
            <Button small variant="ghost" label={t("browse.searchAll")} onPress={search} testID="search" />
          </If>
        </View>
        <SwitchRow label={t("browse.flat")} checked={flat()} onChange={(v) => flat.set(v)} testID="flat" />
        {/* nothing to load once every row is here */}
        <If when={!done()}>
          <Button small variant="outline" label={t("browse.loadAll")} onPress={loadAll} disabled={loadingAll()} testID="load-all" class="min-w-24" />
        </If>
      </View>
      <If when={searchHits()}>
        {(hits) => (
          <Card>
            <View class="flex-row items-center gap-2">
              <Text class="flex-1 text-body font-medium text-foreground">
                {hits().length > 50 ? t("browse.matchesShown", { shown: 50, n: count(hits().length) }) : t("browse.matches", { n: hits().length })} {searchState() ? `· ${searchState()}` : ""}
              </Text>
              <IconButton icon="x" label={t("common.close")} onPress={() => searchHits.set(null)} />
            </View>
            <Each of={hits().slice(0, 50)} key={(h) => h.key}>
              {(h) => (
                <BaseButton accessibilityLabel={h().key} onPress={() => props.onSelect(h())} class="flex-row items-center gap-2 rounded-(--radius-sm) px-1 py-0.5 hover:bg-muted">
                  <Text class="font-mono text-small text-primary">{h().key}</Text>
                </BaseButton>
              )}
            </Each>
          </Card>
        )}
      </If>
      <InWindowMenu class="min-h-0 flex-1" enabled={!systemMenus} actions={() => actions(menuRow())}>
      <DataTable
        testID="objects"
        accessibilityLabel={t("nav.objects")}
        class={`min-h-0 flex-1 ${CARD}`}
        columns={[
          { key: "name", title: t("browse.name"), width: terminal() ? 44 * 8 : 380, minWidth: 120 },
          { key: "size", title: t("browse.size"), width: 96, align: "end" },
          { key: "modified", title: t("browse.modified"), width: 150 },
          { key: "class", title: t("browse.class"), width: 124 },
          { key: "parts", title: "MPU", width: 56, align: "end" },
        ]}
        data={tableData()}
        dataStart={start()}
        rowCount={total()}
        hasMore={!done()}
        loading={loading()}
        rowSelectable
        resizableColumns
        fontSize={TYPE.body}
        rowHeight={rowHeight(30)}
        renderCell={
          terminal()
            ? undefined
            : (c) => {
                if (c.key === "name") {
                  const folder = c.text.startsWith("▸ ");
                  return (
                    <View class="flex-1 flex-row items-center gap-2">
                      <Icon name={folder ? "folder" : "file"} size={14} tint={folder ? "foreground" : "muted-foreground"} />
                      <Text class={`min-w-0 flex-1 text-body line-clamp-1 ${folder ? "font-medium text-foreground" : "text-foreground"}`}>{folder ? c.text.slice(2) : c.text}</Text>
                    </View>
                  );
                }
                // sizes as figures (tabular, at the end, under their title); the rest quiet
                return c.key === "size" || c.key === "parts" ? <Text class="flex-1 text-right text-small tabular-nums text-foreground">{c.text}</Text> : <Text class="text-small text-muted-foreground line-clamp-1">{c.text}</Text>;
              }
        }
        onContextMenu={(e) => {
          if (e.row !== undefined && e.row !== null) menu(e.row);
        }}
        onLoadMore={() => {
          const l = listing();
          if (l) void load(l);
        }}
        onVisibleRowsChange={({ range }) => {
          const want = Math.max(0, range.start - 100);
          if (want < start() || range.end > start() + WINDOW - 50) start.set(want);
        }}
        onRowPress={({ row }) => open(row)}
        classes={terminal() ? undefined : TABLE}
        empty={<Empty icon="folder" text={loading() ? t("common.loading") : t("common.empty")} />}
      />
      </InWindowMenu>
    </View>
  );
}

const detailWidth = rememberedSize("browse.detail", 400);

export function Browse() {
  const selected = signal<Entry | null>(null);
  effect(() => {
    bucket();
    prefix();
    selected.set(null);
  });
  return (
    <View class="flex-1" testID="screen-browse">
      <If when={terminal()}>
        <View class="flex-row gap-2 px-1 pb-1">
          <EndpointPicker />
          <BucketPicker />
        </View>
      </If>
      <If when={endpoint() && bucket()} else={<Empty icon="database" title={endpoint() ? t("store.pickTitle") : undefined} text={endpoint() ? t("store.pickText") : t("common.chooseEndpoint")} />}>
        <If
          when={terminal()}
          else={
            <ResizablePanelGroup class="flex-1" handleClass={SPLIT_HANDLE} handleAccessibilityLabel={t("browse.resizeDetail")}>
              <ResizablePanel minSize={420} class="min-w-0 gap-3 px-5 py-4">
                <Overview />
                <Objects onSelect={(e) => selected.set(e)} selected={selected()?.key ?? null} />
              </ResizablePanel>
              <ResizablePanel visible={!!selected()} size={detailWidth.size()} minSize={300} maxSize={720} onResize={detailWidth.onResize} class="bg-inspector">
              <If when={selected()}>
                {(e) => (
                  <View class="flex-1">
                    <View class="h-10 flex-row items-center border-b border-border px-3">
                      <Text class="flex-1 text-small font-semibold text-muted-foreground">{t("browse.detail")}</Text>
                      <IconButton icon="x" label={t("common.close")} onPress={() => selected.set(null)} testID="detail-close" />
                    </View>
                    <Scroll class="flex-1" contentClass="p-4">
                      {/* one object's: what was asked for another never shows here */}
                      <Each of={[e()]} key={(x) => x.key}>
                        {(x) => <Detail entry={x()} />}
                      </Each>
                    </Scroll>
                  </View>
                )}
              </If>
              </ResizablePanel>
            </ResizablePanelGroup>
          }
        >
          <If when={selected()} else={<Objects onSelect={(e) => selected.set(e)} selected={null} />}>
            {(e) => (
              <Scroll class="flex-1" contentClass="p-1">
                <Detail entry={e()} onBack={() => selected.set(null)} />
              </Scroll>
            )}
          </If>
        </If>
      </If>
    </View>
  );
}

