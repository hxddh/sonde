// Delta Lake's transaction log: `_delta_log/<version:020>.json`, one JSON
// action per line (`add`, `remove`, `metaData`, `protocol`, `commitInfo`,
// `txn`). Replaying the actions in version order gives the table's files.
// (Checkpoints, `*.checkpoint.parquet`, are listed but not read.)

export interface Commit {
  version: number;
  key: string;
  timestamp: number | null;
  operation: string;
  adds: number;
  removes: number;
  bytesAdded: number;
  protocol: string | null;
  schemaChanged: boolean;
}

export interface TableState {
  version: number;
  files: Map<string, number>;
  minReader: number | null;
  minWriter: number | null;
}

export function versionOf(key: string): number | null {
  const m = /_delta_log\/(\d{20})\.json$/.exec(key);
  return m ? Number(m[1]) : null;
}

export function parseCommit(version: number, key: string, text: string): { commit: Commit; actions: Record<string, unknown>[] } {
  const actions: Record<string, unknown>[] = [];
  for (const line of text.split("\n")) {
    if (line.trim()) actions.push(JSON.parse(line) as Record<string, unknown>);
  }
  const commit: Commit = { version, key, timestamp: null, operation: "?", adds: 0, removes: 0, bytesAdded: 0, protocol: null, schemaChanged: false };
  for (const a of actions) {
    if (a.add) {
      commit.adds++;
      commit.bytesAdded += Number((a.add as { size?: number }).size ?? 0);
    }
    if (a.remove) commit.removes++;
    if (a.metaData) commit.schemaChanged = true;
    if (a.protocol) {
      const p = a.protocol as { minReaderVersion?: number; minWriterVersion?: number };
      commit.protocol = `reader ${p.minReaderVersion ?? "?"}, writer ${p.minWriterVersion ?? "?"}`;
    }
    if (a.commitInfo) {
      const c = a.commitInfo as { timestamp?: number; operation?: string };
      commit.timestamp = c.timestamp ?? null;
      commit.operation = c.operation ?? "?";
    }
  }
  return { commit, actions };
}

export function replay(commits: { version: number; actions: Record<string, unknown>[] }[], upTo: number): TableState {
  const state: TableState = { version: -1, files: new Map(), minReader: null, minWriter: null };
  for (const c of [...commits].sort((a, b) => a.version - b.version)) {
    if (c.version > upTo) break;
    state.version = c.version;
    for (const a of c.actions) {
      if (a.add) {
        const add = a.add as { path: string; size?: number };
        state.files.set(add.path, Number(add.size ?? 0));
      }
      if (a.remove) state.files.delete((a.remove as { path: string }).path);
      if (a.protocol) {
        const p = a.protocol as { minReaderVersion?: number; minWriterVersion?: number };
        state.minReader = p.minReaderVersion ?? null;
        state.minWriter = p.minWriterVersion ?? null;
      }
    }
  }
  return state;
}
