// A request in full, with no state of the app's own: the inspector's view,
// in the app and in the web viewer. Its parts fold away (Collapsible); an
// error body is highlighted as the XML it is (TextView).

import { type JSX, signal, Text, View } from "nsh";
import { Button as BaseButton, Collapsible, Editor, type EditorDiagnostic } from "nsh/base";
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
        <Text role="header" class="text-small font-semibold text-muted-foreground">
          {props.count === undefined ? props.title : `${props.title} (${props.count})`}
        </Text>
      </BaseButton>
    </Collapsible>
  );
}

/** An XML document as code, read-only: the S3 error's code marked where it stands. */
export function Document(props: { text: string; testID?: string; mark?: boolean }) {
  const text = () => pretty(props.text.trim());
  const diagnostics = (): EditorDiagnostic[] => {
    if (!props.mark) return [];
    const lines = text().split("\n");
    const at = lines.findIndex((l) => l.includes("<Code>"));
    if (at < 0) return [];
    const code = /<Code>([^<]*)<\/Code>/.exec(lines[at])?.[1] ?? "";
    const message = /<Message>([^<]*)<\/Message>/.exec(text())?.[1] ?? code;
    const col = lines[at].indexOf("<Code>") + 6;
    return [{ range: { start: { line: at, column: col }, end: { line: at, column: col + code.length } }, severity: "error", message, source: "S3", code }];
  };
  return (
    <If when={!terminal()} else={<Text class="text-foreground">{text()}</Text>}>
      <Editor
        testID={props.testID}
        value={text()}
        language="xml"
        readOnly
        lineNumbers={false}
        folding
        softWrap
        minRows={2}
        maxRows={18}
        fontSize={12}
        diagnostics={diagnostics()}
        accessibilityLabel={props.mark ? diagnostics()[0]?.message : undefined}
        class="rounded-(--radius) border border-border bg-card"
      />
    </If>
  );
}

/** One element per line, indented: what S3 sends on one line reads as a document. */
function pretty(xml: string): string {
  if (xml.includes("\n")) return xml;
  let depth = 0;
  return xml
    .replace(/>\s*</g, ">\n<")
    .split("\n")
    .map((line) => {
      if (/^<\//.test(line)) depth = Math.max(0, depth - 1);
      const out = `${"  ".repeat(depth)}${line}`;
      if (/^<[^!?/][^>]*[^/]>$/.test(line) && !/<\/[^>]+>$/.test(line)) depth++;
      return out;
    })
    .join("\n");
}

/** Name and value pairs: names muted, values monospaced and selectable. */
export function Pairs(props: { rows: [string, string][]; mono?: boolean }) {
  return (
    <View selectable class="overflow-hidden rounded-(--radius) border border-border bg-card">
      <Each of={props.rows}>
        {(row, i) => (
          <View class={`flex-row gap-3 px-2.5 py-1 ${i() > 0 ? "border-t border-border" : ""}`}>
            <Text class="w-[136px] shrink-0 font-mono text-caption text-muted-foreground line-clamp-1">{row()[0]}</Text>
            <Text class={`min-w-0 flex-1 text-caption text-foreground ${props.mono === false ? "" : "font-mono"}`}>
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
          <Text class={`font-mono text-small font-semibold ${toneText(tone())}`}>{r().status === null ? t("requests.transport") : String(r().status)}</Text>
          <Text class="text-heading font-semibold text-foreground">{r().operation}</Text>
        </View>
        <Text class="font-mono text-caption leading-4 text-muted-foreground">{`${r().method} ${r().url}`}</Text>
      </View>
      <If when={props.actions}>
        <View class="flex-row flex-wrap gap-1.5">{props.actions}</View>
      </If>
      <If when={r().transportError}>
        {(e) => (
          <View class="flex-row gap-2 rounded-(--radius) border border-destructive/30 bg-destructive/10 px-2.5 py-2">
            <Icon name="circle-x" size={14} tint="destructive" />
            <Text class="flex-1 text-small text-destructive">
              {e()}
            </Text>
          </View>
        )}
      </If>
      <If when={r().errorBody}>
        {(b) => (
          <Fold title={t("requests.errorBody")} testID="record-error">
            <Document text={b()} mark testID="record-error-body" />
          </Fold>
        )}
      </If>
      <If when={r().requestBody}>
        {(b) => (
          <Fold title={r().requestBodyTruncated ? t("requests.bodyCut") : t("requests.body")} testID="record-body">
            <Document text={b()} />
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
