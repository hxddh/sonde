// Multipart health: incomplete uploads, their bytes (ListParts), age and
// cost; the lifecycle verdict; adding an abort rule and aborting stale
// uploads, both behind the guard.

import { cleanup, signal, Text, View } from "nsh";
import { DataTable } from "nsh/base";
import { bytes, count, date, days, money } from "../format";
import { t } from "../i18n";
import { api, watch } from "../native";
import { bucket, endpoint, prefix, report, settings } from "../store";
import type { MpuReport, Plan } from "../types";
import { Badge, Bar, Button, Card, Empty, Field, KeyValue, Mono, Muted, Page, terminal, Title } from "../ui/kit";
import { BucketPicker, EndpointPicker } from "../ui/picker";
import { ConfirmPlan } from "../ui/confirm";
import { here, remember } from "../ui/overview";

export function Multipart() {
  // a window looks at the prefix the objects show; a terminal asks for one
  const pfx = terminal() ? signal("") : prefix;
  const report_ = signal<MpuReport | null>(terminal() ? null : (here().mpu ?? null));
  const progress = signal<string | null>(null);
  const plan = signal<Plan | null>(null);
  const days_ = signal(String(settings()?.["stale-upload-days"] ?? 7));
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
      plan.set(await api.planLifecycle(r.endpoint, r.bucket, r.prefix, Number(days_()) || 7));
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
            <View class="flex-row flex-wrap gap-3">
              <Card class="min-w-48 flex-1">
                <Muted>{t("mpu.uploads", { n: r().totalUploads })}</Muted>
                <Title>{bytes(r().totalBytes)}</Title>
                <Text class={r().staleUploads ? "text-sm text-warn" : "text-sm text-muted-foreground"}>{t("mpu.stale", { n: r().staleUploads, days: r().thresholdDays })}</Text>
              </Card>
              <Card class="min-w-48 flex-1">
                <Muted>{t("mpu.costMonth")}</Muted>
                <Title>{money(r().costPerMonth, r().currency)}</Title>
                <Muted>{`${t("mpu.costSoFar")}: ${money(r().costSoFar, r().currency)}`}</Muted>
              </Card>
              <Card class="min-w-64 flex-[2]" testID="lifecycle-verdict">
                <View class="flex-row items-center gap-2">
                  <Text class="text-sm font-semibold text-foreground">{t("mpu.lifecycle")}</Text>
                  <Badge label={r().lifecycle.status} tone={tone(r().lifecycle.status)} />
                </View>
                <Text class="text-sm text-foreground">{r().lifecycle.message}</Text>
                <If when={r().lifecycle.status !== "covered"}>
                  <View class="flex-row items-end gap-2">
                    <Field class="w-24" label="days" value={days_()} onChange={(v) => days_.set(v)} />
                    <Button small label={t("mpu.generateRule")} onPress={planRule} testID="plan-rule" />
                  </View>
                  <Mono>{r().lifecycle.suggestedRule}</Mono>
                </If>
              </Card>
            </View>
            <If when={r().uploads.length > 0} else={<Empty text={t("mpu.noneFound")} />}>
              <View class="flex-row items-center gap-2">
                <Text class="flex-1 text-sm text-muted-foreground">{`${count(r().totalUploads)} · ${r().bucket}/${r().prefix}`}</Text>
                <Button variant="destructive" small label={t("mpu.abortSelected")} onPress={planAbort} disabled={r().staleUploads === 0} testID="plan-abort" />
              </View>
              <DataTable
                testID="uploads"
                class="h-[420px] rounded-(--radius-card) border border-border bg-card"
                columns={[
                  { key: "flag", title: "", width: 28 },
                  { key: "key", title: "Key", width: 240, minWidth: 120 },
                  { key: "uploadId", title: "Upload ID", width: 160 },
                  { key: "initiated", title: "Initiated", width: 130 },
                  { key: "age", title: t("mpu.age"), width: 70, align: "end" },
                  { key: "parts", title: t("mpu.parts"), width: 56, align: "end" },
                  { key: "bytes", title: t("mpu.bytes"), width: 90, align: "end" },
                  { key: "cost", title: t("mpu.costMonth"), width: 90, align: "end" },
                ]}
                data={r().uploads.map((u) => ({ flag: u.stale ? "!" : "", key: u.key, uploadId: u.uploadId, initiated: date(u.initiated), age: days(u.ageDays), parts: String(u.parts), bytes: u.partsError ? "?" : bytes(u.bytes), cost: money(u.costPerMonth, r().currency) }))}
                rowClasses={r().uploads.map((u) => (u.stale ? "text-warn" : "text-foreground"))}
                fontSize={13}
                rowHeight={terminal() ? undefined : 30}
                classes={terminal() ? undefined : { header: "bg-card border-b border-border", headerCell: "pl-2 pr-1 text-xs font-medium text-muted-foreground", rowHover: "bg-row-hover", rowSelected: "bg-selection" }}
                sort={{ key: "age", direction: "descending" }}
              />
              <KeyValue rows={[["Scanned", r().scannedAt], [t("common.endpoint"), r().endpoint]]} />
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
