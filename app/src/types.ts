// The shapes sonde/native returns (serde JSON of sonde-core, probes and decoders).

export interface EndpointView {
  id: string;
  name: string;
  url: string;
  region: string;
  pathStyle: boolean;
  vendor: string;
  vendorLabel: string;
  hasCa: boolean;
  caFile?: string | null;
  credentials: string;
  buckets: string[];
}

export interface VendorInfo {
  id: string;
  label: string;
  urlTemplate: string;
  region: string;
  pathStyle: boolean;
  stsIdentity: boolean;
}

export interface Prices {
  "default-per-gib-month": number;
  "by-class": Record<string, number>;
  currency: string;
}

export interface Settings {
  "read-only": boolean;
  concurrency: number;
  "rate-limit": number | null;
  prices: Prices;
  "reqlog-capacity": number;
  "reqlog-persist": boolean;
  theme: string;
  "high-contrast"?: boolean;
  language: string;
  "stale-upload-days": number;
  /** Runs name the place in a Python program's code that sent each request. */
  "name-callers"?: boolean;
  "exports-dir": string | null;
  "downloads-dir": string | null;
}

export interface SondeError {
  sonde?: boolean;
  kind: string;
  message: string;
  code?: string | null;
  status?: number | null;
  requestId?: string | null;
  recordId?: number | null;
}

export interface ConnectivityReport {
  endpoint: string;
  ok: boolean;
  latencyMs: number;
  credentialsSource: string | null;
  bucketCount: number | null;
  owner: string | null;
  identity: { arn?: string; userId?: string; account?: string } | null;
  identityNote: string | null;
  error: SondeError | null;
  server: string | null;
}

export interface BucketEntry {
  name: string;
  created: string | null;
  region: string | null;
}

export interface Entry {
  key: string;
  name: string;
  isPrefix: boolean;
  size: number;
  lastModified: string | null;
  etag: string | null;
  storageClass: string | null;
  parts: number | null;
}

export interface EtagInfo {
  value: string;
  weak: boolean;
  kind: { kind: "md5"; md5: string } | { kind: "multipart"; digest: string; parts: number } | { kind: "opaque" };
}

export interface StoredChecksum {
  algorithm: string;
  value: string;
  kind: "fullObject" | "composite";
  parts: number | null;
  kindFromHeader: boolean;
}

export interface ObjectDetail {
  bucket: string;
  key: string;
  size: number | null;
  contentType: string | null;
  lastModified: string | null;
  versionId: string | null;
  storageClass: string;
  etag: EtagInfo | null;
  partSizeGuesses: number[];
  checksums: StoredChecksum[];
  sse: { algorithm?: string | null; kmsKeyId?: string | null; bucketKeyEnabled?: boolean | null; customerAlgorithm?: string | null; customerKeyMd5?: string | null };
  metadata: [string, string][];
  headers: [string, string][];
  restore: string | null;
  objectLock: string | null;
  recordId: number;
}

export interface Progress {
  done: number;
  total: number | null;
  bytes: number;
  message: string;
}

export interface JobInfo {
  id: string;
  kind: string;
  title: string;
  state: "running" | "finished" | "failed" | "cancelled";
  progress: Progress;
  started: string;
  ended: string | null;
  concurrency: number;
  rateLimit: number | null;
  error: SondeError | null;
}

export type JobEvent =
  | { type: "Progress"; job: string; progress: Progress }
  | { type: "Partial"; job: string; data: unknown };

export interface Header {
  name: string;
  value: string;
}

export interface RecordSummary {
  id: number;
  time: string;
  endpoint: string;
  operation: string;
  method: string;
  target: string;
  status: number | null;
  latencyMs: number;
  retries: number;
  requestId: string | null;
  /** Time to the response's first byte, when measured (the proxy, an access log's turnaround). */
  firstByteMs?: number;
}

export interface RequestRecord extends RecordSummary {
  /** Who sent it, as the store's log names them (an imported log only). */
  requester?: string;
  url: string;
  bucket: string | null;
  key: string | null;
  query: [string, string][];
  hostId: string | null;
  requestHeaders: Header[];
  responseHeaders: Header[];
  requestBytes: number | null;
  responseBytes: number | null;
  errorBody: string | null;
  /** A control request's document (DeleteObjects, a lifecycle rule), as sent. */
  requestBody?: string | null;
  requestBodyTruncated?: boolean;
  transportError: string | null;
  region: string | null;
  job: string | null;
}

export interface RequestFilter {
  status?: { kind: "exact" | "class"; value: number } | { kind: "errors" } | { kind: "transport" } | null;
  operation?: string | null;
  endpoint?: string | null;
  text?: string | null;
  retried?: boolean;
}

