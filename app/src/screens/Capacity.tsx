// "Why is this bucket so big?": current vs noncurrent bytes, delete
// markers, incomplete uploads, by storage class, prefix and size.

import { cleanup, effect, Scroll, signal, Text, untracked, View } from "nsh";
import { bytes, count, date, money } from "../format";
import { t } from "../i18n";
import { core } from "../phrases";
import { api, asError, watch } from "../native";
import { bucket, endpoint, notify, prefix, report } from "../store";
import { remember } from "../ui/overview";
import type { DuReport } from "../types";
import { Await, Bar, Button, Card, Empty, Field, Heading, More, Stat, SwitchRow, terminal, Tiles, ToolBar } from "../ui/kit";
import { Chart, HALF } from "../ui/plots";
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
  // the place (endpoint, bucket, prefix) the scan under way is of
  let scanning: string | null = null;
  cleanup(() => {
    if (job) api.cancelJob(job);
  });
  const load = () => {
    const ep = endpoint();
    const b = bucket();
    result.set(ep && b ? api.duCached(ep, b, pfx()) : null);
  };
  // the place shown: what a scan's answer is held against
  let shown = "";
  const scan = async () => {
    const ep = endpoint();
    const b = bucket();
    if (!ep || !b) return;
    const of = shown;
    // busy from the press: a second one starts no second scan (one for another bucket takes its place)
    if (progress() !== null && scanning === of) return;
    scanning = of;
    progress.set(t("common.loading"));
    failure.set(null);
    const p = pfx();
    let mine: string | null = null;
    try {
      mine = job = await api.duScan(ep, b, p, mpu());
      const r = await watch<DuReport>(mine, (e) => e.type === "Progress" && shown === of && progress.set(`${core(e.progress.messageText, e.progress.message)} · ${bytes(e.progress.bytes)}`));
      remember(ep, b, p, { size: r });
      // the answer of a place since left is kept for it, not shown here
      if (shown === of) result.set(r);
    } catch (e) {
      const err = asError(e);
      // over a report already shown, a toast; with nothing to show, the reason in its place
      if (err.kind !== "cancelled" && shown === of) result() ? report(e) : failure.set(err.message);
    }
    if (scanning !== of) return;
    if (job === mine) job = null;
    scanning = null;
    progress.set(null);
  };
  // the sheet follows the endpoint, the bucket and the prefix: another one's
  // report is never left under this one's name, nor its scan left running
  effect(() => {
    const key = `${endpoint()}\n${bucket()}\n${pfx()}`;
    if (key === shown) return;
    const first = shown === "";
    shown = key;
    untracked(() => {
      if (!first && job) api.cancelJob(job);
      failure.set(null);
      try {
        load();
      } catch (e) {
        result.set(null);
        report(e);
      }
      // asked for from a window: measured now, unless it was already
      if (!terminal() && !result()) void scan();
    });
  });
  const exportAs = (f: "json" | "csv") => {
    try {
      notify(t("common.exportedTo", { path: api.duExport(endpoint()!, bucket()!, pfx(), f) }));
    } catch (e) {
      report(e);
    }
  };
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
              <Tiles
                min={128}
                items={[
                  <Stat label={t("overview.size")} value={bytes(r().totalBytes)} detail={`${date(r().scannedAt)} · ${r().source}${r().versionsUnknown ? ` · ${t("du.versionsNotCounted")}` : ""}`} testID="du-size" />,
                  <Stat label={t("nav.objects")} value={count(r().current.objects)} detail={`${t("du.current")} · ${bytes(r().current.bytes)}`} />,
                  <Stat label={t("browse.versions")} value={r().versionsUnknown ? "—" : count(r().noncurrent.objects)} detail={r().versionsUnknown ? t("du.versionsUnknown") : `${t("du.noncurrent")} · ${bytes(r().noncurrent.bytes)} · ${t("du.markers")}: ${count(r().deleteMarkers)}`} />,
                  <Stat label={t("du.multipart")} value={count(r().multipart.objects)} detail={bytes(r().multipart.bytes)} />,
                  <Stat label={t("du.costMonth")} value={money(r().costPerMonth, r().currency)} detail={`${t("du.cost")} · ${t("du.requests", { n: count(r().requests) })}`} />,
                ]}
              />
              <Chart title={t("du.parts")}>
                <StackBar
                  testID="du-stack"
                  // versions that could not be listed are not a part of nothing: left out, and said
                  parts={[
                    { label: t("du.current"), value: r().current.bytes },
                    ...(r().versionsUnknown ? [] : [{ label: t("du.noncurrent"), value: r().noncurrent.bytes }]),
                    { label: t("du.multipart"), value: r().multipart.bytes },
                  ]}
                  note={r().versionsUnknown ? t("du.versionsNotCounted") : undefined}
                />
              </Chart>
              <Card testID="du-findings">
                <Heading>{t("du.why")}</Heading>
                <Each of={r().findings}>{(f, i) => <Text class="text-body text-foreground">{`• ${core(r().findingsText?.[i()], f())}`}</Text>}</Each>
              </Card>
              <View class={terminal() ? "gap-4" : "flex-row flex-wrap gap-4"}>
                <Chart class={HALF} title={t("du.byClass")} empty={r().byClass.length === 0 && t("common.empty")}>
                  <BarChart testID="du-classes" items={r().byClass.map((c) => ({ label: c.name, value: c.current.bytes + c.noncurrent.bytes }))} />
                </Chart>
                <Chart class={HALF} title={t("du.bySize")} empty={r().bySize.length === 0 && t("common.empty")}>
                  <BarChart unit="count" items={r().bySize.map((b) => ({ label: b.label, value: b.objects }))} />
                </Chart>
              </View>
              <Chart title={t("du.byPrefix")} empty={r().byPrefix.length === 0 && t("common.empty")}>
                <PrefixMap testID="du-prefixes" items={r().byPrefix.slice(0, 24).map((p) => ({ label: p.name, value: p.current.bytes + p.noncurrent.bytes }))} />
              </Chart>
            </View>
          )}
        </If>
      </Await>
    </Scroll>
  );
}
