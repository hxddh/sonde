// The layout lens: detects SlateDB, Iceberg, lakeFS and Woodpecker layouts
// (and measures closed formats), decodes them, and replays the logical
// state at a chosen time.

import { cleanup, List, Scroll, signal, Text, View } from "nsh";
import { bytes, count, date } from "../format";
import { type Key, t } from "../i18n";
import { core } from "../phrases";
import { api, asError, watch } from "../native";
import { bucket, endpoint, prefix, report } from "../store";
import { here, LAYOUT_LABEL, remember } from "../ui/overview";
import type { Detection, Layout, Snapshot } from "../types";
import { Await, Badge, Bar, Button, Card, Choice, Empty, Field, Heading, KeyValue, More, Muted, Path, ROW, rowHeight, SimpleTable, terminal, ToolBar, Scrubber, WIDTH } from "../ui/kit";
import { BucketPicker, EndpointPicker } from "../ui/picker";

const LABEL = LAYOUT_LABEL;
/** Words the app has for a decoder's own (a timeline event's kind, an issue's severity); what it has none for, as itself. */
const worded = (key: string, raw: string) => {
  const s = t(key as Key);
  return s === key ? raw : s;
};
const kindLabel = (k: string) => worded(`layouts.kind.${k}`, k);
const severityLabel = (s: string) => worded(`layouts.severity.${s}`, s);

function Replay(props: { job: string; layout: Layout }) {
  const stamp = (s: Snapshot) => (s.time ? new Date(s.time).getTime() : 0);
  const times = () => props.layout.snapshots.map(stamp).filter((x) => x > 0);
  const min = () => Math.min(...times());
  const max = () => Math.max(...times());
  const at = signal(max());
  const state = (): Snapshot | null => api.layoutState(props.job, at());
  // the latest snapshots to jump to, one per moment
  const jumps = () => {
    const seen = new Set<number>();
    return props.layout.snapshots
      .slice(-12)
      .filter((s) => stamp(s) > 0 && !seen.has(stamp(s)) && !!seen.add(stamp(s)))
      .map((s) => ({ value: `${stamp(s)}`, title: s.label }));
  };
  return (
    <If when={times().length > 0}>
      <Card testID="replay">
        <Heading>{t("layouts.replay")}</Heading>
        <Scrubber testID="replay-slider" label={t("layouts.replay")} value={at()} min={min()} max={max() === min() ? min() + 1 : max()} step={1000} onChange={(v) => at.set(v)} />
        {/* one control for the snapshots, not a button each */}
        <Choice class={WIDTH.long} placeholder={t("layouts.snapshot")} value={jumps().some((j) => j.value === `${at()}`) ? `${at()}` : null} items={jumps()} onChange={(v) => at.set(Number(v))} testID="replay-snapshot" />
        <If when={state()}>
          {(s) => (
            <View class="gap-2">
              <Text class="text-body text-foreground">
                {t("layouts.stateAt", { time: date(new Date(at()).toISOString()) })} — {s().label}
              </Text>
              <KeyValue rows={s().facts.map((f) => [core(f.labelText, f.label), core(f.valueText, f.value)])} />
              <Each of={s().tables}>
                {(tb) => (
                  <View class="gap-1">
                    <Muted>{tb().title}</Muted>
                    <SimpleTable mono max={40} columns={tb().columns} rows={tb().rows} />
                  </View>
                )}
              </Each>
            </View>
          )}
        </If>
      </Card>
    </If>
  );
}

