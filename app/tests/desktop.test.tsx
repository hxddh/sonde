// The desktop form, headless (`platform: "macos"`, a window-sized viewport).

import { expect, open, test } from "nsh/test";
import { desktop, place } from "./helpers";

test("a first launch welcomes; its demo makes Run ready; the sidebar hides and comes back", async () => {
  const app = await open("../App.tsx", desktop);
  expect(app.shows("id:welcome")).toBe(true);
  await app.press("id:welcome-demo");
  await app.waitFor("id:screen-run", { timeout: 10000 });
  await app.waitFor("text:/^The demo is ready/");
  // Run has a command ready on the demo: the first result is a press away
  expect(app.shows("text:/curl -s --aws-sigv4/")).toBe(true);
  expect(app.shows("id:sidebar")).toBe(true);
  await app.press("id:toggle-sidebar");
  expect(app.shows("id:sidebar")).toBe(false);
  await app.press("id:toggle-sidebar");
  expect(app.shows("id:sidebar")).toBe(true);
});

test("the demo opens from Settings › Endpoints too", async () => {
  const app = await open("../App.tsx", desktop);
  await place(app, "endpoints");
  await app.press("id:open-demo");
  await app.waitFor("text:Demo (built-in)", { timeout: 10000 });
  await app.press("id:test-demo");
  await app.waitFor("id:test-result", { timeout: 10000 });
  expect(app.texts().some((t) => t.startsWith("Reachable in"))).toBe(true);
  // with an endpoint, Run is Run
  await app.press("id:nav-run");
  await app.waitFor("id:screen-run");
});

test("Store: browse the demo bucket and show object details", async () => {
  const app = await open("../App.tsx", desktop);
  await place(app, "endpoints");
  await app.press("id:open-demo");
  await place(app, "browse");
  await app.waitFor("id:bucket-demo", { timeout: 10000 });
  await app.press("id:bucket-demo");
  await app.waitFor("id:screen-browse");
  await app.waitFor("text:README.md", { timeout: 10000 });
  await app.press("text:README.md");
  await app.waitFor("id:object-detail");
  await app.waitFor("text:Single-part upload (MD5 of the body)", { timeout: 10000 });
  expect(app.shows("text:x-amz-request-id")).toBe(true);
});

test("the palette goes anywhere", async () => {
  const app = await open("../App.tsx", desktop);
  await app.press("id:palette");
  await app.waitFor("id:palette-command");
  await app.type("id:palette-command", "versions");
  await app.press("text:Buckets › Versions");
  await app.waitFor("id:screen-versions");
  expect(app.shows("id:palette-command")).toBe(false);
});

test("updates: asking is the user's move; a development run is up to date", async () => {
  const app = await open("../App.tsx", desktop);
  await place(app, "settings");
  await app.waitFor("id:updates");
  // what runs: the app's own name and version (nsh.json's, read with appInfo), and that this is no release
  await app.waitFor("text:/^sonde \\d+\\.\\d+\\.\\d+ \\(development run\\) · Native Shell /");
  await app.press("id:updates-check");
  await app.waitFor("text:sonde is up to date.", { timeout: 5000 });
});

test("a session file opens from Sessions: a picked .sonde, and a refusal for anything else", async () => {
  const app = await open("../App.tsx", desktop);
  await app.press("id:nav-sessions");
  const encode = (s: string) => new TextEncoder().encode(s);
  app.onPick((p) => (p.filters[0]?.extensions.includes("sonde") ? [{ name: "elsewhere.sonde", data: encode(SESSION) }] : null));
  await app.keyboard.press("mod+o");
  await app.waitFor("id:session-pane", { timeout: 10000 });
  // the session is read off the interface's thread: its header comes when the reading ends
  await app.waitFor("text:$ python gen_delta.py s3://lake/delta", { timeout: 10000 });
  app.onPick(() => [{ name: "notes.sonde", data: encode("hello") }]);
  await app.press("id:open-file");
  await app.waitFor("text:/notes.sonde: not a sonde session/", { timeout: 5000 });
});

