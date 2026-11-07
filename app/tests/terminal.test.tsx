// The terminal form: run with `nsh test tests/terminal.test.tsx --target tui`
// (an in-memory terminal, the real Cell renderer and keyboard).

import { expect, open, test } from "nsh/test";
import { place } from "./helpers";

const term = { width: 960, height: 640 };

async function demo() {
  const app = await open("../App.tsx", term);
  await place(app, "endpoints");
  await app.press("id:open-demo");
  await app.waitFor("text:Demo (built-in)", { timeout: 10000 });
  await app.back();
  return app;
}

async function chooseBucket(app: Awaited<ReturnType<typeof open>>, name: string) {
  await app.press("label:Choose a bucket");
  await app.press(`text:${name}`);
}

test("home lists the four places; digits open them", async () => {
  const app = await open("../App.tsx", term);
  for (const name of ["Run", "Sessions", "Store", "Settings"]) expect(app.shows(`text:${name}`)).toBe(true);
  // with no endpoint yet, Store welcomes, and its demo opens the store
  await app.keyboard.press("3");
  await app.wait(300);
  if (app.shows("id:welcome")) await app.press("id:welcome-demo");
  await app.waitFor("id:screen-store", { timeout: 10000 });
  await app.press("id:tab-layouts");
  await app.waitFor("id:screen-layouts");
  await app.back();
  await app.waitFor("id:nav-run");
});

test("browse: pick a bucket, list it, open an object, go back", async () => {
  const app = await demo();
  await place(app, "browse");
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
  await place(app, "browse");
  await chooseBucket(app, "demo-large");
  await app.waitFor("text:▸ logs/", { timeout: 10000 });
  await app.press("text:▸ logs/");
  await app.waitFor("text:▸ 2026/", { timeout: 10000 });
});

test("capacity in a terminal: tables and a sentence, no canvas", async () => {
  const app = await demo();
  await place(app, "capacity");
  await chooseBucket(app, "demo");
  await app.press("id:du-scan");
  await app.waitFor("id:du-findings", { timeout: 15000 });
  expect(app.shows("id:du-classes")).toBe(false);
  expect(app.texts().some((t) => t.startsWith("STANDARD"))).toBe(true);
});

test("request inspector: list and detail in one column", async () => {
  const app = await demo();
  await place(app, "browse");
  await chooseBucket(app, "demo");
  await app.waitFor("text:README.md", { timeout: 10000 });
  await place(app, "requests");
  await app.waitFor("text:ListObjectsV2", { timeout: 5000 });
  await app.press("text:ListObjectsV2 >> nth=0");
  await app.waitFor("id:record-detail");
  expect(app.shows("id:requests")).toBe(false);
});

test("multipart and the guard", async () => {
  const app = await demo();
  await place(app, "multipart");
  await chooseBucket(app, "demo");
  await app.press("id:mpu-scan");
  await app.waitFor("text:3 incomplete uploads", { timeout: 10000 });
  await app.press("id:plan-abort");
  await app.waitFor("id:confirm-dialog");
  expect(app.enabled("id:confirm-run")).toBe(false);
});

test("sessions in a terminal: sessions › operations › requests › request", async () => {
  const app = await open("../App.tsx", term);
  await place(app, "sessions");
  await app.press("id:open-example");
  await app.waitFor("id:session-summary", { timeout: 10000 });
  expect(app.shows("text:/172 requests · 34 operations/")).toBe(true);
  await app.press("text:WAL 1 written empty (fencing marker)");
  await app.waitFor("id:operation-detail");
  await app.press("id:op-request-1");
  await app.waitFor("id:record-detail");
  await app.press("id:record-back");
  await app.waitFor("id:operation-detail");
  await app.press("id:operation-back");
  await app.waitFor("id:operations");
  await app.press("id:sessions-back");
  await app.waitFor("id:session-0");
});

test("run in a terminal: the program above, its operations below", async () => {
  const app = await demo();
  await place(app, "run");
  await app.type("id:run-command", "curl -s $AWS_ENDPOINT_URL/demo/README.md -o /dev/null");
  await app.press("id:run-start");
  await app.waitFor("id:run-ended", { timeout: 20000 });
  expect(app.shows("text:exited with 0")).toBe(true);
  await app.waitFor("text:/GetObject demo\\/README\\.md/", { timeout: 5000 });
});
