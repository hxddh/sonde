// The request inspector: every request sonde made, redacted, filtered by
// status, operation and endpoint; copy as aws cli or curl; export.

import { batch, cleanup, Clipboard, effect, Scroll, signal, Text, View } from "nsh";
import { DataTable } from "nsh/base";
import { date, ms } from "../format";
import { t } from "../i18n";
import { api } from "../native";
import { endpoints, focusRecord, notify, report } from "../store";
import type { RecordSummary, RequestFilter, RequestRecord } from "../types";
import { Badge, Button, Choice, Empty, Field, Heading, KeyValue, Mono, Muted, SimpleTable, terminal, Toggle } from "../ui/kit";

function statusTone(s: number | null) {
  if (s === null) return "fail" as const;
  if (s >= 500) return "fail" as const;
  if (s >= 400) return "warn" as const;
  return "pass" as const;
}

function RecordDetail(props: { record: RequestRecord; onBack?: () => void }) {
  const r = () => props.record;
  const copy = (text: string | null) => {
    if (!text) return;
    Clipboard.writeText(text).then(() => notify(t("common.copied")), report);
  };
  return (
    <View class="gap-3" testID="record-detail">
      <View class="flex-row items-center gap-2">
        <If when={props.onBack}>
          <Button small variant="ghost" label={`‹ ${t("common.back")}`} onPress={() => props.onBack?.()} />
        </If>
        <Badge label={r().status === null ? t("requests.transport") : String(r().status)} tone={statusTone(r().status)} />
        <Text class="text-base font-semibold text-foreground">{r().operation}</Text>
      </View>
      <Mono>{`${r().method} ${r().url}`}</Mono>
      <KeyValue
        rows={[
          ["Time", r().time],
          [t("common.endpoint"), r().endpoint],
          [t("requests.latency"), ms(r().latencyMs)],
          [t("requests.retries"), String(r().retries)],
          ["x-amz-request-id", r().requestId ?? "—"],
          ["x-amz-id-2", r().hostId ?? "—"],
          ["Region", r().region ?? "—"],
          ["Job", r().job ?? "—"],
        ]}
      />
      <View class="flex-row flex-wrap gap-2">
        <Button small label={t("requests.copyCli")} onPress={() => copy(api.copyAsAwsCli(r().id))} testID="copy-cli" />
        <Button small label={t("requests.copyCurl")} onPress={() => copy(api.copyAsCurl(r().id))} testID="copy-curl" />
      </View>
      <If when={r().transportError}>
        {(e) => (
          <Text class="text-sm text-destructive">
            {e()}
          </Text>
        )}
      </If>
      <If when={r().errorBody}>
        {(b) => (
          <View class="gap-1">
            <Heading>{t("requests.errorBody")}</Heading>
            <Mono class="rounded-(--radius) bg-muted p-2">{b()}</Mono>
          </View>
        )}
      </If>
      <If when={r().query.length > 0}>
        <Heading>{t("requests.query")}</Heading>
        <SimpleTable mono columns={["name", "value"]} rows={r().query.map(([k, v]) => [k, v])} />
      </If>
      <Heading>{t("requests.requestHeaders")}</Heading>
      <SimpleTable mono columns={["header", "value"]} rows={r().requestHeaders.map((h) => [h.name, h.value])} />
      <Heading>{t("requests.responseHeaders")}</Heading>
      <SimpleTable mono columns={["header", "value"]} rows={r().responseHeaders.map((h) => [h.name, h.value])} />
    </View>
  );
}