export interface UploadRow {
  key: string;
  uploadId: string;
  initiated: string | null;
  ageDays: number | null;
  parts: number;
  bytes: number;
  storageClass: string | null;
  costSoFar: number;
  costPerMonth: number;
  stale: boolean;
  partsError: string | null;
}

export interface MpuReport {
  endpoint: string;
  bucket: string;
  prefix: string;
  thresholdDays: number;
  uploads: UploadRow[];
  totalUploads: number;
  staleUploads: number;
  totalBytes: number;
  staleBytes: number;
  costPerMonth: number;
  costSoFar: number;
  currency: string;
  lifecycle: {
    status: "covered" | "partial" | "missing" | "unknown";
    rules: { id: string | null; prefix: string | null; days: number; enabled: boolean }[];
    message: string;
    suggestedRule: string;
    error: SondeError | null;
  };
  scannedAt: string;
}

export interface PlanItem {
  key: string;
  id: string | null;
  bytes: number | null;
  detail: string;
}

export interface Plan {
  id: string;
  action: "abortUploads" | "deleteVersions" | "restoreObject" | "deleteObjects" | "putLifecycle";
  endpoint: string;
  bucket: string;
  items: PlanItem[];
  document: string | null;
  previousDocument: string | null;
  summary: string;
  confirm: string;
  confirmHint: string;
  created: string;
}

export interface Outcome {
  succeeded: number;
  failed: number;
  errors: SondeError[];
}

export interface VersionEntry {
  versionId: string;
  isLatest: boolean;
  isDeleteMarker: boolean;
  lastModified: string | null;
  size: number;
  etag: string | null;
  storageClass: string | null;
}

export interface Timeline {
  bucket: string;
  key: string;
  versions: VersionEntry[];
  deleted: boolean;
  noncurrentBytes: number;
}

export interface VersioningStatus {
  status: string;
  mfaDelete: string | null;
  objectLock: string | null;
}

export interface Agg {
  objects: number;
  bytes: number;
}

export interface Slice {
  name: string;
  current: Agg;
  noncurrent: Agg;
  deleteMarkers: number;
}

export interface DuReport {
  endpoint: string;
  bucket: string;
  prefix: string;
  source: string;
  current: Agg;
  noncurrent: Agg;
  deleteMarkers: number;
  multipart: Agg;
  totalBytes: number;
  byClass: Slice[];
  byPrefix: Slice[];
  bySize: { label: string; min: number; max: number | null; objects: number; bytes: number }[];
  findings: string[];
  costPerMonth: number;
  currency: string;
  scannedAt: string;
  requests: number;
}

export type Verdict = "pass" | "fail" | "differs" | "unsupported" | "error" | "skipped";

export interface ProbeMeta {
  id: string;
  category: string;
  title: string;
  description: string;
  awsBehavior: string;
  needs: string[];
}

export interface Step {
  label: string;
  status: number | null;
  code: string | null;
  headers: [string, string][];
  body: string;
  recordId: number | null;
}

export interface ProbeResult {
  id: string;
  category: string;
  verdict: Verdict;
  summary: string;
  observations: string[];
  steps: Step[];
  exchanges: RequestRecord[];
  durationMs: number;
}

export interface ProbeRun {
  id: string;
  endpoint: string;
  endpointName: string;
  vendor: string;
  server: string | null;
  started: string;
  finished: string;
  sondeVersion: string;
  results: ProbeResult[];
}

export interface RunSummary {
  id: string;
  endpoint: string;
  endpointName: string;
  vendor: string;
  started: string;
  pass: number;
  fail: number;
  differs: number;
  unsupported: number;
  other: number;
}

export interface Matrix {
  columns: { endpoint: string; name: string; vendor: string; server: string | null; run: string; started: string }[];
  rows: { probe: ProbeMeta; cells: ({ verdict: Verdict; summary: string; run: string } | null)[] }[];
}

export interface Change {
  probe: string;
  before: Verdict | null;
  after: Verdict | null;
  beforeSummary: string | null;
  afterSummary: string | null;
  stepsChanged: boolean;
}

export interface Detection {
  format: string;
  root: string;
  evidence: string;
}

export interface Fact {
  label: string;
  value: string;
}

export interface Table {
  title: string;
  columns: string[];
  rows: string[][];
}

export interface Snapshot {
  time: string | null;
  label: string;
  facts: Fact[];
  tables: Table[];
}

