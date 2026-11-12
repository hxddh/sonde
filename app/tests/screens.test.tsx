// The key path of each screen, in the desktop form, on the built-in demo.

import { expect, open, test } from "nsh/test";
import { desktop, place, withDemo } from "./helpers";

test("request inspector: records, details, copy as curl without a signature", async () => {
  const app = await withDemo("demo");
  await app.waitFor("text:README.md", { timeout: 10000 });
  await place(app, "requests");
  await app.waitFor("id:requests");
  await app.waitFor("text:ListObjectsV2", { timeout: 5000 });
  await app.press("text:ListObjectsV2 >> nth=0");
  await app.waitFor("id:record-detail");
  expect(app.shows("text:x-amz-request-id")).toBe(true);
  await app.press("id:copy-curl");
  await app.waitUntil(() => (app.clipboard() ?? "").includes("UNSIGNED"));
  expect(app.clipboard()).not.toContain("Signature=");
  await app.press("id:copy-cli");
  await app.waitUntil(() => (app.clipboard() ?? "").startsWith("aws s3api"));
});

test("multipart: scan, lifecycle verdict, the guard waits for the typed confirmation", async () => {
  const app = await withDemo("demo");
  // the bucket's card says it before the detail opens
  await app.waitFor("text:3 · 172 MiB", { timeout: 10000 });
  await place(app, "multipart");
  await app.waitFor("id:lifecycle-verdict", { timeout: 10000 });
  // the figure is the count; its line says how much they hold and how many are old
  expect(app.shows("text:/2 older than 7 days/")).toBe(true);
  expect(app.shows("text:Missing")).toBe(true);
  await app.press("id:plan-abort");
  await app.waitFor("id:confirm-dialog");
  expect(app.enabled("id:confirm-run")).toBe(false);
  // what is typed is not the count: still nothing runs
  await app.type("id:confirm-input", "9");
  expect(app.enabled("id:confirm-run")).toBe(false);
  await app.press("id:confirm-cancel");
  await app.waitFor("id:confirm-dialog", { state: "gone" });
});

test("versions: the timeline of a deleted key", async () => {
  const app = await withDemo("demo");
  await place(app, "versions");
  await app.type("id:version-key", "config/app.yaml");
  await app.press("id:load-versions");
  await app.waitFor("id:timeline", { timeout: 10000 });
  expect(app.shows("text:The key reads as deleted")).toBe(true);
  expect(app.count("text:Delete marker")).toBeGreaterThanOrEqual(1);
  expect(app.shows("text:On")).toBe(true);
});

test("capacity: why the bucket is this big", async () => {
  const app = await withDemo("demo");
  await place(app, "capacity");
  await app.waitFor("id:du-findings", { timeout: 15000 });
  expect(app.texts().some((t) => t.startsWith("• Incomplete multipart uploads"))).toBe(true);
  expect(app.shows("id:du-stack")).toBe(true);
});

test("layouts: detects and decodes the real SlateDB sample, with replay", async () => {
  const app = await withDemo("demo-layouts");
  await app.waitFor("id:overview-layouts", { timeout: 15000 });
  await place(app, "layouts");
  await app.waitFor("id:analyze-slatedb", { timeout: 15000 });
  expect(app.shows("id:analyze-iceberg")).toBe(true);
  expect(app.shows("id:analyze-lakefs")).toBe(true);
  expect(app.shows("id:analyze-woodpecker")).toBe(true);
  await app.press("id:analyze-slatedb");
  await app.waitFor("id:layout-report", { timeout: 15000 });
  expect(app.texts().some((t) => t.startsWith("SlateDB: manifest"))).toBe(true);
  expect(app.shows("id:replay")).toBe(true);
});

test("compatibility: runs probes on the demo and fills the matrix", async () => {
  const app = await withDemo();
  // probes write to the scratch bucket named: naming it is the consent
  await place(app, "probes");
  // the endpoint in use comes chosen
  await app.type("id:probe-bucket", "demo");
  await app.type("id:probe-only", "range.,consistency.");
  await app.press("id:probe-run");
  await app.waitFor("id:matrix", { timeout: 60000 });
  await app.waitFor("text:Range boundaries", { timeout: 60000 });
  await app.press("id:cell-range.bounds-0");
  await app.waitFor("id:probe-detail");
  expect(app.shows("text:/^aws behavior$/i")).toBe(true);
  expect(app.shows("text:/^requests and responses$/i")).toBe(true);
  await app.window("compat").pressClose();
}, 120_000);

