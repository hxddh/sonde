// Run and Sessions: a program run from sonde, its operations beside it, and
// the session afterwards with the output lined up.

import { expect, open, test } from "nsh/test";
import { desktop, place, runAction } from "./helpers";

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
  // (the button follows the field a frame later)
  await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
  await app.press("id:run-start");
  await app.waitFor("id:run-terminal", { timeout: 10000 });
  await app.waitFor("id:run-ended", { timeout: 20000 });
  expect(app.shows("text:exited with 0")).toBe(true);
  await app.waitFor("text:/GetObject demo\\/README\\.md/", { timeout: 5000 });
  // once it ended: the run in a sentence where the live figures were
  await app.waitFor("id:run-story", { timeout: 5000 });
  expect(app.shows("id:live")).toBe(false);
  await runAction(app, "run-open-session");
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
  // (the button follows the field a frame later)
  await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
  await app.press("id:run-start");
  await app.waitFor("id:run-ended", { timeout: 20000 });
  await app.waitFor("id:run-no-requests", { timeout: 5000 });
  expect(app.shows("text:The program sent sonde no requests")).toBe(true);
  expect(app.shows("text:boto3 / botocore")).toBe(true);
  // not kept: once it is gone, nothing on Run opens another session in its place
  // (the band's commands: as buttons, or behind their `⋯` where the band has no room)
  await app.waitFor("id:run-actions");
  await app.waitUntil(() => !app.shows("id:run-actions"), { timeout: 15000 });
});

test("record without a program: a local endpoint to point an application at", async () => {
  const app = await demo();
  await app.press("id:record");
  await app.waitFor("id:record-form");
  await app.press("label:Block deletes");
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
  expect(app.shows("text:/27 SlateDB writes: conditional create/")).toBe(true);
  expect(app.shows("text:looked for the newest WAL: probed ids 1…128")).toBe(true);
  // acting on the findings, estimated; a finding pressed says what to change, by SDK
  expect(app.shows("text:/^Acting on \\d+ findings?: 172 → \\d+ requests/")).toBe(true);
  await app.press("id:observation-0");
  await app.waitFor("id:fix");
  expect(app.shows("id:fix-code")).toBe(true);
  await app.press("id:fix-sdk-go");
  await app.waitFor("text:Go SDK v2");
  // the advice opened inside its own row, under its title: the row is there to press again
  expect(app.shows("id:finding-advice")).toBe(true);
  await app.press("id:observation-0");
  await app.press("text:manifest 3 written");
  await app.waitFor("id:operation-detail");
  expect(app.shows("text:/^requests · 5$/i")).toBe(true);
  await app.press("id:op-request-4");
  await app.waitFor("id:record-detail");
  expect(app.shows("text:if-none-match")).toBe(true);
  // what stands out narrows the operations to those it is about
  await app.press("id:inspector-close");
  // the request's sections fold away as the inspector goes
  await app.waitUntil(() => !app.shows("id:inspector"), { timeout: 2000 });
  // a line each, so the operations keep the room; the one chosen says why it matters, in place
  await app.press("id:observation-2");
  expect(app.texts().some((t) => t.startsWith("Every operation of this kind made the same requests"))).toBe(true);
  await app.press("id:observation-2");
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
  // (the button follows the field a frame later)
  await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
  await app.press("id:run-start");
  await app.waitFor("id:run-ended", { timeout: 20000 });
  await runAction(app, "run-open-session");
  await app.waitFor("id:session-pane");
  // read off the interface's thread: there once its header is
  await app.waitFor("id:session-summary", { timeout: 10000 });
  // where each operation fell in time is the operations' own When column
  expect(app.shows("id:session-pane-mode-timeline")).toBe(false);
  await app.press("id:replay-open");
  await app.waitFor("id:replay-plan", { timeout: 5000 });
  expect(app.shows("text:/2 requests \\(1 write, 0 deletes\\) under replay-/")).toBe(true);
  await app.press("id:replay-run");
  await app.waitFor("id:compare", { timeout: 20000 });
  await app.waitFor("text:2 requests compared: 2 the same");
  // one view: the answers, before and after, and the transcripts on demand
  await app.waitFor("id:run-change", { timeout: 10000 });
  expect(app.shows("id:compare-diff")).toBe(false);
  await app.press("id:compare-transcripts");
  await app.waitFor("id:compare-diff");
  // the same workload's latency on both stores, kind by kind
  expect(app.shows("id:store-latency")).toBe(true);
  expect(app.shows("text:The same requests on both stores")).toBe(true);
});

