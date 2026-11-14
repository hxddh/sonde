// The first launch: no endpoint yet, so nothing can run or be browsed. One
// screen says what sonde does and offers one way in: the built-in demo,
// which lands on Run with a command ready to press Return on. The AWS
// profiles already on this machine, an endpoint by hand and an example
// session are a line below, for those who know what they want.

import { platform, signal, Text, View } from "nsh";
import { t } from "../i18n";
import { api } from "../native";
import { command } from "../run";
import { endpointsMode, go, notify, openSession, openSessionFile, refreshEndpoints, report, sessionsChanged, startDemo } from "../store";
import type { EndpointView } from "../types";
import { Actions, Button, Card, Check, Heading, READING, terminal, Title, TEXT } from "../ui/kit";

/** A first command for the demo: two reads, one of a key that is not there,
 * signed with the keys sonde gives the program (cmd.exe on Windows). */
export function sampleCommand(): string {
  const win = platform.os === "windows";
  const v = (n: string) => (win ? `%${n}%` : `$${n}`);
  const sink = win ? "NUL" : "/dev/null";
  const get = (k: string) => `curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u ${v("AWS_ACCESS_KEY_ID")}:${v("AWS_SECRET_ACCESS_KEY")} -o ${sink} ${v("AWS_ENDPOINT_URL")}/demo/${k}`;
  return `${get("README.md")} && ${get("missing.txt")}`;
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
    <View testID="welcome" class="flex-1 items-center justify-center p-6">
      <View class={terminal() ? "w-full gap-2" : `${READING} gap-6`}>
        <View class="gap-2">
          <Title page>{t("welcome.title")}</Title>
          <Text class="text-body leading-5 text-muted-foreground">{t("welcome.text")}</Text>
        </View>
        <Card testID="welcome-way-demo">
          <Heading>{t("welcome.demo")}</Heading>
          <Text class="text-small leading-5 text-muted-foreground">{t("welcome.demoText")}</Text>
          <View class="flex-row">
            <Button variant="primary" icon="play" label={t("welcome.demoStart")} onPress={demo} loading={busy()} testID="welcome-demo" />
          </View>
        </Card>
        {/* the other ways in, for those who know what they want */}
        <View class="gap-3">
          <View class="flex-row items-center gap-3">
            <View class="h-px flex-1 bg-border" />
            <Text class={TEXT.meta}>{t("welcome.or")}</Text>
            <View class="h-px flex-1 bg-border" />
          </View>
          {/* one column, aligned with the card above: never a ragged wrap */}
          <View class="items-stretch gap-1">
            <If when={profiles.length > 0}>
              <Button small variant="ghost" icon="key-round" label={t("welcome.importFound", { n: profiles.length })} onPress={() => showProfiles.set(!showProfiles())} testID="welcome-import" class="justify-start" />
            </If>
            <Button
              small
              variant="ghost"
              icon="plus"
              label={t("welcome.addDo")}
              onPress={() => {
                endpointsMode.set("add");
                go("endpoints");
              }}
              testID="welcome-add" class="justify-start"
            />
            {/* production as it ran: an access log, CloudTrail files, a HAR */}
            <If when={!terminal()}>
              <Button small variant="ghost" icon="scroll-text" label={t("welcome.openLog")} onPress={() => void openSessionFile()} testID="welcome-open-log" class="justify-start" />
            </If>
            <Button small variant="ghost" icon="list-clock" label={t("welcome.example")} onPress={example} testID="open-example" class="justify-start" />
          </View>
          <If when={showProfiles()}>
            <Card>
              <Each of={profiles} key={(p) => p.id}>{(p) => <Check label={`${p().name} — ${p().url}`} checked={!!chosen()[p().id]} onChange={(v) => chosen.set({ ...chosen(), [p().id]: v })} />}</Each>
              <Actions>
                <Button variant="outline" label={t("welcome.importDo")} onPress={importChosen} disabled={!Object.values(chosen()).some(Boolean)} testID="welcome-import-do" />
              </Actions>
            </Card>
          </If>
        </View>
      </View>
    </View>
  );
}
