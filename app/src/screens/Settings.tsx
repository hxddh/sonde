// Settings: how sonde behaves, the few a user needs — what a recorded program
// may do, how the app looks, naming the code, prices — then updates and
// where the files are. Saved as they change: a toast says so, with Undo. The
// rest (sonde's own pace, prices by class) are config.toml's.

import { cleanup, effect, signal, Text, untracked, View } from "nsh";
import { language, t } from "../i18n";
import { api, asError } from "../native";
import { notify, refreshEndpoints, refreshSettings, report, settings } from "../store";
import type { Settings } from "../types";
import { Button, Choice, Field, FORM, KeyValue, Labeled, Notice, NumberField, Options, Page, Section, terminal, SwitchRow } from "../ui/kit";
import { Updates } from "../ui/updates";

/** A form's fields, two to a row (one where there is no room), every label above its control. */
export const GRID = "flex-row flex-wrap gap-4";
/** A field of the grid: half the row; an odd one out keeps its half beside an empty cell. */
export const CELL = "min-w-56 flex-1";

/** config.toml did not load: why, and what can be done. While it stands
 * sonde has no endpoints, default settings, and refuses every save, so the
 * file is never written over. */
export function ConfigNotice() {
  const read = () => {
    try {
      return api.configError();
    } catch {
      // an engine without it
      return null;
    }
  };
  const error = signal<string | null>(read());
  const reload = () => {
    try {
      api.reloadConfig();
      refreshEndpoints();
      refreshSettings();
      error.set(read());
      notify(t("config.reloaded"));
    } catch (e) {
      // it still does not load: the notice says why now
      error.set(asError(e).message);
    }
  };
  const reveal = () => {
    try {
      void api.sessions.reveal(`${api.paths().config}`).catch(report);
    } catch (e) {
      report(e);
    }
  };
  return (
    <If when={error()}>
      {(e) => (
        <Notice tone="error" title={t("config.unreadable")} text={e()} testID="config-error">
          <If when={!terminal()}>
            <Button small variant="outline" label={t("config.reveal")} onPress={reveal} testID="config-reveal" />
          </If>
          <Button small variant="outline" icon="refresh-cw" label={t("config.reload")} onPress={reload} testID="config-reload" />
        </Notice>
      )}
    </If>
  );
}

const copyOf = <T,>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

export function SettingsScreen() {
  const draft = signal<Settings | null>(settings() ? copyOf(settings()!) : null);
  let saved: Settings | null = settings() ? copyOf(settings()!) : null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  // a change still waiting to be saved when Settings closes is saved, not dropped
  let pending: (() => void) | null = null;
  cleanup(() => {
    if (timer) clearTimeout(timer);
    pending?.();
  });

  // changed elsewhere (the palette's theme or language, another window):
  // the draft follows, so a later change here never writes the old value back
  effect(() => {
    const now = settings();
    untracked(() => {
      if (!now || pending || JSON.stringify(now) === JSON.stringify(saved)) return;
      saved = copyOf(now);
      draft.set(copyOf(now));
    });
  });
  const write = (draftNext: Settings, undo: Settings | null) => {
    // only what changed here is written: a field changed elsewhere meanwhile
    // (the palette's theme, another window) keeps its new value
    const base = settings();
    const was = saved ?? base;
    const changed = Object.fromEntries(Object.entries(draftNext).filter(([k, v]) => !was || JSON.stringify(v) !== JSON.stringify((was as unknown as Record<string, unknown>)[k])));
    const next = base ? ({ ...copyOf(base), ...changed } as Settings) : draftNext;
    try {
      api.saveSettings(next);
      refreshSettings();
      saved = copyOf(next);
      draft.set(copyOf(next));
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
    pending = null;
    if (now) write(next, undo);
    else {
      pending = () => {
        pending = null;
        write(next, undo);
      };
      timer = setTimeout(() => pending?.(), 700);
    }
  };
  const paths = api.paths();

  return (
    <Page testID="screen-settings">
      <If when={draft()}>
        {(d) => (
          <View class={terminal() ? "gap-2" : `${FORM} gap-6`}>
            <ConfigNotice />
            <Section title={t("settings.appearance")}>
              <View class={GRID}>
                <Labeled class={CELL} label={t("settings.themeLabel")}>
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
                </Labeled>
                <Choice
                  class={CELL}
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
                <If when={!terminal()}>
                  <Labeled class={CELL} label={t("settings.contrastLabel")}>
                    <SwitchRow label={t("settings.contrast")} hint={t("settings.contrastHint")} checked={d()["high-contrast"] ?? false} onChange={(v) => change({ ...d(), "high-contrast": v }, true)} testID="high-contrast" />
                  </Labeled>
                  <View class={CELL} />
                </If>
              </View>
            </Section>

            {/* prices by storage class are config.toml's: the default is the one most stores need */}
            <Section title={t("settings.costs")} description={t("settings.costsHint")}>
              <View class={GRID}>
                <NumberField class={CELL} testID="price" label={t("settings.price")} value={d().prices["default-per-gib-month"]} min={0} step={0.001} onChange={(v) => change({ ...d(), prices: { ...d().prices, "default-per-gib-month": v ?? 0.023 } })} />
                <Field class={CELL} label={t("settings.currency")} value={d().prices.currency} onChange={(v) => change({ ...d(), prices: { ...d().prices, currency: v } })} />
              </View>
            </Section>

            <Updates />

            {/* the one place sonde says it sends nothing anywhere */}
            <Section title={t("settings.paths")} description={t("settings.telemetry")}>
              <KeyValue dense mono rows={Object.entries(paths).map(([k, v]) => [t(`settings.path.${k}` as "settings.path.data"), `${v}`])} />
            </Section>
          </View>
        )}
      </If>
    </Page>
  );
}