export function Layouts() {
  const pfx = terminal() ? signal("") : prefix;
  const found = signal<Detection[] | null>(terminal() ? null : (here().layouts ?? null));
  const layout = signal<{ job: string; layout: Layout } | null>(null);
  const progress = signal<string | null>(null);
  // why the first scan gave nothing: said in place, with Try again
  const failure = signal<string | null>(null);
  let job: string | null = null;
  cleanup(() => {
    if (job) api.cancelJob(job);
  });
  // one job at a time, busy from the press: every command that starts one is off until it ends
  const busy = () => progress() !== null;
  const follow = async <R,>(start: () => Promise<string>): Promise<R> => {
    progress.set(t("common.loading"));
    try {
      job = await start();
      return await watch<R>(job, (e) => e.type === "Progress" && progress.set(core(e.progress.messageText, e.progress.message)));
    } finally {
      job = null;
      progress.set(null);
    }
  };
  const detect = async () => {
    const ep = endpoint();
    const b = bucket();
    if (!ep || !b || busy()) return;
    failure.set(null);
    try {
      const f = await follow<Detection[]>(() => api.detectLayouts(ep, b, pfx()));
      found.set(f);
      remember(ep, b, pfx(), { layouts: f });
    } catch (e) {
      const err = asError(e);
      // over a list already shown, a toast; with nothing to show, the reason in its place
      if (err.kind !== "cancelled") found() ? report(e) : failure.set(err.message);
    }
  };
  const analyze = async (format: string, root: string) => {
    const ep = endpoint();
    const b = bucket();
    if (!ep || !b || busy()) return;
    try {
      let id = "";
      const l = await follow<Layout>(async () => (id = await api.analyzeLayout(ep, b, format, root)));
      layout.set({ job: id, layout: l });
    } catch (e) {
      report(e);
    }
  };
  const tone = (s: string) => (s === "error" ? "fail" : s === "warning" ? "warn" : "info");
  if (!terminal() && !found()) void detect();
  return (
    <Scroll testID="screen-layouts" class="flex-1" contentClass="gap-4 p-4">
      <ToolBar
        scope={terminal() ? undefined : `${bucket() ?? ""}/${pfx()}`}
        // the measure that needs no known layout: any prefix's write cadence
        end={<More testID="layout-more" actions={() => [{ label: t("layouts.generic"), icon: "timer", run: () => void analyze("generic", pfx()), disabled: !bucket() || busy() }]} />}
      >
        <If when={terminal()}>
          <EndpointPicker />
          <BucketPicker />
          <Field bare class={WIDTH.long} label={t("common.prefix")} placeholder={t("common.prefix")} value={pfx()} onChange={(v) => pfx.set(v)} onSubmit={detect} mono testID="layout-prefix" />
        </If>
        <Button small variant={found() ? "outline" : "primary"} label={found() ? t("overview.rescan") : t("common.scan")} onPress={detect} disabled={!bucket() || busy()} testID="layout-detect" />
        <If when={busy()}>
          <Button small variant="ghost" label={t("common.cancel")} onPress={() => job && api.cancelJob(job)} testID="layout-cancel" />
        </If>
      </ToolBar>
      {/* a job over what is already shown (scanning again, decoding a layout) */}
      <If when={found() && progress()}>{(p) => <Bar value={null} label={p()} />}</If>
      <Await loading={!found() && busy()} error={failure()} retry={detect} waiting={progress() ?? undefined} testID="layouts">
        <If when={found()} else={<Empty icon="boxes" text={bucket() ? t("layouts.notScanned") : t("common.chooseBucket")} />}>
          {(f) => (
            <Card>
              <Heading count={f().length}>{t("layouts.found")}</Heading>
              <If when={f().length > 0} else={<Muted>{t("layouts.noneFound")}</Muted>}>
                <Each of={f()}>
                  {(d) => (
                    <View class="flex-row items-center gap-2">
                      <Badge label={LABEL[d().format] ?? d().format} tone="primary" />
                      <Path path={d().root || "/"} class="min-w-0 flex-1 text-foreground" />
                      <Text class="shrink text-small text-muted-foreground line-clamp-1">{d().evidence}</Text>
                      <Button small label={t("layouts.decode")} onPress={() => analyze(d().format, d().root)} disabled={busy()} testID={`analyze-${d().format}`} />
                    </View>
                  )}
                </Each>
              </If>
            </Card>
          )}
        </If>
      </Await>
      <If when={layout()}>
        {(l) => (
          <View class="gap-4" testID="layout-report">
            <Card>
              <View class="flex-row items-center gap-2">
                <View class="min-w-0 flex-1">
                  <Heading>{LABEL[l().layout.format] ?? l().layout.format}</Heading>
                </View>
                <If when={l().layout.formatVersion}>{(v) => <Badge label={v()} />}</If>
                <Badge label={`${t("du.objects", { n: count(l().layout.objects) })} · ${bytes(l().layout.bytes)}`} />
              </View>
              <Text class="text-body text-foreground">{core(l().layout.headlineText, l().layout.headline)}</Text>
              <If when={!l().layout.decoded}>
                <View class="flex-row">
                  <Badge tone="info" label={t("layouts.notDecoded")} />
                </View>
              </If>
              <KeyValue rows={l().layout.facts.map((f) => [core(f.labelText, f.label), core(f.valueText, f.value)])} />
            </Card>
            <If when={l().layout.issues.length > 0}>
              <Card testID="layout-issues">
                <Heading count={l().layout.issues.length}>{t("layouts.issues")}</Heading>
                <Each of={l().layout.issues}>
                  {(i) => (
                    <View class="flex-row gap-2">
                      <Badge label={severityLabel(i().severity)} tone={tone(i().severity)} />
                      <Text class="flex-1 text-body text-foreground">{core(i().messageText, i().message)}</Text>
                    </View>
                  )}
                </Each>
              </Card>
            </If>
            <Replay job={l().job} layout={l().layout} />
            <Each of={l().layout.tables.filter((tb) => tb.rows.length > 0)}>
              {(tb) => (
                <Card>
                  <Heading>{tb().title}</Heading>
                  <SimpleTable mono max={100} columns={tb().columns} rows={tb().rows} />
                </Card>
              )}
            </Each>
            <Card>
              <Heading>{t("layouts.timeline")}</Heading>
              <List each={l().layout.timeline} estimatedItemSize={ROW.dense} viewport="parent">
                {(e) => (
                  <View class="flex-row items-center gap-3" style={{ height: rowHeight(ROW.dense) }}>
                    <Text class={`${WIDTH.date} shrink-0 text-small text-muted-foreground`}>{date(e().time)}</Text>
                    <Badge label={kindLabel(e().kind)} tone={e().kind === "fence" || e().kind === "gap" ? "warn" : "neutral"} />
                    <Text class="flex-1 text-small text-foreground line-clamp-1">{`${core(e().labelText, e().label)}${e().detail ? ` — ${core(e().detailText, e().detail)}` : ""}`}</Text>
                  </View>
                )}
              </List>
            </Card>
          </View>
        )}
      </If>
    </Scroll>
  );
}
