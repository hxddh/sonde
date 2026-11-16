// Shared steps: go to a place, open the app on the demo endpoint.

import { type App, open } from "nsh/test";

export const desktop = { platform: "macos" as const, width: 1280, height: 860 };

const STORE = ["browse", "capacity", "multipart", "layouts"];
const SETTINGS = ["settings", "endpoints"];
const ACTIVITY = ["jobs", "audit"];

/** Goes to a place, or to a tab of one. A window opens Settings in a window
 * of their own (⌘,), Activity from its toolbar, and Buckets' sections from
 * the place's one row of sections; a terminal lists every place on its home
 * list. */
export async function place(app: App, where: string) {
  const area = STORE.includes(where) ? "store" : SETTINGS.includes(where) ? "settings" : ACTIVITY.includes(where) ? "activity" : where;
  const window = app.shows("id:toolbar");
  if (window && area === "settings") {
    await app.keyboard.press("mod+,");
    await app.waitFor("id:screen-settings-area");
    await app.press(`id:tab-${where}`);
    return;
  }
  // Buckets' sections, from its row of them
  if (window && area === "store") {
    // a narrow window folds the sidebar away
    if (!app.shows("id:nav-store")) await app.keyboard.press("mod+b");
    await app.press("id:nav-store");
    await app.press(`id:store-section-${where}`);
    await app.waitFor(`id:screen-${where}`);
    return;
  }
  if (window && area === "activity") {
    if (!app.shows("id:activity")) await app.press("id:activity-open");
    await app.waitFor("id:activity");
    await app.press(`id:activity-tab-${where}`);
    return;
  }
  // a terminal shows the places on its home list only
  if (!app.shows(`id:nav-${area}`)) await app.back();
  await app.press(`id:nav-${area}`);
  // a terminal pushes the place over its home list: there once the list has gone (a place
  // with much to read, a long list of runs, takes a moment)
  if (!window) await app.waitFor(`id:nav-${area}`, { state: "gone" });
  if (area === "store" || area === "settings") await app.press(`id:tab-${where}`);
  if (area === "activity") await app.press(`id:activity-tab-${where}`);
}

/** Opens the app, starts the demo, and goes to Store (selecting `bucket`). */
export async function withDemo(bucket?: string, options: Parameters<typeof open>[1] = desktop): Promise<App> {
  const app = await open("../App.tsx", options);
  await place(app, "endpoints");
  await openDemo(app);
  await app.waitFor("text:Demo (built-in)", { timeout: 10000 });
  await place(app, "browse");
  await app.waitFor("id:bucket-demo", { timeout: 10000 });
  if (bucket) await app.press(`id:bucket-${bucket}`);
  return app;
}

/** Runs the command in Run's field by the command bar's one Run. */
export async function runCommand(app: App) {
  // (the button follows the field a frame later)
  await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
  await app.press("id:run-start");
}

/** Waits for a run that ended to show its own view on Run (its header read off the interface's thread). */
export async function runView(app: App) {
  await app.waitFor("id:run-pane", { timeout: 25000 });
  await app.waitFor("id:run-summary", { timeout: 10000 });
}

/** A run's command besides its one primary (Try under faults…, Compare… and its On another store…): behind its header's `⋯`,
 * by its words. */
export async function runAction(app: App, label: string) {
  await app.press("id:run-more");
  await app.chooseMenuItem(label);
}

/** The words of the run header's `⋯` menu, the menu closed again. */
export async function runMenu(app: App): Promise<string[]> {
  await app.press("id:run-more");
  const labels = app.menu()?.map((m) => m.label) ?? [];
  await app.closeMenu();
  return labels;
}

/** The example run, from its doors: the welcome's, where it shows (no endpoint yet), else the palette's (`zh`: the
 * window is in Chinese). */
export async function openExample(app: App, zh = false) {
  if (app.shows("id:open-example")) return app.press("id:open-example");
  if (app.shows("id:toolbar")) await app.press("id:palette");
  else await app.keyboard.press("shift+;");
  await app.waitFor("id:palette-command");
  await app.type("id:palette-command", zh ? "示例" : "example");
  await app.press(zh ? "id:palette-dialog >> text:打开示例" : "id:palette-dialog >> text:Open the example");
}

/** The demo store, started from the palette (its one door after the welcome) where it is not the endpoint in use
 * yet (an earlier test in the same run may have made it so, and then the palette does not offer it). */
export async function openDemo(app: App) {
  // (the callers are on Endpoints: there the demo says when it is the one in use already)
  if (await app.waitFor("id:endpoint-demo >> text:In use", { timeout: 500 }).then(() => true, () => false)) return;
  if (app.shows("id:toolbar")) await app.press("id:palette");
  else await app.keyboard.press("shift+;");
  await app.waitFor("id:palette-command");
  await app.type("id:palette-command", "demo");
  if (await app.waitFor("id:palette-dialog >> text:Start the demo", { timeout: 2000 }).then(() => true, () => false)) await app.press("id:palette-dialog >> text:Start the demo");
  await app.wait(300);
  if (app.shows("id:palette-command")) await app.keyboard.press("escape");
}