const SESSION = "{\"format\":\"sonde-session\",\"version\":1,\"created\":\"2026-11-07T01:09:10.816413840Z\",\"endpoint\":\"rustfs\",\"url\":\"http://127.0.0.1:9100/\",\"region\":\"us-east-1\",\"command\":\"python gen_delta.py s3://lake/delta\"}\n{\"id\":1,\"time\":\"2026-11-07T01:09:10.924307634Z\",\"endpoint\":\"adhoc\",\"operation\":\"GetObject\",\"method\":\"GET\",\"url\":\"http://127.0.0.1:9100/lake/delta/_delta_log/_last_checkpoint\",\"bucket\":\"lake\",\"key\":\"delta/_delta_log/_last_checkpoint\",\"query\":[],\"status\":404,\"latencyMs\":2.124287,\"retries\":0,\"requestId\":\"2fc6cbee-0ce0-4d54-8f25-cdcda4d2df69\",\"hostId\":null,\"requestHeaders\":[{\"name\":\"host\",\"value\":\"127.0.0.1:42459\"},{\"name\":\"x-amz-date\",\"value\":\"20261107T010910Z\"},{\"name\":\"x-amz-content-sha256\",\"value\":\"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\"},{\"name\":\"authorization\",\"value\":\"AWS4-HMAC-SHA256 Credential=\u00abredacted\u00bb/20261107/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=\u00abredacted\u00bb\"},{\"name\":\"accept\",\"value\":\"*/*\"},{\"name\":\"user-agent\",\"value\":\"object_store/0.13.2\"}],\"responseHeaders\":[{\"name\":\"content-type\",\"value\":\"application/xml\"},{\"name\":\"x-request-id\",\"value\":\"2fc6cbee-0ce0-4d54-8f25-cdcda4d2df69\"},{\"name\":\"x-amz-request-id\",\"value\":\"2fc6cbee-0ce0-4d54-8f25-cdcda4d2df69\"},{\"name\":\"content-length\",\"value\":\"127\"},{\"name\":\"date\",\"value\":\"Sat, 07 Nov 2026 01:09:10 GMT\"}],\"requestBytes\":0,\"responseBytes\":127,\"errorBody\":\"<?xml version=\\\"1.0\\\" encoding=\\\"UTF-8\\\"?><Error><Code>NoSuchKey</Code><Message>The specified key does not exist.</Message></Error>\",\"transportError\":null,\"region\":\"us-east-1\",\"job\":null}\n";

test("a second launch hands over its files: relative ones from where it was started, links and options left out", async () => {
  const { launchFiles } = await import("../src/store");
  // (its sonde:// links reach the app through UrlScheme, as every link does)
  expect(launchFiles(["--verbose", "run.sonde", "/tmp/b.sonde", "sonde://session/x.sonde"], "/home/me/etl")).toEqual(["/home/me/etl/run.sonde", "/tmp/b.sonde"]);
  expect(launchFiles(["run.sonde", "D:\\logs\\b.sonde"], "C:\\Users\\me\\")).toEqual(["C:\\Users\\me\\run.sonde", "D:\\logs\\b.sonde"]);
  expect(launchFiles(["file:///home/me/a%20b.sonde"], "/")).toEqual(["/home/me/a b.sonde"]);
  expect(launchFiles([], "/home/me")).toEqual([]);
});

test("a dropped file's URL becomes its path on every platform", async () => {
  const { pathOf } = await import("../src/store");
  expect(pathOf("file:///home/me/a%20b.sonde")).toBe("/home/me/a b.sonde");
  expect(pathOf("file://localhost/home/me/x.sonde")).toBe("/home/me/x.sonde");
  expect(pathOf("file:///C:/Users/me/x.sonde")).toBe("C:/Users/me/x.sonde");
  expect(pathOf("file://server/share/x.sonde")).toBe("\\\\server\\share\\x.sonde");
  expect(pathOf("file:///tmp/100%.sonde")).toBe("/tmp/100%.sonde");
  expect(pathOf("file:///tmp/a%20b%.sonde")).toBe("/tmp/a b%.sonde");
});

