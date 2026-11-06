// Endpoint and bucket pickers shared by the screens.

import { resource, Text, View } from "nsh";
import { t } from "../i18n";
import { api } from "../native";
import { bucket, chooseBucket, endpoint, endpoints, report } from "../store";
import { Choice } from "./kit";

export function EndpointPicker(props: { testID?: string }) {
  return (
    <Choice
      testID={props.testID ?? "endpoint-picker"}
      placeholder={t("common.chooseEndpoint")}
      value={endpoint()}
      items={endpoints().map((e) => ({ value: e.id, title: e.name }))}
      onChange={(v) => {
        endpoint.set(v);
        chooseBucket(null);
      }}
      class="w-56"
    />
  );
}

export function BucketPicker(props: { testID?: string }) {
  const list = resource(endpoint, async (ep) => {
    try {
      return await api.buckets(ep);
    } catch (e) {
      report(e);
      return [];
    }
  });
  return (
    <View class="flex-row items-center gap-2">
      <Choice
        testID={props.testID ?? "bucket-picker"}
        placeholder={t("common.chooseBucket")}
        value={bucket()}
        items={(list() ?? []).map((b) => ({ value: b.name, title: b.name }))}
        onChange={(v) => chooseBucket(v)}
        class="w-56"
      />
      <If when={list.state() === "pending"}>
        <Text class="text-xs text-muted-foreground">{t("common.loading")}</Text>
      </If>
    </View>
  );
}
