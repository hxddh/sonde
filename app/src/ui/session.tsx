// The pieces of a session shared by Run and Sessions: its header,
// observations, operations, an operation's requests, the program's output,
// and the panel that records without a program.

import { Clipboard, signal, Text, View } from "nsh";
import { t } from "../i18n";
import { api } from "../native";
import { endpoint, endpoints, notify, report } from "../store";
import type { Episode, RecordingView } from "../types";
import { Button, Choice, Mono, Muted, Segmented } from "./kit";
import { OperationView } from "./sessionview";

export { clock, linesOf, Observations, OperationsTable, OutputTable, operationsAt, seconds, SessionHeader, SessionList } from "./sessionview";

export type Policy = "allow" | "no-delete" | "read-only";


export function copy(text: string | null) {
  if (!text) return;
  Clipboard.writeText(text).then(() => notify(t("common.copied")), report);
}


/** Start a recording with no program, or show the one that runs. `policy`:
 * the caller's choice (no switch of its own then). */
export function RecordPanel(props: { recording: RecordingView | null; onChange: () => void; policy?: () => Policy }) {
  const open = signal(false);
  const own = signal<Policy>("allow");
  const policy = () => props.policy?.() ?? own();
  const target = signal<string | null>(endpoint());
  const start = async () => {
    const ep = target() ?? endpoint();
    if (!ep) return;
    try {
      await api.sessions.start(ep, policy());
      open.set(false);
      props.onChange();
    } catch (e) {
      report(e);
    }
  };
  return (
    <If
      when={props.recording}
      else={
        <If when={open()} else={<Button label={t("timeline.record")} onPress={() => open.set(true)} testID="record" />}>
          <View class="gap-2 rounded-(--radius) border border-border p-2" testID="record-form">
            <Text class="text-sm font-semibold text-foreground">{t("timeline.recordTitle")}</Text>
            <Muted class="text-xs">{t("timeline.recordHint")}</Muted>
            <Choice label={t("common.endpoint")} value={target() ?? endpoint()} items={endpoints().map((e) => ({ value: e.id, title: e.name }))} onChange={(v) => target.set(v)} />
            <If when={!props.policy}>
            <Text class="text-xs text-muted-foreground">{t("timeline.policy")}</Text>
            <Segmented
              value={policy()}
              items={[
                { value: "allow", title: t("timeline.allow") },
                { value: "no-delete", title: t("timeline.noDelete") },
                { value: "read-only", title: t("timeline.readOnly") },
              ]}
              onChange={(v) => own.set(v as Policy)}
            />
            </If>
            <View class="flex-row gap-2">
              <Button label={t("timeline.start")} onPress={start} disabled={!(target() ?? endpoint())} testID="record-start" />
              <Button variant="ghost" label={t("common.cancel")} onPress={() => open.set(false)} />
            </View>
          </View>
        </If>
      }
    >
      {(r) => (
        <View class="gap-2 rounded-(--radius) border border-primary p-2" testID="recording">
          <View class="flex-row items-center gap-2">
            <Text class="text-primary">●</Text>
            <Text class="flex-1 text-sm font-semibold text-foreground line-clamp-1">{t("timeline.recording", { url: r().url })}</Text>
          </View>
          <Muted class="text-xs">{t("timeline.pointApp")}</Muted>
          <Mono class="rounded-(--radius) bg-muted p-2 text-xs">{r().exports.trim()}</Mono>
          <View class="flex-row gap-2">
            <Button small label={t("timeline.copyExports")} onPress={() => copy(r().exports)} testID="copy-exports" />
            <Button
              small
              variant="destructive"
              label={t("timeline.stop")}
              onPress={() => {
                api.sessions.stop();
                props.onChange();
              }}
              testID="record-stop"
            />
          </View>
        </View>
      )}
    </If>
  );
}





export function OperationDetail(props: { path: string; episode: Episode; shared: string[]; onRecord: (id: number) => void; onBack?: () => void }) {
  const rows = () => {
    try {
      return api.sessions.records(props.path, props.episode.records);
    } catch (e) {
      report(e);
      return [];
    }
  };
  return (
    <OperationView
      episode={props.episode}
      shared={props.shared}
      rows={rows()}
      onRecord={props.onRecord}
      back={props.onBack ? <Button small variant="ghost" label={`‹ ${t("common.back")}`} onPress={() => props.onBack?.()} testID="operation-back" /> : undefined}
    />
  );
}