test("a command keeps its secrets out and its Windows paths whole; sonde's own sessions read as its words", async () => {
  const { commandLine, programOf, ranCommand, scrubCommand } = await import("../src/ui/sessionview");
  expect(scrubCommand("AWS_SECRET_ACCESS_KEY=abc python x.py")).toBe("AWS_SECRET_ACCESS_KEY=… python x.py");
  expect(scrubCommand("tool --password=hunter2 --region=eu")).toBe("tool --password=… --region=eu");
  expect(scrubCommand("sh -c 'TOKEN=abc python x.py'")).toBe("sh -c 'TOKEN=… python x.py'");
  expect(programOf("C:\\Python\\python.exe C:\\work\\etl.py")).not.toBeNull();
  expect(scrubCommand("C:\\Python\\python.exe C:\\work\\etl.py")).toBe("C:\\Python\\python.exe C:\\work\\etl.py");
  expect(ranCommand("python x.py")).toBe(true);
  expect(ranCommand("logs of s3://demo/logs/")).toBe(false);
  expect(ranCommand("replay of a.sonde")).toBe(false);
  expect(ranCommand("imported from CloudTrail")).toBe(false);
  expect(ranCommand("  ")).toBe(false);
  expect(commandLine("logs of s3://demo/logs/")).toBe("logs of s3://demo/logs/");
  expect(commandLine("python x.py")).toBe("$ python x.py");
});

test("a month of a program reads as money, not per 1,000 runs", async () => {
  const { money } = await import("../src/format");
  const { cost } = await import("../src/ui/plots");
  expect(money(0.004, "USD")).toBe("<0.01 USD");
  expect(cost(0.004, "USD")).not.toBe("<0.01 USD");
});

test("a lane's cell shows the worst of the requests it holds, and says how many", async () => {
  const { laneCells } = await import("../src/ui/plots");
  // 96 slices of 10 ms: three short requests share the first cell of lane 0, the failure not the last of them
  const samples = [
    { tMs: 0, ms: 5, lane: 0, status: 200 },
    { tMs: 6, ms: 2, lane: 0, status: 500 },
    { tMs: 8, ms: 1, lane: 0, status: 200 },
    { tMs: 0, ms: 25, lane: 1, status: 404 },
  ];
  const cells = laneCells(samples, [0, 960], 96, 24);
  // one cell per slice and lane, in order of lane then slice
  expect(cells.length).toBe(4);
  expect(cells[0].lane).toBe(0);
  expect(cells[0].bin).toBe(0);
  expect(cells[0].n).toBe(3);
  // the failure wins, and is the request the cell stands for
  expect(cells[0].tone).toBe("server");
  expect(cells[0].sample).toBe(1);
  // a long request is a run of cells, one request each
  expect(cells[3].lane).toBe(1);
  expect(cells[3].bin).toBe(2);
  expect(cells[3].n).toBe(1);
  expect(cells[3].tone).toBe("miss");
  // the same input gives the same cells: nothing is left to the order of drawing
  expect(JSON.stringify(laneCells(samples, [0, 960], 96, 24))).toBe(JSON.stringify(cells));
});

test("prefixes are named by what tells them apart: the shared start once, the end kept", async () => {
  const { prefixNames, tail } = await import("../src/ui/plots");
  // cut at the start, on a folder's edge: the end is what differs
  expect(tail("slate/db/compactions/", 18)).toBe("…/db/compactions/");
  expect(tail("slate/db/compacted/", 18)).toBe("…/db/compacted/");
  expect(tail("short/", 18)).toBe("short/");
  // what all share is said once; one that stands for the folders under it is marked
  const table = [
    { prefix: "lake/warehouse/db/events/data/dt=2026-11-01/" },
    { prefix: "lake/warehouse/db/events/data/", folders: 24 },
    { prefix: "lake/warehouse/db/events/metadata/" },
    { prefix: "other" },
  ];
  const n = prefixNames(table, 20);
  expect(n.common).toBe("lake/warehouse/db/events/");
  expect(n.name("lake/warehouse/db/events/data/dt=2026-11-01/")).toBe("data/dt=2026-11-01/");
  expect(n.name("lake/warehouse/db/events/data/")).toBe("data/…");
  expect(n.name("lake/warehouse/db/events/metadata/")).toBe("metadata/");
  // a prefix that is the others' parent keeps a name of its own
  const nested = prefixNames([{ prefix: "b/a/" }, { prefix: "b/a/x/" }], 20);
  expect(nested.name("b/a/")).toBe("a/");
  expect(nested.name("b/a/x/")).toBe("a/x/");
  // two never get one name
  const twins = prefixNames([{ prefix: "aaaaaaaaaaaaaaaaaaaaaaaa/same-ending/" }, { prefix: "bbbbbbbbbbbbbbbbbbbbbbbb/same-ending/" }], 16);
  expect(twins.name("aaaaaaaaaaaaaaaaaaaaaaaa/same-ending/")).not.toBe(twins.name("bbbbbbbbbbbbbbbbbbbbbbbb/same-ending/"));
});