export function Requests() {
  const status = signal("all");
  const operation = signal("");
  const ep = signal("");
  const text = signal("");
  const retried = signal(false);
  const latest = signal(api.latestRequest());
  const selected = signal<number | null>(focusRecord());
  effect(() => {
    const f = focusRecord();
    if (f !== null) {
      batch(() => {
        selected.set(f);
        focusRecord.set(null);
      });
    }
  });
  const timer = setInterval(() => {
    const l = api.latestRequest();
    if (l !== latest()) latest.set(l);
  }, 700);
  cleanup(() => clearInterval(timer));

  const filter = (): RequestFilter => ({
    status: status() === "errors" ? { kind: "errors" } : status() === "transport" ? { kind: "transport" } : status() === "all" ? null : { kind: "class", value: Number(status()) },
    operation: operation() || null,
    endpoint: ep() || null,
    text: text() || null,
    retried: retried(),
  });
  const rows = (): RecordSummary[] => {
    latest();
    try {
      return api.requests(filter(), 5000);
    } catch (e) {
      report(e);
      return [];
    }
  };
  const record = () => {
    const id = selected();
    return id === null ? null : api.request(id);
  };
  const exportAs = (f: "json" | "har") => {
    try {
      notify(t("common.exportedTo", { path: api.exportRequests(f, filter()) }));
    } catch (e) {
      report(e);
    }
  };
  const table = () => (
    <DataTable
      testID="requests"
      class="flex-1 rounded-(--radius) border border-border"
      columns={[
        { key: "time", title: "Time", width: 130 },
        { key: "status", title: t("requests.status"), width: 64, align: "end" },
        { key: "operation", title: t("requests.operation"), width: 190 },
        { key: "target", title: "Target", width: 260 },
        { key: "latency", title: t("requests.latency"), width: 80, align: "end" },
        { key: "retries", title: "↻", width: 36, align: "end" },
        { key: "requestId", title: t("common.requestId"), width: 220 },
      ]}
      data={rows().map((r) => ({ time: date(r.time).slice(5), status: r.status === null ? "—" : String(r.status), operation: r.operation, target: r.target, latency: ms(r.latencyMs), retries: r.retries ? String(r.retries) : "", requestId: r.requestId ?? "" }))}
      rowClasses={rows().map((r) => (r.status === null || r.status >= 500 ? "text-fail" : r.status >= 400 ? "text-warn" : ""))}
      rowSelectable
      onRowPress={({ row }) => selected.set(rows()[row]?.id ?? null)}
      empty={<Empty text={t("common.empty")} />}
    />
  );
  return (
    <View class="flex-1 gap-2 p-3" testID="screen-requests">
      <View class="flex-row flex-wrap items-end gap-2">
        <Choice
          class="w-32"
          label={t("requests.status")}
          value={status()}
          items={[
            { value: "all", title: t("requests.all") },
            { value: "errors", title: t("requests.errors") },
            { value: "2", title: "2xx" },
            { value: "3", title: "3xx" },
            { value: "4", title: "4xx" },
            { value: "5", title: "5xx" },
            { value: "transport", title: t("requests.transport") },
          ]}
          onChange={(v) => status.set(v)}
          testID="status-filter"
        />
        <Choice class="w-48" label={t("requests.operation")} value={operation() || "*"} items={[{ value: "*", title: t("requests.all") }, ...api.requestOperations().map((o) => ({ value: o, title: o }))]} onChange={(v) => operation.set(v === "*" ? "" : v)} />
        <Choice class="w-40" label={t("common.endpoint")} value={ep() || "*"} items={[{ value: "*", title: t("requests.all") }, ...endpoints().map((e) => ({ value: e.id, title: e.name }))]} onChange={(v) => ep.set(v === "*" ? "" : v)} />
        <Field class="w-48" label={t("common.search")} value={text()} onChange={(v) => text.set(v)} placeholder="key, bucket, request id" />
        <Toggle label={t("requests.retried")} checked={retried()} onChange={(v) => retried.set(v)} />
        <View class="flex-1" />
        <Button small label={t("requests.exportJson")} onPress={() => exportAs("json")} />
        <Button small label={t("requests.exportHar")} onPress={() => exportAs("har")} testID="export-har" />
        <Button small variant="ghost" label={t("requests.clear")} onPress={() => (api.clearRequests(), latest.set(api.latestRequest()))} />
      </View>
      <Muted>{`${rows().length} records`}</Muted>
      <If
        when={terminal()}
        else={
          <View class="flex-1 flex-row gap-3">
            <View class="flex-1">{table()}</View>
            <View class="w-[420px] border-l border-border pl-3">
              <If when={record()} else={<Empty text={t("requests.select")} />}>
                {(r) => (
                  <Scroll class="flex-1">
                    <RecordDetail record={r()} />
                  </Scroll>
                )}
              </If>
            </View>
          </View>
        }
      >
        <If when={record()} else={table()}>
          {(r) => (
            <Scroll class="flex-1">
              <RecordDetail record={r()} onBack={() => selected.set(null)} />
            </Scroll>
          )}
        </If>
      </If>
    </View>
  );
}
