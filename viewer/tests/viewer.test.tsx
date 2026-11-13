// The shared-session viewer on a real delta-rs session (sample.json, from
// `sonde share`).

import { expect, open, test } from "nsh/test";

test("a shared session reads as in the app: operations and a request", async () => {
  const app = await open("../Demo.tsx", { width: 1280, height: 800 });
  await app.waitFor("id:viewer");
  expect(app.shows("label:/49 requests · 5 operations/")).toBe(true);
  expect(app.shows("text:/4 Delta writes: conditional create/")).toBe(true);
  await app.press("text:/^commit 2/");
  await app.waitFor("id:operation-detail");
  await app.press("id:op-request-0");
  await app.waitFor("id:record-detail");
  expect(app.shows("text:x-amz-request-id")).toBe(true);
  // each operation placed in time by its own column: no separate timeline
  expect(app.shows("label:Timeline")).toBe(false);
});
