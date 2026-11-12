// The shapes sonde/native returns (serde JSON of sonde-core, probes and decoders).

import type { CorePhrase } from "./corephrases";

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
  credentialsText?: CorePhrase[];
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
  /** sonde's own jobs: in config.toml only. */
  concurrency: number;
  "rate-limit": number | null;
  prices: Prices;
  theme: string;
  "high-contrast"?: boolean;
  language: string;
  /** Runs name the place in a Python program's code that sent each request. */
  "name-callers"?: boolean;
  /** What a program recorded by Run may do to the store (no-delete by default). */
  "record-policy"?: "allow" | "no-delete" | "read-only";
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
  messageText?: CorePhrase[];
}

export interface JobInfo {
  id: string;
  kind: string;
  title: string;
  titleText?: CorePhrase[];
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
  /** The fault scenario that answered it, not the store. */
  injected?: string;
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
    messageText?: CorePhrase[];
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
  detailText?: CorePhrase[];
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
  summaryText?: CorePhrase[];
  confirm: string;
  confirmHint: string;
  confirmHintText?: CorePhrase[];
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
  /** Its old versions could not be counted: unknown, not 0. */
  versionsUnknown?: boolean;
}

export interface DuReport {
  /** Why old versions were not counted (the key may not list versions); their figures are then unknown, not 0. */
  versionsUnknown?: string | null;
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
  /** Each finding as phrases, as `findings`. */
  findingsText?: CorePhrase[][];
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
  titleText?: CorePhrase[];
  description: string;
  descriptionText?: CorePhrase[];
  awsBehavior: string;
  awsBehaviorText?: CorePhrase[];
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
  summaryText?: CorePhrase[];
  observations: string[];
  observationsText?: CorePhrase[][];
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
  rows: { probe: ProbeMeta; cells: ({ verdict: Verdict; summary: string; summaryText?: CorePhrase[]; run: string } | null)[] }[];
}

export interface Change {
  probe: string;
  before: Verdict | null;
  after: Verdict | null;
  beforeSummary: string | null;
  afterSummary: string | null;
  beforeSummaryText?: CorePhrase[];
  afterSummaryText?: CorePhrase[];
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
  labelText?: CorePhrase[];
  valueText?: CorePhrase[];
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
  headlineText?: CorePhrase[];
  facts: Fact[];
  issues: { severity: "info" | "warning" | "error"; message: string; messageText?: CorePhrase[] }[];
  timeline: { time: string | null; kind: string; label: string; detail: string; labelText?: CorePhrase[]; detailText?: CorePhrase[]; key: string | null; bytes: number | null }[];
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
  /** The run's trials, when it was tried under faults: how the program stood in each. */
  trials?: { scenario: string; standing: Trial["standing"]; session: string | null }[];
}

/** A phrase of what sonde says: a key and its arguments (crates/proxy/src/phrase.rs). */
export interface Phrase {
  key: string;
  args: Record<string, string>;
}

/** What Group by splits the operations table by (crates/proxy/src/split.rs). */
export type SplitBy = "operation" | "prefix" | "size" | "caller" | "sdk" | "status";

/** One group of a split: the operations table's figures for it. */
export interface SplitRow {
  key: string;
  /** The groups past the table's rows, folded into one. */
  other?: boolean;
  requests: number;
  errors: number;
  errorShare: number;
  p50Ms: number;
  p95Ms: number;
  p99Ms: number;
  waitedMs: number;
  /** Of the session's waiting (the rows add up to 1). */
  share: number;
  cost: number;
  costShare: number;
  /** Of them, slow (at or over `slowMs`). */
  slow: number;
}

/** A session split by one dimension, and where its slow and costly requests are. */
export interface Split {
  by: SplitBy;
  sdk: string | null;
  slowMs: number;
  slow: number;
  cost: number;
  currency: string;
  rows: SplitRow[];
  /** The sentence in English; `phrases` say it in the reader's language. */
  text: string;
  phrases: Phrase[];
}

/** A request of one key, with the operation it belongs to. */
export interface KeyRequest extends RecordSummary {
  operationIndex: number | null;
}

