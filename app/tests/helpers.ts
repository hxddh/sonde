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
