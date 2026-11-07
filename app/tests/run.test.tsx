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
  await app.type("id:run-command", "echo fetching; curl -s $AWS_ENDPOINT_URL/demo/README.md -o /dev/null; echo done");
  await app.press("id:run-start");
  await app.waitFor("id:run-terminal", { timeout: 10000 });
  await app.waitFor("id:run-ended", { timeout: 20000 });
  expect(app.shows("text:exited with 0")).toBe(true);
  await app.waitFor("text:/GetObject demo\\/README\\.md/", { timeout: 5000 });
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

test("record without a program: a local endpoint to point an application at", async () => {
  const app = await demo();
  await app.press("id:record");
  await app.waitFor("id:record-form");
  await app.press("label:No deletes");
  await app.press("id:record-start");
  await app.waitFor("id:recording", { timeout: 10000 });
  expect(app.shows("text:/Recording on http:\\/\\/127\\.0\\.0\\.1:/")).toBe(true);
  expect(app.shows("text:/export AWS_ENDPOINT_URL=http/")).toBe(true);
  expect(app.shows("text:/AWS_SECRET_ACCESS_KEY=sonde-proxy-local-secret/")).toBe(true);
  await app.press("id:record-stop");
  await app.waitFor("id:record");
});

test("an example session reads as operations, each opening on its requests", async () => {
  const app = await open("../App.tsx", desktop);
  await app.press("id:open-example");
  await app.waitFor("id:session-summary", { timeout: 10000 });
  expect(app.shows("text:/172 requests · 34 operations/")).toBe(true);
  expect(app.shows("text:/116 not found/")).toBe(true);
  expect(app.shows("text:/27 SlateDB writes: conditional create/")).toBe(true);
  expect(app.shows("text:looked for the newest WAL: probed ids 1…128")).toBe(true);
  await app.press("text:manifest 3 written");
  await app.waitFor("id:operation-detail");
  expect(app.shows("text:/^requests \\(5\\)$/i")).toBe(true);
  await app.press("id:op-request-4");
  await app.waitFor("id:record-detail");
  expect(app.shows("text:if-none-match")).toBe(true);
});
