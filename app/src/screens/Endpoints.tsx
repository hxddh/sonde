// Endpoints: add, edit, import from AWS profiles, test connectivity, demo.

import { batch, effect, signal, Text, untracked, View } from "nsh";
import { count, duration } from "../format";
import { t } from "../i18n";
import { vendorName } from "../labels";
import { api } from "../native";
import { endpoint, endpoints, endpointsMode, notify, refreshEndpoints, report, showRequest, startDemo } from "../store";
import type { ConnectivityReport, EndpointView } from "../types";
import { Actions, Badge, Button, Card, Check, Choice, Empty, Field, FORM, Heading, IconButton, IconTile, Labeled, Mono, Muted, Notice, Page, PageHeader, Row, terminal, SwitchRow, confirmAlert } from "../ui/kit";
import { openCompatWindow } from "./CompatWindow";
import { CELL, ConfigNotice, GRID } from "./Settings";

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
        // an endpoint with no name is called by its id: the one it is saved under
        name: x.name.trim() || id,
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
      <Heading>{props.editing ? t("endpoints.edit") : t("endpoints.addTitle")}</Heading>
      <View class={GRID}>
        <Field class={CELL} label={t("endpoints.field.name")} value={d().name} onChange={(v) => set("name", v)} onSubmit={save} testID="ep-name" />
        {/* an endpoint's id names it in sessions and settings: set once */}
        <If
          when={!props.editing}
          else={
            <Labeled class={CELL} label={t("endpoints.field.id")}>
              <Mono>{d().id}</Mono>
            </Labeled>
          }
        >
          <Field class={CELL} label={t("endpoints.field.id")} value={d().id} onChange={(v) => set("id", v)} onSubmit={save} testID="ep-id" mono />
        </If>
        <Choice
          class={CELL}
          label={t("endpoints.field.vendor")}
          value={d().vendor}
          items={vendors.map((v) => ({ value: v.id, title: vendorName(v) }))}
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
        <Field class={CELL} label={t("endpoints.field.region")} value={d().region} onChange={(v) => set("region", v)} onSubmit={save} testID="ep-region" mono />
      </View>
      <Field label={t("endpoints.field.url")} value={d().url} onChange={(v) => set("url", v)} onSubmit={save} placeholder="https://s3.eu-west-1.amazonaws.com" testID="ep-url" mono />
      <SwitchRow label={t("endpoints.field.pathStyle")} checked={d().pathStyle} onChange={(v) => set("pathStyle", v)} testID="ep-path-style" />
      <View class={GRID}>
        <Choice class={CELL} label={t("endpoints.field.credentials")} value={d().credentials} items={creds} onChange={(v) => set("credentials", v)} testID="ep-creds" />
        <If when={d().credentials === "profile"} else={<View class={CELL} />}>
          <Field class={CELL} label={t("endpoints.field.profile")} value={d().profile} onChange={(v) => set("profile", v)} onSubmit={save} mono />
        </If>
      </View>
      <Field label={t("endpoints.field.ca")} value={d().caFile} onChange={(v) => set("caFile", v)} onSubmit={save} placeholder="/etc/ssl/my-ca.pem" mono />
      <Field label={t("endpoints.field.buckets")} value={d().buckets} onChange={(v) => set("buckets", v)} onSubmit={save} placeholder="logs, backups" mono />
      <Actions note={<Muted class="text-small">{t("endpoints.cred.note")}</Muted>}>
        <Button variant="ghost" label={t("common.cancel")} onPress={props.onDone} testID="ep-cancel" />
        <Button variant="primary" label={t("common.save")} onPress={save} testID="ep-save" disabled={!d().url} />
      </Actions>
    </View>
  );
}

/** What a test found, said once: reachable (a note) or not (what went wrong, and its request). */
function TestResult(props: { report: ConnectivityReport }) {
  const r = () => props.report;
  const facts = (): string => {
    const out: [string, string][] = [];
    if (r().credentialsSource) out.push([t("endpoints.credentials"), r().credentialsSource!]);
    if (r().bucketCount !== null) out.push([t("browse.buckets"), count(r().bucketCount!)]);
    if (r().owner) out.push([t("endpoints.owner"), r().owner!]);
    if (r().server) out.push([t("endpoints.server"), r().server!]);
    // who the credentials are, or why that is not known: one line
    const who = [r().identity ? `${r().identity!.arn ?? ""} (${r().identity!.account ?? ""})` : "", r().identityNote ?? ""].filter(Boolean).join(" · ");
    if (who) out.push([t("endpoints.identity"), who]);
    return [...(r().error ? [r().error!.message] : []), ...out.map(([k, v]) => `${k}: ${v}`)].join("\n");
  };
  return (
    <Notice testID="test-result" tone={r().ok ? "info" : "error"} icon={r().ok ? "circle-check" : undefined} title={r().ok ? t("endpoints.reachable", { ms: duration(r().latencyMs) }) : t("endpoints.unreachable")} text={facts()}>
      <If when={r().error && r().error!.recordId !== null && r().error!.recordId !== undefined}>
        <Button small label={t("common.showRequest")} onPress={() => showRequest(r().error!.recordId)} />
      </If>
    </Notice>
  );
}

