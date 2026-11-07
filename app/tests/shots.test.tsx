// Screenshots of every place in its key states, on a real window
// (`nsh test tests/shots.test.tsx --target linux` under a display). Written to
// $SONDE_SHOTS (default ./shots), one PNG per state, for review and for the
// visual record CI keeps of each build.

import { open, test } from "nsh/test";

const dir = (globalThis as any).process?.env?.SONDE_SHOTS ?? "shots";
const sizes = [
  { name: "wide", width: 1280, height: 820 },
  { name: "min", width: 900, height: 560 },
];

for (const size of sizes) {
  test(`shots ${size.name}`, async () => {
    const app = await open("../App.tsx", { width: size.width, height: size.height });
    // a narrow window folds the sidebar away: bring it back to go somewhere
    const go = async (area: string) => {
      if (!app.shows(`id:nav-${area}`)) await app.press("id:toggle-sidebar");
      await app.press(`id:nav-${area}`);
      if (size.name === "min") await app.press("id:toggle-sidebar");
    };
    const shot = async (name: string) => {
      await app.wait(400);
      await app.screenshot(`${dir}/${size.name}-${name}.png`);
    };
    await shot("01-welcome");
    await app.press("id:welcome-demo");
    await app.waitFor("id:screen-run", { timeout: 15000 });
    await shot("02-run");
    await app.type("id:run-command", "for k in README.md config/app.yaml images/logo.png; do curl -s $AWS_ENDPOINT_URL/demo/$k -o /dev/null; echo fetched $k; done; curl -s $AWS_ENDPOINT_URL/demo/missing.txt -o /dev/null; echo done");
    await app.press("id:run-start");
    await app.waitFor("id:run-ended", { timeout: 20000 });
    await shot("02b-ran");
    await go("sessions");
    await app.press("id:open-example");
    await app.waitFor("id:session-pane", { timeout: 10000 });
    await shot("03-session");
    await app.press("text:looked for the newest WAL: probed ids 1…128");
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
    await go("settings");
    await shot("05-settings");
  });
}
