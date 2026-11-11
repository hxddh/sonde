// The guard of destructive operations: the dry run, read-only mode, and a
// typed confirmation (the number of items, or the bucket name).

import { signal, Text, View } from "nsh";
import { bytes, count } from "../format";
import { t } from "../i18n";
import { core } from "../phrases";
import { api, asError } from "../native";
import { go, notify, readOnly, report, showRequest } from "../store";
import type { Outcome, Plan } from "../types";
import { Actions, Button, Field, Heading, Modal, Muted, Notice, SimpleTable, Title } from "./kit";
import { Document } from "./record";
import { toast } from "./toast";

export function ConfirmPlan(props: { plan: Plan | null; onClose: (outcome: Outcome | null) => void }) {
  const typed = signal("");
  const running = signal(false);
  const close = (o: Outcome | null) => {
    const p = props.plan;
    if (p && !o) api.discardPlan(p.id);
    typed.set("");
    props.onClose(o);
  };
  // what is typed cannot become the words asked for: said under the field (while it still can, nothing is)
  const mismatch = () => {
    const want = props.plan?.confirm ?? "";
    const got = typed().trim();
    return got && !want.startsWith(got) ? t("confirm.mismatch", { value: want }) : null;
  };
  // what the commit needs: writes allowed, nothing running, and the words typed
  const ready = () => !readOnly() && !running() && props.plan !== null && typed().trim() === props.plan.confirm;
  const run = async () => {
    const p = props.plan;
    if (!p || !ready()) return;
    running.set(true);
    try {
      const o = await api.executePlan(p.id, typed());
      if (o.errors.length === 0) notify(t("confirm.done", { ok: count(o.succeeded), failed: count(o.failed) }));
      else {
        // one toast for all of them: how many failed, and why the first did (its request a press away)
        const first = asError(o.errors[0]);
        const record = first.recordId;
        toast({
          id: "error",
          kind: "error",
          title: t("confirm.done", { ok: count(o.succeeded), failed: count(o.failed) }),
          description: t("confirm.firstError", { reason: first.message }),
          action: record !== null && record !== undefined ? t("common.showRequest") : undefined,
          onAction: () => showRequest(record),
        });
      }
      running.set(false);
      typed.set("");
      props.onClose(o);
    } catch (e) {
      running.set(false);
      report(e);
    }
  };
  return (
    <Modal
      open={props.plan !== null}
      // what will be done, heard as the dialog opens
      describe={props.plan ? core(props.plan.summaryText, props.plan.summary) : undefined}
      // (a table of what will go needs the room: its keys, ids and what each is are never cut to fit)
      size={props.plan?.items.length ? "wide" : "default"}
      // while it runs it stays: what it did is shown, and the lists rescanned
      onClose={() => !running() && close(null)}
    >
      <If when={props.plan}>
        {(p) => (
          <View class="gap-3" testID="confirm-dialog">
            <Title>{t("confirm.title")}</Title>
            <Heading>{t("confirm.dryRun")}</Heading>
            <Text class="text-body text-popover-foreground">{core(p().summaryText, p().summary)}</Text>
            <If when={p().items.length > 0}>
              <SimpleTable max={12} fill={3} end={[2]} columns={[t("confirm.colKey"), t("confirm.colVersion"), t("confirm.colSize"), t("confirm.colDetail")]} rows={p().items.map((i) => [i.key, i.id ?? "", i.bytes === null ? "" : bytes(i.bytes), core(i.detailText, i.detail)])} />
            </If>
            <If when={p().document}>
              {(doc) => (
                <View class="gap-1">
                  <Muted>{t("confirm.newDocument")}</Muted>
                  <Document text={doc()} testID="confirm-document" />
                </View>
              )}
            </If>
            <If
              when={!readOnly()}
              else={
                <Notice tone="warn" text={t("confirm.readOnly")} testID="confirm-read-only">
                  <Button small label={t("settings.open")} onPress={() => (close(null), go("settings"))} testID="confirm-settings" />
                </Notice>
              }
            >
              {/* Return commits, once what is typed matches */}
              <Field label={t("confirm.type", { hint: core(p().confirmHintText, p().confirmHint), value: p().confirm })} value={typed()} onChange={(v) => typed.set(v)} onSubmit={run} testID="confirm-input" mono error={mismatch()} />
            </If>
            <Actions>
              <Button variant="ghost" label={t("common.cancel")} onPress={() => close(null)} disabled={running()} testID="confirm-cancel" />
              <Button variant="destructive" label={t("confirm.run")} disabled={!ready()} onPress={run} testID="confirm-run" />
            </Actions>
          </View>
        )}
      </If>
    </Modal>
  );
}
