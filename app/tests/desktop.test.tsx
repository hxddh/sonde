// The desktop form, headless (`platform: "macos"`, a window-sized viewport).

import { expect, open, test } from "nsh/test";
import { desktop, place } from "./helpers";

test("a first launch welcomes; its demo makes Run ready; the sidebar hides and comes back", async () => {
  const app = await open("../App.tsx", desktop);
  expect(app.shows("id:welcome")).toBe(true);
  await app.press("id:welcome-demo");
  await app.waitFor("id:screen-run", { timeout: 10000 });
  await app.waitFor("text:The demo endpoint is ready");
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
  expect(app.shows("text:$ python gen_delta.py s3://lake/delta")).toBe(true);
  app.onPick(() => [{ name: "notes.sonde", data: encode("hello") }]);
  await app.press("id:open-file");
  await app.waitFor("text:/notes.sonde is not a sonde session/", { timeout: 5000 });
});

const SESSION = "{\"format\":\"sonde-session\",\"version\":1,\"created\":\"2026-11-07T01:09:10.816413840Z\",\"endpoint\":\"rustfs\",\"url\":\"http://127.0.0.1:9100/\",\"region\":\"us-east-1\",\"command\":\"python gen_delta.py s3://lake/delta\"}\n{\"id\":1,\"time\":\"2026-11-07T01:09:10.924307634Z\",\"endpoint\":\"adhoc\",\"operation\":\"GetObject\",\"method\":\"GET\",\"url\":\"http://127.0.0.1:9100/lake/delta/_delta_log/_last_checkpoint\",\"bucket\":\"lake\",\"key\":\"delta/_delta_log/_last_checkpoint\",\"query\":[],\"status\":404,\"latencyMs\":2.124287,\"retries\":0,\"requestId\":\"2fc6cbee-0ce0-4d54-8f25-cdcda4d2df69\",\"hostId\":null,\"requestHeaders\":[{\"name\":\"host\",\"value\":\"127.0.0.1:42459\"},{\"name\":\"x-amz-date\",\"value\":\"20261107T010910Z\"},{\"name\":\"x-amz-content-sha256\",\"value\":\"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\"},{\"name\":\"authorization\",\"value\":\"AWS4-HMAC-SHA256 Credential=\u00abredacted\u00bb/20261107/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature=\u00abredacted\u00bb\"},{\"name\":\"accept\",\"value\":\"*/*\"},{\"name\":\"user-agent\",\"value\":\"object_store/0.13.2\"}],\"responseHeaders\":[{\"name\":\"content-type\",\"value\":\"application/xml\"},{\"name\":\"x-request-id\",\"value\":\"2fc6cbee-0ce0-4d54-8f25-cdcda4d2df69\"},{\"name\":\"x-amz-request-id\",\"value\":\"2fc6cbee-0ce0-4d54-8f25-cdcda4d2df69\"},{\"name\":\"content-length\",\"value\":\"127\"},{\"name\":\"date\",\"value\":\"Sat, 07 Nov 2026 01:09:10 GMT\"}],\"requestBytes\":0,\"responseBytes\":127,\"errorBody\":\"<?xml version=\\\"1.0\\\" encoding=\\\"UTF-8\\\"?><Error><Code>NoSuchKey</Code><Message>The specified key does not exist.</Message></Error>\",\"transportError\":null,\"region\":\"us-east-1\",\"job\":null}\n";

test("a dropped file's URL becomes its path on every platform", async () => {
  const { pathOf } = await import("../src/store");
  expect(pathOf("file:///home/me/a%20b.sonde")).toBe("/home/me/a b.sonde");
  expect(pathOf("file://localhost/home/me/x.sonde")).toBe("/home/me/x.sonde");
  expect(pathOf("file:///C:/Users/me/x.sonde")).toBe("C:/Users/me/x.sonde");
  expect(pathOf("file://server/share/x.sonde")).toBe("\\\\server\\share\\x.sonde");
  expect(pathOf("file:///tmp/100%.sonde")).toBe("/tmp/100%.sonde");
});