export interface Layout {
  format: string;
  root: string;
  formatVersion: string | null;
  decoded: boolean;
  headline: string;
  facts: Fact[];
  issues: { severity: "info" | "warning" | "error"; message: string }[];
  timeline: { time: string | null; kind: string; label: string; detail: string; key: string | null; bytes: number | null }[];
  tables: Table[];
  snapshots: Snapshot[];
  objects: number;
  bytes: number;
}

export interface AuditEntry {
  time: string;
  /** A plan's id; a probe run's id for `runProbes`. */
  plan: string;
  /** `abortUploads`, `deleteVersions`, `restoreObject`, `deleteObjects`, `putLifecycle`, `runProbes` (items = its probes). */
  action: string;
  /** Written as the work starts, before its outcome is known. */
  stage?: "started";
  endpoint: string;
  bucket: string;
  items: number;
  succeeded: number;
  failed: number;
  errors: string[];
  requestIds: string[];
  targets: string[];
}

// --- sessions (sonde run, sonde proxy, the Timeline) ---------------------

export interface SessionInfo {
  path: string;
  name: string;
  created: string;
  endpoint: string;
  command: string | null;
  requests: number;
  live: boolean;
  /** A title the user gave it. */
  title?: string | null;
  /** A finished session's requests over its time, in slices: the list's sparkline. */
  shape?: number[];
}

/** A phrase of what sonde says: a key and its arguments (crates/proxy/src/phrase.rs). */
export interface Phrase {
  key: string;
  args: Record<string, string>;
}

export interface Episode {
  kind: "request" | "multipart" | "retry" | "probe" | "listing" | "step";
  format: "SlateDB" | "Iceberg" | "Delta" | "Lance" | null;
  title: string;
  start: string;
  end: string;
  records: number[];
  bytesUp: number;
  bytesDown: number;
  outcome: { kind: "ok" } | { kind: "failed"; detail: string } | { kind: "refused" };
  notes: string[];
  /** Its lookups answered 404. */
  notFound?: number;
  /** The title and the notes as phrases. */
  titleText?: Phrase[];
  notesText?: Phrase[];
}

export interface Observation {
  level: "warn" | "info";
  text: string;
  phrases?: Phrase[];
  /** The episodes it is about, by index. */
  episodes: number[];
  /** The requests it bears on (findings come ranked by it, warnings first). */
  impact?: number;
  /** What acting on it would save, when that can be counted. */
  saving?: Phrase;
  savingText?: string;
  /** The saving, counted. */
  savesRequests?: number;
  savesMs?: number;
  /** What to change in the program, by SDK. */
  fixes?: { sdk: "python" | "java" | "go" | "js" | "config"; code: string }[];
  /** Where in the program its requests came from (`x-sonde-caller`), most first. */
  callers?: { place: string; requests: number }[];
  /** The SDK that sent them, from their user agent ("boto3 1.34"). */
  sdk?: string;
  /** The same finding across runs: what sonde.toml's `expect` names. */
  identity?: string;
}

/** A session after acting on every finding that counts a saving: an estimate. */
export interface WhatIf {
  requests: [number, number];
  waitingMs: [number, number];
  cost: [number, number];
  currency: string;
  findings: number;
}

export interface SessionSummary {
  requests: number;
  errors: number;
  misses: number;
  refused: number;
  seconds: number;
  bytesUp: number;
  bytesDown: number;
}

/** One kind of request: an operation, its lookups that found nothing apart (crates/proxy/src/profile.rs). */
export interface ProfileGroup {
  operation: string;
  missed: boolean;
  requests: number;
  busyMs: number;
  sumMs: number;
  /** The program's waiting that was theirs (each moment split among the requests then in flight). */
  waitedMs: number;
  share: number;
  p50Ms: number;
  p95Ms: number;
  maxMs: number;
  errors: number;
  bytesUp: number;
  bytesDown: number;
  cost: number;
}