/** A run's file, made up: `n` reads of 50 keys by boto3 on the demo, 5 ms apart from `created`, one in 97 not found,
 * and a line of output every fifth (`processed part 5`, …) — the text of a `.sonde` file, for `importRun`. With
 * `throttled`, every fifth read is refused 503 SlowDown instead (a warning: as many throttles as a check lets a run grow). */
export function runText(n: number, command: string, created: string, throttled = false): string {
  const t0 = Date.parse(created);
  const ua = "Boto3/1.34.0 md/Botocore#1.34.0 ua/2.0 os/linux lang/python#3.12";
  const lines = [JSON.stringify({ format: "sonde-run", version: 2, created, endpoint: "demo", url: "http://127.0.0.1:1/", region: "us-east-1", command, cwd: "/home/me/job" })];
  for (let i = 1; i <= n; i++) {
    const time = new Date(t0 + 5 * i).toISOString();
    const key = `data/part-${`${i % 50}`.padStart(4, "0")}.parquet`;
    const miss = i % 97 === 0;
    const slow = throttled && i % 5 === 0;
    lines.push(
      JSON.stringify({
        id: i, time, endpoint: "demo", kind: "GetObject", method: "GET", url: `http://127.0.0.1:1/demo/${key}`, bucket: "demo", key, query: [], status: slow ? 503 : miss ? 404 : 200,
        latencyMs: 2 + (i % 13) * 0.7, retries: 0, requestId: `R${i}`, hostId: "h", requestHeaders: [{ name: "user-agent", value: ua }],
        responseHeaders: [{ name: "content-length", value: miss ? "0" : "4096" }], requestBytes: 0, responseBytes: miss ? 0 : 4096, firstByteMs: 1,
      }),
    );
    if (i % 5 === 0) lines.push(JSON.stringify({ log: { time, text: `processed part ${i}\r\n` } }));
  }
  return `${lines.join("\n")}\n`;
}

/** Opens a run's text as a file picked in Runs (⌘O) is opened: it joins the runs folder, and its pane shows. */
export async function importRun(app: App, name: string, text: string) {
  if (!app.shows("id:screen-runs")) await app.press("id:nav-runs");
  app.onPick((p) => (p.filters[0]?.extensions.includes("sonde") ? [{ name, data: new TextEncoder().encode(text) }] : null));
  await app.keyboard.press("mod+o");
  await app.waitFor("id:run-pane", { timeout: 10000 });
  await app.waitFor("id:run-summary", { timeout: 10000 });
}

/** A shell command for Run that writes a run outside the runs folder and lists it there as `sonde run -o` does (the
 * runs folder's index names it, `elsewhere`), then sends the demo one request, so the run it is ends and is kept. */
export function elsewhereCommand(command: string): string {
  const head = JSON.stringify({ format: "sonde-run", version: 2, created: "2026-01-02T08:00:00Z", endpoint: "demo", url: "http://127.0.0.1:1/", region: "us-east-1", command, cwd: "/home/me/agent" });
  const rec = JSON.stringify({ id: 1, time: "2026-01-02T08:00:00.005Z", endpoint: "demo", kind: "GetObject", method: "GET", url: "http://127.0.0.1:1/demo/a.txt", bucket: "demo", key: "a.txt", query: [], status: 200, latencyMs: 2, retries: 0, requestId: "R1", hostId: "h", requestHeaders: [], responseHeaders: [], requestBytes: 0, responseBytes: 10 });
  const get = "curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u $AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY -o /dev/null $AWS_ENDPOINT_URL/demo/README.md";
  // a file of its own for each command (the tests share one data folder)
  const file = `${command.replace(/[^A-Za-z0-9]+/g, "-")}.sonde`;
  return `d="$SONDE_DATA_DIR/out" && mkdir -p "$d" "$SONDE_DATA_DIR/runs" && printf '%s\\n' '${head}' '${rec}' > "$d/${file}" && printf '{"name":"%s","set":"elsewhere","value":true}\\n' "$(cd "$d" && pwd -P)/${file}" >> "$SONDE_DATA_DIR/runs/.index.jsonl" && ${get}`;
}

/** The app's language, chosen in Settings (the setting outlives a test: a test that chooses Chinese chooses English
 * again before it ends). */
export async function language(app: App, to: "zh" | "en") {
  await place(app, "settings");
  await app.press(to === "zh" ? "label:Language" : "label:语言");
  await app.press(to === "zh" ? "text:中文" : "text:English");
  await app.waitFor(to === "zh" ? "text:通用" : "text:General");
  await app.keyboard.press("mod+w");
}
