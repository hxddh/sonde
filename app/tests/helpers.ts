// Shared steps: open the app on the demo endpoint.

import { type App, open } from "nsh/test";

export const desktop = { platform: "macos" as const, width: 1280, height: 860 };

/** Opens the app, starts the demo, and selects `bucket` in the sidebar. */
export async function withDemo(bucket?: string, options = desktop): Promise<App> {
  const app = await open("../App.tsx", options);
  await app.press("id:nav-endpoints");
  await app.press("id:open-demo");
  await app.waitFor("id:bucket-demo", { timeout: 10000 });
  if (bucket) await app.press(`id:bucket-${bucket}`);
  return app;
}
