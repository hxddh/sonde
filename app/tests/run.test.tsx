// Runs: a program run from its command bar, its run the newest in the list,
// on its Output: its terminal and then its output lined up beside its operations.

import { expect, open, test } from "nsh/test";
import { elsewhereCommand, importRun, language, openDemo, openExample, desktop, place, reach, runCommand, runText, runView, runAction, runMenu } from "./helpers";

async function demo() {
  const app = await open("../App.tsx", desktop);
  await place(app, "endpoints");
  await openDemo(app);
  await app.waitFor("text:Demo (built-in)", { timeout: 10000 });
  await place(app, "run");
  return app;
}

test("run a command: its requests become operations; once it ends, its Output lines them up with its output", async () => {
  const app = await demo();
  await app.type("id:run-command", "echo fetching; curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY $AWS_ENDPOINT_URL/demo/README.md -o /dev/null; echo done");
  // (the button follows the field a frame later)
  await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
  await app.press("id:run-start");
  // the run is the newest in the list and opens on its Output; once it ended, its output beside its operations
  // (an exit of 0 is no news: nothing said)
  await runView(app);
  expect(app.shows("id:screen-runs")).toBe(true);
  await app.waitFor("id:output", { timeout: 15000 });
  expect(app.shows("id:run-exit")).toBe(false);
  expect(app.shows("text:/GetObject demo\\/README\\.md/")).toBe(true);
  // no command bar over a run: its header's Run again is the one primary, the bar a press away (New run)
  expect(app.shows("id:run-command")).toBe(false);
  await app.waitFor("id:run-again");
  // the toolbar does not say again where the window is (the sidebar and the run's title do)
  expect(app.shows("id:crumb-place")).toBe(false);
  expect(app.shows("text:fetching")).toBe(true);
  expect(app.shows("text:done")).toBe(true);
  // a line printed before the request points at the operation after it
  await app.press("text:fetching");
  await app.waitFor("id:operation-detail");
  expect(app.shows("text:/GetObject/")).toBe(true);
});

test("a program that runs a while is read as it goes: its terminal beside its operations, Stop its primary", async () => {
  const app = await demo();
  const get = "curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY -o /dev/null $AWS_ENDPOINT_URL/demo/README.md";
  await app.type("id:run-command", `for i in $(seq 30); do ${get}; sleep 0.2; done`);
  await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
  await app.press("id:run-start");
  // still running: its run's own view on its Output, the program's terminal beside its operations, read again as
  // they arrive
  await app.waitFor("id:run-terminal", { timeout: 15000 });
  await app.waitFor("id:run-summary", { timeout: 10000 });
  expect(app.shows("id:run-stop")).toBe(true);
  expect(app.shows("id:run-again")).toBe(false);
  // the newest row of the list, said live
  expect(app.shows("id:run-0 >> label:Live")).toBe(true);
  // Operations, while it runs: the strip over what stands out and the operations, as on every run
  await app.press("id:run-pane-mode-operations");
  await app.waitFor("id:run-shape", { timeout: 10000 });
  expect(app.shows("id:run-terminal")).toBe(false);
  // another program, while this one runs: Run says why it cannot
  await app.press("id:nav-runs");
  await app.waitFor("id:run-command");
  await app.press("id:run-start");
  await app.waitFor("text:A program is running: stop it to run another", { timeout: 3000 });
  await app.press("id:run-0");
  // ended: Run again in Stop's place; it printed nothing, so it has no Output to stay on: its operations, with the strip
  await app.waitFor("id:run-stop", { state: "gone", timeout: 30000 });
  await app.waitFor("id:run-again", { timeout: 10000 });
  expect(app.shows("id:run-terminal")).toBe(false);
  expect(app.shows("id:output")).toBe(false);
  expect(app.enabled("id:run-pane-mode-output")).toBe(false);
  expect(app.shows("id:run-shape")).toBe(true);
});

test("a program that sends sonde nothing: how each SDK is pointed at it", async () => {
  const app = await demo();
  await app.type("id:run-command", "echo nothing to store");
  // (the button follows the field a frame later)
  await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
  await app.press("id:run-start");
  await app.waitFor("id:run-no-requests", { timeout: 20000 });
  expect(app.shows("text:The program sent sonde no requests")).toBe(true);
  expect(app.shows("text:boto3 / botocore")).toBe(true);
  // not kept: said, and the command bar takes its place, still saying why
  await app.waitFor("text:/will not be kept/", { timeout: 5000 });
  await app.wait(9000);
  expect(app.shows("id:run-pane")).toBe(false);
  expect(app.shows("id:run-command")).toBe(true);
  expect(app.shows("id:run-no-requests")).toBe(true);
});

test("record without a program: a local endpoint to point an application at", async () => {
  const app = await demo();
  // what it may do: the command bar's ⋯, said at the field's start once it is not the usual
  await app.press("id:run-options");
  await app.chooseMenuItem("Block deletes");
  await app.press("id:record");
  await app.waitFor("id:record-form");
  await app.press("id:record-start");
  await app.waitFor("id:recording", { timeout: 10000 });
  expect(app.shows("text:/Recording on http:\\/\\/127\\.0\\.0\\.1:/")).toBe(true);
  expect(app.shows("text:/export AWS_ENDPOINT_URL=http/")).toBe(true);
  // a secret made for this recording, not a shared one
  expect(app.shows("text:/AWS_SECRET_ACCESS_KEY=[0-9a-f]{64}/")).toBe(true);
  await app.press("id:record-stop");
  await app.waitFor("id:record");
});