function Import(props: { onDone: () => void }) {
  const items = api.importableProfiles();
  const chosen = signal<Record<string, boolean>>({});
  const names = () => items.filter((e) => chosen()[e.id]).map((e) => e.credentials.replace(/^profile /, ""));
  const doImport = () => {
    try {
      const n = api.importProfiles(names());
      refreshEndpoints();
      notify(t("welcome.imported", { n }));
      props.onDone();
    } catch (e) {
      report(e);
    }
  };
  return (
    <View class="gap-4">
      <Heading>{t("endpoints.importTitle")}</Heading>
      <If when={items.length > 0} else={<Muted>{t("endpoints.importNone")}</Muted>}>
        <View class="gap-2">
          <Each of={items}>
            {(e) => <Check label={`${e().name} — ${e().url}`} checked={!!chosen()[e().id]} onChange={(v) => chosen.update((c) => ({ ...c, [e().id]: v }))} />}
          </Each>
        </View>
      </If>
      <Actions>
        <Button variant="ghost" label={t("common.cancel")} onPress={props.onDone} />
        <Button variant="primary" label={t("welcome.importDo")} onPress={doImport} disabled={names().length === 0} testID="import-do" />
      </Actions>
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
    if (!(await confirmAlert({ title: t("endpoints.removeTitle", { name: e.name }), text: t("endpoints.removeText"), commit: t("common.remove") }))) return;
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
      <View testID={`endpoint-${e.id}`} class="gap-2">
        <View class="flex-row items-center gap-3">
          <If when={!terminal()}>
            <IconTile icon="server" tint="foreground" />
          </If>
          <View class="min-w-0 flex-1 gap-1">
            <View class="flex-row items-center gap-2">
              <Heading class="min-w-0 shrink line-clamp-1">{e.name}</Heading>
              <Badge label={vendorName(e)} />
              <If when={endpoint() === e.id}>
                <Badge label={t("endpoints.current")} tone="primary" />
              </If>
            </View>
            <Text class="font-mono text-caption text-muted-foreground line-clamp-1">{`${e.url} · ${e.region || "—"} · ${e.credentials}`}</Text>
          </View>
          <Button small variant="outline" icon="activity" label={r() === "running" ? t("endpoints.testing") : t("endpoints.test")} onPress={() => test(e.id)} disabled={r() === "running"} testID={`test-${e.id}`} />
          <IconButton icon="pencil" label={t("common.edit")} onPress={() => mode.set({ edit: e })} testID={`edit-${e.id}`} />
          <IconButton icon="trash" label={t("endpoints.removeMenu")} onPress={() => void remove(e)} testID={`remove-${e.id}`} />
        </View>
        <If when={typeof r() === "object" ? (r() as ConnectivityReport) : null}>{(rep) => <TestResult report={rep()} />}</If>
      </View>
    );
  };
  return (
    <Page testID="screen-endpoints">
      <View class={terminal() ? "gap-2" : `${FORM} gap-6`}>
        <ConfigNotice />
        <PageHeader title={t("endpoints.title")}>
          <If when={!terminal() && endpoints().length > 0}>
            <Button variant="outline" label={t("compat.open")} tip={t("compat.hint")} onPress={() => openCompatWindow()} testID="open-compat" />
          </If>
          {/* one primary at a time: while a form is open its Save is the one, and these wait for it */}
          <Button variant="outline" label={t("endpoints.import")} onPress={() => mode.set("import")} disabled={mode() !== "list"} testID="import-endpoints" />
          <Button variant={mode() === "list" ? "primary" : "outline"} icon="plus" label={t("endpoints.add")} onPress={() => mode.set("add")} disabled={mode() !== "list"} testID="add-endpoint" />
        </PageHeader>
        {/* the form in place, a card of its own: a sheet in the Settings
            window would cover its tabs and lose its icons (nsh-issues) */}
        <If when={form()}>{(f) => <Card testID="endpoint-form-card">{f()}</Card>}</If>
        <If when={endpoints().length > 0} else={<Empty icon="server" text={t("endpoints.noneYet")} />}>
          <Card pad="none">
            <Each of={endpoints()} key={(e) => e.id}>
              {(e, i) => <Row stack first={i() === 0}>{row(e())}</Row>}
            </Each>
          </Card>
        </If>
        {/* a store to try sonde on, with or without endpoints of one's own */}
        <View class="flex-row">
          <Button small variant="ghost" icon="sparkles" label={t("endpoints.demo")} onPress={() => void startDemo()} testID="open-demo" />
        </View>
      </View>
    </Page>
  );
}
