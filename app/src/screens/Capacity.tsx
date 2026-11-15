// "Why is this bucket so big?": current vs noncurrent bytes, delete
// markers, incomplete uploads, by storage class, prefix and size.

import { cleanup, effect, Scroll, signal, Text, untracked, View } from "nsh";
import { bytes, count, day, money, percent } from "../format";
import { t } from "../i18n";
import { core } from "../phrases";
import { api, asError, watch } from "../native";
import { bucket, endpoint, notify, offerCommands, prefix, report } from "../store";
import { remember } from "../ui/overview";
import type { DuReport } from "../types";
import { sectionLabel } from "../labels";
import { Await, ProgressBar, Button, Empty, Field, More, Stat, SwitchRow, terminal, Tiles, ToolBar, SIZES, WIDTH, TEXT } from "../ui/kit";
import { Chart, HALF } from "../ui/plots";
import { BucketPicker, EndpointPicker } from "../ui/picker";
import { BarChart, PrefixMap, StackBar } from "../ui/charts";

/** How many months the growth chart names, the latest; the rest are one row before them. */
const MONTHS = 12;

/** The months with bytes in the last `MONTHS` calendar months (to the latest month written), and those before. */
function recentMonths(r: DuReport): { recent: DuReport["byMonth"]; earlier: DuReport["byMonth"] } {
  const index = (m: string) => Number(m.slice(0, 4)) * 12 + Number(m.slice(5, 7)) - 1;
  const last = r.byMonth.length ? index(r.byMonth[r.byMonth.length - 1].month) : 0;
  const within = (m: string) => last - index(m) < MONTHS;
  return { recent: r.byMonth.filter((m) => within(m.month)), earlier: r.byMonth.filter((m) => !within(m.month)) };
}

/** The growth chart's rows: the months with bytes in the last `MONTHS`, the latest first, and every earlier one as one row. */
function monthRows(r: DuReport): { label: string; value: number }[] {
  const { recent, earlier } = recentMonths(r);
  const before = earlier.reduce((n, m) => n + m.bytes, 0);
  return [...recent.map((m) => ({ label: m.month, value: m.bytes })).reverse(), ...(before > 0 && recent.length ? [{ label: t("du.before", { month: recent[0].month }), value: before }] : [])];
}

