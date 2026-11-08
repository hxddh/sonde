// Run and Sessions: a program run from sonde, its operations beside it, and
// the session afterwards with the output lined up.

import { expect, open, test } from "nsh/test";
import { desktop, place } from "./helpers";

async function demo() {
  const app = await open("../App.tsx", desktop);
  await place(app, "endpoints");
  await app.press("id:open-demo");
  await app.waitFor("text:Demo (built-in)", { timeout: 10000 });
  await place(app, "run");
  return app;
}

test("run a command: its requests become operations, the session lines them up with its output", async () => {
  const app = await demo();
  await app.type("id:run-command", "echo fetching; curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY $AWS_ENDPOINT_URL/demo/README.md -o /dev/null; echo done");
  await app.press("id:run-start");
  await app.waitFor("id:run-terminal", { timeout: 10000 });
  await app.waitFor("id:run-ended", { timeout: 20000 });
  expect(app.shows("text:exited with 0")).toBe(true);
  await app.waitFor("text:/GetObject demo\\/README\\.md/", { timeout: 5000 });
  // the run as it went: rate, failures, in flight, p95; and a monitor that floats
  expect(app.shows("id:live")).toBe(true);
  expect(app.shows("id:live-p95")).toBe(true);
  await app.press("id:open-monitor");
  await app.waitUntil(() => app.window("monitor").state() !== null);
  await app.press("id:run-open-session");
  await app.waitFor("id:screen-sessions");
  await app.waitFor("id:output", { timeout: 5000 });
  expect(app.shows("text:fetching")).toBe(true);
  expect(app.shows("text:done")).toBe(true);
  // a line printed before the request points at the operation after it
  await app.press("text:fetching");
  await app.waitFor("id:operation-detail");
  expect(app.shows("text:/GetObject/")).toBe(true);
});

test("a program that sends sonde nothing: how each SDK is pointed at it", async () => {
  const app = await demo();
  await app.type("id:run-command", "echo nothing to store");
  await app.press("id:run-start");
  await app.waitFor("id:run-ended", { timeout: 20000 });
  await app.waitFor("id:run-no-requests", { timeout: 5000 });
  expect(app.shows("text:The program sent sonde no requests")).toBe(true);
  expect(app.shows("text:boto3 / botocore")).toBe(true);
});

test("record without a program: a local endpoint to point an application at", async () => {
  const app = await demo();
  await app.press("id:record");
  await app.waitFor("id:record-form");
  await app.press("label:No deletes");
  await app.press("id:record-start");
  await app.waitFor("id:recording", { timeout: 10000 });
  expect(app.shows("text:/Recording on http:\\/\\/127\\.0\\.0\\.1:/")).toBe(true);
  expect(app.shows("text:/export AWS_ENDPOINT_URL=http/")).toBe(true);
  // a secret made for this recording, not a shared one
  expect(app.shows("text:/AWS_SECRET_ACCESS_KEY=[0-9a-f]{64}/")).toBe(true);
  await app.press("id:record-stop");
  await app.waitFor("id:record");
});

test("an example session reads as operations, each opening on its requests", async () => {
  const app = await open("../App.tsx", desktop);
  await app.press("id:open-example");
  await app.waitFor("id:session-summary", { timeout: 10000 });
  expect(app.shows("label:/172 requests · 34 operations/")).toBe(true);
  expect(app.shows("label:/116 not found/")).toBe(true);
  // how the requests were answered, at a glance
  expect(app.shows("id:outcome-ring")).toBe(true);
  expect(app.shows("text:/27 SlateDB writes: conditional create/")).toBe(true);
  expect(app.shows("text:looked for the newest WAL: probed ids 1…128")).toBe(true);
  // acting on the findings, estimated; a finding pressed says what to change, by SDK
  expect(app.shows("text:/^Acting on \\d+ findings?: 172 → \\d+ requests/")).toBe(true);
  await app.press("id:observation-0");
  await app.waitFor("id:fix");
  expect(app.shows("id:fix-code")).toBe(true);
  await app.press("id:fix-go");
  await app.waitFor("text:Go SDK v2");
  // the fixes scrolled into view under the findings; back up to the first
  await app.scroll("id:fix", -2000);
  await app.press("id:observation-0");
  await app.press("text:manifest 3 written");
  await app.waitFor("id:operation-detail");
  expect(app.shows("text:/^requests \\(5\\)$/i")).toBe(true);
  await app.press("id:op-request-4");
  await app.waitFor("id:record-detail");
  expect(app.shows("text:if-none-match")).toBe(true);
  // what stands out narrows the operations to those it is about
  await app.press("id:inspector-close");
  // each says why it matters, in place
  expect(app.texts().some((t) => t.startsWith("Every operation of this kind made the same requests"))).toBe(true);
  // ranked by the requests they bear on, with what acting on them saves
  expect(app.shows("text:/^slate\\/db\\/gc\\/manifest.boundary was read 46 times/")).toBe(true);
  expect(app.shows("id:observation-saving-0")).toBe(true);
  expect(app.shows("text:45 fewer requests (26% of the session)")).toBe(true);
  await app.press("id:observation-0");
  await app.waitFor("text:28 of 34 operations");
  await app.press("id:clear-filter");
  expect(app.shows("id:clear-filter")).toBe(false);
});

