// The error banner and the notice line: the app's own state, shown under a place.

import { Text, View } from "nsh";
import { t } from "../i18n";
import { lastError, notice, showRequest } from "../store";
import { Button } from "./kit";

export function ErrorBanner() {
  return (
    <If when={lastError()}>
      {(e) => (
        <View testID="error-banner" role="alert" class="mx-3 mb-2 flex-row items-center gap-3 rounded-(--radius) border border-destructive/40 bg-destructive/10 px-3 py-2">
          <View class="flex-1 gap-0.5">
            <Text class="text-sm font-medium text-destructive">
              {e().message}
            </Text>
            <If when={e().requestId}>
              <Text class="font-mono text-xs text-muted-foreground">
                {t("common.requestId")}: {e().requestId}
              </Text>
            </If>
          </View>
          <If when={e().recordId !== null && e().recordId !== undefined}>
            <Button small label={t("common.showRequest")} onPress={() => showRequest(e().recordId)} />
          </If>
          <Button small variant="ghost" label={t("common.close")} onPress={() => lastError.set(null)} />
        </View>
      )}
    </If>
  );
}

export function NoticeLine() {
  return (
    <If when={notice()}>
      {(n) => (
        <View class="mx-3 mb-2 self-center rounded-full bg-foreground px-3 py-1">
          <Text class="text-xs text-background">{n()}</Text>
        </View>
      )}
    </If>
  );
}
