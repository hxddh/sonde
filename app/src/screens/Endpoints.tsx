// Endpoints: add, edit, import from AWS profiles, test connectivity, demo.

import { batch, effect, Scroll, signal, Text, untracked, View } from "nsh";
import { AlertDialog } from "nsh/base";
import { t } from "../i18n";
import { api } from "../native";
import { endpoint, endpoints, endpointsMode, notify, refreshEndpoints, report, showRequest, startDemo } from "../store";
import type { ConnectivityReport, EndpointView } from "../types";
import { Icon } from "../ui/icon";
import { openCompatWindow } from "./CompatWindow";
import { Badge, Button, CARD, Card, Check, Choice, Dot, Empty, Field, IconButton, KeyValue, Muted, Page, terminal, Section, SidePanel, SwitchRow, Title } from "../ui/kit";
import { ms as fmtMs } from "../format";

type Draft = {
  id: string;
  name: string;
  url: string;
  region: string;
  pathStyle: boolean;
  caFile: string;
  vendor: string;
  credentials: string;
  profile: string;
  buckets: string;
};

const blank = (): Draft => ({ id: "", name: "", url: "", region: "us-east-1", pathStyle: false, caFile: "", vendor: "other", credentials: "default", profile: "", buckets: "" });

const fromView = (e: EndpointView): Draft => ({ id: e.id, name: e.name, url: e.url, region: e.region, pathStyle: e.pathStyle, caFile: e.caFile ?? "", vendor: e.vendor, credentials: "keep", profile: "", buckets: e.buckets.join(", ") });

function Form(props: { initial: Draft; editing: boolean; onDone: () => void; onSaved?: (id: string) => void }) {
  const d = signal<Draft>(props.initial);
  const set = (k: keyof Draft, v: string | boolean) => d.update((x) => ({ ...x, [k]: v }));
  const vendors = api.vendors();
  // an id from the name: its ASCII letters and digits ("endpoint" for a name
  // with none), made unique among the endpoints there are, so adding never
  // replaces one
  const idFor = (x: Draft) => {
    const typed = x.id.trim();
    if (typed || props.editing) return typed;
    const base = x.name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "endpoint";
    const taken = new Set(endpoints().map((e) => e.id));
    let id = base;
    for (let i = 2; taken.has(id); i++) id = `${base}-${i}`;
    return id;
  };
  const save = () => {
    const x = d();
    const id = idFor(x);
    try {
      api.saveEndpoint({
        adding: !props.editing,
        id,
        name: x.name.trim() || x.id,
        url: x.url.trim(),
        region: x.region.trim(),
        pathStyle: x.pathStyle,
        caFile: x.caFile.trim() || null,
        vendor: x.vendor,
        buckets: x.buckets.split(",").map((b) => b.trim()).filter(Boolean),
        credentials: x.credentials,
        profile: x.profile.trim() || null,
      });
      refreshEndpoints();
      props.onSaved?.(id);
      props.onDone();
    } catch (e) {
      report(e);
    }
  };
  const creds = [
    ...(props.editing ? [{ value: "keep", title: t("endpoints.cred.keep") }] : []),
    { value: "default", title: t("endpoints.cred.default") },
    { value: "profile", title: t("endpoints.cred.profile") },
    { value: "env", title: t("endpoints.cred.env") },
    { value: "imds", title: t("endpoints.cred.imds") },
    { value: "anonymous", title: t("endpoints.cred.anonymous") },
  ];
  return (
    <View testID="endpoint-form" class="gap-4">
      <Title>{props.editing ? t("endpoints.edit") : t("endpoints.addTitle")}</Title>
      <View class="flex-row flex-wrap gap-3">
        <Field class="min-w-48 flex-1" label={t("endpoints.field.name")} value={d().name} onChange={(v) => set("name", v)} testID="ep-name" />
        {/* an endpoint's id names it in sessions and settings: set once */}
        <If when={!props.editing} else={<KeyValue rows={[[t("endpoints.field.id"), d().id]]} />}>
          <Field class="min-w-48 flex-1" label={t("endpoints.field.id")} value={d().id} onChange={(v) => set("id", v)} testID="ep-id" mono />
        </If>
        <Choice
          class="min-w-48 flex-1"
          label={t("endpoints.field.vendor")}
          value={d().vendor}
          items={vendors.map((v) => ({ value: v.id, title: v.id === "other" ? t("endpoints.vendorOther") : v.label }))}
          onChange={(v) => {
            const info = vendors.find((x) => x.id === v);
            batch(() => {
              if (info) {
                // the old vendor's address and region, untouched, follow the vendor
                const was = vendors.find((x) => x.id === d().vendor);
                const wasUrl = was ? was.urlTemplate.replace("{region}", d().region || was.region) : "";
                const region = !d().region || d().region === "us-east-1" || d().region === was?.region ? info.region : d().region;
                if (!d().url || d().url === wasUrl) set("url", info.urlTemplate.replace("{region}", region));
                set("pathStyle", info.pathStyle);
                set("region", region);
              }
              set("vendor", v);
            });
          }}
        />
      </View>
      <View class="flex-row flex-wrap gap-3">
        <Field class="min-w-48 flex-[2]" label={t("endpoints.field.url")} value={d().url} onChange={(v) => set("url", v)} placeholder="https://s3.eu-west-1.amazonaws.com" testID="ep-url" mono />
        <Field class="min-w-48 flex-1" label={t("endpoints.field.region")} value={d().region} onChange={(v) => set("region", v)} testID="ep-region" mono />
      </View>
      <SwitchRow label={t("endpoints.field.pathStyle")} checked={d().pathStyle} onChange={(v) => set("pathStyle", v)} testID="ep-path-style" />
      <Field label={t("endpoints.field.ca")} value={d().caFile} onChange={(v) => set("caFile", v)} placeholder="/etc/ssl/my-ca.pem" mono />
      <View class="flex-row flex-wrap gap-3">
        <Choice class="min-w-48 flex-1" label={t("endpoints.field.credentials")} value={d().credentials} items={creds} onChange={(v) => set("credentials", v)} testID="ep-creds" />
        <If when={d().credentials === "profile"}>
          <Field class="min-w-48 flex-1" label={t("endpoints.field.profile")} value={d().profile} onChange={(v) => set("profile", v)} mono />
        </If>
      </View>
      <Muted>{t("endpoints.cred.note")}</Muted>
      <Field label={t("endpoints.field.buckets")} value={d().buckets} onChange={(v) => set("buckets", v)} placeholder="logs, backups" mono />
      <View class="flex-row gap-2">
        <Button variant="primary" label={t("common.save")} onPress={save} testID="ep-save" disabled={!d().url} />
        <Button variant="ghost" label={t("common.cancel")} onPress={props.onDone} />
      </View>
    </View>
  );
}

