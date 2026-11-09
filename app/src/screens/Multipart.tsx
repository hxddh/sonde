// Multipart health: incomplete uploads, their bytes (ListParts), age and
// cost; the lifecycle verdict; adding an abort rule and aborting stale
// uploads, both behind the guard.

import { cleanup, signal, Text, View } from "nsh";
import { DataTable, type DataTableSort } from "nsh/base";
import { bytes, count, date, day, days, money } from "../format";
import { type Key, t } from "../i18n";
import { Icon } from "../ui/icon";
import { api, watch } from "../native";
import { bucket, endpoint, prefix, report, settings } from "../store";
import type { MpuReport, Plan, UploadRow } from "../types";
import { Badge, Bar, Button, CARD, Card, Empty, Stat, Field, KeyValue, NumberField, Page, terminal, ROW, rowHeight, sortRows, TYPE, TABLE } from "../ui/kit";
import { BucketPicker, EndpointPicker } from "../ui/picker";
import { ConfirmPlan } from "../ui/confirm";
import { Document } from "../ui/record";
import { here, remember } from "../ui/overview";

export function Multipart() {
  // a window looks at the prefix the objects show; a terminal asks for one
  const pfx = terminal() ? signal("") : prefix;
  const report_ = signal<MpuReport | null>(terminal() ? null : (here().mpu ?? null));
  const progress = signal<string | null>(null);
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
    if (!ep || !b) return;
    try {
      job = await api.mpuScan(ep, b, pfx());
      progress.set(t("common.loading"));
      const r = await watch<MpuReport>(job, (e) => e.type === "Progress" && progress.set(e.progress.message));
      report_.set(r);
      remember(r.endpoint, r.bucket, r.prefix, { mpu: r });
    } catch (e) {
      report(e);
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
  if (!terminal() && !report_()) void scan();
  return (
    <Page testID="screen-multipart">
      <View class="flex-row flex-wrap items-end gap-2">
        <If when={terminal()}>
          <EndpointPicker />
          <BucketPicker />
        </If>
        <If when={terminal()}>
          <Field class="w-56" label={t("common.prefix")} value={pfx()} onChange={(v) => pfx.set(v)} mono />
        </If>
        <Button variant={terminal() ? "primary" : "outline"} label={terminal() ? t("common.scan") : t("overview.rescan")} onPress={scan} disabled={!bucket() || !!progress()} testID="mpu-scan" />
        <If when={progress()}>
          <Button label={t("common.cancel")} onPress={cancel} testID="mpu-cancel" />
        </If>
      </View>
      <If when={progress()}>{(p) => <Bar value={null} label={p()} />}</If>
      <If when={report_()} else={<Empty text={bucket() ? t("mpu.title") : t("common.chooseBucket")} />}>
        {(r) => (
          <View class="gap-4">
            {/* the figures side by side, the lifecycle's verdict and rule under them, full width */}
            <View class="flex-row flex-wrap items-start gap-3">
              <Stat class="min-w-44 flex-1" label={t("mpu.uploads", { n: r().totalUploads })} value={bytes(r().totalBytes)} detail={t("mpu.stale", { n: r().staleUploads, age: t("common.days", { n: r().thresholdDays }) })} tone={r().staleUploads ? "warn" : "plain"} testID="mpu-size" />
              <Stat class="min-w-44 flex-1" label={t("mpu.costMonth")} value={money(r().costPerMonth, r().currency)} detail={`${t("mpu.costSoFar")}: ${money(r().costSoFar, r().currency)}`} testID="mpu-cost" />
            </View>
            <View>
              <Card testID="lifecycle-verdict">
                <View class="flex-row items-center gap-2">
                  <Text class="text-body font-semibold text-foreground">{t("mpu.lifecycle")}</Text>
                  <Badge label={t(`mpu.lifecycle.${r().lifecycle.status}` as Key)} tone={tone(r().lifecycle.status)} />
                </View>
                <Text class="text-body text-foreground">{r().lifecycle.message}</Text>
                <If when={r().lifecycle.status !== "covered"}>
                  <View class="flex-row items-end gap-2">
                    <NumberField class="w-28" label={t("mpu.days")} value={days_()} min={1} max={3650} onChange={(v) => days_.set(v)} />
                    <Button small label={t("mpu.generateRule")} onPress={planRule} testID="plan-rule" />
                  </View>
                  <Document text={r().lifecycle.suggestedRule} testID="lifecycle-rule" />
                </If>
              </Card>
            </View>
            <If when={r().uploads.length > 0} else={<Empty text={t("mpu.noneFound")} />}>
              <View class="flex-row items-center gap-2">
                <Text class="flex-1 text-body text-muted-foreground">{`${count(r().totalUploads)} · ${r().bucket}/${r().prefix}`}</Text>
                <Button variant="destructive" small label={t("mpu.abortSelected")} onPress={planAbort} disabled={r().staleUploads === 0} testID="plan-abort" />
              </View>
              <DataTable
                testID="uploads"
                class={`h-[420px] ${CARD}`}
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
                data={sorted(r().uploads).map((u) => ({ flag: u.stale ? "stale" : "", key: u.key, initiated: day(u.initiated), age: days(u.ageDays), parts: String(u.parts), bytes: u.partsError ? "?" : bytes(u.bytes), cost: money(u.costPerMonth, r().currency) }))}
                rowClasses={sorted(r().uploads).map((u) => (u.stale ? "text-warn" : "text-foreground"))}
                fontSize={TYPE.body}
                rowHeight={rowHeight(ROW.table)}
                classes={terminal() ? undefined : TABLE}
                // an old upload is marked, not shouted
                renderCell={terminal() ? undefined : (c) => (c.key === "flag" ? (c.text ? <Icon name="triangle-alert" size={13} tint="warn" /> : <View />) : c.key === "key" || c.key === "initiated" ? <Text class="text-body text-foreground line-clamp-1">{c.text}</Text> : <View class="flex-1 items-end justify-center"><Text class="text-small tabular-nums text-foreground">{c.text}</Text></View>)}
                sort={uploadSort()}
                onSortChange={({ sort }) => uploadSort.set(sort)}
              />
              <KeyValue rows={[[t("mpu.scanned"), date(r().scannedAt)], [t("common.endpoint"), r().endpoint]]} />
            </If>
          </View>
        )}
      </If>
      <ConfirmPlan
        plan={plan()}
        onClose={(o) => {
          plan.set(null);
          if (o) void scan();
        }}
      />
    </Page>
  );
}
