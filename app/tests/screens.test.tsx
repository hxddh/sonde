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

test("multipart: scan, lifecycle verdict, the guard refuses while read-only", async () => {
  const app = await withDemo("demo");
  // the bucket's card says it before the detail opens
  await app.waitFor("text:3 · 172 MiB", { timeout: 10000 });
  await place(app, "multipart");
  await app.waitFor("id:lifecycle-verdict", { timeout: 10000 });
  expect(app.shows("text:2 older than 7 days")).toBe(true);
  expect(app.shows("text:missing")).toBe(true);
  await app.press("id:plan-abort");
  await app.waitFor("id:confirm-dialog");
  expect(app.shows("text:Read-only mode is on. Turn it off in Settings to run this.")).toBe(true);
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
}, 120_000);

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

test("settings: read-only off, a confirmed abort, the audit log", async () => {
  const app = await withDemo("demo");
  await place(app, "settings");
  await app.press("id:read-only");
  await app.waitFor("label:Writes enabled");
  await place(app, "multipart");
  await app.waitFor("id:lifecycle-verdict", { timeout: 10000 });
  await app.press("id:plan-abort");
  await app.waitFor("id:confirm-dialog");
  expect(app.enabled("id:confirm-run")).toBe(false);
  await app.type("id:confirm-input", "2");
  await app.press("id:confirm-run");
  await app.waitFor("text:1 incomplete upload", { timeout: 10000 });
  await place(app, "audit");
  await app.waitFor("text:abortUploads · demo/demo");
  await app.press("id:activity-close");
  await place(app, "settings");
  await app.press("id:read-only");
  await app.waitFor("label:Read-only");
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
  await app.waitFor("id:concurrency");
  await app.clear("id:concurrency");
  await app.type("id:concurrency", "12");
  // the clock stands still: nothing saved yet, however long the test takes
  expect(app.shows("text:Settings saved")).toBe(false);
  await app.clock!.runFor(400);
  expect(app.shows("text:Settings saved")).toBe(false);
  // 0.7 s after the last key it is saved, with Undo (the toast shows in each window)
  await app.clock!.runFor(400);
  await app.waitFor("text:Settings saved >> nth=0");
  await app.press("text:Undo >> nth=0");
});