function TestResult(props: { report: ConnectivityReport }) {
  const r = () => props.report;
  const rows = (): [string, string][] => {
    const out: [string, string][] = [];
    if (r().credentialsSource) out.push([t("endpoints.credentials"), r().credentialsSource!]);
    if (r().bucketCount !== null) out.push([t("browse.buckets"), String(r().bucketCount)]);
    if (r().owner) out.push([t("endpoints.owner"), r().owner!]);
    if (r().server) out.push([t("endpoints.server"), r().server!]);
    if (r().identity) out.push([t("endpoints.identity"), `${r().identity!.arn ?? ""} (${r().identity!.account ?? ""})`]);
    if (r().identityNote) out.push([t("endpoints.identity"), r().identityNote!]);
    return out;
  };
  return (
    <View class="gap-2 rounded-(--radius) bg-muted p-3" testID="test-result">
      <Badge tone={r().ok ? "pass" : "fail"} label={r().ok ? t("endpoints.reachable", { ms: fmtMs(r().latencyMs) }) : t("endpoints.unreachable")} />
      <KeyValue rows={rows()} />
      <If when={r().error}>
        {(e) => (
          <View class="flex-row items-center gap-2">
            <Text class="flex-1 text-body text-destructive">
              {e().message}
            </Text>
            <If when={e().recordId !== null && e().recordId !== undefined}>
              <Button small label={t("common.showRequest")} onPress={() => showRequest(e().recordId)} />
            </If>
          </View>
        )}
      </If>
    </View>
  );
}

function Import(props: { onDone: () => void }) {
  const items = api.importableProfiles();
  const chosen = signal<Record<string, boolean>>({});
  const doImport = () => {
    const names = items.filter((e) => chosen()[e.id]).map((e) => e.credentials.replace(/^profile /, ""));
    try {
      const n = api.importProfiles(names);
      refreshEndpoints();
      notify(`+${n}`);
      props.onDone();
    } catch (e) {
      report(e);
    }
  };
  return (
    <View class="gap-4">
      <Title>{t("endpoints.import")}</Title>
      <If when={items.length > 0} else={<Muted>{t("endpoints.importNone")}</Muted>}>
        <Each of={items}>
          {(e) => <Check label={`${e().name} — ${e().url}`} checked={!!chosen()[e().id]} onChange={(v) => chosen.update((c) => ({ ...c, [e().id]: v }))} />}
        </Each>
      </If>
      <View class="flex-row gap-2">
        <Button variant="primary" label={t("common.add")} onPress={doImport} disabled={items.length === 0} />
        <Button variant="ghost" label={t("common.cancel")} onPress={props.onDone} />
      </View>
    </View>
  );
}