/** Where a session's time went, its rate and concurrency, its requests' cost. */
export interface Profile {
  wallMs: number;
  busyMs: number;
  peakInFlight: number;
  peakAtMs: number;
  meanInFlight: number;
  groups: ProfileGroup[];
  binMs: number;
  rate: { tMs: number; ok: number; miss: number; client: number; server: number; p50Ms: number; p95Ms: number; p99Ms: number; throttled?: number; retried?: number; bytesUp?: number; bytesDown?: number }[];
  inFlight: { tMs: number; n: number }[];
  samples: { id: number; group: number; ms: number; tMs: number; status?: number | null; bytes?: number }[];
  /** The busiest kinds' latency, slice by slice. */
  groupRate?: { group: number; bin: number; n: number; p50Ms: number; p95Ms: number }[];
  /** The places in the program's code that sent requests (x-sonde-caller). */
  callers?: { place: string; requests: number; waitedMs: number; errors: number; cost: number }[];
  /** Whether the requests' times are known (a CloudTrail file has none); absent in older shares. */
  timed?: boolean;
  cost: { writes: number; reads: number; free: number; amount: number; currency: string };
  prices: { per1000Writes: number; per1000Reads: number; currency: string };
  prefixes: { prefix: string; /** Folders under it shown with it (absent: this folder alone). */ folders?: number; requests: number; reads: number; writes: number; errors: number; peakPerSecond: number; peakReads: number; peakWrites: number; cost?: number }[];
  heat: { prefix: number; bin: number; n: number }[];
  flows: { source: string; target: string; value: number }[];
  /** Requests that each waited for the one before, longest first. */
  chains?: { startMs: number; endMs: number; requests: number; ids: number[]; atOnceMs: number }[];
  /** How long requests took, every one: a count per bucket up to `leMs` (null: slower than the last). */
  latency?: { leMs: number | null; n: number }[];
  p50Ms?: number;
  p95Ms?: number;
  p99Ms?: number;
}

/** A kind of request on two stores: the same workload's latency on each. */
export interface LatencyChange {
  operation: string;
  missed: boolean;
  requests: [number, number];
  p50Ms: [number, number];
  p95Ms: [number, number];
  busyMs: [number, number];
}

/** A run of a program, by the figures that move when the program changes. */
export interface RunPoint {
  path: string;
  created: string;
  requests: number;
  busyMs: number;
  wallMs: number;
  cost: number;
  currency: string;
  errors: number;
  warnings: number;
  findings: number;
}

export interface BudgetVerdict {
  what: "requests" | "waiting" | "cost" | "errors" | "warnings" | "p95";
  value: number;
  limit: number;
  against: "budget" | "baseline";
  ok: boolean;
}

/** The commit a run's code was at. */
export interface GitStamp {
  commit: string;
  branch?: string | null;
  dirty: boolean;
}

/** What a run may use against its baseline before `sonde check` fails it. */
export interface BaselineLimits {
  requests: number;
  waitingMs: number;
  cost: number;
  errors: number;
}

/** Where a program went past its baseline: the run, and the commits on either side. */
export interface Since {
  runsAgo: number;
  firstPast: string;
  firstPastCommit?: GitStamp;
  lastWithin?: string;
  lastWithinCommit?: GitStamp;
}

/** What changed in a program's code between two runs, file by file, with the later run's findings on their lines. */
export interface CodeChange {
  from: string;
  to: string;
  files: { path: string; old: string; new: string; notes: { line: number; text: string }[] }[];
  more: number;
}

/** A program over its runs, its baseline and this run checked against it. */
export interface ProgramHistory {
  program: string;
  runs: RunPoint[];
  /** Each run's commit, as `runs`. */
  commits?: (GitStamp | null)[];
  since?: Since | null;
  baseline: string | null;
  baselinePoint?: RunPoint | null;
  limits?: BaselineLimits | null;
  check: { passed: boolean; verdicts: BudgetVerdict[]; newWarnings: string[]; expected?: string[] } | null;
  /** The check as a pull request's comment (`sonde check -f markdown`). */
  markdown?: string | null;
}

/** Two runs of a program compared, before and after. */
export interface RunChange {
  requests: [number, number];
  wallMs: [number, number];
  busyMs: [number, number];
  errors: [number, number];
  cost: [number, number];
  currency: string;
  peakInFlight: [number, number];
  groups: { operation: string; missed: boolean; requests: [number, number]; busyMs: [number, number] }[];
  resolved: { text: string; phrases: Phrase[] }[];
  appeared: { text: string; phrases: Phrase[] }[];
  /** Both are runs of one program; across programs the findings gone and new are left out. */
  sameProgram: boolean;
  /** Both runs' latency on the same bucket edges. */
  latency?: [{ leMs: number | null; n: number }[], { leMs: number | null; n: number }[]];
  /** The busiest prefixes of either run, each run's requests to them. */
  prefixes?: { prefix: string; requests: [number, number] }[];
}

export interface SessionView {
  path: string;
  header: { endpoint: string; url: string; region: string; created: string; command?: string | null; cwd?: string | null; git?: GitStamp | null };
  summary: SessionSummary;
  profile: Profile;
  whatIf?: WhatIf | null;
  episodes: Episode[];
  observations: Observation[];
  sharedNotes: string[];
}

export interface RecordingView {
  url: string;
  path: string;
  endpoint: string;
  requests: number;
  exports: string;
}

export interface LogLine {
  time: string;
  text: string;
}

export type RunEvent = { started: RecordingView } | { data: string };

export interface RunResult {
  exit: number | null;
  path: string;
}
