// The guard of sonde's own destructive operations: the dry run and a typed
// confirmation (the number of items, or the bucket name); each is audited.

import { signal, Text, View } from "nsh";
import { bytes, count, day } from "../format";
import { type Key, t } from "../i18n";
import { core } from "../phrases";
import { api, asError } from "../native";
import { notify, report } from "../store";
import type { Outcome, Plan, PlanItem } from "../types";
import { Actions, Button, Field, Modal, Muted, Notice, SimpleTable, Title } from "./kit";
import { Document } from "./record";
import { toast } from "./toast";

/** What a plan does, by its act and how many ("Abort 2 uploads"): the dialog's title and its commit. */
const ACT: Record<Plan["action"], Key> = { abortUploads: "confirm.act.abortUploads", deleteVersions: "confirm.act.deleteVersions", restoreObject: "confirm.act.restoreObject", deleteObjects: "confirm.act.deleteObjects", putLifecycle: "confirm.act.putLifecycle" };
/** What an item's id is: an upload's, a version's (none: no column). */
const ID: Partial<Record<Plan["action"], Key>> = { abortUploads: "confirm.colUpload", deleteVersions: "confirm.colVersion", restoreObject: "confirm.colVersion" };

/** An item as the reader writes it: an upload's parts and the day it started, in the reader's date. */
function itemDetail(i: PlanItem): string {
  const p = i.detailText?.length === 1 ? i.detailText[0] : null;
  if (p?.key === "plan.item.upload" && p.args?.started) {
    const started = /^\d{4}-\d\d-\d\d$/.test(p.args.started) ? `${p.args.started}T00:00:00` : p.args.started;
    return t("confirm.upload", { n: count(Number(p.args.parts ?? 0)), started: day(started) });
  }
  return core(i.detailText, i.detail);
}

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
  // an id column where the items have one, named for what it is (an upload's, a version's)
  const withId = (p: Plan) => !!ID[p.action] && p.items.some((i) => i.id);
  // what the commit needs: nothing running, and the words typed
  const ready = () => !running() && props.plan !== null && typed().trim() === props.plan.confirm;
  const run = async () => {
    const p = props.plan;
    if (!p || !ready()) return;
    running.set(true);
    try {
      const o = await api.executePlan(p.id, typed());
      if (o.errors.length === 0) notify(t("confirm.done", { ok: count(o.succeeded), failed: count(o.failed) }));
      else {
        // one toast for all of them: how many failed, and why the first did (with the request id to quote)
        const first = asError(o.errors[0]);
        toast({
          id: "error",
          kind: "error",
          title: t("confirm.done", { ok: count(o.succeeded), failed: count(o.failed) }),
          description: [t("confirm.firstError", { reason: first.message }), first.requestId ? `${t("common.requestId")}: ${first.requestId}` : ""].filter(Boolean).join(" · "),
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
            {/* named by its act and how many; where, how much, and that nothing has changed yet under it */}
            <Title>{t(ACT[p().action], { n: count(p().items.length) })}</Title>
            <Text class="text-body text-popover-foreground">
              {p().items.length > 0 ? t("confirm.where", { bucket: p().bucket, size: bytes(p().items.reduce((n, i) => n + (i.bytes ?? 0), 0)) }) : core(p().summaryText, p().summary)}
            </Text>
            <Muted>{t("confirm.dryRun")}</Muted>
            <If when={p().items.length > 0}>
              <SimpleTable
                max={12}
                fill={withId(p()) ? 3 : 2}
                end={[withId(p()) ? 2 : 1]}
                columns={[t("confirm.colKey"), ...(withId(p()) ? [t(ID[p().action]!)] : []), t("confirm.colSize"), t("confirm.colDetail")]}
                rows={p().items.map((i) => [i.key, ...(withId(p()) ? [i.id ?? ""] : []), i.bytes === null ? "" : bytes(i.bytes), itemDetail(i)])}
              />
            </If>
            <If when={p().document}>
              {(doc) => (
                <View class="gap-1">
                  <Muted>{t("confirm.newDocument")}</Muted>
                  <Document text={doc()} testID="confirm-document" />
                </View>
              )}
            </If>
            {/* Return commits, once what is typed matches */}
            <Field label={t("confirm.type", { hint: core(p().confirmHintText, p().confirmHint), value: p().confirm })} value={typed()} onChange={(v) => typed.set(v)} onSubmit={run} testID="confirm-input" mono error={mismatch()} />
            <Actions>
              <Button variant="ghost" label={t("common.cancel")} onPress={() => close(null)} disabled={running()} testID="confirm-cancel" />
              <Button variant="destructive" label={t(ACT[p().action], { n: count(p().items.length) })} disabled={!ready()} onPress={run} testID="confirm-run" />
            </Actions>
          </View>
        )}
      </If>
    </Modal>
  );
}
