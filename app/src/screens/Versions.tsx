// An object's versions, opened from its inspector: the bucket's versioning,
// the key's version timeline with delete markers marked, restoring a deleted
// key and deleting a version, each behind the guard.

import { List, resource, signal, Text, View } from "nsh";
import { bytes, date } from "../format";
import { t } from "../i18n";
import { api } from "../native";
import { bucket, endpoint, report } from "../store";
import type { Plan } from "../types";
import { REVEAL, Await, Back, Badge, Button, Card, Dot, Empty, Heading, IconButton, KeyValue, Notice, Path, Row, WIDTH, TEXT } from "../ui/kit";
import { ConfirmPlan } from "../ui/confirm";
import { versioningLabel } from "../ui/overview";

export function KeyVersions(props: { objectKey: string; onBack: () => void }) {
  const ep = endpoint()!;
  const b = bucket()!;
  const plan = signal<Plan | null>(null);
  const status = resource(
    () => `${ep}\n${b}`,
    async () => {
      try {
        return await api.versioning(ep, b);
      } catch (e) {
        report(e);
        return null;
      }
    },
  );
  // a failure is the resource's: shown where the timeline would be, with Try again
  const timeline = resource(
    () => props.objectKey,
    (key) => api.timeline(ep, b, key),
  );
  const restore = async () => {
    try {
      plan.set(await api.planRestore(ep, b, props.objectKey));
    } catch (e) {
      report(e);
    }
  };
  const del = (versionId: string, size: number) => {
    try {
      plan.set(api.planDeleteVersions(ep, b, [{ key: props.objectKey, versionId, size }]));
    } catch (e) {
      report(e);
    }
  };
  return (
    <View testID="screen-versions" class="gap-4">
      <View class="flex-row items-center gap-2">
        <Back label={t("browse.backToObject")} onPress={props.onBack} testID="versions-back" />
      </View>
      {/* the bucket's settings: there from the start, filled in when they are known */}
      <KeyValue
        dense
        rows={[
          [t("versions.status"), status() ? versioningLabel(status()!.status) : "—"],
          [t("versions.mfa"), status()?.mfaDelete ?? "—"],
          [t("versions.lock"), status()?.objectLock ?? "—"],
        ]}
      />
      <Await loading={timeline.loading() && !timeline()} error={timeline.error()} retry={() => timeline.refetch()} waiting={t("versions.reading")} testID="versions">
        <If when={timeline()}>
          {(tl) => (
            <View class="gap-3" testID="timeline">
              <View class="flex-row items-baseline gap-2">
                <Heading count={tl().versions.length}>{t("browse.versions")}</Heading>
                <Text class={`min-w-0 flex-1 ${TEXT.note} line-clamp-1`}>{`${t("versions.noncurrent")}: ${bytes(tl().noncurrentBytes)}`}</Text>
              </View>
              <If when={tl().deleted}>
                <Notice tone="warn" text={t("versions.deleted")}>
                  <Button small label={t("versions.restoreOpen")} tip={t("versions.restore")} onPress={restore} testID="restore" />
                </Notice>
              </If>
              <If when={tl().versions.length > 0} else={<Empty compact text={t("versions.none")} />}>
                <Card pad="none">
                  {/* a key may hold thousands: only the rows in view are drawn */}
                  <List testID="versions" each={tl().versions} keyExtractor={(v) => v.versionId} estimatedItemSize={52} viewport="parent">
                    {(v, i) => (
                      <Row first={i() === 0}>
                        {/* one column of marks: the latest, a delete marker, or none */}
                        <View class="size-2 shrink-0">
                          <If when={v().isDeleteMarker || v().isLatest}>
                            <Dot tone={v().isDeleteMarker ? "miss" : "live"} />
                          </If>
                        </View>
                        <View class="min-w-0 flex-1 gap-1">
                          <View class="flex-row items-center gap-2">
                            <Text class={TEXT.body}>{date(v().lastModified)}</Text>
                            <If when={v().isLatest}>
                              <Badge label={t("versions.latest")} tone="live" />
                            </If>
                            <If when={v().isDeleteMarker}>
                              <Badge label={t("versions.deleteMarker")} tone="warn" />
                            </If>
                          </View>
                          <Path path={v().versionId} max={64} class="text-muted-foreground" />
                        </View>
                        {/* sizes in one column, at its end */}
                        <Text class={`${WIDTH.size} shrink-0 text-right tabular-nums ${TEXT.note}`}>{v().isDeleteMarker ? "" : bytes(v().size)}</Text>
                        {/* its removal under the pointer or the keyboard only */}
                        <View class={REVEAL}>
                          <IconButton icon="trash" label={t("versions.deleteVersionOpen")} onPress={() => del(v().versionId, v().size)} />
                        </View>
                      </Row>
                    )}
                  </List>
                </Card>
              </If>
            </View>
          )}
        </If>
      </Await>
      <ConfirmPlan
        plan={plan()}
        onClose={(o) => {
          plan.set(null);
          if (o) timeline.refetch();
        }}
      />
    </View>
  );
}