test("an example run reads as operations, each opening on its requests", async () => {
  const app = await open("../App.tsx", desktop);
  await openExample(app);
  await app.waitFor("id:run-summary", { timeout: 10000 });
  expect(app.shows("label:/172 requests · 34 operations/")).toBe(true);
  expect(app.shows("label:/116 not found/")).toBe(true);
  expect(app.shows("text:looked for the newest WAL: probed ids 1–128")).toBe(true);
  // nothing to act on: SlateDB rewrites what it lists and probes keys that are not there, so no copy kept would
  // answer as the store does (the findings and their fixes are the fix loop's test, on a run that has one)
  expect(app.shows("id:observation-0")).toBe(false);
  await app.press("text:manifest 3 written");
  await app.waitFor("id:operation-detail");
  expect(app.shows("text:/^requests · 5$/i")).toBe(true);
  // the note most SlateDB writes share is left out of the table's rows, and said in full in an operation's inspector
  expect(app.shows("id:ops-shared")).toBe(false);
  expect(app.shows("text:/conditional create \\(If-None-Match: \\*\\)/")).toBe(true);
  await app.press("id:op-request-4");
  await app.waitFor("id:record-detail");
  expect(app.shows("text:if-none-match")).toBe(true);
  // what stands out narrows the operations to those it is about
  await app.press("id:inspector-close");
  // the request's sections fold away as the inspector goes
  await app.waitUntil(() => !app.shows("id:inspector"), { timeout: 2000 });
  // a stretch of time is the run's one selection: chosen on the Time pane (here by a link), the Operations
  // strip's too, one chip on both (a headless pointer has no place over a plot to pick a slice by)
  const { api } = await import("../src/native");
  const example = api.runs.list().find((s) => (s.command ?? "").includes("gen-slatedb"))!;
  await app.openWithUrl(`sonde://run/${encodeURIComponent(example.path.split(/[\\/]/).pop()!)}?range=0-200`);
  await app.waitFor("id:time-pane", { timeout: 10000 });
  await app.waitFor("text:/ · in \\+.* – \\+200\u00a0ms$/", { timeout: 5000 });
  await app.press("id:run-pane-mode-operations");
  await app.waitFor("id:operations");
  await app.waitFor("text:/^\\+.* – \\+.* · \\d+ of 34 operations$/", { timeout: 5000 });
  await app.press("id:clear-filter");
  expect(app.shows("id:clear-filter")).toBe(false);
});

test("a run as a timeline, replayed from the app, and the answers compared", async () => {
  const app = await demo();
  await app.type("id:run-command", "curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY -X PUT -H 'If-None-Match: *' --data hello $AWS_ENDPOINT_URL/demo/t1.txt -o /dev/null; curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY $AWS_ENDPOINT_URL/demo/t1.txt");
  // (the button follows the field a frame later)
  await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
  await app.press("id:run-start");
  await runView(app);
  // where each operation fell in time is the operations' own When column
  expect(app.shows("id:run-pane-mode-timeline")).toBe(false);
  // on another store: one item of the run's ⋯
  await runAction(app, "On another store…");
  await app.waitFor("id:replay-plan", { timeout: 5000 });
  expect(app.shows("text:/2 requests \\(1 write, 0 deletes\\) under replay-/")).toBe(true);
  // and, said before it runs, the probe of what it relies on (a write on a condition)
  expect(app.shows("text:/^Then 1 probe of what it relies on/")).toBe(true);
  await app.press("id:replay-run");
  await app.waitFor("id:compare", { timeout: 60000 });
  await app.waitFor("text:2 requests compared: 2 the same");
  // what it relies on, and how that store answered; each store's request cost
  await app.waitFor("id:compare-relies");
  expect(app.shows("text:/^What curl relies on, and how .* answers$/")).toBe(true);
  expect(app.shows("text:/^Request cost: \\S+ USD.* on .+ → \\S+ USD/")).toBe(true);
  // one view: the answers, before and after, and the transcripts on demand
  await app.waitFor("id:run-change", { timeout: 10000 });
  // no phrase is shown with its placeholders unfilled
  expect(app.texts().filter((x) => x.includes("%{"))).toEqual([]);
  expect(app.shows("id:compare-diff")).toBe(false);
  await app.press("id:compare-transcripts");
  await app.waitFor("id:compare-diff");
  // the same workload's latency on both stores, kind by kind
  expect(app.shows("id:store-latency")).toBe(true);
  expect(app.shows("text:Were the same requests slower on one store?")).toBe(true);
});