test("endpoints: adding never replaces one; a name in another script gets an id of its own", async () => {
  const app = await withDemo();
  await place(app, "endpoints");
  for (const [name, port] of [["Prod", 9001], ["Prod", 9002], ["生产", 9003], ["测试", 9004]] as const) {
    await app.press("id:add-endpoint");
    await app.waitFor("id:ep-name");
    await app.type("id:ep-name", name);
    await app.type("id:ep-url", `http://127.0.0.1:${port}`);
    await app.press("id:ep-save");
    // the form goes once the endpoint is saved: the next is added after it has
    await app.waitUntil(() => !app.shows("id:ep-save"), { timeout: 10000 });
    await app.waitFor("id:add-endpoint");
  }
  // each its own: the second Prod beside the first, the Chinese names apart
  for (const id of ["prod", "prod-2", "endpoint", "endpoint-2"]) await app.waitFor(`id:endpoint-${id}`);
  expect(app.shows("text:/^http:\\/\\/127\\.0\\.0\\.1:9001 /")).toBe(true);
  expect(app.shows("text:/^http:\\/\\/127\\.0\\.0\\.1:9002 /")).toBe(true);
});

test("browse: an object's detail is its own, never the one chosen before", async () => {
  const app = await withDemo("demo");
  await app.waitFor("text:README.md", { timeout: 10000 });
  await app.press("text:README.md");
  await app.waitFor("text:s3://demo/README.md", { timeout: 10000 });
  // its sections are there once the object has been read
  await app.waitFor("text:Presigned URL", { timeout: 10000 });
  await app.press("text:Presigned URL");
  await app.press("id:presign");
  await app.waitFor("text:/README\\.md\\?X-Amz-/", { timeout: 10000 });
  await app.press("id:flat");
  await app.waitFor("text:images/logo.png", { timeout: 10000 });
  await app.press("text:images/logo.png");
  await app.waitFor("text:s3://demo/images/logo.png", { timeout: 10000 });
  await app.waitFor("text:Presigned URL", { timeout: 10000 });
  // the URL asked for README.md went with it
  expect(app.shows("text:/README\\.md\\?X-Amz-/")).toBe(false);
  // Refresh lists the same place again
  await app.press("id:listing-refresh");
  await app.waitFor("text:images/logo.png", { timeout: 10000 });
});

test("errors are readable and lead to their request", async () => {
  const app = await withDemo();
  await place(app, "endpoints");
  await app.press("id:add-endpoint");
  await app.type("id:ep-name", "Broken");
  await app.type("id:ep-url", "http://127.0.0.1:9");
  await app.press("id:ep-save");
  await app.waitFor("id:endpoint-broken");
  await app.press("id:test-broken");
  await app.waitFor("id:test-result", { timeout: 15000 });
  expect(app.shows("text:Not reachable")).toBe(true);
  await app.press("text:Show request");
  await app.waitFor("id:record-detail");
});

test("settings: no read-only switch; a confirmed abort, the audit log", async () => {
  const app = await withDemo("demo");
  await place(app, "settings");
  // one safety setting: what a recorded program may do
  expect(app.shows("id:record-policy")).toBe(true);
  expect(app.shows("id:read-only")).toBe(false);
  expect(app.shows("id:toolbar-lock")).toBe(false);
  await place(app, "multipart");
  await app.waitFor("id:lifecycle-verdict", { timeout: 10000 });
  await app.press("id:plan-abort");
  await app.waitFor("id:confirm-dialog");
  expect(app.enabled("id:confirm-run")).toBe(false);
  await app.type("id:confirm-input", "2");
  await app.press("id:confirm-run");
  await app.waitFor("text:1 incomplete upload", { timeout: 10000 });
  await place(app, "audit");
  await app.waitFor("text:Abort uploads · demo/demo");
  await app.press("id:activity-close");
});

test("appearance: higher contrast, kept with the settings", async () => {
  const app = await withDemo();
  await place(app, "settings");
  await app.waitFor("id:high-contrast");
  // secondary text reads stronger, and back
  const hint = "text:Stronger borders and secondary text";
  const before = app.spanStyle(hint).color;
  await app.press("id:high-contrast");
  await app.waitUntil(() => app.spanStyle(hint).color !== before);
  await app.press("id:high-contrast");
  await app.waitUntil(() => app.spanStyle(hint).color === before);
});