test("a trial reads as figures: the faults and retries, the wait, the load, what went wrong", async () => {
  const { trialText } = await import("../src/ui/history");
  const base = { scenario: "slowdown", about: "", exit: 0, cleanExit: 0, attempts: 2, jitter: false, requests: 96, cleanRequests: 64, leftUploads: 0, wroteTwice: 0, droppedReads: 0, droppedWrites: 0, reasons: [] };
  expect(trialText({ ...base, standing: "survived", faults: 35, retried: 35, backoffMs: [101, 103, 105] })).toBe("35 faults, all retried · after 101 ms to 105 ms, in step · 1.5× the requests");
  expect(trialText({ ...base, standing: "broke", faults: 2, retried: 0, droppedWrites: 2, exit: 1, requests: 64 })).toBe("2 faults, 0 retried · 2 writes never sent again · exited 1 (clean run: 0)");
  // too few waits to tell jitter from step: neither is said
  const { jitter: _, ...unsaid } = base;
  expect(trialText({ ...unsaid, standing: "survived", faults: 2, retried: 2, backoffMs: [100, 150, 200], requests: 64 })).toBe("2 faults, all retried · after 100 ms to 200 ms");
  // faults no retry was owed for, writes the store made unheard, requests held back and what that did to the run's time
  expect(trialText({ ...unsaid, standing: "degraded", faults: 3, retried: 1, notRetryable: 2, lostWrites: 1, delayed: 40, wallMs: 4200, cleanWallMs: 900, requests: 64 })).toBe(
    "3 faults, 1 retried · 2 no retry was owed for · 40 requests held back · took 4.20 s (clean run: 900 ms) · 1 write made and never heard of",
  );
});

test("a trial is followed scenario by scenario: waiting, running, done with its standing", async () => {
  const { trialSteps, foldTrial, trialDone, worstStanding, scenarioLabel } = await import("../src/trials");
  let steps = trialSteps([{ name: "slowdown", about: "503 SlowDown" }, { name: "lost-answer", about: "" }]);
  expect(steps.map((s) => s.state)).toEqual(["waiting", "waiting"]);
  steps = foldTrial(steps, { scenario: "slowdown", state: "running" });
  // the program's output and its recording change nothing in the list
  steps = foldTrial(steps, { data: "hello\n", scenario: "slowdown" });
  expect(steps.map((s) => s.state)).toEqual(["running", "waiting"]);
  const trial = { scenario: "slowdown", about: "", standing: "broke", exit: 1, cleanExit: 0, faults: 1, retried: 0, droppedReads: 0, droppedWrites: 1, attempts: 1, requests: 2, cleanRequests: 2, leftUploads: 0, wroteTwice: 0, reasons: ["1 write never sent again"] } as import("../src/types").Trial;
  steps = foldTrial(steps, { scenario: "slowdown", state: "done", trial });
  // a late "running" does not take a verdict back; a scenario not chosen is left out
  steps = foldTrial(steps, { scenario: "slowdown", state: "running" });
  steps = foldTrial(steps, { scenario: "error", state: "running" });
  expect(steps.map((s) => s.state)).toEqual(["done", "waiting"]);
  expect(steps[0].trial?.standing).toBe("broke");
  expect(steps.length).toBe(2);
  expect(trialDone(steps)).toBe(1);
  expect(worstStanding([{ standing: "survived" }, { standing: "broke" }])).toBe("broke");
  expect(worstStanding([{ standing: "survived" }, { standing: "degraded" }])).toBe("degraded");
  expect(worstStanding([{ standing: "survived" }, { standing: "untested" }])).toBe(null);
  expect(worstStanding(undefined)).toBe(null);
  // a project's own scenario keeps its name
  expect(scenarioLabel("lost-answer")).toBe("A write's answer is lost");
  expect(scenarioLabel("my-own")).toBe("my-own");
});

