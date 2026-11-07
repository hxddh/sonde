// The shared-session viewer on a real delta-rs session (sample.json, from
// `sonde share --json`).

import { expect, open, test } from "nsh/test";

test("a shared session reads as in the app: operations, a request, the waterfall", async () => {
  const app = await open("../Demo.tsx", { width: 1280, height: 800 });
  await app.waitFor("id:viewer");
  expect(app.shows("text:/49 requests · 5 operations/")).toBe(true);
  expect(app.shows("text:/4 Delta writes: conditional create/")).toBe(true);
  await app.press("text:/^commit 2/");
  await app.waitFor("id:operation-detail");
  await app.press("id:op-request-0");
  await app.waitFor("id:record-detail");
  expect(app.shows("text:x-amz-request-id")).toBe(true);
  await app.press("label:Waterfall");
  await app.waitFor("id:waterfall");
});