test("sonde:// links: a bucket's folder opens in the store; an unknown endpoint is said", async () => {
  const app = await withDemo();
  await place(app, "settings");
  await app.openWithUrl("sonde://bucket/demo/demo/reports");
  await app.waitFor("id:screen-browse");
  await app.waitFor("text:2026-q1.csv", { timeout: 10000 });
  await app.openWithUrl("sonde://bucket/nowhere/b");
  await app.waitFor("text:/nowhere/");
});

test("sonde:// links keep a stretch of time: a session opens on its Time pane over it", async () => {
  const app = await withDemo();
  await app.press("id:nav-sessions");
  await app.press("id:open-example");
  await app.waitFor("id:session-summary", { timeout: 10000 });
  const { api } = await import("../src/native");
  // (the example: 172 requests over 467 ms, slices enough for a stretch of 200 ms)
  const example = api.sessions.list().find((s) => (s.command ?? "").includes("gen-slatedb"));
  expect(!!example).toBe(true);
  const name = example!.path.split(/[\\/]/).pop()!;
  await app.openWithUrl(`sonde://session/${encodeURIComponent(name)}?range=0-200`);
  await app.waitFor("id:time-pane", { timeout: 10000 });
  // the timeline says the stretch it shows, and offers the whole session back
  await app.waitFor("id:zoom-reset", { timeout: 5000 });
});

test("language: Chinese strings", async () => {
  const app = await withDemo();
  await place(app, "settings");
  await app.press("label:Language");
  await app.press("text:中文");
  await app.waitFor("text:通用");
  // what sonde says about a session is said in Chinese too
  await app.press("id:nav-sessions");
  await app.press("id:open-example");
  await app.waitFor("text:查找最新的 WAL：探测了编号 1…128", { timeout: 10000 });
  expect(app.shows("text:/27 次 SlateDB 写入都是：条件创建/")).toBe(true);
  await app.press("label:语言");
  await app.press("text:English");
  await app.waitFor("text:General");
});

test("reduced motion: the overview's cards are there at once, still", async () => {
  const app = await withDemo("demo", { ...desktop, reducedMotion: true });
  await app.waitFor("id:overview-versions", { timeout: 10000 });
  expect(app.shows("id:overview-multipart")).toBe(true);
  expect(app.shows("id:overview-capacity")).toBe(true);
});

test("a number typed in Settings is saved a moment after the last key", async () => {
  const app = await open("../App.tsx", { ...desktop, clock: {} });
  await place(app, "settings");
  await app.waitFor("id:price");
  await app.clear("id:price");
  await app.type("id:price", "0.02");
  // the clock stands still: nothing saved yet, however long the test takes
  expect(app.shows("text:Settings saved")).toBe(false);
  await app.clock!.runFor(400);
  expect(app.shows("text:Settings saved")).toBe(false);
  // 0.7 s after the last key it is saved, with Undo (the toast shows in the window that has the focus, else the main one)
  await app.clock!.runFor(400);
  await app.waitFor("text:Settings saved >> nth=0");
  await app.press("text:Undo >> nth=0");
});

// --- what failed, said: the engine's answers stood in for (open's `modules`), so no store has to fail for it

const NATIVE = "sonde/native";
const bucketNamed = (name: string) => ({ name, created: null, region: null });

test("a listing that fails says why in the one error state, with a way to try again", async () => {
  // a bucket the store does not have: its listing is the store's own refusal
  const app = await open("../App.tsx", { ...desktop, modules: { [NATIVE]: { "Sonde.buckets": async () => [bucketNamed("gone")] } } });
  await place(app, "endpoints");
  await app.press("id:open-demo");
  await app.waitFor("text:Demo (built-in)", { timeout: 10000 });
  await place(app, "browse");
  await app.waitFor("id:bucket-gone", { timeout: 10000 });
  await app.waitFor("id:listing-error", { timeout: 10000 });
  expect(app.shows("text:Could not load this")).toBe(true);
  expect(app.shows("text:/NoSuchBucket/")).toBe(true);
  // never the words of an empty folder for a failure
  expect(app.shows("id:listing-loading")).toBe(false);
  // again: asked again, and (the bucket still gone) said again
  await app.press("text:Try again");
  await app.waitFor("id:listing-error", { timeout: 10000 });
});

