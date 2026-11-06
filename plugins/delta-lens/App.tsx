// A sonde plugin: Delta Lake's `_delta_log` as a commit timeline and a
// replay of the table's files at any version. It reads only through the
// host's read-only calls (`sonde.read.*`); it has no network, files or
// credentials.

import { host, mount, resource, Scroll, signal, Text, View } from "nsh";
import { Button } from "nsh/base";
import { type Commit, parseCommit, replay, versionOf } from "./delta";

interface Obj {
  key: string;
  size: number;
}

function decode(b64: string): string {
  return new TextDecoder().decode(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)));
}

async function load() {
  const ctx = (await host.call("sonde.read.context")) as { bucket: string; prefix: string };
  const objs = (await host.call("sonde.read.list", { prefix: ctx.prefix, limit: 10000 })) as Obj[];
  const logs = objs.map((o) => ({ ...o, version: versionOf(o.key) })).filter((o) => o.version !== null) as (Obj & { version: number })[];
  const commits = [];
  for (const l of logs.sort((a, b) => a.version - b.version)) {
    const r = (await host.call("sonde.read.get", { key: l.key })) as { base64: string };
    commits.push({ version: l.version, ...parseCommit(l.version, l.key, decode(r.base64)) });
  }
  const checkpoints = objs.filter((o) => o.key.includes("_delta_log/") && o.key.endsWith(".checkpoint.parquet")).length;
  return { ctx, commits, checkpoints, objects: objs.length };
}

function App() {
  const data = resource(load);
  const at = signal<number | null>(null);
  return (
    <View class="flex-1 bg-background p-3">
      <Text class="text-lg font-semibold text-foreground">Delta Lake lens</Text>
      <If when={data.error()}>
        <Text class="text-destructive">{String(data.error())}</Text>
      </If>
      <If when={data()} else={<Text class="text-muted-foreground">Reading _delta_log…</Text>}>
        {(d) => {
          const version = () => at() ?? (d().commits.at(-1)?.version ?? 0);
          const state = () => replay(d().commits, version());
          return (
            <Scroll class="flex-1" contentClass="gap-2 py-2">
              <Text class="text-sm text-muted-foreground">{`${d().ctx.bucket}/${d().ctx.prefix} · ${d().commits.length} commits · ${d().checkpoints} checkpoints · ${d().objects} objects`}</Text>
              <Text class="font-semibold text-foreground">{`Version ${version()}: ${state().files.size} files`}</Text>
              <Text class="text-xs text-muted-foreground">{`protocol reader ${state().minReader ?? "?"}, writer ${state().minWriter ?? "?"}`}</Text>
              <Each of={d().commits.map((c) => c.commit)} key={(c: Commit) => c.version}>
                {(c) => (
                  <Button onPress={() => at.set(c().version)} accessibilityLabel={`version ${c().version}`} class="flex-row gap-3 rounded-sm px-1 hover:bg-muted">
                    <Text class={c().version === version() ? "w-12 font-semibold text-primary" : "w-12 text-foreground"}>{`v${c().version}`}</Text>
                    <Text class="w-32 text-foreground">{c().operation}</Text>
                    <Text class="flex-1 text-muted-foreground">{`+${c().adds} −${c().removes}${c().schemaChanged ? " · schema" : ""}${c().protocol ? ` · ${c().protocol}` : ""}`}</Text>
                  </Button>
                )}
              </Each>
              <Text class="pt-2 font-semibold text-foreground">Files at this version</Text>
              <Each of={[...state().files.entries()]}>{(f) => <Text class="font-mono text-xs text-foreground">{`${f()[0]}  ${f()[1]} B`}</Text>}</Each>
            </Scroll>
          );
        }}
      </If>
    </View>
  );
}

mount(App);
