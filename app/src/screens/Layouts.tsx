// The layout lens: detects SlateDB, Iceberg, lakeFS and Woodpecker layouts
// (and measures closed formats), decodes them, and replays the logical
// state at a chosen time.

import { cleanup, signal, Text, View } from "nsh";
import { Slider } from "nsh/base";
import { bytes, date } from "../format";
import { t } from "../i18n";
import { api, watch } from "../native";
import { bucket, endpoint, prefix, report } from "../store";
import { here, remember } from "../ui/overview";
import type { Detection, Layout, Snapshot } from "../types";
import { Badge, Bar, Button, Card, Empty, Field, Heading, KeyValue, Muted, Page, SimpleTable, terminal, Title } from "../ui/kit";
import { BucketPicker, EndpointPicker } from "../ui/picker";

const LABEL: Record<string, string> = { slatedb: "SlateDB", iceberg: "Apache Iceberg", lakefs: "lakeFS", woodpecker: "Milvus Woodpecker", turbopuffer: "turbopuffer", warpstream: "WarpStream", generic: "Write cadence" };

function Replay(props: { job: string; layout: Layout }) {
  const times = () => props.layout.snapshots.map((s) => (s.time ? new Date(s.time).getTime() : 0)).filter((x) => x > 0);
  const min = () => Math.min(...times());
  const max = () => Math.max(...times());
  const at = signal(max());
  const state = (): Snapshot | null => api.layoutState(props.job, at());
  return (
    <If when={times().length > 0}>
      <Card testID="replay">
        <Heading>{t("layouts.replay")}</Heading>
        <Slider
          testID="replay-slider"
          accessibilityLabel={t("layouts.replay")}
          value={at()}
          min={min()}
          max={max() === min() ? min() + 1 : max()}
          step={1000}
          onChange={({ value }) => at.set(typeof value === "number" ? value : value[0])}
          class="h-6 justify-center"
          trackClass="h-1.5 rounded-full bg-muted"
          indicatorClass="h-1.5 rounded-full bg-primary"
          thumbClass="h-4 w-4 rounded-full border border-border bg-background shadow"
        />
        <View class="flex-row flex-wrap gap-2">
          <Each of={props.layout.snapshots.slice(-12)}>
            {(s) => <Button small variant="ghost" label={s().label} onPress={() => s().time && at.set(new Date(s().time!).getTime())} />}
          </Each>
        </View>
        <If when={state()}>
          {(s) => (
            <View class="gap-2">
              <Text class="text-sm font-medium text-foreground">{t("layouts.stateAt", { time: date(new Date(at()).toISOString()) })} — {s().label}</Text>
              <KeyValue rows={s().facts.map((f) => [f.label, f.value])} />
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
  let job: string | null = null;
  cleanup(() => {
    if (job) api.cancelJob(job);
  });
  const follow = async <R,>(id: string): Promise<R> => {
    job = id;
    progress.set(t("common.loading"));
    try {
      return await watch<R>(id, (e) => e.type === "Progress" && progress.set(e.progress.message));
    } finally {
      job = null;
      progress.set(null);
    }
  };
  const detect = async () => {
    try {
      const ep = endpoint()!;
      const b = bucket()!;
      const f = await follow<Detection[]>(await api.detectLayouts(ep, b, pfx()));
      found.set(f);
      remember(ep, b, pfx(), { layouts: f });
    } catch (e) {
      report(e);
    }
  };
  const analyze = async (format: string, root: string) => {
    try {
      const id = await api.analyzeLayout(endpoint()!, bucket()!, format, root);
      layout.set({ job: id, layout: await follow<Layout>(id) });
    } catch (e) {
      report(e);
    }
  };
  const tone = (s: string) => (s === "error" ? "fail" : s === "warning" ? "warn" : "info");
  if (!terminal() && !found()) void detect();
  return (
    <Page testID="screen-layouts">
      <View class="flex-row flex-wrap items-end gap-2">
        <If when={terminal()}>
          <EndpointPicker />
          <BucketPicker />
        </If>
        <If when={terminal()}>
          <Field class="w-64" label={t("common.prefix")} value={pfx()} onChange={(v) => pfx.set(v)} mono testID="layout-prefix" />
        </If>
        <Button variant={terminal() ? "primary" : "outline"} label={t("layouts.detect")} onPress={detect} disabled={!bucket() || !!progress()} testID="layout-detect" />
        <Button label={t("layouts.generic")} onPress={() => analyze("generic", pfx())} disabled={!bucket() || !!progress()} />
        <If when={progress()}>
          <Button label={t("common.cancel")} onPress={() => job && api.cancelJob(job)} />
        </If>
      </View>
      <If when={progress()}>{(p) => <Bar value={null} label={p()} />}</If>
      <If when={found()}>
        {(f) => (
          <Card>
            <Heading>{t("layouts.found")}</Heading>
            <If when={f().length > 0} else={<Muted>{t("layouts.noneFound")}</Muted>}>
              <Each of={f()}>
                {(d) => (
                  <View class="flex-row items-center gap-2">
                    <Badge label={LABEL[d().format] ?? d().format} tone="primary" />
                    <Text class="flex-1 font-mono text-xs text-foreground">{d().root || "/"}</Text>
                    <Text class="text-xs text-muted-foreground">{d().evidence}</Text>
                    <Button small label={t("common.run")} onPress={() => analyze(d().format, d().root)} testID={`analyze-${d().format}`} />
                  </View>
                )}
              </Each>
            </If>
          </Card>
        )}
      </If>
      <If when={layout()} else={<Empty text={t("layouts.title")} />}>
        {(l) => (
          <View class="gap-4" testID="layout-report">
            <Card>
              <View class="flex-row items-center gap-2">
                <Title class="flex-1">{LABEL[l().layout.format] ?? l().layout.format}</Title>
                <If when={l().layout.formatVersion}>{(v) => <Badge label={v()} />}</If>
                <Badge label={`${l().layout.objects} objects · ${bytes(l().layout.bytes)}`} />
              </View>
              <Text class="text-sm text-foreground">{l().layout.headline}</Text>
              <If when={!l().layout.decoded}>
                <Badge tone="info" label={t("layouts.notDecoded")} />
              </If>
              <KeyValue rows={l().layout.facts.map((f) => [f.label, f.value])} />
            </Card>
            <If when={l().layout.issues.length > 0}>
              <Card testID="layout-issues">
                <Heading>{t("layouts.issues")}</Heading>
                <Each of={l().layout.issues}>
                  {(i) => (
                    <View class="flex-row gap-2">
                      <Badge label={i().severity} tone={tone(i().severity)} />
                      <Text class="flex-1 text-sm text-foreground">{i().message}</Text>
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
              <Each of={l().layout.timeline.slice(-300)}>
                {(e) => (
                  <View class="flex-row gap-3">
                    <Text class="w-36 text-xs text-muted-foreground">{date(e().time)}</Text>
                    <Badge label={e().kind} tone={e().kind === "fence" || e().kind === "gap" ? "warn" : "neutral"} />
                    <Text class="flex-1 text-xs text-foreground">{`${e().label}${e().detail ? ` — ${e().detail}` : ""}`}</Text>
                  </View>
                )}
              </Each>
            </Card>
          </View>
        )}
      </If>
    </Page>
  );
}