export function Endpoints() {
  const mode = signal<"list" | "add" | "import" | { edit: EndpointView }>(endpointsMode());
  // the welcome asked for the form
  effect(() => {
    const m = endpointsMode();
    if (m !== "list") untracked(() => batch(() => (mode.set(m), endpointsMode.set("list"))));
  });
  const tests = signal<Record<string, ConnectivityReport | "running">>({});
  const test = async (id: string) => {
    tests.update((x) => ({ ...x, [id]: "running" }));
    try {
      const r = await api.testEndpoint(id);
      tests.update((x) => ({ ...x, [id]: r }));
    } catch (e) {
      report(e);
      tests.update((x) => {
        const { [id]: _, ...rest } = x;
        return rest;
      });
    }
  };
  const forget = (id: string) =>
    tests.update((x) => {
      const { [id]: _, ...rest } = x;
      return rest;
    });
  const remove = async (e: EndpointView) => {
    const { action } = await AlertDialog.show({
      title: t("endpoints.removeTitle", { name: e.name }),
      description: t("endpoints.removeText"),
      actions: [
        { id: "cancel", label: t("common.cancel"), role: "cancel" },
        { id: "remove", label: t("common.remove"), role: "destructive" },
      ],
    });
    if (action !== "remove") return;
    try {
      api.removeEndpoint(e.id);
      forget(e.id);
      refreshEndpoints();
      notify(t("endpoints.removed", { name: e.name }));
    } catch (err) {
      report(err);
    }
  };
  const form = () => {
    const m = mode();
    if (m === "list") return null;
    if (m === "import") return <Import onDone={() => mode.set("list")} />;
    if (m === "add") return <Form initial={blank()} editing={false} onDone={() => mode.set("list")} onSaved={forget} />;
    return <Form initial={fromView(m.edit)} editing onDone={() => mode.set("list")} onSaved={forget} />;
  };
  const row = (e: EndpointView) => {
    const r = () => tests()[e.id];
    return (
      <View testID={`endpoint-${e.id}`} class="gap-2 px-4 py-3">
        <View class="flex-row items-center gap-3">
          <If when={!terminal()}>
            <View class="size-8 items-center justify-center rounded-(--radius) bg-muted">
              <Icon name="server" size={16} tint="foreground" />
            </View>
          </If>
          <View class="min-w-0 flex-1 gap-0.5">
            <View class="flex-row items-center gap-2">
              <Text class="text-body font-semibold text-foreground">{e.name}</Text>
              <Badge label={e.vendorLabel} />
              <If when={endpoint() === e.id}>
                <Badge label={t("endpoints.current")} tone="primary" />
              </If>
            </View>
            <Text class="font-mono text-caption text-muted-foreground line-clamp-1">{`${e.url} · ${e.region || "—"} · ${e.credentials}`}</Text>
          </View>
          <Button small variant="outline" icon="activity" label={r() === "running" ? t("endpoints.testing") : t("endpoints.test")} onPress={() => test(e.id)} disabled={r() === "running"} testID={`test-${e.id}`} />
          <IconButton icon="pencil" label={t("common.edit")} onPress={() => mode.set({ edit: e })} testID={`edit-${e.id}`} />
          <IconButton icon="trash" label={t("common.remove")} onPress={() => void remove(e)} testID={`remove-${e.id}`} />
        </View>
        <If when={typeof r() === "object" ? (r() as ConnectivityReport) : null}>{(rep) => <TestResult report={rep()} />}</If>
      </View>
    );
  };
  return (
    <Page testID="screen-endpoints">
      <View class={terminal() ? "gap-2" : "w-full max-w-[860px] gap-5"}>
        <View class="flex-row flex-wrap items-center gap-2">
          <View class="flex-1">
            <Title>{t("endpoints.title")}</Title>
          </View>
          <Button variant="outline" icon="plus" label={t("endpoints.add")} onPress={() => mode.set("add")} testID="add-endpoint" />
          <Button variant="outline" icon="key-round" label={t("endpoints.import")} onPress={() => mode.set("import")} />
          <Button variant="outline" icon="sparkles" label={t("endpoints.demo")} onPress={() => void startDemo()} testID="open-demo" />
        </View>
        <If when={terminal() && form()}>{(f) => <Card>{f()}</Card>}</If>
        <If when={endpoints().length > 0} else={<Empty icon="server" text={t("endpoints.noneYet")} />}>
          <View class={`overflow-hidden ${CARD}`}>
            <Each of={endpoints()} key={(e) => e.id}>
              {(e, i) => <View class={i() > 0 ? "border-t border-border" : ""}>{row(e())}</View>}
            </Each>
          </View>
        </If>
        <If when={!terminal() && endpoints().length > 0}>
          <View class={`flex-row items-center gap-3 ${CARD} p-4`}>
            <Icon name="flask-conical" size={16} tint="muted-foreground" />
            <View class="min-w-0 flex-1 gap-0.5">
              <Text class="text-body font-medium text-foreground">{t("compat.title")}</Text>
              <Text class="text-small text-muted-foreground">{t("compat.hint")}</Text>
            </View>
            <Button variant="outline" label={t("compat.open")} onPress={() => openCompatWindow()} testID="open-compat" />
          </View>
        </If>
        <Text class="text-small text-muted-foreground">{t("settings.telemetry")}</Text>
      </View>
      <If when={!terminal()}>
        <SidePanel open={mode() !== "list"} onClose={() => mode.set("list")}>
          <Scroll class="flex-1" contentClass="p-6">
            {form()}
          </Scroll>
        </SidePanel>
      </If>
    </Page>
  );
}