test("prefixes with a doubled or a leading slash keep their own names", async () => {
  const { prefixNames } = await import("../src/ui/plots");
  // what they share is cut off as written: an empty segment shifts nothing
  const doubled = prefixNames([{ prefix: "a//b/x/" }, { prefix: "a//b/y/" }], 20);
  expect(doubled.common).toBe("a//b/");
  expect(doubled.name("a//b/x/")).toBe("x/");
  expect(doubled.name("a//b/y/")).toBe("y/");
  const leading = prefixNames([{ prefix: "/a/b/" }, { prefix: "/a/c/" }], 20);
  expect(leading.common).toBe("/a/");
  expect(leading.name("/a/b/")).toBe("b/");
  expect(leading.name("/a/c/")).toBe("c/");
  // slashes alone are nothing shared
  expect(prefixNames([{ prefix: "/a/" }, { prefix: "/b/" }], 20).common).toBe("");
  // the rest of them is said in words, never as the raw "other"
  expect(leading.name("other")).toBe("Other prefixes");
});

test("every chart over one stretch of time names the same round moments", async () => {
  const { timeTicks, sessionSpan } = await import("../src/ui/plots");
  // 1, 2 or 5 of a power of ten apart, three or four of them
  expect(timeTicks([0, 467])).toEqual([0, 200, 400]);
  expect(timeTicks([0, 600])).toEqual([0, 200, 400, 600]);
  expect(timeTicks([0, 60000])).toEqual([0, 20000, 40000, 60000]);
  // a zoomed stretch: round moments of the session, not offsets of the stretch
  expect(timeTicks([230, 460])).toEqual([300, 400]);
  expect(timeTicks([5, 5])).toEqual([5]);
  // slices and wall time give one span: a chart of slices and a chart of moments agree
  expect(sessionSpan({ binMs: 7.8, wallMs: 467 }, 60)).toEqual([0, 468]);
  expect(sessionSpan({ binMs: 10, wallMs: 467 }, 40)).toEqual([0, 467]);
});

test("a table that narrows drops its columns by rank, whole, and clips none", async () => {
  const { operationColumns, OP_NAME } = await import("../src/ui/sessionview");
  const all = { format: true, bar: true, notes: true };
  const sized = { bar: 150, notes: 320 };
  // not measured yet, and wide: every column, in rank order
  expect(operationColumns(0, all, sized).shown).toEqual(["status", "title", "duration", "requests", "time", "bar", "format", "notes"]);
  expect(operationColumns(1400, all, sized).shown).toEqual(["status", "title", "duration", "requests", "time", "bar", "format", "notes"]);
  // narrower: the notes go first, then the format, the bar, the time, the requests
  expect(operationColumns(900, all, sized).shown).toEqual(["status", "title", "duration", "requests", "time", "bar", "format"]);
  expect(operationColumns(700, all, sized).shown).toEqual(["status", "title", "duration", "requests", "time"]);
  expect(operationColumns(480, all, sized).shown).toEqual(["status", "title", "duration", "requests"]);
  // the outcome, the operation and how long it took always show
  expect(operationColumns(300, all, sized).shown).toEqual(["status", "title", "duration"]);
  // whatever shows, the operation keeps its room and the columns add up to the table: none is clipped
  for (const w of [460, 520, 640, 780, 900, 1100, 1400]) {
    const l = operationColumns(w, all, sized);
    const used = l.shown.filter((c) => c !== "title").reduce((n, c) => n + l.widths[c], 0);
    expect(l.title >= OP_NAME).toBe(true);
    expect(used + l.title + 2 <= w).toBe(true);
  }
  // a column that is not wanted is not a reason to drop the ones after it
  expect(operationColumns(1400, { format: false, bar: true, notes: true }, sized).shown).toEqual(["status", "title", "duration", "requests", "time", "bar", "notes"]);
});

