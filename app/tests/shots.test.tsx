// Screenshots of every place in its key states, on a real window
// (`nsh test tests/shots.test.tsx --target linux` under a display): wide and
// at the least window size, light and dark, English and Chinese. Written to
// $SONDE_SHOTS (default ./shots), one PNG per state, for review and for the
// visual record CI keeps of each build.

import { type App, open, test } from "nsh/test";

const dir = (globalThis as any).process?.env?.SONDE_SHOTS ?? "shots";

interface Variant {
  name: string;
  width: number;
  height: number;
  theme?: "dark";
  lang?: "zh";
}

const variants: Variant[] = [
  { name: "wide", width: 1280, height: 820 },
  { name: "min", width: 900, height: 560 },
  { name: "dark", width: 1280, height: 820, theme: "dark" },
  { name: "zh", width: 1280, height: 820, lang: "zh" },
];

for (const v of variants) {
  test(`shots ${v.name}`, async () => {
    const app: App = await open("../App.tsx", { width: v.width, height: v.height });
    const shot = async (name: string) => {
      await app.wait(450);
      await app.screenshot(`${dir}/${v.name}-${name}.png`);
    };
    // a narrow window folds the sidebar away: bring it back to go somewhere
    const go = async (area: string) => {
      if (!app.shows(`id:nav-${area}`)) await app.press("id:toggle-sidebar");
      await app.press(`id:nav-${area}`);
      if (v.name === "min") await app.press("id:toggle-sidebar");
    };

    await shot("01-welcome");
    await app.press("id:welcome-demo");
    await app.waitFor("id:screen-run", { timeout: 15000 });
    if (v.theme || v.lang) {
      await app.keyboard.press("mod+,");
      await app.waitFor("id:screen-settings-area");
      await app.press(`id:theme-${v.theme ?? "light"}`);
      if (v.lang) {
        await app.press("label:Language");
        await app.press("text:中文");
      }
      await app.wait(300);
      await app.window("settings").pressClose();
    }
    await shot("02-run");
    await app.type("id:run-command", "for k in README.md config/app.yaml images/logo.png; do curl -s $AWS_ENDPOINT_URL/demo/$k -o /dev/null; echo fetched $k; done; curl -s $AWS_ENDPOINT_URL/demo/missing.txt -o /dev/null; echo done");
    await app.press("id:run-start");
    await app.waitFor("id:run-ended", { timeout: 20000 });
    await shot("02b-ran");

    await go("sessions");
    await app.press("id:open-example");
    await app.waitFor("id:session-pane", { timeout: 10000 });
    await shot("03-session");
    await app.press(v.lang ? "text:查找最新的 WAL：探测了编号 1…128" : "text:looked for the newest WAL: probed ids 1…128");
    await app.waitFor("id:inspector");
    await shot("03b-operation");
    await app.press("id:op-request-0");
    await app.waitFor("id:record-detail");
    await shot("03c-request");
    await app.press("id:inspector-close");
    await app.press("id:observation-1");
    await shot("03d-filtered");
    await app.press("id:session-pane-mode-timeline");
    await app.waitFor("id:timeline");
    await shot("03e-timeline");

    await go("store");
    await shot("04-store");
    await app.press("id:bucket-demo");
    await app.waitFor("text:README.md", { timeout: 10000 });
    // the cards fill in as their scans finish
    await app.wait(2500);
    await shot("04b-objects");
    await app.press("text:README.md");
    await app.waitFor("id:object-detail");
    await app.wait(600);
    await shot("04c-object");
    await app.press("id:overview-multipart");
    await app.waitFor("id:lifecycle-verdict", { timeout: 10000 });
    await shot("04d-uploads");
    await app.press("id:sheet-close");

    await app.press("id:activity-open");
    await app.waitFor("id:requests");
    await shot("05-activity");
    await app.press("id:activity-close");

    // Settings: a window of their own
    const settings = async (name: string) => {
      await app.wait(450);
      await app.window("settings").screenshot(`${dir}/${v.name}-${name}.png`);
    };
    await app.keyboard.press("mod+,");
    await app.waitFor("id:screen-settings-area");
    await app.press("id:tab-settings");
    await settings("05b-settings");
    await app.press("id:tab-endpoints");
    await settings("05c-endpoints");
    await app.press("id:open-compat");
    await app.waitFor("id:screen-probes");
    await app.wait(450);
    await app.window("compat").screenshot(`${dir}/${v.name}-05e-compat.png`);
    await app.window("compat").pressClose();
    await app.press("id:add-endpoint");
    await app.waitFor("id:endpoint-form");
    await settings("05d-endpoint-form");
    await app.keyboard.press("Escape");
    await app.window("settings").pressClose();
    await app.press("id:palette");
    await app.waitFor("id:palette-command");
    await shot("06-palette");
    await app.type("id:palette-command", "ver");
    await shot("06b-palette-search");
    // a whole tour of the app: minutes on a slow runner, not seconds
  }, 300000);
}
