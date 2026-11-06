// Browse: prefixes and objects of a bucket (ListObjectsV2, loaded page by
// page and kept in Rust), and an object's details.

import { batch, cleanup, Clipboard, effect, resource, Scroll, signal, Text, untracked, View } from "nsh";
import { Button as BaseButton, DataTable } from "nsh/base";
import { bytes, count, date } from "../format";
import { t } from "../i18n";
import { api, Listing, watch } from "../native";
import { bucket, endpoint, go, notify, prefix, report, showRequest, versionKey } from "../store";
import type { Entry, ObjectDetail } from "../types";
import { Badge, Bar, Button, Card, Choice, Empty, Field, Heading, KeyValue, Mono, Muted, SimpleTable, terminal, Toggle } from "../ui/kit";
import { BucketPicker, EndpointPicker } from "../ui/picker";

const WINDOW = 400;

function Breadcrumbs() {
  const parts = () => {
    const p = prefix();
    const segs = p.split("/").filter(Boolean);
    return segs.map((s, i) => ({ name: s, prefix: `${segs.slice(0, i + 1).join("/")}/` }));
  };
  return (
    <View class="flex-row flex-wrap items-center gap-1">
      <BaseButton accessibilityLabel={bucket() ?? ""} onPress={() => prefix.set("")} class="rounded-sm px-1 hover:bg-muted">
        <Text class="text-sm font-medium text-foreground">{bucket()}</Text>
      </BaseButton>
      <Each of={parts()}>
        {(p) => (
          <View class="flex-row items-center gap-1">
            <Text class="text-sm text-muted-foreground">/</Text>
            <BaseButton accessibilityLabel={p().name} onPress={() => prefix.set(p().prefix)} class="rounded-sm px-1 hover:bg-muted">
              <Text class="text-sm text-foreground">{p().name}</Text>
            </BaseButton>
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
    if (downloadJob) api.cancelJob(downloadJob);
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
    <View class="flex-1 gap-3" testID="object-detail">
      <View class="flex-row items-center gap-2">
        <If when={props.onBack}>
          <Button small variant="ghost" label={`‹ ${t("common.back")}`} onPress={() => props.onBack?.()} testID="detail-back" />
        </If>
        <Text class="flex-1 text-base font-semibold text-foreground line-clamp-2">
          {props.entry.name}
        </Text>
      </View>
      <If when={detail()} else={<Muted>{t("common.loading")}</Muted>}>
        {(d) => (
          <View class="gap-3">
            <KeyValue
              rows={[
                ["Key", d().key],
                [t("browse.size"), `${bytes(d().size)} (${count(d().size ?? 0)} B)`],
                ["Content-Type", d().contentType ?? "—"],
                [t("browse.modified"), d().lastModified ?? "—"],
                ["Version", d().versionId ?? "null"],
                [t("browse.storageClass"), d().storageClass],
              ]}
            />
            <View class="flex-row flex-wrap gap-2">
              <Button small variant="primary" label={t("browse.download")} onPress={download} disabled={!!progress()} testID="download" />
              <Button small label={t("browse.versions")} onPress={() => batch(() => (versionKey.set(d().key), go("versions")))} />
              <Button small variant="ghost" label={t("common.showRequest")} onPress={() => showRequest(d().recordId)} />
            </View>
            <If when={progress()}>{(p) => <Bar value={null} label={p()} />}</If>
            <Heading>{t("browse.etag")}</Heading>
            <Mono>{d().etag?.value ?? "—"}</Mono>
            <Muted>{etagText(d())}</Muted>
            <If when={d().partSizeGuesses.length > 0}>
              <Muted>
                {t("browse.partSizes")}: {d().partSizeGuesses.map((s) => bytes(s)).join(", ")}
              </Muted>
            </If>
            <Heading>{t("browse.checksums")}</Heading>
            <If when={d().checksums.length > 0} else={<Muted>{t("browse.noChecksum")}</Muted>}>
              <Each of={d().checksums}>
                {(c) => (
                  <View class="flex-row items-center gap-2">
                    <Badge label={c().algorithm} tone="info" />
                    <Mono class="flex-1">{c().value}</Mono>
                    <Text class="text-xs text-muted-foreground">
                      {c().kind === "composite" ? t("browse.composite", { n: c().parts ?? "?" }) : t("browse.fullObject")}
                      {c().kindFromHeader ? "" : ` · ${t("browse.inferred")}`}
                    </Text>
                  </View>
                )}
              </Each>
            </If>
            <Heading>{t("browse.encryption")}</Heading>
            <KeyValue
              rows={[
                ["SSE", d().sse.customerAlgorithm ? `SSE-C (${d().sse.customerAlgorithm})` : (d().sse.algorithm ?? t("browse.noEncryption"))],
                ...(d().sse.kmsKeyId ? ([["KMS key", d().sse.kmsKeyId]] as [string, string][]) : []),
                ...(d().sse.bucketKeyEnabled !== null && d().sse.bucketKeyEnabled !== undefined ? ([["Bucket key", String(d().sse.bucketKeyEnabled)]] as [string, string][]) : []),
                ...(d().objectLock ? ([["Object Lock", d().objectLock!]] as [string, string][]) : []),
                ...(d().restore ? ([["Restore", d().restore!]] as [string, string][]) : []),
              ]}
            />
            <Heading>{t("browse.metadata")}</Heading>
            <If when={d().metadata.length > 0} else={<Muted>{t("common.none")}</Muted>}>
              <KeyValue mono rows={d().metadata} />
            </If>
            <Heading>{t("browse.presign")}</Heading>
            <View class="flex-row items-end gap-2">
              <Choice
                class="w-40"
                label={t("browse.expires")}
                value={presignSecs()}
                items={[
                  { value: "300", title: "5 min" },
                  { value: "3600", title: "1 h" },
                  { value: "86400", title: "1 day" },
                  { value: "604800", title: "7 days" },
                ]}
                onChange={(v) => presignSecs.set(v)}
              />
              <Button small label={t("browse.presign")} onPress={presign} testID="presign" />
            </View>
            <If when={url()}>
              {(u) => (
                <View class="gap-1">
                  <Mono>{u()}</Mono>
                  <Button small label={t("common.copy")} onPress={() => Clipboard.writeText(u()).then(() => notify(t("common.copied")), report)} />
                </View>
              )}
            </If>
            <Heading>{t("browse.headers")}</Heading>
            <SimpleTable mono columns={["header", "value"]} rows={d().headers.map(([k, v]) => [k, v])} />
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
  let searchJob: string | null = null;

  effect(() => {
    const ep = endpoint();
    const b = bucket();
    const p = prefix();
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
  cleanup(() => {
    if (searchJob) api.cancelJob(searchJob);
  });

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
    if (searchJob) api.cancelJob(searchJob);
    searchHits.set([]);
    try {
      searchJob = await api.search(ep, b, prefix(), searchText());
      const r = await watch<{ matches: Entry[]; scanned: number }>(searchJob, (ev) => {
        if (ev.type === "Progress") searchState.set(ev.progress.message);
        if (ev.type === "Partial") searchHits.update((h) => [...(h ?? []), ...(ev.data as Entry[])]);
      });
      batch(() => {
        searchHits.set(r.matches);
        searchState.set(`${count(r.scanned)} scanned`);
      });
    } catch (e) {
      report(e);
    }
    searchJob = null;
  };

  return (
    <View class="flex-1 gap-2">
      <View class="flex-row flex-wrap items-center gap-2">
        <Breadcrumbs />
        <View class="flex-1" />
        <Text class="text-xs text-muted-foreground" testID="listing-count">
          {t("browse.objects", { n: count(total()) })}
          {done() ? "" : ` · ${t("browse.more")}`}
        </Text>
        <Toggle label={t("browse.flat")} checked={flat()} onChange={(v) => flat.set(v)} testID="flat" />
        <Button small label={loadingAll() ? t("common.loading") : t("browse.loadAll")} onPress={loadAll} disabled={done() || loadingAll()} testID="load-all" />
        <Button small label={t("common.refresh")} onPress={() => prefix.set(prefix())} />
      </View>
      <View class="flex-row gap-2">
        <Field
          class="flex-1"
          label={t("common.filter")}
          value={filter()}
          placeholder={t("browse.filterPlaceholder")}
          onChange={(v) => {
            filter.set(v);
            listing()?.setFilter(v);
            batch(() => {
              start.set(0);
              version.update((x) => x + 1);
            });
          }}
          testID="listing-filter"
        />
        <Field class="flex-1" label={t("browse.searchAll")} value={searchText()} onChange={(v) => searchText.set(v)} testID="search-input" />
        <View class="justify-end">
          <Button small label={t("common.search")} onPress={search} testID="search" />
        </View>
      </View>
      <If when={searchHits()}>
        {(hits) => (
          <Card>
            <View class="flex-row items-center gap-2">
              <Text class="flex-1 text-sm font-medium text-foreground">
                {hits().length} matches {searchState() ? `· ${searchState()}` : ""}
              </Text>
              <Button small variant="ghost" label={t("common.close")} onPress={() => searchHits.set(null)} />
            </View>
            <Each of={hits().slice(0, 50)} key={(h) => h.key}>
              {(h) => (
                <Text role="button" class="font-mono text-xs text-primary" onPress={() => props.onSelect(h())}>
                  {h().key}
                </Text>
              )}
            </Each>
          </Card>
        )}
      </If>
      <DataTable
        testID="objects"
        class="flex-1 rounded-(--radius) border border-border"
        columns={[
          { key: "name", title: t("browse.name"), width: terminal() ? 44 * 8 : 380, minWidth: 120 },
          { key: "size", title: t("browse.size"), width: 96, align: "end" },
          { key: "modified", title: t("browse.modified"), width: 140 },
          { key: "class", title: t("browse.class"), width: 120 },
          { key: "parts", title: "MPU", width: 56, align: "end" },
        ]}
        data={tableData()}
        dataStart={start()}
        rowCount={total()}
        hasMore={!done()}
        loading={loading()}
        rowSelectable
        stripe
        resizableColumns
        onLoadMore={() => {
          const l = listing();
          if (l) void load(l);
        }}
        onVisibleRowsChange={({ range }) => {
          const want = Math.max(0, range.start - 100);
          if (want < start() || range.end > start() + WINDOW - 50) start.set(want);
        }}
        onRowPress={({ row }) => open(row)}
        classes={{ header: "bg-muted", headerCell: "px-2", row: "px-2", rowSelected: "bg-accent" }}
        empty={<Empty text={loading() ? t("common.loading") : t("common.empty")} />}
      />
    </View>
  );
}

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
      <If when={endpoint() && bucket()} else={<Empty text={endpoint() ? t("common.chooseBucket") : t("common.chooseEndpoint")} />}>
        <If
          when={terminal()}
          else={
            <View class="flex-1 flex-row">
              <View class="flex-1 p-3">
                <Objects onSelect={(e) => selected.set(e)} selected={selected()?.key ?? null} />
              </View>
              <View class="w-96 border-l border-border">
                <If when={selected()} else={<Empty text={t("browse.selectObject")} />}>
                  {(e) => (
                    <Scroll class="flex-1" contentClass="p-4">
                      <Detail entry={e()} />
                    </Scroll>
                  )}
                </If>
              </View>
            </View>
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

