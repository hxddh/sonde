// Shared steps: go to a place, open the app on the demo endpoint.

import { type App, open } from "nsh/test";

export const desktop = { platform: "macos" as const, width: 1280, height: 860 };

const STORE = ["browse", "multipart", "versions", "capacity", "layouts", "probes"];
const SETTINGS = ["settings", "endpoints", "requests", "jobs", "audit"];

/** Goes to an area, or to a tab of Store or Settings. In a terminal, from wherever it is. */
export async function place(app: App, where: string) {
  const area = STORE.includes(where) ? "store" : SETTINGS.includes(where) ? "settings" : where;
  // a terminal shows the places on its home list only
  if (!app.shows(`id:nav-${area}`)) await app.back();
  await app.press(`id:nav-${area}`);
  if (area === "store" || area === "settings") await app.press(`id:tab-${where}`);
}

/** Opens the app, starts the demo, and goes to Store (selecting `bucket`). */
export async function withDemo(bucket?: string, options = desktop): Promise<App> {
  const app = await open("../App.tsx", options);
  await place(app, "endpoints");
  await app.press("id:open-demo");
  await app.waitFor("text:Demo (built-in)", { timeout: 10000 });
  await place(app, "browse");
  await app.waitFor("id:bucket-demo", { timeout: 10000 });
  if (bucket) await app.press(`id:bucket-${bucket}`);
  return app;
}