test("runs are looked after: renamed, found, deleted, dropped on the window", async () => {
  const app = await open("../App.tsx", desktop);
  await place(app, "runs");
  // Runs opens on its command bar: no run chosen yet
  await app.waitFor("id:run-command", { timeout: 10000 });
  // each row says where its run went and how many requests it sent
  // (one line: how many first, where last)
  expect(app.shows("id:run-0 >> text:/^\\d[\\d,]* requests? · (.* · )?demo$/")).toBe(true);
  // a title of the user's, from the row's context menu
  await app.rightClick("id:run-0");
  expect(app.menu()!.map((m) => m.label)).toEqual(["Open in a window", "Share as a web page", "Copy file path", "Rename…", "Delete…"]);
  await app.chooseMenuItem("Rename…");
  await app.waitFor("id:rename-dialog");
  await app.clear("id:rename-input");
  await app.type("id:rename-input", "Nightly compaction");
  await app.press("id:rename-save");
  await app.waitFor("text:Nightly compaction");
  // found by its title in the palette (⌘K): the sidebar has no search field of its own
  expect(app.shows("id:run-search")).toBe(false);
  await app.press("id:palette");
  await app.type("id:palette-command", "nightly");
  await app.waitFor("id:palette-dialog >> text:Nightly compaction");
  await app.keyboard.press("escape");
  // its path, for dropping it back on the window
  await app.rightClick("id:run-0");
  await app.chooseMenuItem("Copy file path");
  const path = app.clipboard()!;
  expect(path.endsWith(".sonde")).toBe(true);
  // deleted, after asking
  const asked: string[] = [];
  app.onDialog((d) => (asked.push(d.title ?? ""), "Delete"));
  await app.rightClick("id:run-1");
  await app.chooseMenuItem("Delete…");
  await app.waitFor("text:/^Deleted “/");
  expect(asked[0]).toMatch(/^Delete “/);
  // a file dropped on the window opens; anything else is said not to be a run
  await app.dropFiles("id:shell", [`file://${path}`]);
  await app.waitFor("id:run-summary");
  await app.dropFiles("id:shell", ["file:///tmp/notes.txt"]);
  await app.waitFor("text:notes.txt is not a run file");
});

test("a run's time: where it went, and two runs before and after", async () => {
  const app = await open("../App.tsx", desktop);
  await openExample(app);
  await app.waitFor("id:run-summary", { timeout: 10000 });
  // the shape of the run beside its figures (the example's: the run shown before it goes first)
  await app.waitFor("id:run-shape", { timeout: 10000 });
  expect(app.shows("label:/^Waiting on S3 .* of .*; up to 8 requests in flight/")).toBe(true);
  // the header's figures in one line, the four signals and in flight, each said with what it counts; a press opens
  // the signal's chart on the Time pane, its detail where the pointer rests
  for (const id of ["rate", "errors", "latency", "bytes", "inFlight"]) expect(app.shows(`id:signal-${id}`)).toBe(true);
  expect(app.shows("text:172 requests")).toBe(true);
  expect(app.shows("text:/^median \\d/")).toBe(true);
  expect(app.shows("text:8 at once")).toBe(true);
  // (a run under a second has totals, not a rate: 172 requests in 467 ms are not "368 a second")
  expect(app.shows("label:/^Requests: 172, in 467\u00a0ms$/")).toBe(true);
  // what found nothing is no failure, and is said beside them
  expect(app.shows("label:/^Errors: 0%, 67% not found/")).toBe(true);
  // no tiles: no chevron, no line over the run in the header
  expect(app.shows("id:signal-rate-opens")).toBe(false);
  // where and when it ran, and its SDK: the title's tooltip and the first line of its ⋯
  expect((await runMenu(app))[0]).toMatch(/^rustfs · .* · from object_store/);
  // on Time the header is the same one line and its sentence
  await app.press("id:run-pane-mode-time");
  await app.waitFor("id:time-pane");
  expect(app.shows("id:run-story")).toBe(true);
  // the board in its groups, each chart in the one frame
  expect(app.shows("id:board-traffic")).toBe(true);
  expect(app.shows("id:time-shape")).toBe(true);
  // a signal pressed brings its chart into view (the board builds a chart as it comes into view)
  await app.press("id:signal-latency");
  await app.waitFor("id:latency-over-time");
  // no chart hides a view behind a switch: the latencies' grid, the requests by size, each a card of its own
  await reach(app, "id:latency-heat-chart");
  // the busiest kinds over time, a chart of its own (v21's, dropped in 22)
  await reach(app, "text:Which kind of request got slow, and when?");
  expect(app.shows("id:kinds-over-time")).toBe(true);
  expect(app.shows("text:/^The slowest kind: PutObject, p95 /")).toBe(true);
  // a chart's how to read opens from its title: no mark of its own beside it
  expect(app.shows("id:chart-kinds-how")).toBe(true);
  await reach(app, "id:scatter-size-chart");
  for (const id of ["latency-as", "scatter-by", "spent-by", "sizes-by", "prefix-measure"]) expect(app.shows(`id:${id}`)).toBe(false);
  await reach(app, "text:Where did the time go?");
  // its part of the waiting: requests at once count once, the kinds add up to it
  expect(app.shows("label:/^Most of the waiting: PutObject, \\d+%/")).toBe(true);
  // scrolled, the header is as it was: it does not fold
  expect(app.shows("text:/^median \\d/")).toBe(true);
  // where by prefix: one bar chart, each prefix's requests and its busiest second against S3's limit
  await reach(app, "id:prefix-use");
  expect(app.shows("label:/^Most at .*: \\d+ requests?, \\d+\\/s at its busiest$/")).toBe(true);
  // what the run cost over time: its requests, its bytes out (said as egress if read from outside the region, where
  // that is not known) and what it left stored, stacked on the one time axis
  await reach(app, "text:What did the run cost, and when?");
  expect(app.shows("id:cost-time-chart")).toBe(true);
  expect(app.shows("id:cost-over-time")).toBe(true);
  expect(app.shows("label:/^Requests .* · egress .* if read from outside the region · stored .* a month$/")).toBe(true);
  await reach(app, "id:profile-cost");
  // the operations sort by how long they took
  await app.press("id:run-pane-mode-operations");
  // compared with another run: before and after
  await app.wait(500);
  await runAction(app, "Compare…");
  // another program's runs are under their own heading, and say so once compared
  const run = app.menu()!.map((m) => m.label).find((l) => l.startsWith("Other programs > curl"))!;
  await app.chooseMenuItem(run);
  await app.waitFor("id:compare");
  await app.waitFor("id:run-change", { timeout: 10000 });
  // no tiles: the charts' sentences carry the figures before → after (the requests and the time each took first)
  await app.waitFor("id:compare-by-kind", { timeout: 10000 });
  expect(app.shows("id:compare-figures")).toBe(false);
  expect(app.shows("text:/^172 → [\\d,]+ requests, /")).toBe(true);
  expect(app.shows("text:/Request cost /")).toBe(true);
  expect(app.shows("id:compare-signals")).toBe(false);
  expect(app.shows("id:other-program")).toBe(true);
  expect(app.shows("text:/^Gone · /")).toBe(false);
});