test("sessions are looked after: renamed, searched, deleted, dropped on the window", async () => {
  const app = await open("../App.tsx", desktop);
  await place(app, "sessions");
  // five sessions or more by now (the tests before): enough for a search field
  await app.waitFor("id:session-search");
  // the session shown is read first: a menu opened while its pane arrives is closed by it
  await app.waitFor("id:session-summary", { timeout: 10000 });
  // a title of the user's, from the row's context menu
  await app.rightClick("id:session-0");
  expect(app.menu()!.map((m) => m.label)).toEqual(["Open in a window", "Share as a web page", "Copy file path", "Rename…", "Delete…"]);
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
  // the list narrows a frame or two after the last key
  await app.waitUntil(() => !app.shows("id:session-1"), { timeout: 2000 });
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
  // the shape of the session beside its figures (the example's: the session shown before it goes first)
  await app.waitFor("id:session-shape", { timeout: 10000 });
  expect(app.shows("label:/^Waiting on S3 .* of .*; up to 8 requests in flight/")).toBe(true);
  // the header's four signals and in flight: a figure each, and a press opens the signal's chart on the Time pane
  for (const id of ["rate", "errors", "latency", "bytes", "inFlight"]) expect(app.shows(`id:signal-${id}`)).toBe(true);
  // (a run under a second has totals, not a rate: 172 requests in 467 ms are not "368 a second")
  expect(app.shows("label:/^Requests: 172, in 467 ms, shows its chart$/")).toBe(true);
  expect(app.shows("label:/^Errors: 0%, No request failed/")).toBe(true);
  await app.press("id:signal-latency");
  await app.waitFor("id:time-pane");
  await app.waitFor("id:latency-over-time");
  // the board in its groups, each chart in the one frame
  expect(app.shows("text:Latency")).toBe(true);
  expect(app.shows("id:board-traffic")).toBe(true);
  expect(app.shows("id:time-shape")).toBe(true);
  await app.waitFor("text:Where the time went");
  // (the pane scrolls its chart into view a moment after it is laid out)
  await app.wait(200);
  // its part of the waiting: requests at once count once, the kinds add up to it
  expect(app.shows("label:/^Most of the waiting: PutObject, \\d+%/")).toBe(true);
  expect(app.shows("id:profile-cost")).toBe(true);
  // the busiest kinds over time: a mode of the requests' chart, not a chart of its own
  await app.press("id:scatter-by-kinds");
  await app.waitFor("id:kinds-over-time");
  expect(app.shows("id:request-scatter")).toBe(false);
  await app.press("id:scatter-by-time");
  // where by prefix: one map, three measures
  await app.scroll("id:time-pane", 900);
  await app.waitFor("id:prefix-use");
  await app.press("id:prefix-measure-peak");
  expect(app.shows("label:/^Most at .*: \\d+\\/s$/")).toBe(true);
  // the operations sort by how long they took
  await app.press("id:session-pane-mode-operations");
  // compared with another run: before and after
  await app.press("id:compare-open");
  // another program's runs are under their own heading, and say so once compared
  const run = app.menu()!.map((m) => m.label).find((l) => l.startsWith("Other programs > curl"))!;
  await app.chooseMenuItem(run);
  await app.waitFor("id:compare");
  await app.waitFor("id:run-change", { timeout: 10000 });
  // the two runs in five tiles, each fact once: no second strip of the same figures
  await app.waitFor("id:compare-figures", { timeout: 10000 });
  expect(app.shows("id:compare-requests")).toBe(true);
  expect(app.shows("text:/^172 → /")).toBe(true);
  expect(app.shows("id:compare-latency")).toBe(true);
  expect(app.shows("id:compare-cost")).toBe(true);
  expect(app.shows("id:compare-signals")).toBe(false);
  expect(app.shows("id:other-program")).toBe(true);
  expect(app.shows("text:/^Gone · /")).toBe(false);
});

test("a program over its runs: history, a baseline, and the flows of its requests", async () => {
  const app = await demo();
  const cmd = "curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY $AWS_ENDPOINT_URL/demo/README.md -o /dev/null; curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY $AWS_ENDPOINT_URL/demo/missing.txt -o /dev/null";
  for (let i = 0; i < 2; i++) {
    await app.clear("id:run-command");
    await app.type("id:run-command", cmd);
    // the button follows the field a frame later
    await app.waitUntil(() => app.enabled("id:run-start"));
    await app.press("id:run-start");
    await app.waitFor("id:run-ended", { timeout: 20000 });
  }
  await runAction(app, "run-open-session");
  await app.waitFor("id:session-pane");
  // read off the interface's thread: there once its header is
  await app.waitFor("id:session-summary", { timeout: 10000 });
  await app.press("id:session-pane-mode-runs");
  await app.waitFor("id:history-pane", { timeout: 10000 });
  // the runs of a stretch of days
  expect(app.shows("id:history-range")).toBe(true);
  expect(app.shows("text:/, over [2-9] runs$/")).toBe(true);
  expect(app.shows("id:trend-requests")).toBe(true);
  await app.press("id:history-baseline");
  // a state has its verb as its label
  await app.waitFor("label:Clear baseline");
  // where its requests went
  await app.press("id:session-pane-mode-time");
  await app.waitFor("id:time-pane");
  // how long every request took, and where they went, on demand
  expect(app.shows("text:/^median .* · p95 .* · p99 /")).toBe(true);
  await app.waitFor("id:flows");
  expect(app.shows("label:/^2 requests: 50% answered/")).toBe(true);
});