test("tiles go in rows of equal count: four are two and two, never three and one; too narrow for two, a line each", async () => {
  const { tilesPerRow, tilesFit } = await import("../src/ui/kit");
  expect(tilesPerRow(700, 4, 144)).toBe(4);
  // room for three: two and two
  expect(tilesPerRow(500, 4, 144)).toBe(2);
  expect(tilesPerRow(320, 4, 144)).toBe(2);
  // no room for two as they read: a line each, so no label and no figure is cut
  expect(tilesPerRow(200, 4, 144)).toBe(1);
  expect(tilesPerRow(300, 4, 144)).toBe(2);
  expect(tilesPerRow(200, 1, 144)).toBe(1);
  expect(tilesPerRow(500, 5, 128)).toBe(3);
  // not measured yet: all abreast
  expect(tilesPerRow(0, 4, 144)).toBe(4);
  // a ranked row holds what fits and never wraps: the last ranked go, two stay
  expect(tilesFit(0, 5, 168)).toBe(5);
  expect(tilesFit(1000, 5, 168)).toBe(5);
  expect(tilesFit(520, 5, 168)).toBe(2);
  expect(tilesFit(100, 5, 168)).toBe(2);
});

test("runs of one command are told apart by the argument that differs", async () => {
  const { tellApart } = await import("../src/ui/sessionview");
  const long = "-s --aws-sigv4 aws:amz:us-east-1:s3 -o /dev/null http://127.0.0.1:9000/demo/";
  expect(tellApart([`${long}README.md`, `${long}missing.txt`])).toEqual(["… http://127.0.0.1:9000/demo/README.md", "… http://127.0.0.1:9000/demo/missing.txt"]);
  // the same to the letter, or different from the start: left whole
  expect(tellApart(["load.py", "load.py"])).toEqual(["load.py", "load.py"]);
  expect(tellApart(["a.py --x", "b.py --y"])).toEqual(["a.py --x", "b.py --y"]);
});

test("one function writes a duration, and a scenario has words", async () => {
  const format = await import("../src/format");
  expect(format.duration(0.42)).toBe("0.4 ms");
  expect(format.duration(840)).toBe("840 ms");
  expect(format.duration(2350)).toBe("2.35 s");
  expect(format.duration(185000)).toBe("3 min 05 s");
  // the older names are gone
  expect("ms" in format).toBe(false);
  expect(format.factor(1.5)).toBe("1.5");
  expect(format.rate(4.25)).toBe("4.3");
  expect(format.rate(120.4)).toBe("120");
  const { scenarioLabel } = await import("../src/ui/history");
  expect(scenarioLabel("slowdown")).toBe("The store asks to slow down");
  // one sonde has no words for keeps its own name
  expect(scenarioLabel("made-up")).toBe("made-up");
});