test("a program over its runs: history, a baseline, and the flows of its requests", async () => {
  const app = await demo();
  const cmd = "curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY $AWS_ENDPOINT_URL/demo/README.md -o /dev/null; curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY $AWS_ENDPOINT_URL/demo/missing.txt -o /dev/null";
  for (let i = 0; i < 2; i++) {
    if (!app.shows("id:run-command")) await app.press("id:nav-runs");
    await app.clear("id:run-command");
    await app.type("id:run-command", cmd);
    await runCommand(app);
    await runView(app);
  }
  await app.press("id:run-pane-mode-runs");
  await app.waitFor("id:history-pane", { timeout: 10000 });
  // the runs of a stretch of days
  expect(app.shows("id:history-range")).toBe(true);
  expect(app.shows("text:/^This command over [2-9] runs$/")).toBe(true);
  expect(app.shows("id:trend-requests")).toBe(true);
  await app.press("id:history-baseline");
  // a state has its verb as its label
  await app.waitFor("label:Clear baseline");
  // where its requests went
  await app.press("id:run-pane-mode-time");
  await app.waitFor("id:time-pane");
  // how long every request took (of two: said so), and where they went, on demand
  await reach(app, "text:/^Half took .* or less; the slowest .* · of only 2 requests$/");
  await reach(app, "id:flows");
  expect(app.shows("label:/^2 requests: 50% answered/")).toBe(true);
});


