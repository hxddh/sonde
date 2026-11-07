// The desktop form, headless (`platform: "macos"`, a window-sized viewport).

import { expect, open, test } from "nsh/test";

const desktop = { platform: "macos" as const, width: 1280, height: 820 };

test("starts on the timeline; the demo opens from endpoints", async () => {
  const app = await open("../App.tsx", desktop);
  expect(app.shows("id:screen-timeline")).toBe(true);
  await app.press("id:nav-endpoints");
  await app.press("id:open-demo");
  await app.waitFor("text:Demo (built-in)", { timeout: 10000 });
  await app.press("id:test-demo");
  await app.waitFor("id:test-result", { timeout: 10000 });
  expect(app.texts().some((t) => t.startsWith("Reachable in"))).toBe(true);
});

test("browses the demo bucket and shows object details", async () => {
  const app = await open("../App.tsx", desktop);
  await app.press("id:nav-endpoints");
  await app.press("id:open-demo");
  await app.waitFor("id:bucket-demo", { timeout: 10000 });
  await app.press("id:bucket-demo");
  await app.waitFor("id:screen-browse");
  await app.waitFor("text:README.md", { timeout: 10000 });
  await app.press("text:README.md");
  await app.waitFor("id:object-detail");
  await app.waitFor("text:Single-part upload (MD5 of the body)", { timeout: 10000 });
  expect(app.shows("text:x-amz-request-id")).toBe(true);
});
