// Typed access to sonde/native, and error parsing.

import { Listing, Runs, Sonde } from "sonde/native";
import type * as T from "./types";

export { Listing, Runs, Sonde };

/** The structured error sonde/native rejects or throws with. */
export function asError(e: unknown): T.SondeError {
  // one of ours already (a press that says why: `report({ kind: "config", message })`), not "[object Object]"
  if (e && typeof e === "object" && !(e instanceof Error) && typeof (e as { message?: unknown }).message === "string") return e as T.SondeError;
  const message = e instanceof Error ? e.message : String(e);
  try {
    const v = JSON.parse(message);
    if (v && typeof v === "object" && "message" in v) return v as T.SondeError;
  } catch {
    // not JSON: a runtime error
  }
  return { kind: "runtime", message };
}

export const api = {
  endpoints: () => Sonde.endpoints() as T.EndpointView[],
  vendors: () => Sonde.vendors() as T.VendorInfo[],
  saveEndpoint: (input: Record<string, unknown>) => Sonde.saveEndpoint(input) as T.EndpointView,
  removeEndpoint: (id: string) => Sonde.removeEndpoint(id),
  importableProfiles: () => Sonde.importableProfiles() as T.EndpointView[],
  importProfiles: (names: string[]) => Sonde.importProfiles(names),
  testEndpoint: async (id: string) => (await Sonde.testEndpoint(id)) as T.ConnectivityReport,
  startDemo: async () => (await Sonde.startDemo()) as T.EndpointView,
  /** Why config.toml did not load, or null: while it stands there are no endpoints, default settings, and no save. */
  configError: () => (Sonde as unknown as { configError(): string | null }).configError(),
  /** Reads config.toml again; throws with why it still does not load. */
  reloadConfig: () => (Sonde as unknown as { reloadConfig(): void }).reloadConfig(),
  settings: () => Sonde.settings() as T.Settings,
  saveSettings: (s: T.Settings) => Sonde.saveSettings(s),
  paths: () => Sonde.paths() as Record<string, string>,
  buckets: async (ep: string) => (await Sonde.buckets(ep)) as T.BucketEntry[],
  objectDetail: async (ep: string, b: string, k: string, v?: string | null) => (await Sonde.objectDetail(ep, b, k, v ?? null)) as T.ObjectDetail,
  presign: (ep: string, b: string, k: string, secs: number) => Sonde.presign(ep, b, k, secs, null),
  download: (ep: string, b: string, k: string) => Sonde.download(ep, b, k),
  search: (ep: string, b: string, p: string, needle: string) => Sonde.search(ep, b, p, needle),
  jobs: () => Sonde.jobs() as T.JobInfo[],
  cancelJob: (id: string) => Sonde.cancelJob(id),
  /** Follows the engine instead of asking it on a timer: `request` with the
   * newest record's id as records come, `jobs` with the number running as
   * jobs change; each at once with the value now. Stops with the view that
   * called it, or with the function it returns. */
  on: ((event: string, listener: (v: number) => void) => Sonde.on(event, (v) => listener(v as number))) as (event: "jobs", listener: (v: number) => void) => () => void,
  mpuScan: (ep: string, b: string, p: string) => Sonde.mpuScan(ep, b, p),
  planAbort: (ep: string, b: string, rows: T.UploadRow[]) => Sonde.planAbort(ep, b, rows) as T.Plan,
  planLifecycle: async (ep: string, b: string, p: string, days: number) => (await Sonde.planLifecycle(ep, b, p, days)) as T.Plan,
  versioning: async (ep: string, b: string) => (await Sonde.versioning(ep, b)) as T.VersioningStatus,
  timeline: async (ep: string, b: string, k: string) => (await Sonde.timeline(ep, b, k)) as T.Timeline,
  planRestore: async (ep: string, b: string, k: string) => (await Sonde.planRestore(ep, b, k)) as T.Plan,
  planDeleteVersions: (ep: string, b: string, targets: { key: string; versionId: string; size: number | null }[]) => Sonde.planDeleteVersions(ep, b, targets) as T.Plan,
  executePlan: async (plan: string, confirmation: string) => (await Sonde.executePlan(plan, confirmation)) as T.Outcome,
  discardPlan: (plan: string) => Sonde.discardPlan(plan),
  auditLog: (n: number) => Sonde.auditLog(n) as T.AuditEntry[],
  duScan: (ep: string, b: string, p: string, mpu: boolean) => Sonde.duScan(ep, b, p, mpu),
  duCached: (ep: string, b: string, p: string) => Sonde.duCached(ep, b, p) as T.DuReport | null,
  duExport: (ep: string, b: string, p: string, format: "json" | "csv") => Sonde.duExport(ep, b, p, format),
  /** A chart's rows as CSV, written into the exports folder; the file's path. */
  detectLayouts: (ep: string, b: string, p: string) => Sonde.detectLayouts(ep, b, p),
  analyzeLayout: (ep: string, b: string, format: string, root: string) => Sonde.analyzeLayout(ep, b, format, root),
  layoutState: (job: string, time: number) => Sonde.layoutState(job, time) as T.Snapshot | null,
  runs: {
    list: () => Runs.list() as T.RunInfo[],
    /** The programs run here, newest first, with their runs' trend. */
    programs: async () => (await Runs.programs()) as T.ProgramInfo[],
    /** A run as a window opens it: its totals, signals, profile and findings, and the first page of its operations. Read off the interface's thread. */
    view: async (path: string) => (await Runs.view(path)) as T.RunView,
    /** The requests that started in a stretch of it (ms from its first request's start), reckoned as a run of their own: what the Time pane's charts not drawn over time read while a stretch is chosen. Off the interface's thread. */
    stretch: async (path: string, fromMs: number, toMs: number) => (await Runs.stretch(path, fromMs, toMs)) as T.Stretch,
    /** A range of its operations, sorted and narrowed as the table asks. */
    episodes: async (path: string, query: T.EpisodeQuery) => (await Runs.episodes(path, query)) as T.EpisodePage,
    /** One operation by its index, with every one of its requests' ids. */
    episode: (path: string, index: number) => Runs.episode(path, index) as T.EpisodeRow | null,
    /** The operation a request belongs to. */
    episodeOf: (path: string, recordId: number) => Runs.episodeOf(path, recordId),
    /** The row the table shows an operation at under a query; -1 where the query leaves it out. */
    episodeRow: (path: string, query: T.EpisodeQuery, index: number) => Runs.episodeRow(path, query, index),
    /** The operations running at a moment (at most 200), else the nearest one. */
    episodesAt: (path: string, time: string) => Runs.episodesAt(path, time) as number[],
    /** A range of the operations a finding is about (a view lists its first 200). */
    observationEpisodes: (path: string, observation: number, offset: number, limit: number) => Runs.observationEpisodes(path, observation, offset, limit) as number[],
    records: (path: string, ids: number[]) => Runs.records(path, ids) as T.RecordSummary[],
    /** The operations table split by a dimension (Group by), with its sentence. Read off the interface's thread. */
    split: async (path: string, by: T.SplitBy) => (await Runs.split(path, by)) as T.Split,
    /** Every request of one key, whatever its operation, in time order (the inspector's history). */
    keyHistory: async (path: string, bucket: string, key: string) => (await Runs.keyHistory(path, bucket, key)) as T.KeyRequest[],
    /** One request in full. Off the interface's thread (a large run's is read from its file). */
    record: async (path: string, id: number) => (await Runs.record(path, id)) as T.RequestRecord | null,
    copy: (path: string, id: number, kind: "cli" | "curl") => Runs.copy(path, id, kind) as string | null,
    start: async (endpoint: string, policy: "allow" | "no-delete" | "read-only") => (await Runs.start(endpoint, policy)) as T.RecordingView,
    current: () => Runs.current() as T.RecordingView | null,
    /** Stops the recording begun with no program: its run's path once the file is whole. */
    stop: async () => (await Runs.stop()) as string | null,
    example: () => Runs.example() as string,
    /** The budget the run's program has in its project's sonde.toml, for the charts' lines; null without one. */
    budget: (path: string) => Runs.budget(path) as T.ProjectBudget | null,
    import: (name: string, text: string) => Runs.import(name, text) as string,
    /** A run file or a log of traffic at a path added to the runs (one of them already: opened as it is). Off the interface's thread. */
    importPath: (path: string) => Runs.importPath(path) as Promise<string>,
    pick: (runsKind: string, trafficKind: string) => Runs.pick(runsKind, trafficKind) as Promise<string | null>,
    pickFolder: () => Runs.pickFolder() as Promise<string | null>,
    /** The demo's job, written afresh into its folder: the command that plays it on the demo store, and that folder. */
    demoJob: () => Runs.demoJob() as { command: string; cwd: string },
    /** The demo job's finding fixed in its file, as a person would change the code. */
    demoFix: () => Runs.demoFix() as boolean,
    /** The commands a folder likely runs, the likeliest first (none when nothing is found). */
    suggest: async (cwd: string) => (await Runs.suggest(cwd)) as T.SuggestedCommand[],
    openCode: (run: string, place: string) => Runs.openCode(run, place) as Promise<void>,
    rename: (path: string, title: string) => Runs.rename(path, title) as boolean,
    /** To the system's trash (deleted where there is none). */
    remove: (path: string) => Runs.remove(path) as Promise<boolean>,
    /** Shows a file or folder in the file manager. */
    reveal: (path: string) => Runs.reveal(path) as Promise<void>,
    share: async (path: string) => (await Runs.share(path)) as { file: string },
    openPage: (path: string) => Runs.openPage(path) as Promise<void>,
    run: (command: string, cwd: string | null, endpoint: string, policy: "allow" | "no-delete" | "read-only", cols: number, rows: number, onEvent: (e: T.RunEvent) => void) =>
      Runs.run(command, cwd, endpoint, policy, cols, rows, (e) => onEvent(e as T.RunEvent)) as Promise<T.RunResult>,
    input: (data: string) => Runs.input(data),
    resize: (cols: number, rows: number) => Runs.resize(cols, rows),
    kill: () => Runs.kill(),
    /** The app is quitting: the program being recorded and a running trial are ended, and the recording is written as a finished run, within the 200 ms a quitting app has. */
    quit: async () => (await Runs.quit()) as { program: boolean; trial: boolean; run: string | null },
    /** What trying a run's program under faults would run, said before it runs; throws with why there is no trial. */
    trialPlan: (path: string) => Runs.trialPlan(path) as T.TrialPlan,
    /** The run's command again, once per scenario (`slowdown:3`: its first three attempts at each request), sonde answering in the store's place; `kill` cancels (and nothing is kept). */
    trial: (path: string, scenarios: string[], policy: "allow" | "no-delete" | "read-only", cols: number, rows: number, onEvent: (e: T.TrialEvent) => void) =>
      Runs.trial(path, scenarios, policy, cols, rows, (e) => onEvent(e as T.TrialEvent)) as Promise<T.TrialResult>,
    /** Its output as timed lines. Off the interface's thread (a run not yet read is read whole). */
    logLines: async (path: string) => (await Runs.logLines(path)) as T.LogLine[],
    replayPlan: (path: string, endpoint: string) => Runs.replayPlan(path, endpoint),
    replay: (path: string, endpoint: string, prefix: string) => Runs.replay(path, endpoint, prefix),
    compare: async (a: string, b: string) => await Runs.compare(a, b),
    history: async (path: string) => (await Runs.history(path)) as T.ProgramHistory | null,
    setBaseline: (path: string, on: boolean) => Runs.setBaseline(path, on) as boolean,
    compareRuns: async (before: string, after: string) => (await Runs.compareRuns(before, after)) as T.RunChange,
    /** A run against its program's run before it: its findings with their state and measured savings; null for no run of a program. */
    sinceLast: async (path: string) => (await Runs.sinceLast(path)) as T.SinceLast | null,
    codeChange: async (before: string, after: string) => (await Runs.codeChange(before, after)) as T.CodeChange,
  },
};

/**
 * Follows a job until it ends: `onEvent` gets progress and partial results.
 * Cancels it when the calling component goes (call from a component body or
 * handler: the returned `cancel` also stops it).
 */
export function watch<R>(id: string, onEvent?: (e: T.JobEvent) => void): Promise<R> {
  return Sonde.watchJob(id, (e) => onEvent?.(e as T.JobEvent)) as Promise<R>;
}
