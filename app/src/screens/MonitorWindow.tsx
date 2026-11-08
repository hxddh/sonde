// A recording at a glance, above other windows: a small panel that floats
// while the program runs elsewhere — requests a second, failures, in flight,
// the p95, and the shape of the last minutes. Opened from Run or the tray.

import { cleanup, signal, Text, View } from "nsh";
import { Window } from "nsh/desktop";
import { analysis } from "../analysis";
import { t } from "../i18n";
import { running } from "../run";
import { LatencyBand, liveFigures, SessionShape } from "../ui/plots";
import { ms } from "../format";

function Monitor() {
  const tick = signal(0);
  const timer = setInterval(() => tick.set(tick() + 1), 1000);
  cleanup(() => clearInterval(timer));
  const view = analysis(
    () => running()?.path ?? null,
    () => tick(),
  ).view;
  const figure = (value: string, label: string, tone = "text-foreground") => (
    <View class="min-w-0 flex-1 gap-0">
      <Text class={`tabular-nums text-heading font-semibold ${tone}`}>{value}</Text>
      <Text class="text-caption text-muted-foreground line-clamp-1">{label}</Text>
    </View>
  );
  return (
    <View testID="monitor" class="flex-1 gap-2 bg-background p-3">
      <If when={running() && view()} else={<Text class="text-small text-muted-foreground">{t("monitor.idle")}</Text>}>
        {(v) => {
          const f = () => liveFigures(v().profile);
          return (
            <View class="flex-1 gap-2">
              <Text class="font-mono text-small text-muted-foreground line-clamp-1">{`$ ${running()?.command ?? ""}`}</Text>
              <View class="flex-row gap-2">
                {figure(f().perSecond.toFixed(f().perSecond < 10 ? 1 : 0), t("live.perSecond"))}
                {figure(`${Math.round(f().failedShare * 100)}%`, t("live.failed"), f().failedShare > 0 ? "text-status-server" : "text-foreground")}
                {figure(String(f().inFlight), t("live.inFlight"))}
                {figure(ms(f().p95), t("live.p95"))}
              </View>
              <SessionShape profile={v().profile} class="h-8 w-full" testID="monitor-shape" />
              <LatencyBand profile={v().profile} class="h-10 w-full" testID="monitor-band" />
            </View>
          );
        }}
      </If>
    </View>
  );
}

/** Opens the monitor, or brings it forward: a floating panel of its own. */
export function openMonitor() {
  const open = Window.get("monitor");
  if (open) {
    open.show();
    return;
  }
  Window.open(() => <Monitor />, {
    id: "monitor",
    kind: "panel",
    level: "floating",
    title: t("monitor.title"),
    defaultSize: { width: 340, height: 220 },
    minSize: { width: 280, height: 180 },
    restoreBounds: true,
  });
}
