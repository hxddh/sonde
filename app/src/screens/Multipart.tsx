// Multipart health: incomplete uploads, their bytes (ListParts), age and
// cost; the lifecycle verdict; adding an abort rule and aborting stale
// uploads, both behind the guard.

import { cleanup, type JSX, Scroll, signal, Text, View } from "nsh";
import { DataTable, type DataTableSort } from "nsh/base";
import { bytes, count, date, day, days, money } from "../format";
import { type Key, t } from "../i18n";
import { core } from "../phrases";
import { Icon } from "../ui/icon";
import { api, asError, watch } from "../native";
import { bucket, endpoint, prefix, report, settings } from "../store";
import type { MpuReport, Plan, UploadRow } from "../types";
import { Await, Badge, Bar, Button, Card, Disclosure, Empty, Field, Heading, NumberField, Path, ROW, rowHeight, sortRows, Stat, TABLE, terminal, ToolBar, TYPE } from "../ui/kit";
import { BucketPicker, EndpointPicker } from "../ui/picker";
import { ConfirmPlan } from "../ui/confirm";
import { Document } from "../ui/record";
import { here, remember } from "../ui/overview";

/** The sheet's page: in a window it has the sheet's height, so its table fills what the rest leaves; a terminal scrolls. */
function Sheet(props: { children: JSX.Element }) {
  return terminal() ? (
    <Scroll testID="screen-multipart" class="flex-1" contentClass="gap-4 p-4">
      {props.children}
    </Scroll>
  ) : (
    <View testID="screen-multipart" class="min-h-0 flex-1 gap-4 p-4">
      {props.children}
    </View>
  );
}
const FILL = terminal() ? "gap-4" : "min-h-0 flex-1 gap-4";
const TABLE_BOX = terminal() ? "h-96" : "min-h-40 flex-1";

