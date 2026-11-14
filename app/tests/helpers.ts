// Shared steps: go to a place, open the app on the demo endpoint.

import { type App, open } from "nsh/test";

export const desktop = { platform: "macos" as const, width: 1280, height: 860 };

const STORE = ["browse", "multipart", "versions", "capacity", "layouts", "probes"];
const SETTINGS = ["settings", "endpoints"];
const ACTIVITY = ["jobs", "audit"];

/** Goes to a place, or to a tab of one. A window opens Settings in a window
 * of their own (⌘,), Activity from its toolbar, and Buckets' sections (a
 * bucket's tools, compatibility) from the place's one row of sections; a
 * terminal lists every place on its home list. */
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
  // with much to read, a long list of sessions, takes a moment)
  if (!window) await app.waitFor(`id:nav-${area}`, { state: "gone" });
  if (area === "store" || area === "settings") await app.press(`id:tab-${where}`);
  if (area === "activity") await app.press(`id:activity-tab-${where}`);
}

/** Opens the app, starts the demo, and goes to Store (selecting `bucket`). */
export async function withDemo(bucket?: string, options: Parameters<typeof open>[1] = desktop): Promise<App> {
  const app = await open("../App.tsx", options);
  await place(app, "endpoints");
  await app.press("id:open-demo");
  await app.waitFor("text:Demo (built-in)", { timeout: 10000 });
  await place(app, "browse");
  await app.waitFor("id:bucket-demo", { timeout: 10000 });
  if (bucket) await app.press(`id:bucket-${bucket}`);
  return app;
}

/** Runs the command in Run's field: by Run before any run, by the ended band's one Run after one (the field's
 * command when it was changed, "Run again" when it was not). */
export async function runCommand(app: App) {
  const id = app.shows("id:run-again-band") ? "id:run-again-band" : "id:run-start";
  // (the button follows the field a frame later)
  await app.waitUntil(() => app.enabled(id), { timeout: 10000 });
  await app.press(id);
}

/** Presses one of the commands at the end of Run's band: as a button where
 * the band has room for it, behind `⋯` where it has not (they never wrap). */
export async function runAction(app: App, id: "run-open-session" | "run-try" | "run-now-yours") {
  await app.waitFor("id:run-actions", { timeout: 15000 });
  if (app.shows(`id:${id}`)) return app.press(`id:${id}`);
  // by its words, in the language the window is in
  const labels = { "run-open-session": ["See the whole run", "查看完整运行"], "run-try": ["Try under faults…", "在故障下试运行…"], "run-now-yours": ["Now your own store…", "换成你自己的存储…"] }[id];
  await app.press("id:run-actions-more");
  const item = app.menu()?.map((m) => m.label).find((l) => labels.includes(l));
  if (!item) throw new Error(`${id}: not in the band, not behind its menu`);
  await app.chooseMenuItem(item);
}

/** A session's command besides its one primary (Try under faults…, Replay on…, Compare…): behind its header's `⋯`,
 * by its words. */
export async function sessionAction(app: App, label: string) {
  await app.press("id:session-more");
  await app.chooseMenuItem(label);
}

/** The words of the session header's `⋯` menu, the menu closed again. */
export async function sessionMenu(app: App): Promise<string[]> {
  await app.press("id:session-more");
  const labels = app.menu()?.map((m) => m.label) ?? [];
  await app.closeMenu();
  return labels;
}
