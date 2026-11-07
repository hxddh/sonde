// The Timeline: sessions, their operations, requests, and live recording.

import { expect, open, test } from "nsh/test";
import { desktop } from "./helpers";

test("an example session reads as operations, each opening on its requests", async () => {
  const app = await open("../App.tsx", desktop);
  await app.waitFor("id:screen-timeline");
  await app.press("id:open-example");
  await app.waitFor("id:session-summary", { timeout: 10000 });
  expect(app.shows("text:/172 requests · 34 operations/")).toBe(true);
  expect(app.shows("text:/116 not found/")).toBe(true);
  // said once for the session, not on every line
  expect(app.shows("text:/27 SlateDB writes: conditional create/")).toBe(true);
  expect(app.shows("text:looked for the newest WAL: probed ids 1…128")).toBe(true);
  await app.press("text:manifest 3 written");
  await app.waitFor("id:operation-detail");
  expect(app.shows("text:/^requests \\(5\\)$/i")).toBe(true);
  await app.press("id:op-request-4");
  await app.waitFor("id:record-detail");
  expect(app.shows("text:PutObject")).toBe(true);
  expect(app.shows("text:if-none-match")).toBe(true);
});

test("recording: a local endpoint, what to point the application at, stop", async () => {
  const app = await open("../App.tsx", desktop);
  await app.press("id:nav-endpoints");
  await app.press("id:open-demo");
  await app.waitFor("text:Demo (built-in)", { timeout: 10000 });
  await app.press("id:nav-timeline");
  await app.press("id:record");
  await app.waitFor("id:record-form");
  await app.press("label:No deletes");
  await app.press("id:record-start");
  await app.waitFor("id:recording", { timeout: 10000 });
  expect(app.shows("text:/Recording on http:\\/\\/127\\.0\\.0\\.1:/")).toBe(true);
  expect(app.shows("text:/export AWS_ENDPOINT_URL=http/")).toBe(true);
  expect(app.shows("text:/AWS_SECRET_ACCESS_KEY=sonde-proxy-local-secret/")).toBe(true);
  // the new session is listed while it records
  await app.waitFor("text:live", { timeout: 5000 });
  await app.press("id:record-stop");
  await app.waitFor("id:record");
});