test("a shared session opens in the browser, on a local address", async () => {
  const app = await open("../App.tsx", desktop);
  await app.press("id:open-example");
  await app.waitFor("id:session-summary", { timeout: 10000 });
  await app.press("id:session-more");
  await app.chooseMenuItem("Share as a web page");
  const noViewer = "text:Copied the command that makes the web page";
  await app.waitUntil(() => app.openedUrls().length > 0 || app.shows(noViewer), { timeout: 10000 });
  if (app.shows(noViewer)) {
    // a build without the viewer (CI's test job) copies the command that makes the page instead
    expect(app.openedUrls()).toEqual([]);
    expect(app.clipboard()).toContain("sonde share");
    return;
  }
  // the openUrl capability lets the app open its own page (http) and nothing else
  expect(app.openedUrls()[0]).toMatch(/^http:\/\/127\.0\.0\.1:\d+\/$/);
});

test("relative times read in the app's language", async () => {
  const app = await demo();
  // a job of sonde's own: a bucket's multipart scan, listed with its age
  await place(app, "multipart");
  await app.waitFor("id:lifecycle-verdict", { timeout: 10000 });
  await place(app, "jobs");
  await app.waitUntil(() => app.texts().some((x) => /^(\d+ (min|h|d)|now)$/.test(x)), { timeout: 5000 });
  await place(app, "settings");
  await app.press("label:Language");
  await app.press("text:中文");
  await app.waitFor("text:通用");
  await app.keyboard.press("mod+w");
  await place(app, "jobs");
  // its age, in Chinese, and no longer in English
  await app.waitUntil(() => app.texts().some((x) => /^(\d+ (分钟|小时|天)|刚刚)$/.test(x)), { timeout: 5000 });
  expect(app.texts().some((x) => /^(\d+ (min|h|d)|now)$/.test(x))).toBe(false);
  await place(app, "settings");
  await app.press("label:语言");
  await app.press("text:English");
  await app.waitFor("text:General");
});


test("programs: each with its runs at a glance; one opens on its runs", async () => {
  const app = await open("../App.tsx", desktop);
  await place(app, "sessions");
  await app.press("id:open-example");
  await app.waitFor("id:session-summary", { timeout: 10000 });
  await app.press("label:Programs");
  await app.waitFor("id:program-list", { timeout: 10000 });
  expect(app.shows("text:gen-slatedb")).toBe(true);
  await app.press("id:program-row >> nth=0");
  await app.waitFor("id:history-pane", { timeout: 10000 });
});

test("run again: the program after a change, compared with the run before", async () => {
  const app = await demo();
  await app.type("id:run-command", "curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY $AWS_ENDPOINT_URL/demo/README.md -o /dev/null");
  // (the button follows the field a frame later)
  await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
  await app.press("id:run-start");
  await app.waitFor("id:run-ended", { timeout: 20000 });
  await runAction(app, "run-open-session");
  await app.waitFor("id:run-again", { timeout: 10000 });
  await app.press("id:run-again");
  // it ran, and opened compared with this one: the same program, so findings compare too
  await app.waitFor("id:compare", { timeout: 30000 });
  await app.waitFor("id:run-change", { timeout: 10000 });
  expect(app.shows("id:other-program")).toBe(false);
});