test("a run reads as four signals and in flight; what a log without times cannot say is said, never 0", async () => {
  const { signalsFromProfile, signalTiles, brief, operationStats, sizeLabel, sizeRange, nearestLimit } = await import("../src/signals");
  const bin = (tMs: number, ok: number, server = 0, retried = 0) => ({ tMs, ok, miss: 0, client: 0, server, p50Ms: 2, p95Ms: 6, p99Ms: 9, retried, bytesUp: 1024 * ok, bytesDown: 0 });
  const profile = {
    wallMs: 2000,
    binMs: 1000,
    peakInFlight: 4,
    peakAtMs: 1200,
    rate: [bin(0, 30), bin(1000, 9, 1, 2)],
    inFlight: [{ tMs: 100, n: 2 }, { tMs: 1200, n: 4 }],
    p50Ms: 2,
    p95Ms: 6,
    p99Ms: 9,
    cost: { amount: 0, currency: "USD" },
  } as never;
  const summary = { requests: 40, errors: 1, misses: 0, refused: 0, seconds: 2, bytesUp: 39936, bytesDown: 0 };
  const s = signalsFromProfile(profile, summary);
  expect(s.rate.value).toBe(20);
  expect(s.inFlight.series).toEqual([2, 4]);
  const tiles = signalTiles(s);
  expect(tiles.map((x) => x.id)).toEqual(["rate", "errors", "latency", "bytes", "inFlight"]);
  expect(tiles[0].value).toBe("20/s");
  expect(tiles[0].detail).toBe("40 requests in 2.00 s");
  expect(tiles[1].value).toBe("3%");
  expect(tiles[1].detail).toBe("1 server · 2 retries");
  expect(tiles[1].tone).toBe("bad");
  expect(tiles[2].value).toBe("2.0 ms");
  expect(tiles[2].detail).toBe("p95 6.0 ms · p99 9.0 ms");
  expect(tiles[3].value).toBe("19.5 KiB/s");
  expect(tiles[3].detail).toBe("↑ 19.5 KiB/s ↓ 0 B/s");
  expect(tiles[4].value).toBe("4");
  // a CloudTrail file has no times: no rate, no latency, no pace of bytes, and nothing drawn over time
  const untimed = signalTiles(signalsFromProfile({ ...(profile as object), timed: false } as never, summary));
  expect(untimed[0].value).toBe("Not timed");
  expect(untimed[0].detail).toBe("40 requests");
  expect(untimed[2].value).toBe("Not timed");
  expect(untimed[3].value).toBe("39.0 KiB");
  expect(untimed[3].detail).toBe("↑ 39.0 KiB ↓ 0 B");
  expect(untimed[4].value).toBe("Not timed");
  expect(untimed.every((x) => x.id === "errors" || x.series.length === 0)).toBe(true);
  // a run of a moment says totals, not rates, and draws no line: 9 requests in a third of a second
  expect(brief(s)).toBe(false);
  const few = { requests: 9, errors: 2, misses: 0, refused: 0, seconds: 0.334, bytesUp: 0, bytesDown: 3145728 };
  const moment = signalsFromProfile({ ...(profile as object), wallMs: 334, binMs: 167, peakAtMs: 0, rate: [bin(0, 5, 2), bin(167, 2)] } as never, few);
  expect(brief(moment)).toBe(true);
  const short = signalTiles(moment);
  expect(short[0].label).toBe("Requests");
  expect(short[0].value).toBe("9 requests");
  expect(short[0].detail).toBe("in 334 ms");
  expect(short[1].value).toBe("2 of 9");
  // a percentile of nine requests is one of them: said
  expect(short[2].detail).toBe("p95 6.0 ms · of only 9");
  expect(short[3].value).toBe("3.0 MiB");
  expect(short[3].detail).toBe("↑ 0 B ↓ 3.0 MiB");
  expect(short[4].detail).toBe("of 9 requests");
  expect(short.every((x) => x.series.length === 0)).toBe(true);
  // Rust's own flag wins where the signals carry it
  expect(brief({ ...s, short: true })).toBe(true);
  // the run by operation: Rust's table, or what an older share's profile holds of it
  const groups = [
    { operation: "GetObject", missed: false, requests: 30, errors: 3, p50Ms: 2, p95Ms: 6 },
    { operation: "GetObject", missed: true, requests: 10, errors: 0, p50Ms: 1, p95Ms: 2 },
    { operation: "PutObject", missed: false, requests: 5, errors: 0, p50Ms: 9, p95Ms: 12 },
  ];
  const ops = operationStats({ summary, profile: { ...(profile as object), groups } as never });
  expect(ops.map((o) => [o.operation, o.count, o.few])).toEqual([["GetObject", 40, false], ["PutObject", 5, true]]);
  expect(ops[0].errorShare).toBe(3 / 40);
  expect(ops[0].p99Ms).toBe(null);
  const own = operationStats({ summary: { ...summary, operations: [{ operation: "PutObject", count: 12, errors: 6, misses: 0, errorShare: 0.5, bytesUp: 0, bytesDown: 0, p50Ms: 1, p95Ms: 2, p99Ms: 3, maxMs: 4, few: true }] }, profile: profile as never });
  expect(own).toEqual([{ operation: "PutObject", count: 12, errorShare: 0.5, p50Ms: 1, p95Ms: 2, p99Ms: 3, few: true }]);
  // sizes by their bounds; a prefix by the nearer of its two limits
  expect(sizeLabel(0)).toBe("empty");
  expect(sizeLabel(1024)).toBe("≤ 1.0 KiB");
  expect(sizeRange(1024)).toBe("512 B – 1.0 KiB");
  expect(sizeRange(0)).toBe("empty");
  expect(nearestLimit({ readShare: 0.1, writeShare: 0.4 })).toEqual({ share: 0.4, kind: "writes" });
});
