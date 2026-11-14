// The shared-session viewer on a real delta-rs session (sample.json, from
// `sonde share`).

import { expect, open, test } from "nsh/test";

test("a shared session reads as in the app: operations and a request", async () => {
  const app = await open("../Demo.tsx", { width: 1280, height: 800 });
  await app.waitFor("id:viewer");
  // the run's figures as the app shows them: its signals, each a tile
  expect(app.shows("id:session-summary")).toBe(true);
  expect(app.shows("text:in 171 ms")).toBe(true);
  expect(app.shows("text:/conditional create \\(If-None-Match/")).toBe(true);
  await app.press("text:/^commit 2/");
  await app.waitFor("id:operation-detail");
  await app.press("id:op-request-0");
  await app.waitFor("id:record-detail");
  expect(app.shows("text:x-amz-request-id")).toBe(true);
  // each operation placed in time by its own column: no separate timeline
  expect(app.shows("label:Timeline")).toBe(false);
});

test("a shared page has the app's Time pane: its charts, titles and sentences", async () => {
  const app = await open("../Demo.tsx", { width: 1280, height: 800 });
  await app.waitFor("id:viewer");
  await app.press("id:session-pane-mode-time");
  await app.waitFor("id:time-pane");
  await app.waitFor("id:chart-timeline");
  expect(app.shows("text:When were requests sent?")).toBe(true);
  expect(app.shows("text:/^49 requests, the most at \\+\\d+ ms: \\d+ in [\\d.]+ ms$/")).toBe(true);
  expect(app.shows("id:latency-heat-chart")).toBe(true);
});
