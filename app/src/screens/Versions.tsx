// A key's version timeline, delete markers highlighted; restoring a deleted
// key and deleting a version, behind the guard; the bucket's versioning.

import { List, resource, signal, Text, View } from "nsh";
import { bytes, date } from "../format";
import { t } from "../i18n";
import { api } from "../native";
import { bucket, endpoint, report, versionKey } from "../store";
import type { Plan, Timeline } from "../types";
import { Badge, Button, Card, Empty, Field, KeyValue, Mono, Muted, Page, terminal } from "../ui/kit";
import { BucketPicker, EndpointPicker } from "../ui/picker";
import { ConfirmPlan } from "../ui/confirm";
import { versioningLabel } from "../ui/overview";

export function Versions() {
  const keyDraft = signal(versionKey());
  const plan = signal<Plan | null>(null);
  const status = resource(
    () => (endpoint() && bucket() ? `${endpoint()}\n${bucket()}` : null),
    async (k) => {
      const [ep, b] = k.split("\n");
      try {
        return await api.versioning(ep, b);
      } catch (e) {
        report(e);
        return null;
      }
    },
  );
  const timeline = resource(
    () => (endpoint() && bucket() && versionKey() ? `${endpoint()}\n${bucket()}\n${versionKey()}` : null),
    async (k): Promise<Timeline | null> => {
      const [ep, b, key] = k.split("\n");
      try {
        return await api.timeline(ep, b, key);
      } catch (e) {
        report(e);
        return null;
      }
    },
  );
  const restore = async () => {
    try {
      plan.set(await api.planRestore(endpoint()!, bucket()!, versionKey()));
    } catch (e) {
      report(e);
    }
  };
  const del = (versionId: string, size: number) => {
    try {
      plan.set(api.planDeleteVersions(endpoint()!, bucket()!, [{ key: versionKey(), versionId, size }]));
    } catch (e) {
      report(e);
    }
  };
  return (
    <Page testID="screen-versions">
      <View class="flex-row flex-wrap items-end gap-2">
        <If when={terminal()}>
          <EndpointPicker />
          <BucketPicker />
        </If>
        <Field class="flex-1" label={t("versions.key")} value={keyDraft()} onChange={(v) => keyDraft.set(v)} mono testID="version-key" />
        <Button variant="primary" label={t("common.refresh")} onPress={() => (versionKey() === keyDraft() ? timeline.refetch() : versionKey.set(keyDraft()))} disabled={!bucket()} testID="load-versions" />
      </View>
      <If when={status()}>
        {(s) => (
          <Card>
            <KeyValue
              rows={[
                [t("versions.status"), versioningLabel(s().status)],
                [t("versions.mfa"), s().mfaDelete ?? "—"],
                [t("versions.lock"), s().objectLock ?? "—"],
              ]}
            />
          </Card>
        )}
      </If>
      <If when={timeline()} else={<Empty text={t("versions.enterKey")} />}>
        {(tl) => (
          <View class="gap-2" testID="timeline">
            <View class="flex-row items-center gap-2">
              <If when={tl().deleted}>
                <Badge label={t("versions.deleted")} tone="warn" />
                <Button small label={t("versions.restore")} onPress={restore} testID="restore" />
              </If>
              <Muted>{`${t("versions.noncurrent")}: ${bytes(tl().noncurrentBytes)}`}</Muted>
            </View>
            {/* a key may hold thousands: only the rows in view are drawn */}
            <List testID="versions" each={tl().versions} keyExtractor={(v) => v.versionId} estimatedItemSize={62} viewport="parent">
              {(v) => (
                <View class={`mb-1.5 flex-row items-center gap-3 rounded-(--radius) border px-3 py-2 ${v().isDeleteMarker ? "border-warn/50 bg-warn/10" : "border-border bg-card"}`}>
                  <View class={`h-2.5 w-2.5 rounded-full ${v().isDeleteMarker ? "bg-warn" : v().isLatest ? "bg-primary" : "bg-muted-foreground/40"}`} />
                  <View class="flex-1 gap-0.5">
                    <View class="flex-row items-center gap-2">
                      <Text class="text-body text-foreground">{date(v().lastModified)}</Text>
                      <If when={v().isLatest}>
                        <Badge label={t("versions.latest")} tone="primary" />
                      </If>
                      <If when={v().isDeleteMarker}>
                        <Badge label={t("versions.deleteMarker")} tone="warn" />
                      </If>
                    </View>
                    <Mono class="text-muted-foreground">{v().versionId}</Mono>
                  </View>
                  <Text class="w-20 text-right text-body text-muted-foreground">{v().isDeleteMarker ? "" : bytes(v().size)}</Text>
                  <Button small variant="ghost" label={t("versions.deleteVersion")} onPress={() => del(v().versionId, v().size)} />
                </View>
              )}
            </List>
          </View>
        )}
      </If>
      <ConfirmPlan
        plan={plan()}
        onClose={(o) => {
          plan.set(null);
          if (o) timeline.refetch();
        }}
      />
    </Page>
  );
}
