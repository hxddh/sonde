// A request in full, with no state of the app's own: the inspector's view,
// in the app and in the web viewer. Its parts fold away (Collapsible); an
// error body is highlighted as the XML it is (TextView).

import { type JSX, signal, Text, View } from "nsh";
import { Button as BaseButton, Collapsible, TextView } from "nsh/base";
import { bytes, ms } from "../format";
import { t } from "../i18n";
import type { Header, RequestRecord } from "../types";
import { Icon } from "./icon";
import { Button, Dot, terminal } from "./kit";
import { statusTone, toneText } from "./sessionview";

/** A part of the request that folds: its title and count, open or not. */
export function Fold(props: { title: string; count?: number; open?: boolean; children: JSX.Element; testID?: string }) {
  const open = signal(props.open ?? true);
  return (
    <Collapsible
      testID={props.testID}
      open={open()}
      reveal={!terminal()}
      class="gap-1.5"
      content={<View class="pb-1">{props.children}</View>}
    >
      <BaseButton accessibilityLabel={props.title} onPress={() => open.set(!open())} class="flex-row items-center gap-1.5 self-start rounded-(--radius-sm) py-0.5 pr-1 hover:bg-muted">
        <If when={!terminal()} else={<Text class="text-muted-foreground">{open() ? "▾" : "▸"}</Text>}>
          <Icon name={open() ? "chevron-down" : "chevron-right"} size={13} />
        </If>
        <Text role="header" class="text-xs font-semibold text-muted-foreground">
          {props.count === undefined ? props.title : `${props.title} (${props.count})`}
        </Text>
      </BaseButton>
    </Collapsible>
  );
}

/** Name and value pairs: names muted, values monospaced and selectable. */
export function Pairs(props: { rows: [string, string][]; mono?: boolean }) {
  return (
    <View selectable class="overflow-hidden rounded-(--radius) border border-border bg-card">
      <Each of={props.rows}>
        {(row, i) => (
          <View class={`flex-row gap-3 px-2.5 py-1 ${i() > 0 ? "border-t border-border" : ""}`}>
            <Text class="w-[136px] shrink-0 font-mono text-[11px] text-muted-foreground line-clamp-1">{row()[0]}</Text>
            <Text class={`min-w-0 flex-1 text-[11px] text-foreground ${props.mono === false ? "" : "font-mono"}`}>
              {row()[1]}
            </Text>
          </View>
        )}
      </Each>
    </View>
  );
}

const headerRows = (h: Header[]): [string, string][] => h.map((x) => [x.name, x.value]);

/** A request in full: status, timing, ids, then its parts: error, query, headers. */
export function RecordView(props: { record: RequestRecord; onBack?: () => void; actions?: JSX.Element }) {
  const r = () => props.record;
  const tone = () => statusTone(r().status);
  const facts = (): [string, string][] =>
    [
      [t("column.time"), r().time],
      [t("common.endpoint"), r().endpoint],
      [t("requests.latency"), ms(r().latencyMs)],
      [t("requests.size"), `↑${bytes(r().requestBytes ?? 0)} ↓${bytes(r().responseBytes ?? 0)}`],
      [t("requests.retries"), String(r().retries)],
      ["x-amz-request-id", r().requestId ?? "—"],
      ["x-amz-id-2", r().hostId ?? "—"],
      ["Region", r().region ?? "—"],
      ...(r().job ? ([["Job", r().job!]] as [string, string][]) : []),
    ] as [string, string][];
  return (
    <View selectable class="gap-4" testID="record-detail">
      <View class="gap-1.5">
        <View class="flex-row items-center gap-2">
          <If when={props.onBack}>
            <Button small variant="ghost" icon="chevron-left" label={t("common.back")} onPress={() => props.onBack?.()} testID="record-back" />
          </If>
          <Dot tone={tone()} />
          <Text class={`font-mono text-xs font-semibold ${toneText(tone())}`}>{r().status === null ? t("requests.transport") : String(r().status)}</Text>
          <Text class="text-[15px] font-semibold text-foreground">{r().operation}</Text>
        </View>
        <Text class="font-mono text-[11px] leading-4 text-muted-foreground">{`${r().method} ${r().url}`}</Text>
      </View>
      <If when={props.actions}>
        <View class="flex-row flex-wrap gap-1.5">{props.actions}</View>
      </If>
      <If when={r().transportError}>
        {(e) => (
          <View class="flex-row gap-2 rounded-(--radius) border border-destructive/30 bg-destructive/10 px-2.5 py-2">
            <Icon name="circle-x" size={14} tint="destructive" />
            <Text class="flex-1 text-xs text-destructive">
              {e()}
            </Text>
          </View>
        )}
      </If>
      <If when={r().errorBody}>
        {(b) => (
          <Fold title={t("requests.errorBody")} testID="record-error">
            <If when={!terminal()} else={<Text class="text-foreground">{b()}</Text>}>
              <TextView
                selectable
                markdown={"```xml\n" + b().trim() + "\n```"}
                codeBlockActions={[{ id: "copy" }]}
                classes={{ codeBlock: "rounded-(--radius) border border-border bg-card", codeBlockCode: "p-2.5 text-[11px]" }}
              />
            </If>
          </Fold>
        )}
      </If>
      <Fold title={t("requests.facts")}>
        <Pairs rows={facts()} />
      </Fold>
      <If when={r().query.length > 0}>
        <Fold title={t("requests.query")} count={r().query.length}>
          <Pairs rows={r().query.map(([k, v]) => [k, v])} />
        </Fold>
      </If>
      <Fold title={t("requests.requestHeaders")} count={r().requestHeaders.length}>
        <Pairs rows={headerRows(r().requestHeaders)} />
      </Fold>
      <Fold title={t("requests.responseHeaders")} count={r().responseHeaders.length}>
        <Pairs rows={headerRows(r().responseHeaders)} />
      </Fold>
    </View>
  );
}
