// A request in full, with no state of the app's own: the inspector's view,
// in the app and in the web viewer.

import { type JSX, Text, View } from "nsh";
import { ms } from "../format";
import { t } from "../i18n";
import type { RequestRecord } from "../types";
import { Badge, Button, Heading, KeyValue, Mono, SimpleTable } from "./kit";

function statusTone(s: number | null) {
  if (s === null) return "fail" as const;
  if (s >= 500) return "fail" as const;
  if (s >= 400) return "warn" as const;
  return "pass" as const;
}

/** A request in full: status, timing, ids, headers, query and error body. */
export function RecordView(props: { record: RequestRecord; onBack?: () => void; actions?: JSX.Element }) {
  const r = () => props.record;

  return (
    <View class="gap-3" testID="record-detail">
      <View class="flex-row items-center gap-2">
        <If when={props.onBack}>
          <Button small variant="ghost" label={`‹ ${t("common.back")}`} onPress={() => props.onBack?.()} testID="record-back" />
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
      {props.actions}
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
