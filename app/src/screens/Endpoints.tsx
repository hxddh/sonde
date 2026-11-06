// Endpoints: add, edit, import from AWS profiles, test connectivity, demo.

import { batch, signal, Text, View } from "nsh";
import { t } from "../i18n";
import { api } from "../native";
import { endpoint, endpoints, notify, refreshEndpoints, report, showRequest } from "../store";
import type { ConnectivityReport, EndpointView } from "../types";
import { Badge, Button, Card, Check, Choice, Empty, Field, Heading, KeyValue, Muted, Page, Title, Toggle } from "../ui/kit";
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

const fromView = (e: EndpointView): Draft => ({ id: e.id, name: e.name, url: e.url, region: e.region, pathStyle: e.pathStyle, caFile: "", vendor: e.vendor, credentials: "keep", profile: "", buckets: e.buckets.join(", ") });

function Form(props: { initial: Draft; editing: boolean; onDone: () => void }) {
  const d = signal<Draft>(props.initial);
  const set = (k: keyof Draft, v: string | boolean) => d.update((x) => ({ ...x, [k]: v }));
  const vendors = api.vendors();
  const save = () => {
    const x = d();
    try {
      api.saveEndpoint({
        id: x.id.trim() || x.name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-"),
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
    <Card testID="endpoint-form">
      <Title>{props.editing ? t("common.edit") : t("endpoints.add")}</Title>
      <View class="flex-row flex-wrap gap-3">
        <Field class="w-56" label={t("endpoints.field.name")} value={d().name} onChange={(v) => set("name", v)} testID="ep-name" />
        <Field class="w-40" label={t("endpoints.field.id")} value={d().id} onChange={(v) => set("id", v)} testID="ep-id" mono />
        <Choice
          class="w-48"
          label={t("endpoints.field.vendor")}
          value={d().vendor}
          items={vendors.map((v) => ({ value: v.id, title: v.label }))}
          onChange={(v) => {
            const info = vendors.find((x) => x.id === v);
            batch(() => {
              set("vendor", v);
              if (info) {
                if (!d().url) set("url", info.urlTemplate.replace("{region}", d().region || info.region));
                set("pathStyle", info.pathStyle);
                if (!d().region || d().region === "us-east-1") set("region", info.region);
              }
            });
          }}
        />
      </View>
      <View class="flex-row flex-wrap gap-3">
        <Field class="flex-1" label={t("endpoints.field.url")} value={d().url} onChange={(v) => set("url", v)} placeholder="https://s3.eu-west-1.amazonaws.com" testID="ep-url" mono />
        <Field class="w-36" label={t("endpoints.field.region")} value={d().region} onChange={(v) => set("region", v)} testID="ep-region" mono />
      </View>
      <Toggle label={t("endpoints.field.pathStyle")} checked={d().pathStyle} onChange={(v) => set("pathStyle", v)} testID="ep-path-style" />
      <Field label={t("endpoints.field.ca")} value={d().caFile} onChange={(v) => set("caFile", v)} placeholder="/etc/ssl/my-ca.pem" mono />
      <View class="flex-row flex-wrap gap-3">
        <Choice class="w-64" label={t("endpoints.field.credentials")} value={d().credentials} items={creds} onChange={(v) => set("credentials", v)} testID="ep-creds" />
        <If when={d().credentials === "profile"}>
          <Field class="w-48" label={t("endpoints.field.profile")} value={d().profile} onChange={(v) => set("profile", v)} mono />
        </If>
      </View>
      <Muted>{t("endpoints.cred.note")}</Muted>
      <Field label={t("endpoints.field.buckets")} value={d().buckets} onChange={(v) => set("buckets", v)} placeholder="logs, backups" mono />
      <View class="flex-row gap-2">
        <Button variant="primary" label={t("common.save")} onPress={save} testID="ep-save" disabled={!d().url} />
        <Button variant="ghost" label={t("common.cancel")} onPress={props.onDone} />
      </View>
    </Card>
  );
}

function TestResult(props: { report: ConnectivityReport }) {
  const r = () => props.report;
  const rows = (): [string, string][] => {
    const out: [string, string][] = [];
    if (r().credentialsSource) out.push([t("endpoints.credentials"), r().credentialsSource!]);
    if (r().bucketCount !== null) out.push([t("browse.buckets"), String(r().bucketCount)]);
    if (r().owner) out.push(["Owner", r().owner!]);
    if (r().server) out.push(["Server", r().server!]);
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
            <Text class="flex-1 text-sm text-destructive">
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
    <Card>
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
    </Card>
  );
}

export function Endpoints() {
  const mode = signal<"list" | "add" | "import" | { edit: EndpointView }>("list");
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
  const demo = async () => {
    try {
      const e = await api.startDemo();
      refreshEndpoints();
      endpoint.set(e.id);
    } catch (e) {
      report(e);
    }
  };
  const remove = (id: string) => {
    try {
      api.removeEndpoint(id);
      refreshEndpoints();
    } catch (e) {
      report(e);
    }
  };
  return (
    <Page testID="screen-endpoints">
      <View class="flex-row flex-wrap items-center gap-2">
        <Title class="flex-1">{t("endpoints.title")}</Title>
        <Button variant="primary" label={t("endpoints.add")} onPress={() => mode.set("add")} testID="add-endpoint" />
        <Button label={t("endpoints.import")} onPress={() => mode.set("import")} />
        <Button label={t("endpoints.demo")} onPress={demo} testID="open-demo" />
      </View>
      <If when={mode() === "add"}>
        <Form initial={blank()} editing={false} onDone={() => mode.set("list")} />
      </If>
      <If when={mode() === "import"}>
        <Import onDone={() => mode.set("list")} />
      </If>
      <If when={typeof mode() === "object" ? (mode() as { edit: EndpointView }) : null}>
        {(m) => <Form initial={fromView(m().edit)} editing onDone={() => mode.set("list")} />}
      </If>
      <If when={endpoints().length > 0} else={<Empty text={t("endpoints.noneYet")} />}>
        <Each of={endpoints()} key={(e) => e.id}>
          {(e) => (
            <Card testID={`endpoint-${e().id}`}>
              <View class="flex-row items-center gap-2">
                <Text class="text-base font-semibold text-foreground">{e().name}</Text>
                <Badge label={e().vendorLabel} tone="primary" />
                <If when={e().pathStyle}>
                  <Badge label="path-style" />
                </If>
                <View class="flex-1" />
                <Button small label={tests()[e().id] === "running" ? t("endpoints.testing") : t("endpoints.test")} onPress={() => test(e().id)} disabled={tests()[e().id] === "running"} testID={`test-${e().id}`} />
                <Button small variant="ghost" label={t("common.edit")} onPress={() => mode.set({ edit: e() })} />
                <Button small variant="ghost" label={t("common.remove")} onPress={() => remove(e().id)} />
              </View>
              <KeyValue rows={[["URL", e().url], [t("endpoints.field.region"), e().region || "—"], [t("endpoints.credentials"), e().credentials]]} />
              <If when={typeof tests()[e().id] === "object" ? (tests()[e().id] as ConnectivityReport) : null}>{(r) => <TestResult report={r()} />}</If>
            </Card>
          )}
        </Each>
      </If>
      <Heading>{t("settings.telemetry")}</Heading>
    </Page>
  );
}
