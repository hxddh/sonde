// A key's version timeline, delete markers highlighted; restoring a deleted
// key and deleting a version, behind the guard; the bucket's versioning.

import { List, resource, Scroll, signal, Text, View } from "nsh";
import { bytes, date } from "../format";
import { t } from "../i18n";
import { api } from "../native";
import { bucket, endpoint, report, versionKey } from "../store";
import type { Plan, Timeline } from "../types";
import { Await, Badge, Button, Card, Dot, Empty, Field, Heading, IconButton, KeyValue, Notice, Path, terminal, ToolBar } from "../ui/kit";
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
  // a failure is the resource's: shown where the timeline would be, with Try again
  const asked = () => (endpoint() && bucket() && versionKey() ? `${endpoint()}\n${bucket()}\n${versionKey()}` : null);
  const timeline = resource(asked, async (k): Promise<{ asked: string; timeline: Timeline }> => {
    const [ep, b, key] = k.split("\n");
    return { asked: k, timeline: await api.timeline(ep, b, key) };
  });
  // the timeline of the key asked for: another key's (kept while this one loads) is not shown under it
  const shown = () => (timeline()?.asked === asked() ? timeline()!.timeline : null);
  // the key typed, or the same one read again (the field's Return and the button)
  const load = () => {
    if (!bucket()) return;
    if (versionKey() === keyDraft()) timeline.refetch();
    else versionKey.set(keyDraft());
  };
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
    <Scroll testID="screen-versions" class="flex-1" contentClass="gap-4 p-4">
      {/* what it looks at is typed here: a key, and Return (or the button) reads its history */}
      <ToolBar>
        <If when={terminal()}>
          <EndpointPicker />
          <BucketPicker />
        </If>
        <Field bare class="min-w-48 flex-1" label={t("versions.key")} placeholder={t("versions.key")} value={keyDraft()} onChange={(v) => keyDraft.set(v)} onSubmit={load} mono testID="version-key" />
        <Button variant={shown() ? "outline" : "primary"} label={t("common.refresh")} onPress={load} disabled={!bucket()} testID="load-versions" />
      </ToolBar>
      {/* the bucket's settings: there from the start, filled in when they are known */}
      <Card>
        <KeyValue
          rows={[
            [t("versions.status"), status() ? versioningLabel(status()!.status) : "—"],
            [t("versions.mfa"), status()?.mfaDelete ?? "—"],
            [t("versions.lock"), status()?.objectLock ?? "—"],
          ]}
        />
      </Card>
      <If when={versionKey()} else={<Empty icon="file-clock" text={t("versions.enterKey")} />}>
        <Await loading={timeline.loading() && !shown()} error={timeline.error()} retry={() => timeline.refetch()} waiting={t("versions.reading")} testID="versions">
          <If when={shown()}>
            {(tl) => (
              <View class="gap-3" testID="timeline">
                <View class="flex-row items-baseline gap-2">
                  <Heading count={tl().versions.length}>{t("browse.versions")}</Heading>
                  <Text class="min-w-0 flex-1 text-small text-muted-foreground line-clamp-1">{`${t("versions.noncurrent")}: ${bytes(tl().noncurrentBytes)}`}</Text>
                </View>
                <If when={tl().deleted}>
                  <Notice tone="warn" text={t("versions.deleted")}>
                    <Button small label={t("versions.restoreOpen")} tip={t("versions.restore")} onPress={restore} testID="restore" />
                  </Notice>
                </If>
                <If when={tl().versions.length > 0} else={<Empty text={t("versions.none")} />}>
                  <Card pad="none">
                    {/* a key may hold thousands: only the rows in view are drawn */}
                    <List testID="versions" each={tl().versions} keyExtractor={(v) => v.versionId} estimatedItemSize={52} viewport="parent">
                      {(v, i) => (
                        <View class={`flex-row items-center gap-3 px-3 py-2 ${i() > 0 ? "border-t border-border" : ""}`}>
                          {/* one column of marks: the latest, a delete marker, or none */}
                          <View class="size-2 shrink-0">
                            <If when={v().isDeleteMarker || v().isLatest}>
                              <Dot tone={v().isDeleteMarker ? "miss" : "live"} />
                            </If>
                          </View>
                          <View class="min-w-0 flex-1 gap-0.5">
                            <View class="flex-row items-center gap-2">
                              <Text class="text-body text-foreground">{date(v().lastModified)}</Text>
                              <If when={v().isLatest}>
                                <Badge label={t("versions.latest")} tone="primary" />
                              </If>
                              <If when={v().isDeleteMarker}>
                                <Badge label={t("versions.deleteMarker")} tone="warn" />
                              </If>
                            </View>
                            <Path path={v().versionId} max={64} class="text-muted-foreground" />
                          </View>
                          {/* sizes in one column, at its end */}
                          <Text class="w-20 shrink-0 text-right text-small tabular-nums text-muted-foreground">{v().isDeleteMarker ? "" : bytes(v().size)}</Text>
                          <IconButton icon="trash" label={t("versions.deleteVersionOpen")} onPress={() => del(v().versionId, v().size)} />
                        </View>
                      )}
                    </List>
                  </Card>
                </If>
              </View>
            )}
          </If>
        </Await>
      </If>
      <ConfirmPlan
        plan={plan()}
        onClose={(o) => {
          plan.set(null);
          if (o) timeline.refetch();
        }}
      />
    </Scroll>
  );
}
