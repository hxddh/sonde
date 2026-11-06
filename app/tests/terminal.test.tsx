// The terminal form: run with `nsh test tests/terminal.test.tsx --target tui`
// (an in-memory terminal, the real Cell renderer and keyboard).

import { expect, open, test } from "nsh/test";

const term = { width: 960, height: 640 };

async function demo() {
  const app = await open("../App.tsx", term);
  await app.press("id:nav-endpoints");
  await app.press("id:open-demo");
  await app.waitFor("text:Demo (built-in)", { timeout: 10000 });
  await app.back();
  return app;
}

async function chooseBucket(app: Awaited<ReturnType<typeof open>>, name: string) {
  await app.press("label:Choose a bucket");
  await app.press(`text:${name}`);
}

test("home lists the sections; digits open them", async () => {
  const app = await open("../App.tsx", term);
  expect(app.shows("text:Endpoints")).toBe(true);
  expect(app.shows("text:Compatibility")).toBe(true);
  await app.keyboard.press("8");
  await app.waitFor("id:screen-layouts");
  await app.back();
  await app.waitFor("id:nav-browse");
});

test("browse: pick a bucket, list it, open an object, go back", async () => {
  const app = await demo();
  await app.press("id:nav-browse");
  await app.waitFor("id:screen-browse");
  await chooseBucket(app, "demo");
  await app.waitFor("text:README.md", { timeout: 10000 });
  await app.press("text:README.md");
  await app.waitFor("id:object-detail");
  expect(app.shows("text:Single-part upload (MD5 of the body)")).toBe(true);
  await app.press("id:detail-back");
  await app.waitFor("id:objects");
});

test("large listing: 100 000 keys load page by page", async () => {
  const app = await demo();
  await app.press("id:nav-browse");
  await chooseBucket(app, "demo-large");
  await app.waitFor("text:▸ logs/", { timeout: 10000 });
  await app.press("text:▸ logs/");
  await app.waitFor("text:▸ 2026/", { timeout: 10000 });
});

test("capacity in a terminal: tables and a sentence, no canvas", async () => {
  const app = await demo();
  await app.press("id:nav-capacity");
  await chooseBucket(app, "demo");
  await app.press("id:du-scan");
  await app.waitFor("id:du-findings", { timeout: 15000 });
  expect(app.shows("id:du-classes")).toBe(false);
  expect(app.texts().some((t) => t.startsWith("STANDARD"))).toBe(true);
});

test("request inspector: list and detail in one column", async () => {
  const app = await demo();
  await app.press("id:nav-browse");
  await chooseBucket(app, "demo");
  await app.waitFor("text:README.md", { timeout: 10000 });
  await app.back();
  await app.press("id:nav-requests");
  await app.waitFor("text:ListObjectsV2", { timeout: 5000 });
  await app.press("text:ListObjectsV2 >> nth=0");
  await app.waitFor("id:record-detail");
  expect(app.shows("id:requests")).toBe(false);
});

test("multipart and the guard", async () => {
  const app = await demo();
  await app.press("id:nav-multipart");
  await chooseBucket(app, "demo");
  await app.press("id:mpu-scan");
  await app.waitFor("text:3 incomplete uploads", { timeout: 10000 });
  await app.press("id:plan-abort");
  await app.waitFor("id:confirm-dialog");
  expect(app.enabled("id:confirm-run")).toBe(false);
});
