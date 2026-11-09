// A request in full, with no state of the app's own: the inspector's view,
// in the app and in the web viewer. Its parts fold away (Accordion); an
// error body is highlighted as the XML it is (TextView).

import { type JSX, signal, Text, View } from "nsh";
import { Editor, type EditorDiagnostic } from "nsh/base";
import { bytes, day, ms } from "../format";
import { t } from "../i18n";
import type { Header, RequestRecord } from "../types";
import { Icon } from "./icon";
import { Button, Disclosure, Dot, KeyValue, terminal } from "./kit";
import { clock, StatusMark, statusTone, toneText } from "./sessionview";

/** A part of the request that folds: its title and count, open or not
 * (the kit's Disclosure: a heading, and a button that says it expands). */
export function Fold(props: { title: string; count?: number; open?: boolean; children: JSX.Element; testID?: string }) {
  return (
    <Disclosure title={props.title} count={props.count} open={props.open ?? true} testID={props.testID}>
      <View class="pb-1">{props.children}</View>
    </Disclosure>
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

/**
 * An XML document one element per line, indented: what S3 sends on one line
 * (after its declaration) reads as a document. Text stays as it was, an
 * element with only text (or nothing) stays on one line, and a document that
 * is already laid out is left alone.
 */
export function pretty(xml: string): string {
  const body = xml.replace(/^<\?xml[^>]*\?>\s*/, "");
  if (/\n\s+</.test(body)) return xml.trim();
  const tokens = body.match(/<!\[CDATA\[[\s\S]*?\]\]>|<!--[\s\S]*?-->|<[^>]+>|[^<]+/g) ?? [];
  const lines: string[] = [];
  let depth = 0;
  for (let k = 0; k < tokens.length; k++) {
    const tok = tokens[k];
    const pad = "  ".repeat(depth);
    if (tok.startsWith("</")) {
      depth = Math.max(0, depth - 1);
      lines.push("  ".repeat(depth) + tok);
    } else if (tok.startsWith("<") && !tok.startsWith("<!") && !tok.startsWith("<?") && !tok.endsWith("/>")) {
      // <a>text</a> and <a></a> on one line, the text as it was
      const next = tokens[k + 1];
      const after = tokens[k + 2];
      if (next?.startsWith("</")) {
        lines.push(pad + tok + next);
        k += 1;
      } else if (next !== undefined && !next.startsWith("<") && after?.startsWith("</")) {
        lines.push(pad + tok + next + after);
        k += 2;
      } else {
        lines.push(pad + tok);
        depth++;
      }
    } else if (!tok.startsWith("<") && !tok.trim()) {
      // whitespace between elements
    } else {
      lines.push(pad + (tok.startsWith("<") ? tok : tok.trim()));
    }
  }
  const decl = /^<\?xml[^>]*\?>/.exec(xml.trim())?.[0];
  return [decl, ...lines].filter(Boolean).join("\n");
}


const headerRows = (h: Header[]): [string, string][] => h.map((x) => [x.name, x.value]);

/** A request in full: status, timing, ids, then its parts: error, query, headers. */
export function RecordView(props: { record: RequestRecord; onBack?: () => void; actions?: JSX.Element }) {
  const r = () => props.record;
  const tone = () => statusTone(r().status);
  const facts = (): [string, string][] =>
    [
      [t("column.time"), `${day(r().time)} ${clock(r().time)}`],
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
          <StatusMark status={r().status} text={r().status === null ? t("requests.transport") : undefined} />
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
        <KeyValue boxed rows={facts()} />
      </Fold>
      <If when={r().query.length > 0}>
        <Fold title={t("requests.query")} count={r().query.length}>
          <KeyValue boxed rows={r().query.map(([k, v]) => [k, v])} />
        </Fold>
      </If>
      <Fold title={t("requests.requestHeaders")} count={r().requestHeaders.length}>
        <KeyValue boxed rows={headerRows(r().requestHeaders)} />
      </Fold>
      <Fold title={t("requests.responseHeaders")} count={r().responseHeaders.length}>
        <KeyValue boxed rows={headerRows(r().responseHeaders)} />
      </Fold>
    </View>
  );
}
