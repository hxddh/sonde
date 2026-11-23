// The pieces of a run shared by Run and Runs: its header,
// observations, operations, an operation's requests, the program's output,
// and recording without a program.

import { signal, Text, View } from "nsh";
import { t } from "../i18n";
import { api } from "../native";
import { copy, endpoint, endpoints, report } from "../store";
import { record } from "../run";
import type { EpisodeRow } from "../types";
import { Button, Card, Dot, Heading, IconTile, Options, Prose, Row, TEXT } from "./kit";
import { OperationView } from "./runview";

export { clock, episodeTone, linesOf, OperationsTable, OutputTable, RunHeader, RunList, statusTone } from "./runview";

export type Policy = "allow" | "no-delete" | "read-only";


/** Record… with no program: the endpoint it signs for and what the application may do, then Start (the
 * recording is a run like any other: it opens as the newest, and Stop is its own). */
export function RecordPanel() {
  const policy = signal<Policy>("allow");
  const open = signal(false);
  const start = async () => {
    const ep = endpoint();
    // a press with no endpoint says why, never a control that does nothing
    if (!ep) return report({ kind: "config", message: t("run.chooseEndpoint") });
    if (await record(ep, policy())) open.set(false);
  };
  const ep = () => endpoints().find((e) => e.id === endpoint());
  return (
    <Card pad="none" testID={open() ? "record-form" : undefined}>
      <Row first>
        <IconTile icon="radio" tint="foreground" />
        <View class="min-w-0 flex-1 gap-1">
          <Heading>{t("timeline.recordTitle")}</Heading>
          <Text class={TEXT.note}>{t("timeline.recordHint")}</Text>
        </View>
        <If when={!open()}>
          <Button variant="outline" label={t("timeline.record")} onPress={() => open.set(true)} testID="record" />
        </If>
      </Row>
      <If when={open()}>
        <Row stack>
          <Text class={TEXT.note}>{ep() ? t("run.goesTo", { name: ep()!.name }) : t("run.chooseEndpoint")}</Text>
          <View class="flex-row">
            <Options
              testID="record-policy"
              value={policy()}
              items={[
                { value: "allow", title: t("timeline.allow") },
                { value: "no-delete", title: t("timeline.noDelete") },
                { value: "read-only", title: t("timeline.readOnly") },
              ]}
              onChange={(v) => policy.set(v as Policy)}
            />
          </View>
          <View class="flex-row gap-2">
            <Button small variant="primary" icon="radio" label={t("timeline.start")} onPress={start} testID="record-start" />
            <Button small variant="ghost" label={t("common.cancel")} onPress={() => open.set(false)} testID="record-cancel" />
          </View>
        </Row>
      </If>
    </Card>
  );
}

/** A recording with no program, where a program's terminal would stand: its address, and the exports that point
 * an application at it. */
export function RecordingNote(props: { url: string; exports: string }) {
  return (
    <Card testID="recording">
      <View class="flex-row items-center gap-2">
        <Dot tone="live" />
        <Heading class="flex-1 line-clamp-1">{t("timeline.recording", { url: props.url })}</Heading>
      </View>
      <Prose text={t("timeline.pointApp")} />
      <View selectable class="gap-0 rounded-(--radius) bg-muted/60 px-3 py-2">
        <Each of={props.exports.trim().split("\n")}>{(line) => <Text class={TEXT.code}>{line()}</Text>}</Each>
      </View>
      <View class="flex-row">
        <Button small variant="outline" icon="copy" label={t("timeline.copyExports")} onPress={() => copy(props.exports)} testID="copy-exports" />
      </View>
    </Card>
  );
}

/** An operation in the inspector, its requests read from its run (`episode`: whole, with every request's id). */
export function OperationDetail(props: { path: string; episode: EpisodeRow; shared: string[]; onRecord: (id: number) => void; onBack?: () => void }) {
  const rows = () => {
    try {
      return api.runs.records(props.path, props.episode.records);
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