test("a session as a timeline, replayed from the app, and the answers compared", async () => {
  const app = await demo();
  // a replay writes: sonde must not be read-only
  await place(app, "settings");
  await app.press("id:read-only");
  await app.waitFor("label:Writes enabled");
  await place(app, "run");
  await app.type("id:run-command", "curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY -X PUT --data hello $AWS_ENDPOINT_URL/demo/t.txt -o /dev/null; curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY $AWS_ENDPOINT_URL/demo/t.txt");
  await app.press("id:run-start");
  await app.waitFor("id:run-ended", { timeout: 20000 });
  await app.press("id:run-open-session");
  await app.waitFor("id:session-pane");
  // where each operation fell in time is the operations' own When column
  expect(app.shows("id:session-pane-mode-timeline")).toBe(false);
  await app.press("id:replay-open");
  await app.waitFor("id:replay-plan", { timeout: 5000 });
  expect(app.shows("text:/2 requests \\(1 writes, 0 deletes\\) under replay-/")).toBe(true);
  await app.press("id:replay-run");
  await app.waitFor("id:compare", { timeout: 20000 });
  await app.waitFor("text:2 requests compared: 2 the same");
  expect(app.shows("id:compare-diff")).toBe(true);
  // the same workload's latency on both stores, kind by kind
  expect(app.shows("id:store-latency")).toBe(true);
  expect(app.shows("text:The same requests on both stores")).toBe(true);
});

test("sessions are looked after: renamed, searched, deleted, dropped on the window", async () => {
  const app = await open("../App.tsx", desktop);
  await place(app, "sessions");
  // five sessions or more by now (the tests before): enough for a search field
  await app.waitFor("id:session-search");
  // a title of the user's, from the row's context menu
  await app.rightClick("id:session-0");
  expect(app.menu()!.map((m) => m.label)).toEqual(["Open in a window", "Share as a web page…", "Copy file path", "Rename…", "Delete…"]);
  await app.chooseMenuItem("Rename…");
  await app.waitFor("id:rename-dialog");
  await app.clear("id:rename-input");
  await app.type("id:rename-input", "Nightly compaction");
  await app.press("id:rename-save");
  await app.waitFor("text:Nightly compaction");
  // search narrows the list
  await app.type("id:session-search", "nightly");
  await app.waitFor("id:session-0");
  expect(app.count("label:/^Nightly compaction$/")).toBe(1);
  expect(app.shows("id:session-1")).toBe(false);
  await app.clear("id:session-search");
  await app.waitFor("id:session-1");
  // its path, for dropping it back on the window
  await app.rightClick("id:session-0");
  await app.chooseMenuItem("Copy file path");
  const path = app.clipboard()!;
  expect(path.endsWith(".sonde")).toBe(true);
  // deleted, after asking
  const asked: string[] = [];
  app.onDialog((d) => (asked.push(d.title ?? ""), "Delete"));
  await app.rightClick("id:session-1");
  await app.chooseMenuItem("Delete…");
  await app.waitFor("text:/^Deleted “/");
  expect(asked[0]).toMatch(/^Delete “/);
  // a file dropped on the window opens; anything else is said not to be a session
  await app.dropFiles("id:shell", [`file://${path}`]);
  await app.waitFor("id:session-summary");
  await app.dropFiles("id:shell", ["file:///tmp/notes.txt"]);
  await app.waitFor("text:notes.txt is not a session file");
});

