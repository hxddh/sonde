// Endpoint and bucket pickers shared by the screens.

import { resource, View } from "nsh";
import { t } from "../i18n";
import { api } from "../native";
import { bucket, chooseBucket, endpoint, endpoints, report, useEndpoint } from "../store";
import { Choice, Loading, WIDTH } from "./kit";

export function EndpointPicker(props: { testID?: string; class?: string }) {
  return (
    <Choice
      testID={props.testID ?? "endpoint-picker"}
      placeholder={t("common.chooseEndpoint")}
      value={endpoint()}
      items={endpoints().map((e) => ({ value: e.id, title: e.name }))}
      onChange={(v) => useEndpoint(v)}
      class={props.class ?? WIDTH.field}
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
        class={WIDTH.field}
      />
      <If when={list.state() === "pending"}>
        <Loading />
      </If>
    </View>
  );
}