export interface Episode {
  kind: "request" | "multipart" | "retry" | "probe" | "listing" | "step";
  format: "SlateDB" | "Iceberg" | "Delta" | "Lance" | "Hudi" | "Paimon" | null;
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

/** An operation as a table and the inspector read it (crates/proxy/src/view.rs): an `Episode` by its
 * index, with how many requests it has and the first of their ids. A view carries a page of these;
 * the rest are asked for in ranges. */
export interface EpisodeRow extends Omit<Episode, "title"> {
  /** Its index among the session's operations in time order: what findings and events name. It does not change with sorting or filtering. */
  index: number;
  /** Rust's English, only where the phrases do not say it. */
  title?: string;
  /** How many requests it has. */
  requests: number;
  /** Its requests' ids: at most the first 64 in a page (`Sessions.episode` has them all). */
  records: number[];
  /** A lookup that found nothing. */
  miss?: true;
}

/** How the operations' table asks for its rows: sorted and narrowed in Rust. */
export interface EpisodeQuery {
  offset?: number;
  limit?: number;
  sort?: "time" | "duration" | "requests";
  descending?: boolean;
  /** Only the operations of this finding. */
  finding?: number;
  /** Only the operations running in this window, in ms from the session's first operation's start. */
  slice?: [number, number];
  text?: string;
  failed?: boolean;
}

/** A range of a session's operations: `rows[i]` is the table's row `offset + i`, of `total` that match. */
export interface EpisodePage {
  total: number;
  offset: number;
  rows: EpisodeRow[];
}

export interface Observation {
  level: "warn" | "info";
  text: string;
  phrases?: Phrase[];
  /** The episodes it is about, by index: in a view, the first 200 of `episodesTotal`. */
  episodes: number[];
  /** How many operations it is about. */
  episodesTotal?: number;
  /** Its first operation's start (where it stands on the session's shape); absent when it names none. */
  firstStart?: string;
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
  /** The run by operation, busiest first (absent before 16.0). */
  operations?: { operation: string; count: number; errors: number; misses: number; errorShare: number; bytesUp: number; bytesDown: number; p50Ms: number; p95Ms: number; p99Ms: number; maxMs: number; few?: true }[];
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

/** What a project's `sonde.toml` `[check]` allows its program, drawn on the charts it is about (null: not given). */
export interface ProjectBudget {
  maxP95Ms: number | null;
  maxErrors: number | null;
  maxWaitingMs: number | null;
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
/** What one fault scenario did to a program (`sonde run --trial`). */
export interface Trial {
  scenario: string;
  about: string;
  standing: "survived" | "degraded" | "broke" | "untested" | "refused";
  exit: number;
  cleanExit: number;
  faults: number;
  retried: number;
  droppedReads: number;
  droppedWrites: number;
  /** Writes the store did and the program never heard of. */
  lostWrites?: number;
  /** Faults the program was right not to retry. */
  notRetryable?: number;
  /** Requests the scenario held back. */
  delayed?: number;
  attempts: number;
  /** The waits before a retry: shortest, median, longest (ms). */
  backoffMs?: [number, number, number];
  /** Whether the waits differ within one attempt; absent: not said (too few retries to tell). */
  jitter?: boolean;
  requests: number;
  cleanRequests: number;
  wallMs?: number;
  cleanWallMs?: number;
  leftUploads: number;
  wroteTwice: number;
  /** The attempts in a row it failed at each request, when asked to press harder than once (`slowdown:3`). */
  times?: number | null;
  reasons: string[];
  /** The same reasons as phrases, each a sentence of one or more (`trial.why.*`); absent in a verdict kept before 16.0. */
  reasonsText?: { key: string; args?: Record<string, string> }[][];
  /** The request the verdict turns on, when it is not "survived": its operation, `bucket/key`, and the program's line when the run named callers. */
  where?: { operation: string; key: string; caller?: string } | null;
  /** What to change, as a phrase (`trial.advice.*`: the app has its words in both languages). */
  advice?: { key: string; args?: Record<string, string> } | null;
  /** The trial's own session (an absolute path), when it is still there. */
  session?: string | null;
  /** The scenario's verdicts for the program, oldest first, up to this one (`at` RFC 3339; `run` a session's file name). */
  history?: { at: string; standing: Trial["standing"]; run: string }[];
}

export interface ProgramHistory {
  program: string;
  runs: RunPoint[];
  /** The run under fault scenarios, when it was tried. */
  trials?: Trial[];
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
  /** Findings gone at any threshold (fixed). */
  resolved: { text: string; phrases: Phrase[] }[];
  appeared: { text: string; phrases: Phrase[] }[];
  /** Each finding of either run, where it stands. */
  findings: FindingChange[];
  /** Both are runs of one program; across programs the findings gone and new are left out. */
  sameProgram: boolean;
  /** Both runs' latency on the same bucket edges. */
  latency?: [{ leMs: number | null; n: number }[], { leMs: number | null; n: number }[]];
  /** The busiest prefixes of either run, each run's requests to them. */
  prefixes?: { prefix: string; requests: [number, number] }[];
}

/** Requests not sent and time not waited. */
export interface Saving {
  requests: number;
  ms: number;
}

/** A finding between two runs of a program: fixed (gone at any threshold), less (under its threshold), back (fixed before, here again), new or open. */
export interface FindingChange {
  identity: string;
  state: "fixed" | "less" | "back" | "new" | "open";
  level: "warn" | "info";
  text: string;
  phrases: Phrase[];
  /** As it read in the run before, when it reads otherwise now (less). */
  was?: { text: string; phrases: Phrase[] };
  predicted: Saving;
  /** Measured between the runs (fixed, less). */
  measured?: Saving;
  /** The measured saving against the predicted (`fix.saved`, `fix.savedTime`). */
  saved?: Phrase;
  savedText?: string;
}

/** A run against its program's run before it. */
export interface SinceLast {
  program: string;
  /** The run before, by path; absent for a program's first run. */
  previous?: string;
  change?: RunChange;
  findings: FindingChange[];
  /** Why the program's kept findings were not read or written. */
  unsaved?: string;
}

/** A size bucket of bodies: up to `leBytes` (each twice the last; 0 is the empty body). */
export interface SizeBucket {
  leBytes: number;
  count: number;
  bytes: number;
}

/** A run by its four signals and how many requests were in flight, each a figure
 * with its line over the session's slices (crates/proxy/src/signals.rs). A figure
 * that cannot be said (no time, no request) is null, never 0. */
export interface Signals {
  /** A run too short to have a pace (under a second, or a handful of requests): totals are said, not rates. Absent before 16.0. */
  short?: boolean;
  timed: boolean;
  wallMs: number;
  binMs: number;
  bins: number;
  rate: { value: number | null; unit: string; series: number[]; requests: number };
  errors: {
    value: number | null;
    series: number[];
    count: number;
    counts: { client: number; server: number; transport: number; injected: number };
    client: number[];
    server: number[];
    transport: number[];
    injected: number[];
    retries: number[];
    retriesTotal: number;
  };
  latency: { value: number | null; series: number[]; p50Ms: number | null; p95Ms: number | null; p99Ms: number | null; p95: number[]; p99: number[] };
  bytes: { value: number | null; series: number[]; upPerSec: number | null; downPerSec: number | null; up: number[]; down: number[]; upBytes: number; downBytes: number };
  inFlight: { value: number | null; series: number[]; peak: number; peakAtMs: number };
  /** Null when no request's first byte was timed. */
  firstByte: { share: number; count: number; kinds: { operation: string; count: number; firstByteMs: number; restMs: number }[] } | null;
  sizes: { request: SizeBucket[]; response: SizeBucket[]; throughput: { operation: string; bucket: number; count: number; mibPerSec: number }[]; mibPerSec: number | null };
  statuses: { status: number | null; code: string | null; count: number; injected: number; firstMs: number; lastMs: number }[];
  cost: { amount: number; currency: string; operations: { operation: string; class: "write" | "list" | "read" | "free"; requests: number; billed: number; cost: number; share: number }[] };
  prefixes: { prefix: string; folders: number; requests: number; peakReads: number; peakWrites: number; readShare: number; writeShare: number }[];
}

/** What a trial of a session would run (`Sessions.trialPlan`). */
export interface TrialPlan {
  command: string;
  cwd: string | null;
  /** `maxTimes`: the attempts in a row it may be asked to fail (`name:N`), for the two that take a count. */
  scenarios: { name: string; about: string; maxTimes?: number | null }[];
  /** The scenarios are sonde.toml's own. */
  project: boolean;
  /** The command names no `{trial}`: the runs write where the clean run wrote. */
  sharesPlace: boolean;
  cleanExit: number;
}

/** A trial as the app hears it run. */
export type TrialEvent = { scenario: string; state: "running" } | { started: RecordingView; scenario: string } | { data: string; scenario: string } | { scenario: string; state: "done"; trial: Trial };

export interface TrialResult {
  path: string;
  trials: Trial[];
  cancelled: boolean;
  /** `trialAgain` only: the fresh clean run's exit (no trial when not 0). */
  cleanExit?: number | null;
}

export interface SessionView {
  /** The run's four signals (absent in a share made before 15.0). */
  signals?: Signals;
  /** A trial's session: its scenario, the clean run it is a trial of (and that run's file, when it is still there). */
  trial?: { scenario: string; of: string; clean: string | null };
  /** A clean run's trials, each with its own session's path. */
  trials?: Trial[];
  /** The requests a fault scenario answered, by id. */
  injected?: number[];
  path: string;
  header: { endpoint: string; url: string; region: string; created: string; command?: string | null; cwd?: string | null; git?: GitStamp | null; /** a trial's session: the scenario its requests met */ scenario?: string | null };
  summary: SessionSummary;
  profile: Profile;
  whatIf?: WhatIf | null;
  /** The first page of the session's operations (500 in time order), of `episodesTotal`. */
  episodes: EpisodeRow[];
  /** How many operations the session has. */
  episodesTotal: number;
  /** Its first operation's start and its last operation's end; null when it has none. */
  span: { start: string; end: string } | null;
  /** Its operations by family: how many of each format and of each kind. */
  families: { formats: Record<string, number>; kinds: Record<string, number> };
  observations: Observation[];
  sharedNotes: string[];
}

/** A session whole, every operation in it: what `sonde share` writes into a page (`Episode`s with their English titles). */
export interface WholeSession extends Omit<SessionView, "episodes" | "episodesTotal" | "span" | "families"> {
  episodes: Episode[];
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
