// Settings: how sonde behaves. Grouped by what they are about — appearance,
// safety, requests, costs, the request log, updates, files — and saved as
// they change: a toast says so, with Undo.

import { cleanup, signal, Text, View } from "nsh";
import { Input } from "nsh/base";
import { language, t } from "../i18n";
import { api } from "../native";
import { notify, refreshSettings, report, settings } from "../store";
import type { Settings } from "../types";
import { Choice, Field, Page, Section, Segmented, terminal, Toggle } from "../ui/kit";
import { Fold, Pairs } from "../ui/record";
import { Updates } from "../ui/updates";

const copyOf = <T,>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

export function SettingsScreen() {
  const draft = signal<Settings | null>(settings() ? copyOf(settings()!) : null);
  let saved: Settings | null = settings() ? copyOf(settings()!) : null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  cleanup(() => timer && clearTimeout(timer));

  const write = (next: Settings, undo: Settings | null) => {
    try {
      api.saveSettings({ ...next, "read-only": settings()?.["read-only"] ?? true });
      refreshSettings();
      saved = copyOf(next);
      if (undo)
        notify(t("settings.saved"), {
          action: t("common.undo"),
          onAction: () => {
            draft.set(copyOf(undo));
            language.set(undo.language);
            write(undo, null);
          },
        });
    } catch (e) {
      report(e);
    }
  };
  /** Changes the draft; saved a moment later (typing a number saves once). */
  const change = (next: Settings, now = false) => {
    draft.set(next);
    if (timer) clearTimeout(timer);
    const undo = saved;
    if (now) write(next, undo);
    else timer = setTimeout(() => write(next, undo), 700);
  };
  const num = (v: string, d: number) => (Number.isFinite(Number(v)) && v.trim() !== "" ? Number(v) : d);
  const paths = api.paths();

  return (
    <Page testID="screen-settings">
      <If when={draft()}>
        {(d) => (
          <View class={terminal() ? "gap-2" : "w-full max-w-[720px] gap-7"}>
            <Section title={t("settings.appearance")}>
              <View class="flex-row flex-wrap items-center gap-x-8 gap-y-3">
                <View class="gap-1.5">
                  <Text class="text-small font-medium text-muted-foreground">{t("settings.themeLabel")}</Text>
                  <Segmented
                    testID="theme"
                    value={d().theme}
                    items={[
                      { value: "system", title: t("settings.system"), icon: "monitor" },
                      { value: "light", title: t("settings.light"), icon: "sun" },
                      { value: "dark", title: t("settings.dark"), icon: "moon" },
                    ]}
                    onChange={(v) => change({ ...d(), theme: v }, true)}
                  />
                </View>
                <If when={!terminal()}>
                  <Toggle label={t("settings.contrast")} hint={t("settings.contrastHint")} checked={d()["high-contrast"] ?? false} onChange={(v) => change({ ...d(), "high-contrast": v }, true)} testID="high-contrast" />
                </If>
                <Choice
                  class="w-44"
                  label={t("settings.language")}
                  value={d().language}
                  items={[
                    { value: "auto", title: t("settings.system") },
                    { value: "en", title: "English" },
                    { value: "zh", title: "中文" },
                  ]}
                  onChange={(v) => {
                    language.set(v);
                    change({ ...d(), language: v }, true);
                  }}
                  testID="language"
                />
              </View>
            </Section>

            <Section title={t("settings.safety")}>
              <Toggle
                testID="read-only"
                label={t("settings.readOnly")}
                hint={t("settings.readOnlyHint")}
                checked={settings()?.["read-only"] ?? true}
                onChange={(v) => {
                  try {
                    api.setReadOnly(v);
                    refreshSettings();
                  } catch (e) {
                    report(e);
                  }
                }}
              />
            </Section>

            <Section title={t("settings.requests")} description={t("settings.requestsHint")}>
              <View class="flex-row flex-wrap gap-3">
                <Field class="w-52" label={t("settings.concurrency")} value={String(d().concurrency)} onChange={(v) => change({ ...d(), concurrency: num(v, d().concurrency) })} testID="concurrency" />
                <Field class="w-64" label={t("settings.rate")} value={d()["rate-limit"] === null ? "" : String(d()["rate-limit"])} onChange={(v) => change({ ...d(), "rate-limit": v.trim() === "" ? null : num(v, 0) })} />
                <Field class="w-52" label={t("settings.staleDays")} value={String(d()["stale-upload-days"])} onChange={(v) => change({ ...d(), "stale-upload-days": num(v, 7) })} />
              </View>
            </Section>

            <Section title={t("settings.costs")} description={t("settings.costsHint")}>
              <View class="flex-row flex-wrap gap-3">
                <Field class="w-52" label={t("settings.price")} value={String(d().prices["default-per-gib-month"])} onChange={(v) => change({ ...d(), prices: { ...d().prices, "default-per-gib-month": num(v, 0.023) } })} />
                <Field class="w-28" label={t("settings.currency")} value={d().prices.currency} onChange={(v) => change({ ...d(), prices: { ...d().prices, currency: v } })} />
              </View>
              <Fold title={t("settings.byClass")} count={Object.keys(d().prices["by-class"]).length} open={false}>
                <View class="overflow-hidden rounded-(--radius) border border-border bg-card">
                  <Each of={Object.entries(d().prices["by-class"])}>
                    {(e, i) => (
                      <View class={`h-9 flex-row items-center gap-3 px-3 ${i() > 0 ? "border-t border-border" : ""}`}>
                        <Text class="flex-1 font-mono text-small text-foreground">{e()[0]}</Text>
                        <Input
                          value={String(e()[1])}
                          accessibilityLabel={e()[0]}
                          onChange={({ value }) => change({ ...d(), prices: { ...d().prices, "by-class": { ...d().prices["by-class"], [e()[0]]: num(value, e()[1]) } } })}
                          class="h-7 w-28 rounded-(--radius-sm) border border-input bg-background px-2 text-right font-mono text-small text-foreground focus:border-ring"
                        />
                        <Text class="w-20 text-small text-muted-foreground">{`${d().prices.currency} / GiB·mo`}</Text>
                      </View>
                    )}
                  </Each>
                </View>
              </Fold>
            </Section>

            <Section title={t("settings.log")}>
              <Field class="w-52" label={t("settings.reqlog")} value={String(d()["reqlog-capacity"])} onChange={(v) => change({ ...d(), "reqlog-capacity": num(v, 5000) })} />
              <Toggle label={t("settings.persist")} checked={d()["reqlog-persist"]} onChange={(v) => change({ ...d(), "reqlog-persist": v }, true)} />
            </Section>

            <Updates />

            <Section title={t("settings.paths")} description={t("settings.telemetry")}>
              <Pairs rows={Object.entries(paths)} />
            </Section>
          </View>
        )}
      </If>
    </Page>
  );
}