test("a store that is gone: the buckets say why and come back with it; the endpoint's test says it is not reachable", async () => {
  let up = false;
  const app = await open("../App.tsx", {
    ...desktop,
    modules: {
      [NATIVE]: {
        "Sonde.buckets": async () => {
          if (!up) throw new Error("connection refused: nothing listens at 127.0.0.1:9");
          return [bucketNamed("demo")];
        },
        "Sonde.testEndpoint": async (id: string) => ({ endpoint: id, ok: false, latencyMs: 0, credentialsSource: null, bucketCount: null, owner: null, identity: null, identityNote: null, error: { kind: "network", message: "connection refused: nothing listens at 127.0.0.1:9", recordId: null }, server: null }),
      },
    },
  });
  await place(app, "endpoints");
  await app.press("id:open-demo");
  await app.waitFor("text:Demo (built-in)", { timeout: 10000 });
  // the endpoint, tested: not reachable, and why
  await app.press("id:test-demo");
  await app.waitFor("id:test-result");
  expect(app.shows("text:Not reachable")).toBe(true);
  expect(app.shows("text:/connection refused/")).toBe(true);
  await app.window("settings").pressClose();
  // its buckets: why there are none, held open in the sidebar, not an empty list
  await place(app, "browse");
  await app.waitFor("id:buckets-error", { timeout: 10000 });
  expect(app.shows("text:/connection refused/")).toBe(true);
  expect(app.shows("id:bucket-demo")).toBe(false);
  // the store is back: asked again, the buckets are there
  up = true;
  await app.press("id:buckets-refresh");
  await app.waitFor("id:bucket-demo", { timeout: 10000 });
  await app.waitFor("id:buckets-error", { state: "gone" });
});

test("a session file that cannot be read says so, and why", async () => {
  const app = await open("../App.tsx", { ...desktop, modules: { [NATIVE]: { "Sessions.view": async () => {
    throw new Error("line 3 is cut short: not a whole record");
  } } } });
  await app.press("id:open-example");
  await app.waitFor("id:session-pane", { timeout: 10000 });
  await app.waitFor("id:session-error", { timeout: 10000 });
  expect(app.shows("text:Could not load this")).toBe(true);
  expect(app.shows("text:/line 3 is cut short/")).toBe(true);
  // no figure of a session that was not read
  expect(app.shows("id:session-summary")).toBe(false);
});

// --- a session of any size: its operations come in pages, as the table scrolls, sorts and narrows

/** A session of `n` operations as the bridge would answer for it, whatever file is asked for (the
 * calls that answer at once are the bridge's own, so the session opened is a real one): the view
 * carries the totals and the first page only; a page is made when it is asked for, and every ask is kept. */