test("a shared run is one file, opened by the system, never as a file: address", async () => {
  // the system's opener stood in for: what sonde asks it to open
  const opened: string[] = [];
  const app = await open("../App.tsx", { ...desktop, modules: { "sonde/native": { "Runs.openPage": async (path: string) => void opened.push(path) } } });
  await openExample(app);
  await app.waitFor("id:run-summary", { timeout: 10000 });
  await app.press("id:run-more");
  await app.chooseMenuItem("Share as a web page");
  const noViewer = "text:Copied the command that makes the web page";
  const shared = "text:The run is open in your browser";
  await app.waitUntil(() => app.shows(shared) || app.shows(noViewer), { timeout: 10000 });
  // never openUrl: it opens no file: address (Native Shell refuses it)
  expect(app.openedUrls()).toEqual([]);
  if (app.shows(noViewer)) {
    // a build without the viewer (CI's test job) copies the command that makes the page instead
    expect(app.clipboard()).toContain("sonde share");
    return;
  }
  await app.waitFor("text:/^The page is one file, .*\\.html: send it as you would the run\\.$/");
  expect(opened).toHaveLength(1);
  expect(opened[0]).toMatch(/\.html$/);
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


test("run again from a run: the program after a change, the newest run, shown as itself (Compare is its since sentence's link)", async () => {
  const app = await demo();
  await app.type("id:run-command", "curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY $AWS_ENDPOINT_URL/demo/README.md -o /dev/null");
  // (the button follows the field a frame later)
  await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
  await app.press("id:run-start");
  await runView(app);
  // the Runs place has no list of programs beside its runs: a program over its runs is a run's History
  expect(app.shows("id:runs-by")).toBe(false);
  // the run in a line: its time account without the counts its figures say, where most of it went in its tooltip
  expect(app.shows("text:/^Time .*: your program's own .* · S3 round trips [^(·]*$/")).toBe(true);
  expect(app.shows("text:/indicative/")).toBe(false);
  await app.waitFor("id:run-again", { timeout: 10000 });
  await app.press("id:run-again");
  // it ran as the newest run, and its own view stays once it ended: no comparison opened in its place
  await app.wait(500);
  await app.waitUntil(() => !app.shows("id:run-0 >> label:Live") && app.shows("id:run-again"), { timeout: 30000 });
  await app.wait(1500);
  expect(app.shows("id:compare")).toBe(false);
  expect(app.shows("id:run-pane")).toBe(true);
  expect(app.shows("id:run-0 >> label:Live")).toBe(false);
});

test("a run tried under faults: said before it runs, followed on its History as it goes, its trial opened and marked as one", async () => {
  const app = await demo();
  await app.type("id:run-command", "curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY -X PUT --data hello $AWS_ENDPOINT_URL/demo/trial.txt -o /dev/null");
  // (the button follows the field a frame later)
  await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
  await app.press("id:run-start");
  await runView(app);
  // the run's view: its ⋯ (and the palette) tries it under faults
  await runAction(app, "Try under faults…");
  await app.waitFor("id:trial-plan", { timeout: 10000 });
  // what the run did chooses the scenarios: the two every program meets, and (it writes) the lost answer
  expect(app.shows("id:trial-scenario-slowdown")).toBe(true);
  expect(app.shows("id:trial-scenario-error")).toBe(true);
  // a clock skew only for an SDK that retries it (curl does not)
  expect(app.shows("id:trial-scenario-skew")).toBe(false);
  expect(app.shows("id:trial-scenario-lost-answer")).toBe(true);
  expect(app.shows("id:trial-scenario-fail-complete")).toBe(false);
  expect(app.shows("label:Run 3 trials")).toBe(true);
  // one scenario taken off, the others run (the recording policy is the only limit)
  // the command names no {trial}: the runs write where the clean run wrote
  expect(app.shows("id:trial-shares")).toBe(true);
  await app.press("id:trial-scenario-error");
  await app.waitFor("label:Run 2 trials");
  await app.press("id:trial-start");
  // started, the dialog closes: the run's History follows the trial, how far it is, its Stop
  await app.waitFor("id:trial-dialog", { state: "gone", timeout: 10000 });
  await app.waitFor("id:history-trials", { timeout: 10000 });
  expect(app.shows("id:trial-error")).toBe(false);
  await app.waitUntil(() => !app.shows("id:trial-cancel"), { timeout: 60000 });
  expect(app.shows("id:trial-slowdown")).toBe(true);
  expect(app.shows("id:trial-lost-answer")).toBe(true);
  expect(app.shows("text:Waiting")).toBe(false);
  // a finished scenario, pressed: its own run (it is in no list), in Runs, marked as a trial (a row opens once the
  // whole trial has ended and its verdicts are kept)
  await app.waitUntil(() => app.enabled("id:trial-slowdown"), { timeout: 10000 });
  await app.press("id:trial-slowdown");
  await app.waitFor("id:trial-note", { timeout: 10000 });
  expect(app.shows("text:/^A trial of curl under “The store asks to slow down”/")).toBe(true);
  // no run of its own: not run again, no Runs of its own
  expect(app.shows("id:run-again")).toBe(false);
  // (its tab stays in its place, dimmed: tabs do not come and go by run)
  expect(app.enabled("id:run-pane-mode-runs")).toBe(false);
  // its injected request is marked where requests are listed, and in full
  await app.press("id:run-pane-mode-operations");
  await app.press("text:/^PutObject demo\\/trial.txt/");
  await app.waitFor("id:operation-detail");
  expect(app.shows("id:op-request-injected")).toBe(true);
  await app.press("id:op-request-0");
  await app.waitFor("id:record-injected");
  await app.press("id:inspector-close");
  // and the way back to the run it is a trial of
  await app.press("id:trial-open-clean");
  await app.waitFor("id:trial-note", { state: "gone", timeout: 10000 });
  await app.waitUntil(() => app.shows("id:run-pane-mode-runs") && app.enabled("id:run-pane-mode-runs"), { timeout: 10000 });
  expect(app.shows("id:trial-note")).toBe(false);
  // the verdicts stay on the run's History, each a way into its run again
  await app.press("id:run-pane-mode-runs");
  await app.waitFor("id:history-trials", { timeout: 10000 });
  expect(app.shows("id:trial-slowdown")).toBe(true);
}, 120000);

test("quitting ends the program being recorded and leaves its run a finished file", async () => {
  const app = await demo();
  // a request, then a program that would outlive the app
  await app.type("id:run-command", "curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY $AWS_ENDPOINT_URL/demo/README.md -o /dev/null; echo fetched; sleep 60");
  await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
  await app.press("id:run-start");
  await app.waitFor("id:run-terminal", { timeout: 10000 });
  // (its request is answered before it prints; a moment for the record to be written)
  await app.waitFor("text:fetched", { timeout: 10000 });
  await app.wait(500);
  expect(app.shows("id:run-stop")).toBe(true);
  // what `Application.on("quit")` runs: the system waits 200 ms for it
  const { api } = await import("../src/native");
  const began = Date.now();
  const ended = await api.runs.quit();
  const took = Date.now() - began;
  expect(ended.program).toBe(true);
  expect(ended.trial).toBe(false);
  expect(typeof ended.run).toBe("string");
  expect(took < 200).toBe(true);
  // the program is gone, and the run is a finished one: nothing is still recording
  await app.waitFor("id:run-stop", { state: "gone", timeout: 5000 });
  const kept = api.runs.list().find((s) => s.path === ended.run);
  expect(kept?.live).toBe(false);
  expect(kept?.requests).toBe(1);
  // nothing left to end: quitting again says so
  const again = await api.runs.quit();
  expect(again.program).toBe(false);
  expect(again.run).toBe(null);
});

/** A command that reads one key ten times on its first run and once after: the finding it makes
 * ("read 10 times") is fixed by the second run. The marker that tells the runs apart is the test's own. */
function fixable() {
  const marker = `/tmp/sonde-fixed-${Date.now()}`;
  const get = "curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY $AWS_ENDPOINT_URL/demo/README.md -o /dev/null";
  // (a pause first: the run opens as it starts, before it has anything to compare)
  return `sleep 1; if [ -e ${marker} ]; then n=1; else n=10; touch ${marker}; fi; for i in $(seq $n); do ${get}; done`;
}

test("the fix loop: a finding fixed by the next run is said fixed, with what it saved, in its run and compared", async () => {
  const app = await demo();
  const cmd = fixable();
  for (let i = 0; i < 2; i++) {
    if (!app.shows("id:run-command")) await app.press("id:nav-runs");
    await app.clear("id:run-command");
    await app.type("id:run-command", cmd);
    await runCommand(app);
    await runView(app);
    // the first run: its finding, and no run before it to be held to
    if (i === 0) {
      await app.waitFor("text:/demo\\/README\\.md was read 10 times/", { timeout: 10000 });
      expect(app.shows("id:since-run")).toBe(false);
      // acting on it, estimated, said once: on its row (no line for all of them, for one)
      expect(app.shows("id:observation-saving-0")).toBe(true);
      expect(app.shows("id:what-if")).toBe(false);
    }
  }
  // the second: what changed since the run before, the finding fixed with what it saved
  await app.waitFor("id:since-run", { timeout: 10000 });
  // in a sentence: the finding fixed with what it saved, then the figures
  // (the one fixed, named: what it was, then what it saved)
  expect(app.shows("text:/^Since the run before: fixed: demo\\/README\\.md was read 10 times.* — [^·]*(less|more|within noise)\\)? · Requests\\s10\\s→\\s1/")).toBe(true);
  // compared: the two runs, the finding fixed with its measured saving
  await app.press("id:since-compare");
  await app.waitFor("id:run-change", { timeout: 10000 });
  await app.waitFor("id:compare-finding-0");
  expect(app.shows("id:compare-saved-0")).toBe(true);
  // the run itself says it too, and which SDK sent its requests (the first line of its ⋯)
  await app.press("id:compare-back");
  await app.waitFor("id:since-run", { timeout: 10000 });
  expect((await runMenu(app))[0]).toMatch(/from curl /);
});

test("what broke under faults is a finding of the run, its fix in the inspector; Run trials again asks the plan anew, each scenario with its history", async () => {
  const app = await demo();
  await app.type("id:run-command", "curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY -X PUT --data hello $AWS_ENDPOINT_URL/demo/again.txt -o /dev/null");
  await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
  await app.press("id:run-start");
  await runView(app);
  // the slowdown alone: curl does not retry the write it was refused, and exits 0
  const slowdownAlone = async () => {
    await runAction(app, "Try under faults…");
    await app.waitFor("id:trial-plan", { timeout: 10000 });
    for (const s of ["error", "skew", "lost-answer"]) if (app.shows(`id:trial-scenario-${s}`)) await app.press(`id:trial-scenario-${s}`);
    await app.waitFor("label:Run 1 trial");
    await app.press("id:trial-start");
    await app.waitFor("id:trial-dialog", { state: "gone", timeout: 10000 });
    await app.waitFor("id:history-trials", { timeout: 10000 });
    await app.waitUntil(() => !app.shows("id:trial-cancel"), { timeout: 60000 });
  };
  await slowdownAlone();
  await app.waitFor("label:Broke: The store asks to slow down", { timeout: 10000 });
  // a row per scenario: what to change is the trial finding's fix (in the inspector), not a row of its own here
  expect(app.shows("id:trial-advice")).toBe(false);
  // where it turned: the request it broke at
  expect(app.shows("text:/ · at PutObject \\S*again\\.txt$/")).toBe(true);
  // no dialog of results, no "Try again what broke": the plan is asked anew
  expect(app.shows("id:trial-again-here")).toBe(false);
  // what broke is among the run's findings, like any other: chosen, its fix in the inspector
  await app.press("id:run-pane-mode-operations");
  await app.waitFor("text:/^broke under slowdown: /i", { timeout: 10000 });
  await app.press("text:/^broke under slowdown: /i");
  await app.waitFor("id:inspector >> id:fix", { timeout: 10000 });
  expect(app.shows("id:fix-code")).toBe(true);
  // Run trials again, from the History: the plan again, then its history over the program's runs
  await app.press("id:run-pane-mode-runs");
  await app.waitFor("id:trial-again", { timeout: 10000 });
  await app.press("id:trial-again");
  await app.waitFor("id:trial-plan", { timeout: 10000 });
  await app.press("id:trial-close");
  await app.waitFor("id:trial-dialog", { state: "gone", timeout: 10000 });
  await slowdownAlone();
  await app.waitFor("id:trial-history-slowdown", { timeout: 10000 });
  expect(app.shows("text:/^Broke in \\d+ of \\d+ runs, since /")).toBe(true);
}, 180000);

test("why slow: Group by splits the operations with a sentence; a request's inspector shows its key's history", async () => {
  const app = await open("../App.tsx", desktop);
  await openExample(app);
  await app.waitFor("id:run-summary", { timeout: 10000 });
  // Group by is the operations' title: a press opens its choices
  await app.press("id:group-by");
  await app.chooseMenuItem("Kind of request");
  await app.waitFor("id:split-table", { timeout: 10000 });
  expect(app.shows("id:split-sentence")).toBe(true);
  expect(app.shows("id:operations")).toBe(false);
  // the Time pane's table, one component: its columns, the cost's unit once in its title
  expect(app.shows("text:Kind of request")).toBe(true);
  // (once the pane has its width: narrower, a table drops its last columns first)
  await app.waitFor("text:/^Cost \\(USD/", { timeout: 5000 });
  // a kind pressed is its operations: select, inspect, act
  await app.press("id:split-table >> text:PutObject");
  await app.waitFor("id:operations");
  await app.waitFor("text:/^PutObject · \\d+ of 34 operations$/");
  await app.press("id:clear-filter");
  await app.press("id:group-by");
  await app.chooseMenuItem("Prefix");
  await app.waitFor("id:split-table", { timeout: 10000 });
  // grouped, it says so over them; its ✕ is each operation again: the operations show
  await app.press("id:clear-group");
  await app.waitFor("id:operations");
  expect(app.shows("id:split-table")).toBe(false);
  // the hot key: every one of its requests, the one shown marked, another a press away
  await app.press("text:manifest 3 written");
  await app.waitFor("id:operation-detail");
  await app.press("id:op-request-4");
  await app.waitFor("id:record-detail");
  await app.waitFor("id:key-history", { timeout: 10000 });
  await app.press("id:key-request-0");
  await app.waitFor("id:record-detail");
});

test("the least window keeps what a run is: its command, its sentence and its figures in a line", async () => {
  const app = await open("../App.tsx", { ...desktop, width: 900, height: 560 });
  await openExample(app);
  await app.waitFor("id:run-summary", { timeout: 10000 });
  expect(app.shows("id:run-story")).toBe(true);
  for (const id of ["rate", "errors", "latency", "bytes", "inFlight"]) expect(app.shows(`id:signal-${id}`)).toBe(true);
  // History too
  await app.press("id:run-pane-mode-runs");
  await app.wait(300);
  expect(app.shows("id:run-story")).toBe(true);
});

test("a program's newest run past its baseline says so in the sidebar", async () => {
  const app = await demo();
  // one read, then (the marker there) ten: the second run is past the first, its baseline
  const marker = `/tmp/sonde-worse-${Date.now()}`;
  const get = "curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY $AWS_ENDPOINT_URL/demo/README.md -o /dev/null";
  const cmd = `if [ -e ${marker} ]; then n=10; else n=1; touch ${marker}; fi; for i in $(seq $n); do ${get}; done`;
  await app.clear("id:run-command");
  await app.type("id:run-command", cmd);
  await runCommand(app);
  await runView(app);
  await app.press("id:run-pane-mode-runs");
  await app.waitFor("id:history-pane", { timeout: 10000 });
  await app.press("id:history-baseline");
  await app.waitFor("label:Clear baseline");
  await place(app, "run");
  await app.clear("id:run-command");
  await app.type("id:run-command", cmd);
  await runCommand(app);
  await runView(app);
  await place(app, "runs");
  await app.waitFor("id:run-0 >> id:run-past", { timeout: 10000 });
  expect(app.shows("id:run-0 >> text:past its baseline")).toBe(true);
  // the run before it is the baseline: within, no mark
  expect(app.shows("id:run-1 >> id:run-past")).toBe(false);
}, 120000);

test("a run written with -o is listed and opened, and never offered Rename, Delete or Set as baseline", async () => {
  const app = await demo();
  await app.clear("id:run-command");
  await app.type("id:run-command", elsewhereCommand("python agent_elsewhere.py"));
  await runCommand(app);
  await runView(app);
  await place(app, "runs");
  await app.waitFor("text:agent_elsewhere.py", { timeout: 10000 });
  // its row's menu: opened, shared, its path copied; nothing that would fail on a file that is not sonde's
  await app.rightClick("text:agent_elsewhere.py");
  const row = app.menu()!.map((m) => m.label);
  await app.closeMenu();
  expect(row).toContain("Copy file path");
  expect(row).not.toContain("Rename…");
  expect(row).not.toContain("Delete…");
  // its own ⋯ the same
  await app.press("text:agent_elsewhere.py");
  await app.waitFor("text:$ python agent_elsewhere.py", { timeout: 10000 });
  const more = await runMenu(app);
  expect(more).toContain("Copy file path");
  expect(more).not.toContain("Rename…");
  expect(more).not.toContain("Delete…");
  // History's baseline stays, and says why it cannot be pressed
  await app.press("id:run-pane-mode-runs");
  await app.waitFor("id:history-baseline", { timeout: 10000 });
  expect(app.enabled("id:history-baseline")).toBe(false);
  expect(app.shows("label:/^Set as baseline, Written outside sonde's runs folder \\(-o\\)/")).toBe(true);
  // a run of the runs folder still has all three
  await app.rightClick("id:run-0");
  const own = app.menu()!.map((m) => m.label);
  await app.closeMenu();
  expect(own).toContain("Rename…");
  expect(own).toContain("Delete…");
}, 120000);

test("Run with no command says why on press", async () => {
  const app = await demo();
  await app.clear("id:run-command");
  expect(app.enabled("id:run-start")).toBe(true);
  await app.press("id:run-start");
  await app.waitFor("text:Type the command to run", { timeout: 3000 });
  expect(app.shows("id:run-pane")).toBe(false);
});

test("a count of thousands is grouped in the sidebar, in English and Chinese (1,500 requests)", async () => {
  const app = await open("../App.tsx", desktop);
  await importRun(app, "grouped.sonde", runText(1500, "python grouped_counts.py", "2026-01-03T09:00:00Z"));
  // (its row, among however many runs the tests before made: the run opened, its row in view)
  // (the row's line starts with its count)
  const row = (n: string) => app.texts().some((x) => x.trim().startsWith(n));
  await app.waitUntil(() => row("1,500 requests ·"), { timeout: 10000 });
  expect(row("1500 requests")).toBe(false);
  await language(app, "zh");
  try {
    await app.press("id:nav-runs");
    await app.waitUntil(() => row("1,500 个请求 ·"), { timeout: 10000 });
  } finally {
    await language(app, "en");
  }
}, 120000);

test("a run with output keeps its strip on Operations; its Output lines the output up beside the operations", async () => {
  const app = await open("../App.tsx", desktop);
  await importRun(app, "printed.sonde", runText(400, "python printed_lines.py", "2026-01-04T09:00:00Z"));
  // every Time chart is kept: the strip over the operations, on a run with output too
  await app.waitFor("id:run-shape", { timeout: 5000 });
  expect(app.shows("id:operations")).toBe(true);
  expect(app.shows("id:output")).toBe(false);
  await app.press("id:run-pane-mode-output");
  await app.waitFor("id:output", { timeout: 5000 });
  expect(app.shows("id:operations")).toBe(true);
  expect(app.shows("id:run-shape")).toBe(false);
  // an operation chosen there: its inspector from the edge, the output and the operations keep their columns
  await app.press("id:operations >> text:/^GetObject/ >> nth=0");
  await app.waitFor("id:operation-detail");
  expect(app.shows("role:dialog >> id:inspector-close")).toBe(true);
  await app.press("id:inspector-close");
  // scrolled, the header stays as it was
  await app.scroll("id:output", 1500);
  await app.wait(300);
  expect(app.shows("id:run-story") || app.shows("id:run-summary")).toBe(true);
}, 120000);

test("History in Chinese: the check's new warnings and where the runs went past the baseline, in its words", async () => {
  const app = await open("../App.tsx", desktop);
  await language(app, "zh");
  try {
    // a few reads, then many, a fifth of them throttled: the second run is past the first, and warns of what the first did not
    await importRun(app, "warn1.sonde", runText(100, "python warn_twice.py", "2026-01-05T09:00:00Z"));
    await app.press("id:run-pane-mode-runs");
    await app.waitFor("id:history-baseline", { timeout: 10000 });
    await app.press("id:history-baseline");
    await app.waitFor("label:取消基线");
    await importRun(app, "warn2.sonde", runText(1000, "python warn_twice.py", "2026-01-06T09:00:00Z", true));
    await app.press("id:run-pane-mode-runs");
    await app.waitFor("id:history-check", { timeout: 10000 });
    await app.waitFor("id:history-since", { timeout: 10000 });
    const texts = app.texts();
    // no run here has a commit: each is named by its time, never "无提交"
    const since = texts.find((t) => t.startsWith("最新一次运行"))!;
    expect(since).toBeDefined();
    expect(since).not.toMatch(/无提交/);
    expect(since).toMatch(/最后一次在基线内是 \S/);
    // the new warnings in Chinese, as the run's own findings are
    // (its figures first: requests past the baseline's, then the warnings that are new)
    const check = texts.find((t) => t.startsWith("请求数: ") && t.includes(" > "))!;
    expect(check).toBeDefined();
    expect(check).toMatch(/503/);
    expect(check).not.toMatch(/throttled|requests/);
  } finally {
    await language(app, "en");
  }
}, 120000);