test("a session's time: where it went, and two runs before and after", async () => {
  const app = await open("../App.tsx", desktop);
  await place(app, "sessions");
  await app.press("id:open-example");
  await app.waitFor("id:session-summary", { timeout: 10000 });
  // the shape of the session beside its figures
  expect(app.shows("id:session-shape")).toBe(true);
  expect(app.shows("label:/^Waiting on S3 .* of .*; up to 8 requests in flight/")).toBe(true);
  await app.press("id:session-pane-mode-time");
  await app.waitFor("id:time-pane");
  expect(app.shows("text:Where the time went")).toBe(true);
  expect(app.shows("label:/^Most of the waiting: PutObject, 65%/")).toBe(true);
  expect(app.shows("id:profile-cost")).toBe(true);
  // the operations sort by how long they took
  await app.press("id:session-pane-mode-operations");
  // compared with another run: before and after
  await app.press("id:compare-open");
  const run = app.menu()!.map((m) => m.label).find((l) => l.startsWith("echo"))!;
  await app.chooseMenuItem(run);
  await app.waitFor("id:compare");
  await app.waitFor("id:run-change", { timeout: 10000 });
  expect(app.shows("id:run-change-requests")).toBe(true);
  expect(app.shows("text:/^172 → /")).toBe(true);
});

test("a program over its runs: history, a baseline, and the flows of its requests", async () => {
  const app = await demo();
  const cmd = "curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY $AWS_ENDPOINT_URL/demo/README.md -o /dev/null; curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY $AWS_ENDPOINT_URL/demo/missing.txt -o /dev/null";
  for (let i = 0; i < 2; i++) {
    await app.clear("id:run-command");
    await app.type("id:run-command", cmd);
    await app.press("id:run-start");
    await app.waitFor("id:run-ended", { timeout: 20000 });
  }
  await app.press("id:run-open-session");
  await app.waitFor("id:session-pane");
  await app.press("id:session-pane-mode-runs");
  await app.waitFor("id:history-pane", { timeout: 10000 });
  // the runs of a stretch of days
  expect(app.shows("id:history-range")).toBe(true);
  expect(app.shows("text:/, over [2-9] runs$/")).toBe(true);
  expect(app.shows("id:trend-requests")).toBe(true);
  await app.press("id:history-baseline");
  await app.waitFor("label:Not the baseline");
  // where its requests went
  await app.press("id:session-pane-mode-time");
  await app.waitFor("id:time-pane");
  expect(app.shows("id:flows")).toBe(true);
  expect(app.shows("label:/^2 requests: 50% answered/")).toBe(true);
  expect(app.shows("id:spread")).toBe(true);
});


test("a shared session opens in the browser, on a local address", async () => {
  const app = await open("../App.tsx", desktop);
  await app.press("id:open-example");
  await app.waitFor("id:session-summary", { timeout: 10000 });
  await app.press("id:session-more");
  await app.chooseMenuItem("Share as a web page…");
  const noViewer = "text:/this build has no web viewer/";
  await app.waitUntil(() => app.openedUrls().length > 0 || app.shows(noViewer), { timeout: 10000 });
  if (app.shows(noViewer)) {
    // a build without the viewer (CI's test job) says so, and opens nothing
    expect(app.openedUrls()).toEqual([]);
    return;
  }
  // the openUrl capability lets the app open its own page (http) and nothing else
  expect(app.openedUrls()[0]).toMatch(/^http:\/\/127\.0\.0\.1:\d+\/$/);
});

test("relative times read in the app's language", async () => {
  const app = await open("../App.tsx", desktop);
  await app.press("id:open-example");
  await app.waitFor("id:session-summary", { timeout: 10000 });
  await app.waitUntil(() => app.texts().some((x) => /^(\d+ (min|h|d)|now)$/.test(x)), { timeout: 5000 });
  await place(app, "settings");
  await app.press("label:Language");
  await app.press("text:中文");
  await app.waitFor("text:通用");
  await app.press("id:nav-sessions");
  // a session's age, in Chinese, and no longer in English
  await app.waitUntil(() => app.texts().some((x) => /^(\d+ (分钟|小时|天)|刚刚)$/.test(x)), { timeout: 5000 });
  expect(app.texts().some((x) => /^(\d+ (min|h|d)|now)$/.test(x))).toBe(false);
  await place(app, "settings");
  await app.press("label:语言");
  await app.press("text:English");
  await app.waitFor("text:General");
});