function bigSession(n: number) {
  const t0 = Date.parse("2026-11-07T00:00:00Z");
  const row = (i: number) => ({ index: i, kind: "request", format: null, title: `operation ${i}`, start: new Date(t0 + i * 10).toISOString(), end: new Date(t0 + i * 10 + 4).toISOString(), records: [i + 1], requests: 1 + (i % 7), bytesUp: 0, bytesDown: 128, outcome: { kind: "ok" }, notes: [] });
  const zeros = [0, 0];
  const signals = {
    timed: true, short: false, wallMs: n * 10, binMs: n * 5, bins: 2,
    rate: { value: 100, unit: "1/s", series: [100, 100], requests: n },
    errors: { value: 0, series: zeros, count: 0, counts: { client: 0, server: 0, transport: 0, injected: 0 }, client: zeros, server: zeros, transport: zeros, injected: zeros, retries: zeros, retriesTotal: 0 },
    latency: { value: 4, series: [4, 4], p50Ms: 4, p95Ms: 4, p99Ms: 4, p95: [4, 4], p99: [4, 4] },
    bytes: { value: 12800, series: [12800, 12800], upPerSec: 0, downPerSec: 12800, up: zeros, down: [12800, 12800], upBytes: 0, downBytes: n * 128 },
    inFlight: { value: 1, series: [1, 1], peak: 1, peakAtMs: 0 },
    firstByte: null, sizes: { request: [], response: [], throughput: [], mibPerSec: null }, statuses: [], cost: { amount: 0, currency: "USD", operations: [] }, prefixes: [],
  };
  const profile = { wallMs: n * 10, busyMs: n * 4, peakInFlight: 1, peakAtMs: 0, meanInFlight: 0.4, groups: [], binMs: n * 5, rate: [], inFlight: [], samples: [], cost: { writes: 0, reads: n, free: 0, amount: 0, currency: "USD" }, prices: { per1000Writes: 0, per1000Reads: 0, currency: "USD" }, prefixes: [], heat: [], flows: [] };
  const asked: { offset: number; limit: number; sort?: string; descending?: boolean; finding?: number }[] = [];
  // the order a query asks for: by time, or the busiest first (ties by index)
  const order = (q: { sort?: string; descending?: boolean; finding?: number }) => {
    const all = q.finding === 0 ? Array.from({ length: 300 }, (_, k) => k * 3) : Array.from({ length: n }, (_, i) => i);
    if (q.sort === "requests") all.sort((a, b) => ((a % 7) - (b % 7)) * (q.descending ? -1 : 1) || a - b);
    else if (q.descending) all.reverse();
    return all;
  };
  const modules = {
    "Sessions.view": async (path: string) => ({
      path, header: { endpoint: "demo", url: "http://127.0.0.1:9000", region: "us-east-1", created: new Date(t0).toISOString(), command: "etl --all" },
      summary: { requests: n, errors: 0, misses: 0, refused: 0, seconds: n / 100, bytesUp: 0, bytesDown: n * 128, operations: [{ operation: "GetObject", count: n, errors: 0, misses: 0, errorShare: 0, bytesUp: 0, bytesDown: n * 128, p50Ms: 4, p95Ms: 4, p99Ms: 4, maxMs: 4 }] },
      profile, signals, whatIf: null, injected: [], sharedNotes: [],
      episodes: Array.from({ length: 500 }, (_, i) => row(i)), episodesTotal: n,
      span: { start: new Date(t0).toISOString(), end: new Date(t0 + n * 10).toISOString() }, families: { formats: {}, kinds: { request: n } },
      observations: [{ level: "info", text: "300 operations of every third", episodes: Array.from({ length: 200 }, (_, k) => k * 3), episodesTotal: 300, firstStart: new Date(t0).toISOString() }],
    }),
    "Sessions.episodes": async (_: string, q: { offset?: number; limit?: number; sort?: string; descending?: boolean; finding?: number }) => {
      const offset = q.offset ?? 0;
      const limit = Math.min(q.limit ?? 500, 5000);
      asked.push({ offset, limit, sort: q.sort, descending: q.descending, finding: q.finding });
      const all = order(q);
      return { total: all.length, offset, rows: all.slice(offset, offset + limit).map(row) };
    },
  };
  return { modules, asked };
}

test("a session of a hundred thousand operations opens on its totals and first page, and reads on in pages", async () => {
  const N = 100000;
  const big = bigSession(N);
  const began = Date.now();
  const app = await open("../App.tsx", { ...desktop, modules: { [NATIVE]: big.modules } });
  await app.press("id:open-example");
  // its totals at once, from the view: nothing waits for the operations to be whole
  await app.waitFor("id:session-summary", { timeout: 10000 });
  await app.waitFor("text:operation 0", { timeout: 10000 });
  expect(app.shows("label:/100,000 requests · 100,000 operations/")).toBe(true);
  // (opened and drawn well inside the budget; the window's own start is most of it here)
  expect(Date.now() - began).toBeLessThan(15000);
  // the first page came with the view: nothing past the table's start was asked for
  expect(big.asked.every((q) => q.offset === 0)).toBe(true);
  // down the table, past what is held: the rows there are asked for, a window of them, once
  await app.scroll("id:operations", 30 * 1700);
  await app.waitUntil(() => big.asked.some((q) => q.offset > 0), { timeout: 10000 });
  await app.waitFor("text:/^operation 17\\d\\d$/", { timeout: 10000 });
  expect(big.asked.every((q) => q.limit <= 5000)).toBe(true);
  expect(big.asked.length).toBeLessThan(6);
  // a finding narrows them where they are kept (as a sort does): its 300, though the view listed 200 of them
  await app.press("id:observation-0");
  await app.waitUntil(() => big.asked.some((q) => q.finding === 0), { timeout: 10000 });
  await app.waitFor("text:/^300 of 100,000 operations$/", { timeout: 10000 });
  // (scrolling a table row by row takes the headless window a while)
}, 120000);
