// Shared steps: go to a place, open the app on the demo endpoint.

import { type App, open } from "nsh/test";

export const desktop = { platform: "macos" as const, width: 1280, height: 860 };

const STORE = ["browse", "multipart", "versions", "capacity", "layouts", "probes"];
const SETTINGS = ["settings", "endpoints"];
const ACTIVITY = ["requests", "jobs", "audit"];

/** Goes to a place, or to a tab of one. A window opens Settings in a window
 * of their own (⌘,), Activity from its toolbar, a bucket's details from its
 * cards and compatibility from the endpoints; a terminal lists every
 * place on its home list. */
export async function place(app: App, where: string) {
  const area = STORE.includes(where) ? "store" : SETTINGS.includes(where) ? "settings" : ACTIVITY.includes(where) ? "activity" : where;
  const window = app.shows("id:toolbar");
  if (window && area === "settings") {
    await app.keyboard.press("mod+,");
    await app.waitFor("id:screen-settings-area");
    await app.press(`id:tab-${where}`);
    return;
  }
  if (window && where === "probes") {
    await place(app, "endpoints");
    await app.press("id:open-compat");
    await app.waitFor("id:screen-probes");
    return;
  }
  // a bucket's details open in a sheet over its objects, from its cards
  if (window && area === "store") {
    if (app.shows("id:store-sheet")) await app.press("id:sheet-close");
    // a narrow window folds the sidebar away
    if (!app.shows("id:nav-store")) await app.keyboard.press("mod+b");
    await app.press("id:nav-store");
    if (where !== "browse") {
      await app.press(`id:overview-${where}`);
      await app.waitFor(`id:screen-${where}`);
    }
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
