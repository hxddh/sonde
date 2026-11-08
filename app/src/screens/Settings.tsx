// Settings: how sonde behaves. Grouped by what they are about — appearance,
// safety, requests, costs, the request log, updates, files — and saved as
// they change: a toast says so, with Undo.

import { cleanup, signal, Text, View } from "nsh";
import { language, t } from "../i18n";
import { api } from "../native";
import { notify, refreshSettings, report, settings } from "../store";
import type { Settings } from "../types";
import { Choice, Field, KeyValue, NumberField, Options, Page, Section, terminal, Toggle } from "../ui/kit";
import { Fold } from "../ui/record";
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
                  <Options
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
                <NumberField class="w-52" label={t("settings.concurrency")} value={d().concurrency} min={1} max={256} onChange={(v) => change({ ...d(), concurrency: v ?? d().concurrency })} testID="concurrency" />
                <NumberField class="w-64" label={t("settings.rate")} value={d()["rate-limit"]} min={0} step={10} onChange={(v) => change({ ...d(), "rate-limit": v })} />
                <NumberField class="w-52" label={t("settings.staleDays")} value={d()["stale-upload-days"]} min={1} max={3650} onChange={(v) => change({ ...d(), "stale-upload-days": v ?? 7 })} />
              </View>
            </Section>

            <Section title={t("settings.costs")} description={t("settings.costsHint")}>
              <View class="flex-row flex-wrap gap-3">
                <NumberField class="w-52" label={t("settings.price")} value={d().prices["default-per-gib-month"]} min={0} step={0.001} onChange={(v) => change({ ...d(), prices: { ...d().prices, "default-per-gib-month": v ?? 0.023 } })} />
                <Field class="w-28" label={t("settings.currency")} value={d().prices.currency} onChange={(v) => change({ ...d(), prices: { ...d().prices, currency: v } })} />
              </View>
              <Fold title={t("settings.byClass")} count={Object.keys(d().prices["by-class"]).length} open={false}>
                <View class="overflow-hidden rounded-(--radius) border border-border bg-card">
                  <Each of={Object.entries(d().prices["by-class"])}>
                    {(e, i) => (
                      <View class={`h-9 flex-row items-center gap-3 px-3 ${i() > 0 ? "border-t border-border" : ""}`}>
                        <Text class="flex-1 font-mono text-small text-foreground">{e()[0]}</Text>
                        <NumberField bare class="w-32" label={e()[0]} value={e()[1]} min={0} step={0.001} onChange={(v) => change({ ...d(), prices: { ...d().prices, "by-class": { ...d().prices["by-class"], [e()[0]]: v ?? e()[1] } } })} />
                        <Text class="w-20 text-small text-muted-foreground">{`${d().prices.currency} / GiB·mo`}</Text>
                      </View>
                    )}
                  </Each>
                </View>
              </Fold>
            </Section>

            <Section title={t("settings.log")}>
              <NumberField class="w-52" label={t("settings.reqlog")} value={d()["reqlog-capacity"]} min={100} step={1000} onChange={(v) => change({ ...d(), "reqlog-capacity": v ?? 5000 })} />
              <Toggle label={t("settings.persist")} checked={d()["reqlog-persist"]} onChange={(v) => change({ ...d(), "reqlog-persist": v }, true)} />
            </Section>

            <Updates />

            <Section title={t("settings.paths")} description={t("settings.telemetry")}>
              <KeyValue boxed rows={Object.entries(paths)} />
            </Section>
          </View>
        )}
      </If>
    </Page>
  );
}
