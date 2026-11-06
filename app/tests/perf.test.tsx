// Performance: 100 000 rows in the object table, scrolled end to end.
// Not part of the regular suites; run with scripts/perf.sh (it adds --trace
// and --target tui), which writes the numbers into docs/performance.md.

import { expect, open, test } from "nsh/test";

test("100 000 rows: load and scroll", async () => {
  const app = await open("../App.tsx", { width: 960, height: 640 });
  await app.press("id:nav-endpoints");
  await app.press("id:open-demo");
  await app.waitFor("text:Demo (built-in)", { timeout: 10000 });
  await app.back();
  await app.press("id:nav-browse");
  await app.press("label:Choose a bucket");
  await app.press("text:demo-large");
  await app.waitFor("text:▸ logs/", { timeout: 10000 });
  await app.press("id:flat");
  const loadStart = Date.now();
  await app.press("id:load-all");
  await app.waitFor("text:100,000 items", { timeout: 240000 });
  const loadMs = Date.now() - loadStart;
  const scrollStart = Date.now();
  // 10 points per frame: 5 000 points is 500 frames (~310 rows of 16 points)
  await app.scroll("id:objects", 5000);
  const scrollMs = Date.now() - scrollStart;
  const back = Date.now();
  await app.scroll("id:objects", -5000);
  const backMs = Date.now() - back;
  console.log(`PERF load100k_ms=${loadMs} scroll500frames_ms=${scrollMs} frames_per_s=${(500 / (scrollMs / 1000)).toFixed(1)} back500frames_ms=${backMs}`);
  expect(app.shows("text:100,000 items")).toBe(true);
}, 300_000);