test("a run tried under faults: said before it runs, followed as it goes, its trial opened and marked as one", async () => {
  const app = await demo();
  await app.type("id:run-command", "curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY -X PUT --data hello $AWS_ENDPOINT_URL/demo/trial.txt -o /dev/null");
  // (the button follows the field a frame later)
  await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
  await app.press("id:run-start");
  await app.waitFor("id:run-ended", { timeout: 20000 });
  // read-only: the plan is said, and nothing runs
  if (app.shows("label:Writes enabled")) {
    await place(app, "settings");
    await app.press("id:read-only");
    await app.waitFor("label:Read-only");
    await place(app, "run");
  }
  await runAction(app, "run-try");
  await app.waitFor("id:trial-plan", { timeout: 10000 });
  // what the run did chooses the scenarios: the two every program meets, and (it writes) the lost answer
  expect(app.shows("id:trial-scenario-slowdown")).toBe(true);
  expect(app.shows("id:trial-scenario-error")).toBe(true);
  expect(app.shows("id:trial-scenario-lost-answer")).toBe(true);
  expect(app.shows("id:trial-scenario-fail-complete")).toBe(false);
  // the command names no {trial}: the runs write where the clean run wrote
  expect(app.shows("id:trial-shares")).toBe(true);
  expect(app.shows("id:trial-read-only")).toBe(true);
  expect(app.shows("label:Run 3 trials")).toBe(true);
  await app.press("id:trial-start").catch(() => {});
  expect(app.shows("id:trial-progress")).toBe(false);
  await app.press("id:trial-close");
  // writes allowed: one scenario taken off, the others run
  await place(app, "settings");
  await app.press("id:read-only");
  await app.waitFor("label:Writes enabled");
  await place(app, "run");
  await runAction(app, "run-try");
  await app.waitFor("id:trial-plan", { timeout: 10000 });
  expect(app.shows("id:trial-read-only")).toBe(false);
  await app.press("id:trial-scenario-error");
  await app.waitFor("label:Run 2 trials");
  await app.press("id:trial-start");
  await app.waitFor("id:trial-progress", { timeout: 10000 });
  expect(app.shows("id:trial-step-error")).toBe(false);
  await app.waitFor("text:/^How the program stood/", { timeout: 60000 });
  expect(app.shows("id:trial-step-slowdown")).toBe(true);
  expect(app.shows("id:trial-step-lost-answer")).toBe(true);
  expect(app.shows("text:Waiting")).toBe(false);
  // closed, the run opens on its Runs pane, where the verdicts stay
  await app.press("id:trial-close");
  await app.waitFor("id:history-trials", { timeout: 10000 });
  // a trial's row opens its own session (it is in no list), marked as a trial
  await app.press("id:trial-slowdown");
  await app.waitFor("id:trial-note", { timeout: 10000 });
  expect(app.shows("text:/^A trial of curl under “The store asks to slow down”/")).toBe(true);
  // no run of its own: not run again, no Runs of its own
  expect(app.shows("id:run-again")).toBe(false);
  // (its tab stays in its place, dimmed: tabs do not come and go by session)
  expect(app.enabled("id:session-pane-mode-runs")).toBe(false);
  // its injected request is marked where requests are listed, and in full
  await app.press("id:session-pane-mode-operations");
  await app.press("text:/^PutObject demo\\/trial.txt/");
  await app.waitFor("id:operation-detail");
  expect(app.shows("id:op-request-injected")).toBe(true);
  await app.press("id:op-request-0");
  await app.waitFor("id:record-injected");
  await app.press("id:inspector-close");
  // and the way back to the run it is a trial of
  await app.press("id:trial-open-clean");
  await app.waitFor("id:trial-note", { state: "gone", timeout: 10000 });
  await app.waitUntil(() => app.shows("id:session-pane-mode-runs") && app.enabled("id:session-pane-mode-runs"), { timeout: 10000 });
  expect(app.shows("id:trial-note")).toBe(false);
}, 120000);

test("quitting ends the program being recorded and leaves its session a finished file", async () => {
  const app = await demo();
  // a request, then a program that would outlive the app
  await app.type("id:run-command", "curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY $AWS_ENDPOINT_URL/demo/README.md -o /dev/null; echo fetched; sleep 60");
  await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
  await app.press("id:run-start");
  await app.waitFor("id:run-terminal", { timeout: 10000 });
  await app.waitFor("text:/GetObject demo\\/README\\.md/", { timeout: 10000 });
  expect(app.shows("id:run-ended")).toBe(false);
  // what `Application.on("quit")` runs: the system waits 200 ms for it
  const { api } = await import("../src/native");
  const began = Date.now();
  const ended = await api.sessions.quit();
  const took = Date.now() - began;
  expect(ended.program).toBe(true);
  expect(ended.trial).toBe(false);
  expect(typeof ended.session).toBe("string");
  expect(took < 200).toBe(true);
  // the program is gone, and the session is a finished one: nothing is still recording
  await app.waitFor("id:run-ended", { timeout: 5000 });
  const kept = api.sessions.list().find((s) => s.path === ended.session);
  expect(kept?.live).toBe(false);
  expect(kept?.requests).toBe(1);
  // nothing left to end: quitting again says so
  const again = await api.sessions.quit();
  expect(again.program).toBe(false);
  expect(again.session).toBe(null);
});
