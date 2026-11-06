// Plugins: signed packages, verified against trusted roots and revocation
// lists, enabled or disabled, opened in their own sandboxed engine.

import { signal, Text, View } from "nsh";
import { t } from "../i18n";
import { api } from "../native";
import { bucket, endpoint, notify, prefix, report } from "../store";
import type { PluginInfo, TrustRoot } from "../types";
import { Badge, Button, Card, Empty, Field, Heading, KeyValue, Mono, Muted, Page, Title, Toggle } from "../ui/kit";

export function Plugins() {
  const list = signal<PluginInfo[]>(api.plugins());
  const roots = signal<TrustRoot[]>(api.trustRoots());
  const path = signal("");
  const label = signal("");
  const key = signal("");
  const revPath = signal("");
  const hostAvailable = api.pluginHostAvailable();
  const refresh = () => {
    list.set(api.plugins());
    roots.set(api.trustRoots());
  };
  const act = (f: () => unknown, done?: string) => {
    try {
      f();
      refresh();
      if (done) notify(done);
    } catch (e) {
      report(e);
    }
  };
  return (
    <Page testID="screen-plugins">
      <Title>{t("plugins.title")}</Title>
      <Muted>{t("plugins.sandbox")}</Muted>
      <If when={!hostAvailable}>
        <Badge tone="info" label={t("plugins.noHost")} />
      </If>
      <Card>
        <Heading>{t("plugins.install")}</Heading>
        <View class="flex-row items-end gap-2">
          <Field class="flex-1" label={t("plugins.path")} value={path()} onChange={(v) => path.set(v)} mono testID="plugin-path" />
          <Button variant="primary" label={t("plugins.install")} onPress={() => act(() => api.installPlugin(path()), t("plugins.enabled"))} disabled={!path()} testID="plugin-install" />
        </View>
      </Card>
      <If when={list().length > 0} else={<Empty text={t("plugins.none")} />}>
        <Each of={list()} key={(p) => p.id}>
          {(p) => (
            <Card testID={`plugin-${p().id}`}>
              <View class="flex-row items-center gap-2">
                <Text class="text-base font-semibold text-foreground">{p().name}</Text>
                <Badge label={`${p().version} (${p().build})`} />
                <If when={p().revoked}>
                  <Badge tone="fail" label={t("plugins.revoked")} />
                </If>
                <View class="flex-1" />
                <Button small label={t("plugins.open")} disabled={!p().enabled || !hostAvailable || !endpoint() || !bucket()} onPress={() => act(() => api.openPlugin(p().id, { endpoint: endpoint()!, bucket: bucket()!, prefix: prefix() }))} />
                <Button small variant="ghost" label={t("common.remove")} onPress={() => act(() => api.uninstallPlugin(p().id))} />
              </View>
              <Toggle label={t("plugins.enabled")} checked={p().enabled} disabled={p().revoked} onChange={(v) => act(() => api.enablePlugin(p().id, v))} />
              <KeyValue mono rows={[["id", p().id], ["signed by", p().keyId], ["sha256", p().sha256], ["host calls", p().hostCalls.join(", ")], ["installed", p().installed]]} />
            </Card>
          )}
        </Each>
      </If>
      <Card>
        <Heading>{t("plugins.roots")}</Heading>
        <Each of={roots()} key={(r) => r.id}>
          {(r) => (
            <View class="flex-row items-center gap-2">
              <Text class="w-40 text-sm text-foreground">{r().label}</Text>
              <Mono class="flex-1">{r().id}</Mono>
              <Button small variant="ghost" label={t("common.remove")} onPress={() => act(() => api.removeTrustRoot(r().id))} />
            </View>
          )}
        </Each>
        <View class="flex-row items-end gap-2">
          <Field class="w-40" label={t("plugins.rootLabel")} value={label()} onChange={(v) => label.set(v)} />
          <Field class="flex-1" label={t("plugins.rootKey")} value={key()} onChange={(v) => key.set(v)} mono testID="root-key" />
          <Button label={t("plugins.addRoot")} onPress={() => act(() => (api.addTrustRoot(label() || "root", key()), key.set("")))} disabled={!key()} testID="add-root" />
        </View>
        <View class="flex-row items-end gap-2">
          <Field class="flex-1" label={t("plugins.revocations")} value={revPath()} onChange={(v) => revPath.set(v)} mono />
          <Button label={t("plugins.revocations")} onPress={() => act(() => api.applyRevocations(revPath()))} disabled={!revPath()} />
        </View>
      </Card>
    </Page>
  );
}
