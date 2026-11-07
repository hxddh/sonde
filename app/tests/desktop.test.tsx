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