/** What each of the bucket's charts answers, in a sentence of its figures. */
const share = (part: number, whole: number) => percent(whole > 0 ? part / whole : 0);
const partsSaid = (r: DuReport) => t("du.partsText", { pct: share(r.current.bytes, r.totalBytes), size: bytes(r.current.bytes) });
const biggest = <T,>(xs: T[], v: (x: T) => number) => [...xs].sort((a, b) => v(b) - v(a))[0];
function classSaid(r: DuReport) {
  const top = biggest(r.byClass, (c) => c.current.bytes + c.noncurrent.bytes);
  return top ? t("du.byClassText", { name: top.name, pct: share(top.current.bytes + top.noncurrent.bytes, r.current.bytes + r.noncurrent.bytes) }) : "";
}
function sizeSaid(r: DuReport) {
  const top = biggest(r.bySize, (b) => b.objects);
  return top && top.objects > 0 ? t("du.bySizeText", { label: top.label, pct: share(top.objects, r.current.objects) }) : "";
}
function prefixSaid(r: DuReport) {
  const top = r.byPrefix[0];
  return top ? t("du.byPrefixText", { name: top.name, pct: share(top.current.bytes + top.noncurrent.bytes, r.current.bytes + r.noncurrent.bytes) }) : "";
}
function monthSaid(r: DuReport) {
  const all = r.byMonth.reduce((n, m) => n + m.bytes, 0);
  const top = biggest(r.byMonth, (m) => m.bytes);
  if (!top || all === 0) return "";
  // one month: said once; all within the window: since when, not "100%"
  if (r.byMonth.length === 1) return t("du.byMonthOne", { month: top.month });
  const { recent, earlier } = recentMonths(r);
  if (earlier.length === 0) return t("du.byMonthSince", { first: r.byMonth[0].month, month: top.month, size: bytes(top.bytes) });
  return t("du.byMonthText", { pct: share(recent.reduce((n, m) => n + m.bytes, 0), all), n: MONTHS, month: top.month, size: bytes(top.bytes) });
}

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
  // the section's commands: in the palette and on their shortcuts, as on its row
  offerCommands(
    () => sectionLabel("capacity"),
    () => [
      { id: "du-scan", label: result() ? t("overview.rescan") : t("common.scan"), icon: "refresh-cw", shortcut: "mod+shift+r", disabled: !bucket() || progress() !== null, run: () => void scan() },
      { id: "du-mpu", label: t("du.includeMultipart"), icon: "layers", shortcut: "mod+shift+u", run: () => mpu.set(!mpu()) },
      { id: "du-csv", label: t("du.exportCsv"), icon: "download", shortcut: "mod+shift+e", disabled: !result(), run: () => exportAs("csv") },
      { id: "du-json", label: t("du.exportJson"), icon: "download", disabled: !result(), run: () => exportAs("json") },
    ],
  );
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
          <Field bare class={WIDTH.field} label={t("common.prefix")} placeholder={t("common.prefix")} value={pfx()} onChange={(v) => (pfx.set(v), load())} mono />
        </If>
        {/* what the next scan counts: before the command, so Cancel coming and going moves nothing */}
        <SwitchRow label={t("du.includeMultipart")} checked={mpu()} onChange={(v) => mpu.set(v)} />
        <Button small variant={result() ? "outline" : "primary"} label={result() ? t("overview.rescan") : t("common.scan")} shortcut="mod+shift+r" onPress={scan} disabled={!bucket() || progress() !== null} testID="du-scan" />
        <If when={progress()}>
          <Button small variant="ghost" label={t("common.cancel")} onPress={() => job && api.cancelJob(job)} testID="du-cancel" />
        </If>
      </ToolBar>
      {/* scanning again over what the last scan found */}
      <If when={result() && progress()}>{(p) => <ProgressBar value={null} label={p()} />}</If>
      <Await loading={!result() && progress() !== null} error={failure()} retry={scan} waiting={progress() ?? undefined} testID="du">
        <If when={result()} else={<Empty icon="chart-pie" text={bucket() ? t("du.notMeasured") : t("common.chooseBucket")} />}>
          {(r) => (
            <View class="gap-4" testID="du-report">
              {/* the facts as figures: how big, how many of what, what it costs */}
              <Tiles
                min={SIZES.capacityTile}
                items={[
                  <Stat label={t("overview.size")} value={bytes(r().totalBytes)} detail={`${day(r().scannedAt)}${r().versionsUnknown ? ` · ${t("du.versionsNotCounted")}` : ""}`} testID="du-size" />,
                  <Stat label={t("nav.objects")} value={count(r().current.objects)} detail={`${t("du.current")} · ${bytes(r().current.bytes)}`} />,
                  <Stat label={t("browse.versions")} value={r().versionsUnknown ? "—" : count(r().noncurrent.objects)} detail={r().versionsUnknown ? t("du.versionsUnknown") : `${bytes(r().noncurrent.bytes)} · ${t("du.markers")}: ${count(r().deleteMarkers)}`} />,
                  <Stat label={t("du.multipart")} value={count(r().multipart.objects)} detail={bytes(r().multipart.bytes)} />,
                  <Stat label={t("du.costMonth")} value={money(r().costPerMonth, r().currency)} detail={`${t("du.cost")} · ${t("du.requests", { n: count(r().requests) })}`} />,
                ]}
              />
              <Chart title={t("du.parts")} text={partsSaid(r())}>
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
                {/* why it is this big, under what it is made of: a plain list, a line a reason, and how it was listed */}
                <View testID="du-findings" accessibilityLabel={t("du.why")} class="gap-1">
                  <Each of={r().findings}>{(f, i) => <Text class={TEXT.small}>{core(r().findingsText?.[i()], f())}</Text>}</Each>
                  <Text class={TEXT.note}>{t("du.source", { api: r().source })}</Text>
                </View>
              </Chart>
              <View class="flex-row flex-wrap gap-4 terminal:flex-col terminal:flex-nowrap">
                <Chart class={HALF} title={t("du.byClass")} text={classSaid(r())} empty={r().byClass.length === 0 && t("common.empty")}>
                  <BarChart testID="du-classes" items={r().byClass.map((c) => ({ label: c.name, value: c.current.bytes + c.noncurrent.bytes }))} />
                </Chart>
                <Chart class={HALF} title={t("du.bySize")} text={sizeSaid(r())} empty={r().bySize.length === 0 && t("common.empty")}>
                  <BarChart unit="count" items={r().bySize.map((b) => ({ label: b.label, value: b.objects }))} />
                </Chart>
              </View>
              {/* how the bucket grew: the bytes by the month they were written, from the listing alone */}
              <Chart testID="du-months" title={t("du.byMonth")} text={monthSaid(r())} how={t("du.byMonthHint")} empty={r().byMonth.length === 0 && t("common.empty")}>
                <BarChart testID="du-months-bars" items={monthRows(r())} />
              </Chart>
              <Chart title={t("du.byPrefix")} text={prefixSaid(r())} empty={r().byPrefix.length === 0 && t("common.empty")}>
                <PrefixMap testID="du-prefixes" items={r().byPrefix.slice(0, 24).map((p) => ({ label: p.name, value: p.current.bytes + p.noncurrent.bytes }))} />
              </Chart>
            </View>
          )}
        </If>
      </Await>
    </Scroll>
  );
}