export function Multipart() {
  // a window looks at the prefix the objects show; a terminal asks for one
  const pfx = terminal() ? signal("") : prefix;
  const report_ = signal<MpuReport | null>(terminal() ? null : (here().mpu ?? null));
  const progress = signal<string | null>(null);
  // why the first scan gave nothing: said in place, with Try again
  const failure = signal<string | null>(null);
  const plan = signal<Plan | null>(null);
  const days_ = signal<number | null>(settings()?.["stale-upload-days"] ?? 7);
  // the oldest first; a press on a title sorts by its column's values, not their text
  const uploadSort = signal<DataTableSort | null>({ key: "age", direction: "descending" });
  const sorted = (rows: UploadRow[]) =>
    sortRows(rows, uploadSort(), (u, key) =>
      key === "age" ? (u.ageDays ?? -1) : key === "parts" ? u.parts : key === "bytes" ? u.bytes : key === "cost" ? u.costPerMonth : key === "initiated" ? (u.initiated ?? "") : key === "key" ? u.key : key === "uploadId" ? u.uploadId : u.stale ? 1 : 0,
    );
  let job: string | null = null;
  cleanup(() => {
    if (job) api.cancelJob(job);
  });
  const scan = async () => {
    const ep = endpoint();
    const b = bucket();
    if (!ep || !b || progress() !== null) return;
    // busy from the press: a second one starts no second scan
    progress.set(t("common.loading"));
    failure.set(null);
    try {
      job = await api.mpuScan(ep, b, pfx());
      const r = await watch<MpuReport>(job, (e) => e.type === "Progress" && progress.set(core(e.progress.messageText, e.progress.message)));
      report_.set(r);
      remember(r.endpoint, r.bucket, r.prefix, { mpu: r });
    } catch (e) {
      const err = asError(e);
      // over a report already shown, a toast; with nothing to show, the reason in its place
      if (err.kind !== "cancelled") report_() ? report(e) : failure.set(err.message);
    }
    job = null;
    progress.set(null);
  };
  const cancel = () => job && api.cancelJob(job);
  const planAbort = () => {
    const r = report_();
    if (!r) return;
    try {
      plan.set(api.planAbort(r.endpoint, r.bucket, r.uploads.filter((u) => u.stale)));
    } catch (e) {
      report(e);
    }
  };
  const planRule = async () => {
    const r = report_();
    if (!r) return;
    try {
      plan.set(await api.planLifecycle(r.endpoint, r.bucket, r.prefix, days_() ?? 7));
    } catch (e) {
      report(e);
    }
  };
  const tone = (s: string) => (s === "covered" ? "pass" : s === "missing" ? "fail" : "warn");
  const age = (r: MpuReport) => t("common.days", { n: r.thresholdDays });
  if (!terminal() && !report_()) void scan();
  return (
    <Sheet>
      <ToolBar scope={terminal() ? undefined : `${bucket() ?? ""}/${pfx()}`}>
        <If when={terminal()}>
          <EndpointPicker />
          <BucketPicker />
          <Field bare class="w-56" label={t("common.prefix")} placeholder={t("common.prefix")} value={pfx()} onChange={(v) => pfx.set(v)} onSubmit={scan} mono />
        </If>
        <Button small variant={report_() ? "outline" : "primary"} label={report_() ? t("overview.rescan") : t("common.scan")} onPress={scan} disabled={!bucket() || progress() !== null} testID="mpu-scan" />
        <If when={progress()}>
          <Button small variant="ghost" label={t("common.cancel")} onPress={cancel} testID="mpu-cancel" />
        </If>
      </ToolBar>
      {/* scanning again over what the last scan found */}
      <If when={report_() && progress()}>{(p) => <Bar value={null} label={p()} />}</If>
      <Await loading={!report_() && progress() !== null} error={failure()} retry={scan} waiting={progress() ?? undefined} testID="mpu">
        <If when={report_()} else={<Empty icon="layers" text={bucket() ? t("mpu.notScanned") : t("common.chooseBucket")} />}>
          {(r) => (
            <View class={FILL}>
              {/* the figures side by side, the lifecycle's verdict and rule under them, full width */}
              <View class="flex-row flex-wrap gap-3">
                <Stat
                  class="min-w-44 flex-1"
                  label={t("overview.uploads")}
                  value={count(r().totalUploads)}
                  detail={`${bytes(r().totalBytes)} · ${t("mpu.stale", { n: count(r().staleUploads), age: age(r()) })}`}
                  tone={r().staleUploads ? "warn" : "plain"}
                  testID="mpu-size"
                />
                <Stat class="min-w-44 flex-1" label={t("mpu.costMonth")} value={money(r().costPerMonth, r().currency)} detail={`${t("mpu.costSoFar")}: ${money(r().costSoFar, r().currency)}`} testID="mpu-cost" />
              </View>
              <Card testID="lifecycle-verdict">
                {/* the verdict, and on its row what can be done about it */}
                <View class="min-h-8 flex-row flex-wrap items-center gap-2">
                  <Heading>{t("mpu.lifecycle")}</Heading>
                  {/* on the row's middle (a badge alone keeps to its start) */}
                  <View class="justify-center">
                    <Badge label={t(`mpu.lifecycle.${r().lifecycle.status}` as Key)} tone={tone(r().lifecycle.status)} />
                  </View>
                  <If when={r().lifecycle.status !== "covered"}>
                    <View class="flex-1" />
                    <Text class="text-small text-muted-foreground">{t("mpu.days")}</Text>
                    <NumberField bare class="w-28" label={t("mpu.days")} value={days_()} min={1} max={3650} onChange={(v) => days_.set(v)} />
                    <Button label={t("mpu.generateRule")} onPress={planRule} testID="plan-rule" />
                  </If>
                </View>
                <Text class="text-body text-foreground">{core(r().lifecycle.messageText, r().lifecycle.message)}</Text>
                <If when={r().lifecycle.status !== "covered"}>
                  <Disclosure title={t("mpu.suggestedRule")}>
                    <Document text={r().lifecycle.suggestedRule} testID="lifecycle-rule" />
                  </Disclosure>
                </If>
              </Card>
              <If when={r().uploads.length > 0} else={<Empty text={t("mpu.noneFound")} />}>
                <View class="flex-row items-center gap-2">
                  {/* what the table under it lists, and where */}
                  <Heading class="shrink-0">{t("mpu.uploads", { n: count(r().totalUploads) })}</Heading>
                  <Path path={`${r().bucket}/${r().prefix}`} class="min-w-0 flex-1 text-muted-foreground" />
                  <Button variant="destructive" small label={t("mpu.abortOlder", { age: age(r()) })} onPress={planAbort} disabled={r().staleUploads === 0} testID="plan-abort" />
                </View>
                {/* the table takes the sheet's remaining height: no box of its own to scroll in a scroll */}
                <Card pad="none" class={TABLE_BOX}>
                  <DataTable
                    testID="uploads"
                    class="min-h-0 flex-1"
                    columns={[
                      // fits the detail panel (760pt) without a sideways scroll: the date as a day
                      // the upload's id is the request inspector's: the columns that decide stay whole
                      { key: "flag", title: "", width: 24, resizable: false },
                      { key: "key", title: t("mpu.key"), width: 220, minWidth: 120, sortable: true },
                      { key: "initiated", title: t("mpu.initiated"), width: 104, sortable: true },
                      { key: "age", title: t("mpu.age"), width: 72, align: "end", sortable: true },
                      { key: "parts", title: t("mpu.parts"), width: 64, align: "end", sortable: true },
                      { key: "bytes", title: t("mpu.bytes"), width: 88, align: "end", sortable: true },
                      { key: "cost", title: t("mpu.costMonth"), width: 104, align: "end", sortable: true },
                    ]}
                    // an old upload is marked in its first cell (an icon; "!" in a terminal), not shouted: the one way a row says it
                    data={sorted(r().uploads).map((u) => ({ flag: u.stale ? "!" : "", key: u.key, initiated: day(u.initiated), age: days(u.ageDays), parts: count(u.parts), bytes: u.partsError ? "?" : bytes(u.bytes), cost: money(u.costPerMonth, r().currency) }))}
                    fontSize={TYPE.body}
                    rowHeight={rowHeight(ROW.table)}
                    classes={terminal() ? undefined : TABLE}
                    // figures at the end of their cells (no tabular digits in a cell: GTK cuts the last one)
                    renderCell={
                      terminal()
                        ? undefined
                        : (c) =>
                            c.key === "flag" ? (
                              c.text ? (
                                <Icon name="triangle-alert" size={12} tint="warn" />
                              ) : (
                                <View />
                              )
                            ) : c.key === "key" || c.key === "initiated" ? (
                              <Text class="text-body text-foreground line-clamp-1">{c.text}</Text>
                            ) : (
                              <View class="flex-1 items-end justify-center">
                                <Text class="text-small text-foreground">{c.text}</Text>
                              </View>
                            )
                    }
                    sort={uploadSort()}
                    onSortChange={({ sort }) => uploadSort.set(sort)}
                  />
                </Card>
                <Text class="text-caption text-muted-foreground">{`${t("mpu.scanned")} ${date(r().scannedAt)} · ${r().endpoint}`}</Text>
              </If>
            </View>
          )}
        </If>
      </Await>
      <ConfirmPlan
        plan={plan()}
        onClose={(o) => {
          plan.set(null);
          if (o) void scan();
        }}
      />
    </Sheet>
  );
}
