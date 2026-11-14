// The pieces of a session shared by Run and Sessions: its header,
// observations, operations, an operation's requests, the program's output,
// and the panel that records without a program.

import { signal, Text, View } from "nsh";
import { t } from "../i18n";
import { api } from "../native";
import { copy, endpoint, endpoints, report } from "../store";
import type { EpisodeRow, RecordingView } from "../types";
import { Button, Card, Dot, Heading, IconTile, Options, terminal, Row } from "./kit";
import { OperationView } from "./sessionview";

export { clock, episodeTone, linesOf, Observations, OperationsTable, OutputTable, SessionHeader, SessionList, statusTone } from "./sessionview";

export type Policy = "allow" | "no-delete" | "read-only";


/** Start a recording with no program, or show the one that runs: the
 * endpoint and keys to point an application at. `policy`: the caller's
 * choice (no switch of its own then). */
export function RecordPanel(props: { recording: RecordingView | null; onChange: () => void; policy?: () => Policy }) {
  const own = signal<Policy>("no-delete");
  const policy = () => props.policy?.() ?? own();
  const open = signal(false);
  const start = async () => {
    const ep = endpoint();
    if (!ep) return;
    try {
      await api.sessions.start(ep, policy());
      open.set(false);
      props.onChange();
    } catch (e) {
      report(e);
    }
  };
  const ep = () => endpoints().find((e) => e.id === endpoint());
  return (
    <If
      when={props.recording}
      else={
        <Card pad="none" testID={open() ? "record-form" : undefined}>
          <Row first>
            <If when={!terminal()}>
              <IconTile icon="radio" tint="foreground" />
            </If>
            <View class="min-w-0 flex-1 gap-1">
              <Heading>{t("timeline.recordTitle")}</Heading>
              <Text class="text-small text-muted-foreground">{t("timeline.recordHint")}</Text>
            </View>
            <If when={!open()}>
              <Button variant="outline" label={t("timeline.record")} onPress={() => open.set(true)} testID="record" />
            </If>
          </Row>
          <If when={open()}>
            <Row stack>
              <Text class="text-small text-muted-foreground">{ep() ? t("run.goesTo", { name: ep()!.name }) : t("run.chooseEndpoint")}</Text>
              <If when={!props.policy}>
                <View class="flex-row">
                <Options
                  value={policy()}
                  items={[
                    { value: "allow", title: t("timeline.allow") },
                    { value: "no-delete", title: t("timeline.noDelete") },
                    { value: "read-only", title: t("timeline.readOnly") },
                  ]}
                  onChange={(v) => own.set(v as Policy)}
                />
                </View>
              </If>
              <View class="flex-row gap-2">
                <Button small variant="primary" icon="radio" label={t("timeline.start")} onPress={start} disabled={!endpoint()} testID="record-start" />
                <Button small variant="ghost" label={t("common.cancel")} onPress={() => open.set(false)} testID="record-cancel" />
              </View>
            </Row>
          </If>
        </Card>
      }
    >
      {(r) => (
        <Card class="border-primary/40" testID="recording">
          <View class="flex-row items-center gap-2">
            <Dot tone="live" />
            <Heading class="flex-1 line-clamp-1">{t("timeline.recording", { url: r().url })}</Heading>
            <Button small variant="destructive" icon="square" label={t("timeline.stop")} onPress={() => { api.sessions.stop(); props.onChange(); }} testID="record-stop" />
          </View>
          <Text class="text-small text-muted-foreground">{t("timeline.pointApp")}</Text>
          <View selectable class="gap-0 rounded-(--radius) border border-border bg-muted/50 px-3 py-2">
            <Each of={r().exports.trim().split("\n")}>{(line) => <Text class="font-mono text-caption text-foreground">{line()}</Text>}</Each>
          </View>
          <View class="flex-row">
            <Button small variant="outline" icon="copy" label={t("timeline.copyExports")} onPress={() => copy(r().exports)} testID="copy-exports" />
          </View>
        </Card>
      )}
    </If>
  );
}

/** An operation in the inspector, its requests read from its session (`episode`: whole, with every request's id). */
export function OperationDetail(props: { path: string; episode: EpisodeRow; shared: string[]; onRecord: (id: number) => void; onBack?: () => void }) {
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
      back={props.onBack ? <Button small variant="ghost" icon="chevron-left" label={t("common.back")} onPress={() => props.onBack?.()} testID="operation-back" /> : undefined}
    />
  );
}





