// Running and recent jobs: progress, limits, cancel.

import { cleanup, signal, Text, View } from "nsh";
import { ago } from "../format";
import { t } from "../i18n";
import { api } from "../native";
import { showRequest } from "../store";
import type { JobInfo } from "../types";
import { Badge, Bar, Button, Card, Empty, Muted, Page, Title } from "../ui/kit";

export function Jobs() {
  const jobs = signal<JobInfo[]>(api.jobs());
  const timer = setInterval(() => jobs.set(api.jobs()), 500);
  cleanup(() => clearInterval(timer));
  const tone = (s: string) => (s === "finished" ? "pass" : s === "failed" ? "fail" : s === "cancelled" ? "unsupported" : "primary");
  return (
    <Page testID="screen-jobs">
      <Title>{t("jobs.title")}</Title>
      <If when={jobs().length > 0} else={<Empty text={t("jobs.none")} />}>
        <Each of={jobs()} key={(j) => j.id}>
          {(j) => (
            <Card>
              <View class="flex-row items-center gap-2">
                <Text class="flex-1 text-sm font-medium text-foreground">{j().title}</Text>
                <Badge label={j().state} tone={tone(j().state)} />
                <If when={j().state === "running"}>
                  <Button small label={t("jobs.cancel")} onPress={() => api.cancelJob(j().id)} />
                </If>
              </View>
              <If when={j().state === "running"}>
                <Bar value={j().progress.total ? j().progress.done / Math.max(1, j().progress.total!) : null} label={j().progress.message} />
              </If>
              <Muted>{`${j().id} · ${ago(j().started)} · ${j().concurrency} in flight${j().rateLimit ? ` · ${j().rateLimit}/s` : ""}`}</Muted>
              <If when={j().error}>
                {(e) => (
                  <View class="flex-row items-center gap-2">
                    <Text class="flex-1 text-sm text-destructive">{e().message}</Text>
                    <If when={e().recordId !== null && e().recordId !== undefined}>
                      <Button small label={t("common.showRequest")} onPress={() => showRequest(e().recordId)} />
                    </If>
                  </View>
                )}
              </If>
            </Card>
          )}
        </Each>
      </If>
    </Page>
  );
}
