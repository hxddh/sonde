// The first launch: no endpoint yet, so nothing can run or be browsed. One
// screen says what sonde does and offers the three ways in — the built-in
// demo, the AWS profiles already on this machine, or an endpoint by hand —
// and an example session to look at first.

import { type JSX, signal, Text, View } from "nsh";
import { Button as BaseButton } from "nsh/base";
import { t } from "../i18n";
import { api } from "../native";
import { endpointsMode, go, notify, openSession, refreshEndpoints, report, sessionsChanged, startDemo } from "../store";
import type { EndpointView } from "../types";
import { Icon, type IconName } from "../ui/icon";
import { Button, Check, terminal } from "../ui/kit";

function Way(props: { icon: IconName; title: string; text: string; action: JSX.Element; children?: JSX.Element; testID?: string }) {
  return (
    <View testID={props.testID} class="gap-3 rounded-(--radius-card) border border-border bg-card px-4 py-3.5">
      <View class="flex-row items-center gap-3.5">
        <If when={!terminal()}>
          <View class="size-9 items-center justify-center rounded-(--radius) bg-muted">
            <Icon name={props.icon} size={18} tint="foreground" />
          </View>
        </If>
        <View class="min-w-0 flex-1 gap-0.5">
          <Text class="text-body font-semibold text-card-foreground">{props.title}</Text>
          <Text class="text-small text-muted-foreground">{props.text}</Text>
        </View>
        {props.action}
      </View>
      {props.children}
    </View>
  );
}

export function Welcome() {
  const profiles = (() => {
    try {
      return api.importableProfiles();
    } catch {
      return [] as EndpointView[];
    }
  })();
  const showProfiles = signal(false);
  const chosen = signal<Record<string, boolean>>(Object.fromEntries(profiles.map((p) => [p.id, true])));
  const busy = signal(false);

  const demo = async () => {
    busy.set(true);
    if (await startDemo()) notify(t("welcome.demoReady"));
    busy.set(false);
  };
  const importChosen = () => {
    const names = profiles.filter((e) => chosen()[e.id]).map((e) => e.credentials.replace(/^profile /, ""));
    try {
      const n = api.importProfiles(names);
      refreshEndpoints();
      notify(t("welcome.imported", { n }));
    } catch (e) {
      report(e);
    }
  };
  const example = () => {
    try {
      openSession.set(api.sessions.example());
      sessionsChanged();
      go("sessions");
    } catch (e) {
      report(e);
    }
  };

  return (
    <View testID="welcome" class="flex-1 items-center justify-center p-8">
      <View class="w-full max-w-[560px] gap-6">
        <View class="gap-2">
          <Text class="text-title font-semibold text-foreground">{t("welcome.title")}</Text>
          <Text class="text-body leading-5 text-muted-foreground">{t("welcome.text")}</Text>
        </View>
        <View class="gap-2.5">
          <Way
            testID="welcome-way-demo"
            icon="sparkles"
            title={t("welcome.demo")}
            text={t("welcome.demoText")}
            action={<Button variant="primary" label={busy() ? t("common.loading") : t("welcome.demoStart")} onPress={demo} disabled={busy()} testID="welcome-demo" />}
          />
          <Way
            icon="key-round"
            title={t("welcome.import")}
            text={profiles.length ? profiles.length === 1 ? t("welcome.importFoundOne") : t("welcome.importFound", { n: profiles.length }) : t("welcome.importNone")}
            action={<Button variant="outline" label={t("welcome.importChoose")} onPress={() => showProfiles.set(!showProfiles())} disabled={profiles.length === 0} testID="welcome-import" />}
          >
            <If when={showProfiles()}>
              <View class="gap-2 border-t border-border pt-3">
                <Each of={profiles}>
                  {(p) => <Check label={`${p().name} — ${p().url}`} checked={!!chosen()[p().id]} onChange={(v) => chosen.set({ ...chosen(), [p().id]: v })} />}
                </Each>
                <View class="flex-row">
                  <Button small variant="primary" label={t("welcome.importDo")} onPress={importChosen} disabled={!Object.values(chosen()).some(Boolean)} testID="welcome-import-do" />
                </View>
              </View>
            </If>
          </Way>
          <Way
            icon="plus"
            title={t("welcome.add")}
            text={t("welcome.addText")}
            action={
              <Button
                variant="outline"
                label={t("welcome.addDo")}
                onPress={() => {
                  endpointsMode.set("add");
                  go("endpoints");
                }}
                testID="welcome-add"
              />
            }
          />
        </View>
        <BaseButton accessibilityLabel={t("welcome.example")} onPress={example} testID="open-example" class="flex-row items-center gap-1.5 self-start rounded-(--radius-sm) px-1 py-0.5 hover:bg-muted">
          <Icon name="scroll-text" size={14} />
          <Text class="text-small text-muted-foreground">{t("welcome.example")}</Text>
        </BaseButton>
      </View>
    </View>
  );
}
