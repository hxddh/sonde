// "Why is this bucket so big?": current vs noncurrent bytes, delete
// markers, incomplete uploads, by storage class, prefix and size.

import { cleanup, signal, Text, View } from "nsh";
import { bytes, count, money } from "../format";
import { t } from "../i18n";
import { api, watch } from "../native";
import { bucket, endpoint, notify, prefix, report } from "../store";
import { remember } from "../ui/overview";
import type { DuReport } from "../types";
import { Bar, Button, Card, Empty, Field, Heading, KeyValue, Muted, Page, terminal, Title, Toggle } from "../ui/kit";
import { BucketPicker, EndpointPicker } from "../ui/picker";
import { BarChart, PrefixMap, StackBar } from "../ui/charts";

export function Capacity() {
  const pfx = terminal() ? signal("") : prefix;
  const mpu = signal(true);
  const result = signal<DuReport | null>(null);
  const progress = signal<string | null>(null);
  let job: string | null = null;
  cleanup(() => {
    if (job) api.cancelJob(job);
  });
  const load = () => {
    const ep = endpoint();
    const b = bucket();
    if (ep && b) result.set(api.duCached(ep, b, pfx()));
  };
  load();
  const scan = async () => {
    const ep = endpoint();
    const b = bucket();
    if (!ep || !b) return;
    try {
      job = await api.duScan(ep, b, pfx(), mpu());
      progress.set(t("common.loading"));
      const r = await watch<DuReport>(job, (e) => e.type === "Progress" && progress.set(`${e.progress.message} · ${bytes(e.progress.bytes)}`));
      result.set(r);
      remember(ep, b, pfx(), { size: r });
    } catch (e) {
      report(e);
    }
    job = null;
    progress.set(null);
  };
  // asked for from a window: measured now, unless it was already
  if (!terminal() && !result()) void scan();
  const exportAs = (f: "json" | "csv") => {
    try {
      notify(t("common.exportedTo", { path: api.duExport(endpoint()!, bucket()!, pfx(), f) }));
    } catch (e) {
      report(e);
    }
  };
  return (
    <Page testID="screen-capacity">
      <View class="flex-row flex-wrap items-end gap-2">
        <If when={terminal()}>
          <EndpointPicker />
          <BucketPicker />
        </If>
        <If when={terminal()}>
          <Field class="w-56" label={t("common.prefix")} value={pfx()} onChange={(v) => (pfx.set(v), load())} mono />
        </If>
        <Toggle label={t("du.includeMultipart")} checked={mpu()} onChange={(v) => mpu.set(v)} />
        <Button variant={terminal() ? "primary" : "outline"} label={terminal() || !result() ? t("common.scan") : t("overview.rescan")} onPress={scan} disabled={!bucket() || !!progress()} testID="du-scan" />
        <If when={progress()}>
          <Button label={t("common.cancel")} onPress={() => job && api.cancelJob(job)} />
        </If>
        <If when={result()}>
          <Button small label="CSV" onPress={() => exportAs("csv")} />
          <Button small label="JSON" onPress={() => exportAs("json")} />
        </If>
      </View>
      <If when={progress()}>{(p) => <Bar value={null} label={p()} />}</If>
      <If when={result()} else={<Empty text={bucket() ? t("du.why") : t("common.chooseBucket")} />}>
        {(r) => (
          <View class="gap-4" testID="du-report">
            <Card>
              <View class="flex-row items-baseline gap-3">
                <Title>{bytes(r().totalBytes)}</Title>
                <Muted>{`${t("du.cost")}: ${money(r().costPerMonth, r().currency)}/month · ${r().source} · ${count(r().requests)} requests · ${r().scannedAt}`}</Muted>
              </View>
              <StackBar
                testID="du-stack"
                parts={[
                  { label: t("du.current"), value: r().current.bytes },
                  { label: t("du.noncurrent"), value: r().noncurrent.bytes },
                  { label: t("du.multipart"), value: r().multipart.bytes },
                ]}
              />
              <KeyValue
                rows={[
                  [t("du.current"), `${count(r().current.objects)} objects · ${bytes(r().current.bytes)}`],
                  [t("du.noncurrent"), `${count(r().noncurrent.objects)} versions · ${bytes(r().noncurrent.bytes)}`],
                  [t("du.markers"), count(r().deleteMarkers)],
                  [t("du.multipart"), `${count(r().multipart.objects)} uploads · ${bytes(r().multipart.bytes)}`],
                ]}
              />
            </Card>
            <Card testID="du-findings">
              <Heading>{t("du.why")}</Heading>
              <Each of={r().findings}>{(f) => <Text class="text-body text-foreground">{`• ${f()}`}</Text>}</Each>
            </Card>
            <View class={terminal() ? "gap-4" : "flex-row flex-wrap gap-4"}>
              <Card class="min-w-80 flex-1">
                <Heading>{t("du.byClass")}</Heading>
                <BarChart testID="du-classes" items={r().byClass.map((c) => ({ label: c.name, value: c.current.bytes + c.noncurrent.bytes }))} />
              </Card>
              <Card class="min-w-80 flex-1">
                <Heading>{t("du.bySize")}</Heading>
                <BarChart unit="count" items={r().bySize.map((b) => ({ label: b.label, value: b.objects }))} />
              </Card>
            </View>
            <Card>
              <Heading>{t("du.byPrefix")}</Heading>
              <PrefixMap testID="du-prefixes" items={r().byPrefix.slice(0, 24).map((p) => ({ label: p.name, value: p.current.bytes + p.noncurrent.bytes }))} />
            </Card>
          </View>
        )}
      </If>
    </Page>
  );
}
