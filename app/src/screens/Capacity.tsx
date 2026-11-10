// "Why is this bucket so big?": current vs noncurrent bytes, delete
// markers, incomplete uploads, by storage class, prefix and size.

import { cleanup, Scroll, signal, Text, View } from "nsh";
import { bytes, count, date, money } from "../format";
import { t } from "../i18n";
import { api, asError, watch } from "../native";
import { bucket, endpoint, notify, prefix, report } from "../store";
import { remember } from "../ui/overview";
import type { DuReport } from "../types";
import { Await, Bar, Button, Card, Empty, Field, Heading, More, Stat, SwitchRow, terminal, ToolBar } from "../ui/kit";
import { BucketPicker, EndpointPicker } from "../ui/picker";
import { BarChart, PrefixMap, StackBar } from "../ui/charts";

export function Capacity() {
  const pfx = terminal() ? signal("") : prefix;
  const mpu = signal(true);
  const result = signal<DuReport | null>(null);
  const progress = signal<string | null>(null);
  // why the first scan gave nothing: said in place, with Try again
  const failure = signal<string | null>(null);
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
    if (!ep || !b || progress() !== null) return;
    // busy from the press: a second one starts no second scan
    progress.set(t("common.loading"));
    failure.set(null);
    try {
      job = await api.duScan(ep, b, pfx(), mpu());
      const r = await watch<DuReport>(job, (e) => e.type === "Progress" && progress.set(`${e.progress.message} · ${bytes(e.progress.bytes)}`));
      result.set(r);
      remember(ep, b, pfx(), { size: r });
    } catch (e) {
      const err = asError(e);
      // over a report already shown, a toast; with nothing to show, the reason in its place
      if (err.kind !== "cancelled") result() ? report(e) : failure.set(err.message);
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
  const TILE = "min-w-32 flex-1";
  return (
    <Scroll testID="screen-capacity" class="flex-1" contentClass="gap-4 p-4">
      <ToolBar
        scope={terminal() ? undefined : `${bucket() ?? ""}/${pfx()}`}
        end={
          <If when={result()}>
            <More
              testID="du-more"
              actions={() => [
                { label: t("du.exportCsv"), icon: "download", run: () => exportAs("csv") },
                { label: t("du.exportJson"), icon: "download", run: () => exportAs("json") },
              ]}
            />
          </If>
        }
      >
        <If when={terminal()}>
          <EndpointPicker />
          <BucketPicker />
          <Field bare class="w-56" label={t("common.prefix")} placeholder={t("common.prefix")} value={pfx()} onChange={(v) => (pfx.set(v), load())} mono />
        </If>
        {/* what the next scan counts: before the command, so Cancel coming and going moves nothing */}
        <SwitchRow label={t("du.includeMultipart")} checked={mpu()} onChange={(v) => mpu.set(v)} />
        <Button small variant={result() ? "outline" : "primary"} label={result() ? t("overview.rescan") : t("common.scan")} onPress={scan} disabled={!bucket() || progress() !== null} testID="du-scan" />
        <If when={progress()}>
          <Button small variant="ghost" label={t("common.cancel")} onPress={() => job && api.cancelJob(job)} testID="du-cancel" />
        </If>
      </ToolBar>
      {/* scanning again over what the last scan found */}
      <If when={result() && progress()}>{(p) => <Bar value={null} label={p()} />}</If>
      <Await loading={!result() && progress() !== null} error={failure()} retry={scan} waiting={progress() ?? undefined} testID="du">
        <If when={result()} else={<Empty icon="chart-pie" text={bucket() ? t("du.notMeasured") : t("common.chooseBucket")} />}>
          {(r) => (
            <View class="gap-4" testID="du-report">
              {/* the facts as figures: how big, how many of what, what it costs */}
              <View class="flex-row flex-wrap gap-3">
                <Stat class={TILE} label={t("overview.size")} value={bytes(r().totalBytes)} detail={`${date(r().scannedAt)} · ${r().source}`} testID="du-size" />
                <Stat class={TILE} label={t("nav.objects")} value={count(r().current.objects)} detail={`${t("du.current")} · ${bytes(r().current.bytes)}`} />
                <Stat class={TILE} label={t("browse.versions")} value={count(r().noncurrent.objects)} detail={`${t("du.noncurrent")} · ${bytes(r().noncurrent.bytes)} · ${t("du.markers")}: ${count(r().deleteMarkers)}`} />
                <Stat class={TILE} label={t("du.multipart")} value={count(r().multipart.objects)} detail={bytes(r().multipart.bytes)} />
                <Stat class={TILE} label={t("du.costMonth")} value={money(r().costPerMonth, r().currency)} detail={`${t("du.cost")} · ${t("du.requests", { n: count(r().requests) })}`} />
              </View>
              <Card>
                <StackBar
                  testID="du-stack"
                  parts={[
                    { label: t("du.current"), value: r().current.bytes },
                    { label: t("du.noncurrent"), value: r().noncurrent.bytes },
                    { label: t("du.multipart"), value: r().multipart.bytes },
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
      </Await>
    </Scroll>
  );
}
