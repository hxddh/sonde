// Settings: read-only mode, limits, prices, request log, appearance, language.

import { signal, View } from "nsh";
import { language, t } from "../i18n";
import { api } from "../native";
import { refreshSettings, report, settings } from "../store";
import type { Settings } from "../types";
import { Button, Card, Choice, Field, Heading, KeyValue, Muted, Page, Title, Toggle } from "../ui/kit";
import { Updates } from "../ui/updates";

export function SettingsScreen() {
  const s = settings();
  const draft = signal<Settings | null>(s ? structuredCopy(s) : null);
  const save = (next: Settings) => {
    try {
      api.saveSettings(next);
      refreshSettings();
      draft.set(structuredCopy(next));
    } catch (e) {
      report(e);
    }
  };
  const num = (v: string, d: number) => (Number.isFinite(Number(v)) && v.trim() !== "" ? Number(v) : d);
  const paths = api.paths();
  return (
    <Page testID="screen-settings">
      <Title>{t("settings.title")}</Title>
      <Updates />
      <If when={draft()}>
        {(d) => (
          <View class="gap-4">
            <Card>
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
            </Card>
            <Card>
              <View class="flex-row flex-wrap gap-3">
                <Field class="w-60" label={t("settings.concurrency")} value={String(d().concurrency)} onChange={(v) => draft.set({ ...d(), concurrency: num(v, d().concurrency) })} testID="concurrency" />
                <Field class="w-72" label={t("settings.rate")} value={d()["rate-limit"] === null ? "" : String(d()["rate-limit"])} onChange={(v) => draft.set({ ...d(), "rate-limit": v.trim() === "" ? null : num(v, 0) })} />
                <Field class="w-60" label={t("settings.staleDays")} value={String(d()["stale-upload-days"])} onChange={(v) => draft.set({ ...d(), "stale-upload-days": num(v, 7) })} />
              </View>
              <View class="flex-row flex-wrap gap-3">
                <Field class="w-60" label={t("settings.price")} value={String(d().prices["default-per-gib-month"])} onChange={(v) => draft.set({ ...d(), prices: { ...d().prices, "default-per-gib-month": num(v, 0.023) } })} />
                <Field class="w-32" label={t("settings.currency")} value={d().prices.currency} onChange={(v) => draft.set({ ...d(), prices: { ...d().prices, currency: v } })} />
              </View>
              <Heading>STANDARD · STANDARD_IA · GLACIER · …</Heading>
              <View class="flex-row flex-wrap gap-3">
                <Each of={Object.entries(d().prices["by-class"])}>
                  {(e) => <Field class="w-44" label={e()[0]} value={String(e()[1])} onChange={(v) => draft.set({ ...d(), prices: { ...d().prices, "by-class": { ...d().prices["by-class"], [e()[0]]: num(v, e()[1]) } } })} />}
                </Each>
              </View>
              <View class="flex-row flex-wrap gap-3">
                <Field class="w-60" label={t("settings.reqlog")} value={String(d()["reqlog-capacity"])} onChange={(v) => draft.set({ ...d(), "reqlog-capacity": num(v, 5000) })} />
              </View>
              <Toggle label={t("settings.persist")} checked={d()["reqlog-persist"]} onChange={(v) => draft.set({ ...d(), "reqlog-persist": v })} />
              <View class="flex-row flex-wrap gap-3">
                <Choice
                  class="w-48"
                  label={t("settings.theme")}
                  value={d().theme}
                  items={[
                    { value: "system", title: t("settings.system") },
                    { value: "light", title: t("settings.light") },
                    { value: "dark", title: t("settings.dark") },
                  ]}
                  onChange={(v) => draft.set({ ...d(), theme: v })}
                />
                <Choice
                  class="w-48"
                  label={t("settings.language")}
                  value={d().language}
                  items={[
                    { value: "auto", title: t("settings.system") },
                    { value: "en", title: "English" },
                    { value: "zh", title: "中文" },
                  ]}
                  onChange={(v) => {
                    draft.set({ ...d(), language: v });
                    language.set(v);
                  }}
                  testID="language"
                />
              </View>
              <View class="flex-row gap-2">
                <Button variant="primary" label={t("common.save")} onPress={() => save({ ...d(), "read-only": settings()?.["read-only"] ?? true })} testID="save-settings" />
              </View>
            </Card>
            <Card>
              <Heading>{t("settings.paths")}</Heading>
              <KeyValue mono rows={Object.entries(paths)} />
              <Muted>{t("settings.telemetry")}</Muted>
            </Card>
          </View>
        )}
      </If>
    </Page>
  );
}

function structuredCopy<T>(v: T): T {
  return JSON.parse(JSON.stringify(v)) as T;
}
