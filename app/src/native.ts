// Typed access to sonde/native, and error parsing.

import { Listing, Sessions, Sonde } from "sonde/native";
import type * as T from "./types";

export { Listing, Sessions, Sonde };

/** The structured error sonde/native rejects or throws with. */
export function asError(e: unknown): T.SondeError {
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
  version: () => Sonde.version(),
  endpoints: () => Sonde.endpoints() as T.EndpointView[],
  vendors: () => Sonde.vendors() as T.VendorInfo[],
  saveEndpoint: (input: Record<string, unknown>) => Sonde.saveEndpoint(input) as T.EndpointView,
  removeEndpoint: (id: string) => Sonde.removeEndpoint(id),
  importableProfiles: () => Sonde.importableProfiles() as T.EndpointView[],
  importProfiles: (names: string[]) => Sonde.importProfiles(names),
  testEndpoint: async (id: string) => (await Sonde.testEndpoint(id)) as T.ConnectivityReport,
  startDemo: async () => (await Sonde.startDemo()) as T.EndpointView,
  settings: () => Sonde.settings() as T.Settings,
  saveSettings: (s: T.Settings) => Sonde.saveSettings(s),
  setReadOnly: (on: boolean) => Sonde.setReadOnly(on),
  paths: () => Sonde.paths() as Record<string, string>,
  buckets: async (ep: string) => (await Sonde.buckets(ep)) as T.BucketEntry[],
  objectDetail: async (ep: string, b: string, k: string, v?: string | null) => (await Sonde.objectDetail(ep, b, k, v ?? null)) as T.ObjectDetail,
  presign: (ep: string, b: string, k: string, secs: number) => Sonde.presign(ep, b, k, secs, null),
  download: (ep: string, b: string, k: string) => Sonde.download(ep, b, k),
  search: (ep: string, b: string, p: string, needle: string) => Sonde.search(ep, b, p, needle),
  jobs: () => Sonde.jobs() as T.JobInfo[],
  cancelJob: (id: string) => Sonde.cancelJob(id),
  requests: (f: T.RequestFilter, limit: number) => Sonde.requests(f, limit) as T.RecordSummary[],
  latestRequest: () => Sonde.latestRequest(),
  request: (id: number) => Sonde.request(id) as T.RequestRecord | null,
  findRequest: (rid: string) => Sonde.findRequest(rid) as T.RequestRecord | null,
  requestOperations: () => Sonde.requestOperations() as string[],
  copyAsAwsCli: (id: number) => Sonde.copyAsAwsCli(id),
  copyAsCurl: (id: number) => Sonde.copyAsCurl(id),
  exportRequests: (format: "json" | "har", f: T.RequestFilter) => Sonde.exportRequests(format, f),
  clearRequests: () => Sonde.clearRequests(),
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
  probeCatalog: () => Sonde.probeCatalog() as T.ProbeMeta[],
  runProbes: (eps: string[], o: { bucket: string; allowTempBuckets: boolean; includeSlow: boolean; only: string[] }) => Sonde.runProbes(eps, o),
  probeRuns: () => Sonde.probeRuns() as T.RunSummary[],
  probeRun: (id: string, ep: string) => Sonde.probeRun(id, ep) as T.ProbeRun | null,
  probeMatrix: (runs?: string[]) => Sonde.probeMatrix(runs ?? null) as T.Matrix,
  probeDiff: (a: string, b: string) => Sonde.probeDiff(a, b) as T.Change[],
  exportProbes: (format: "markdown" | "json", runs?: string[]) => Sonde.exportProbes(format, runs ?? null),
  detectLayouts: (ep: string, b: string, p: string) => Sonde.detectLayouts(ep, b, p),
  analyzeLayout: (ep: string, b: string, format: string, root: string) => Sonde.analyzeLayout(ep, b, format, root),
  layoutState: (job: string, time: number) => Sonde.layoutState(job, time) as T.Snapshot | null,
  plugins: () => Sonde.plugins() as T.PluginInfo[],
  installPlugin: (path: string) => Sonde.installPlugin(path) as T.PluginInfo,
  enablePlugin: (id: string, on: boolean) => Sonde.enablePlugin(id, on),
  uninstallPlugin: (id: string) => Sonde.uninstallPlugin(id),
  trustRoots: () => Sonde.trustRoots() as T.TrustRoot[],
  addTrustRoot: (label: string, key: string) => Sonde.addTrustRoot(label, key) as T.TrustRoot,
  removeTrustRoot: (id: string) => Sonde.removeTrustRoot(id),
  applyRevocations: (path: string) => Sonde.applyRevocations(path) as { sequence: number; revoked: string[] },
  pluginHostAvailable: () => Sonde.pluginHostAvailable(),
  openPlugin: (id: string, ctx: { endpoint: string; bucket: string; prefix: string }) => Sonde.openPlugin(id, ctx),
  sessions: {
    list: () => Sessions.list() as T.SessionInfo[],
    open: (path: string) => Sessions.open(path) as T.SessionView,
    records: (path: string, ids: number[]) => Sessions.records(path, ids) as T.RecordSummary[],
    record: (path: string, id: number) => Sessions.record(path, id) as T.RequestRecord | null,
    copy: (path: string, id: number, kind: "cli" | "curl") => Sessions.copy(path, id, kind) as string | null,
    start: async (endpoint: string, policy: "allow" | "no-delete" | "read-only") => (await Sessions.start(endpoint, policy)) as T.RecordingView,
    current: () => Sessions.current() as T.RecordingView | null,
    stop: () => Sessions.stop() as string | null,
    example: () => Sessions.example() as string,
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
