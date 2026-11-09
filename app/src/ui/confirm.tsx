// The guard of destructive operations: the dry run, read-only mode, and a
// typed confirmation (the number of items, or the bucket name).

import { signal, Text, View } from "nsh";
import { Dialog } from "nsh/base";
import { bytes } from "../format";
import { t } from "../i18n";
import { api } from "../native";
import { go, notify, readOnly, report } from "../store";
import type { Outcome, Plan } from "../types";
import { Button, Field, Heading, Muted, SCRIM, SimpleTable } from "./kit";
import { Document } from "./record";

export function ConfirmPlan(props: { plan: Plan | null; onClose: (outcome: Outcome | null) => void }) {
  const typed = signal("");
  const running = signal(false);
  const close = (o: Outcome | null) => {
    const p = props.plan;
    if (p && !o) api.discardPlan(p.id);
    typed.set("");
    props.onClose(o);
  };
  const run = async () => {
    const p = props.plan;
    if (!p) return;
    running.set(true);
    try {
      const o = await api.executePlan(p.id, typed());
      notify(t("confirm.done", { ok: o.succeeded, failed: o.failed }));
      for (const e of o.errors) report(e);
      running.set(false);
      typed.set("");
      props.onClose(o);
    } catch (e) {
      running.set(false);
      report(e);
    }
  };
  return (
    <Dialog
      open={props.plan !== null}
      onOpenChange={(e) => {
        // while it runs it stays: what it did is shown, and the lists rescanned
        if (!e.open && !running()) close(null);
      }}
      class="m-auto w-[560px] max-w-[95%] gap-3 rounded-(--radius-card) border border-border bg-popover p-5"
      backdropClass={SCRIM}
    >
      <If when={props.plan}>
        {(p) => (
          <View class="gap-3" testID="confirm-dialog">
            <Text class="text-heading font-semibold text-popover-foreground">{t("confirm.title")}</Text>
            <Heading>{t("confirm.dryRun")}</Heading>
            <Text class="text-body text-popover-foreground">{p().summary}</Text>
            <If when={p().items.length > 0}>
              <SimpleTable max={12} columns={["key", "id", "bytes", ""]} rows={p().items.map((i) => [i.key, i.id ?? "", i.bytes === null ? "" : bytes(i.bytes), i.detail])} />
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
                <View class="gap-2 rounded-(--radius) bg-muted p-3">
                  <Text class="text-body text-foreground">{t("confirm.readOnly")}</Text>
                  <Button small label={t("nav.settings")} onPress={() => (close(null), go("settings"))} />
                </View>
              }
            >
              <Field label={t("confirm.type", { hint: p().confirmHint, value: p().confirm })} value={typed()} onChange={(v) => typed.set(v)} testID="confirm-input" mono />
            </If>
            <View class="flex-row justify-end gap-2">
              <Button variant="ghost" label={t("common.cancel")} onPress={() => close(null)} disabled={running()} testID="confirm-cancel" />
              <Button variant="destructive" label={t("confirm.run")} disabled={readOnly() || running() || typed().trim() !== p().confirm} onPress={run} testID="confirm-run" />
            </View>
          </View>
        )}
      </If>
    </Dialog>
  );
}
