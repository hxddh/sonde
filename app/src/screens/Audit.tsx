// The audit log of destructive operations.

import { Text, View } from "nsh";
import { date } from "../format";
import { t } from "../i18n";
import { api } from "../native";
import { Badge, Card, Empty, Mono, Muted, Page, Title } from "../ui/kit";

export function Audit() {
  const entries = api.auditLog(200);
  return (
    <Page testID="screen-audit">
      <Title>{t("audit.title")}</Title>
      <Muted>{(api.paths() as Record<string, string>).audit}</Muted>
      <If when={entries.length > 0} else={<Empty text={t("audit.none")} />}>
        <Each of={entries}>
          {(e) => (
            <Card>
              <View class="flex-row items-center gap-2">
                <Text class="flex-1 text-sm font-medium text-foreground">{`${e().action} · ${e().endpoint}/${e().bucket}`}</Text>
                <Badge label={`${e().succeeded}/${e().items}`} tone={e().failed ? "warn" : "pass"} />
                <Text class="text-xs text-muted-foreground">{date(e().time)}</Text>
              </View>
              <Mono>{e().targets.slice(0, 10).join("\n")}</Mono>
              <If when={e().errors.length > 0}>
                <Text class="text-xs text-destructive">{e().errors.join("; ")}</Text>
              </If>
            </Card>
          )}
        </Each>
      </If>
    </Page>
  );
}
