// The plugin against a host played by the test: sonde.read.* answered
// with a small Delta table.

import { expect, open, test } from "nsh/test";

const enc = (s: string) => btoa(s);
const log0 = [
  JSON.stringify({ protocol: { minReaderVersion: 1, minWriterVersion: 2 } }),
  JSON.stringify({ metaData: { id: "t", schemaString: "{}" } }),
  JSON.stringify({ add: { path: "part-0.parquet", size: 100 } }),
  JSON.stringify({ commitInfo: { timestamp: 1700000000000, operation: "CREATE TABLE" } }),
].join("\n");
const log1 = [JSON.stringify({ add: { path: "part-1.parquet", size: 50 } }), JSON.stringify({ remove: { path: "part-0.parquet" } }), JSON.stringify({ commitInfo: { timestamp: 1700000100000, operation: "WRITE" } })].join("\n");
const files: Record<string, string> = {
  "t/_delta_log/00000000000000000000.json": log0,
  "t/_delta_log/00000000000000000001.json": log1,
};

const handlers = {
  "sonde.read.context": { bucket: "lake", prefix: "t/" },
  "sonde.read.list": Object.keys(files).map((key) => ({ key, size: files[key].length })).concat([{ key: "t/part-1.parquet", size: 50 }]),
  "sonde.read.get": ({ key }: { key: string }) => ({ base64: enc(files[key]) }),
};

test("replays the log", async () => {
  const app = await open("./App.tsx", { host: { handlers } });
  await app.waitFor("text:Version 1: 1 files");
  expect(app.shows("text:part-1.parquet  50 B")).toBe(true);
  await app.press("label:version 0");
  await app.waitFor("text:Version 0: 1 files");
  expect(app.shows("text:part-0.parquet  100 B")).toBe(true);
  expect(app.host.calls().every((c) => c.name.startsWith("sonde.read."))).toBe(true);
});
