// The first launch: no endpoint yet, so nothing can run or be browsed. One
// screen says what sonde does and offers one way in: the built-in demo,
// which lands on Run with a command ready to press Return on. The AWS
// profiles already on this machine, an endpoint by hand and an example
// session are a line below, for those who know what they want.

import { platform, signal, Text, View } from "nsh";
import { Button as BaseButton } from "nsh/base";
import { t } from "../i18n";
import { api } from "../native";
import { command } from "../run";
import { endpointsMode, go, notify, openSession, refreshEndpoints, report, sessionsChanged, startDemo } from "../store";
import type { EndpointView } from "../types";
import { Icon, type IconName } from "../ui/icon";
import { Button, CARD, Check, terminal } from "../ui/kit";

/** A first command for the demo: two reads, one of a key that is not there,
 * signed with the keys sonde gives the program (cmd.exe on Windows). */
export function sampleCommand(): string {
  const win = platform.os === "windows";
  const v = (n: string) => (win ? `%${n}%` : `$${n}`);
  const sink = win ? "NUL" : "/dev/null";
  const get = (k: string) => `curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u ${v("AWS_ACCESS_KEY_ID")}:${v("AWS_SECRET_ACCESS_KEY")} -o ${sink} ${v("AWS_ENDPOINT_URL")}/demo/${k}`;
  return `${get("README.md")} && ${get("missing.txt")}`;
}

function Link(props: { icon: IconName; label: string; onPress: () => void; disabled?: boolean; testID?: string }) {
  return (
    <BaseButton accessibilityLabel={props.label} onPress={props.onPress} disabled={props.disabled} testID={props.testID} class="flex-row items-center gap-1.5 rounded-(--radius-sm) px-1.5 py-1 hover:bg-muted disabled:opacity-50">
      <If when={!terminal()}>
        <Icon name={props.icon} size={14} />
      </If>
      <Text class="text-small text-muted-foreground">{props.label}</Text>
    </BaseButton>
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

  // the one way in: the demo, and Run with a command ready
  const demo = async () => {
    busy.set(true);
    if (await startDemo()) {
      command.set(sampleCommand());
      go("run");
      notify(t("welcome.demoReady"));
    }
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
      <View class={terminal() ? "w-full gap-2" : "w-full max-w-[520px] gap-6"}>
        <View class="gap-2">
          <Text class="text-title font-semibold text-foreground">{t("welcome.title")}</Text>
          <Text class="text-body leading-5 text-muted-foreground">{t("welcome.text")}</Text>
        </View>
        <View testID="welcome-way-demo" class={`gap-3 ${CARD} p-4`}>
          <Text class="text-body font-semibold text-card-foreground">{t("welcome.demo")}</Text>
          <Text class="text-small leading-5 text-muted-foreground">{t("welcome.demoText")}</Text>
          <Button variant="primary" icon="play" label={busy() ? t("common.loading") : t("welcome.demoStart")} onPress={demo} disabled={busy()} testID="welcome-demo" class="self-start" />
        </View>
        {/* the other ways in, for those who know what they want */}
        <View class="gap-1">
          <Text class="px-1.5 text-caption font-medium text-muted-foreground">{t("welcome.or")}</Text>
          <View class="flex-row flex-wrap gap-x-2">
            <Link
              icon="key-round"
              label={profiles.length ? t("welcome.importFound", { n: profiles.length }) : t("welcome.importNone")}
              onPress={() => showProfiles.set(!showProfiles())}
              disabled={profiles.length === 0}
              testID="welcome-import"
            />
            <Link
              icon="plus"
              label={t("welcome.addDo")}
              onPress={() => {
                endpointsMode.set("add");
                go("endpoints");
              }}
              testID="welcome-add"
            />
            <Link icon="scroll-text" label={t("welcome.example")} onPress={example} testID="open-example" />
          </View>
          <If when={showProfiles()}>
            <View class={`gap-2 ${CARD} p-4`}>
              <Each of={profiles}>{(p) => <Check label={`${p().name} — ${p().url}`} checked={!!chosen()[p().id]} onChange={(v) => chosen.set({ ...chosen(), [p().id]: v })} />}</Each>
              <View class="flex-row">
                <Button small variant="primary" label={t("welcome.importDo")} onPress={importChosen} disabled={!Object.values(chosen()).some(Boolean)} testID="welcome-import-do" />
              </View>
            </View>
          </If>
        </View>
      </View>
    </View>
  );
}
