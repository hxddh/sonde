// Screenshots of every place in its key states, on a real window
// (`nsh test tests/shots.test.tsx --target linux` under a display): wide and
// at the least window size, light and dark, English and Chinese. Written to
// $SONDE_SHOTS (default ./shots), one PNG per state, for review and for the
// visual record CI keeps of each build.

import { type App, open, test } from "nsh/test";

const dir = (globalThis as any).process?.env?.SONDE_SHOTS ?? "shots";

/** The tour's program in the shell Run starts it in: sh, or cmd.exe on
 * Windows. The recording endpoint is in AWS_ENDPOINT_URL either way. */
function command(cmd: boolean) {
  const url = cmd ? "%AWS_ENDPOINT_URL%" : "$AWS_ENDPOINT_URL";
  const sink = cmd ? "NUL" : "/dev/null";
  // signed with the keys sonde gives the program: its proxy refuses anything else
  const keys = cmd ? "%AWS_ACCESS_KEY_ID%:%AWS_SECRET_ACCESS_KEY%" : "$AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY";
  const curl = `curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u ${keys}`;
  const get = (k: string) => `${curl} -o ${sink} ${url}/demo/${k} && echo fetched ${k}`;
  return [...["README.md", "config/app.yaml", "images/logo.png"].map(get), `${curl} -o ${sink} ${url}/demo/missing.txt`, "echo done"].join(" && ");
}

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

// a window no larger than the screen it is on: the macOS runner's is
// 1024×768, and a larger window is cut off (scripts/shots.sh sets it)
const [maxW, maxH] = String((globalThis as any).process?.env?.SONDE_SHOT_MAX ?? "").split("x").map(Number);

for (const v of variants) {
  test(`shots ${v.name}`, async () => {
    const width = maxW ? Math.min(v.width, maxW) : v.width;
    const height = maxH ? Math.min(v.height, maxH) : v.height;
    const app: App = await open("../App.tsx", { width, height });
    const shot = async (name: string) => {
      await app.wait(450);
      await app.screenshot(`${dir}/${v.name}-${name}.png`);
    };
    // The title bar's buttons are reached by their shortcuts: on macOS a
    // test's press in the hidden title bar's area does not reach the view
    // (FINDINGS 12). A narrow window folds the sidebar away: bring it back
    // to go somewhere.
    const go = async (area: string) => {
      if (!app.shows(`id:nav-${area}`)) await app.keyboard.press("mod+b");
      await app.press(`id:nav-${area}`);
      if (v.name === "min") await app.keyboard.press("mod+b");
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
    // which shell Run uses: only cmd.exe expands %OS%
    await app.type("id:run-command", "echo %OS%");
    await app.press("id:run-start");
    await app.waitFor("id:run-ended", { timeout: 20000 });
    const cmd = app.texts().some((t) => t.includes("Windows_NT"));
    await app.clear("id:run-command");
    await app.type("id:run-command", command(cmd));
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
    // a slice of the session's shape: the operations running then
    await app.press("id:clear-filter");
    if (app.shows("id:session-shape")) {
      await app.hover("id:session-shape");
      await app.press("id:session-shape");
      await shot("03f-slice");
    }
    await app.press("id:session-pane-mode-time");
    await app.waitFor("id:time-pane");
    await shot("03g-time");
    // further down the Time pane: where requests went, when and where, how long
    await app.scroll("id:time-pane", 380);
    await shot("03i-flows");
    await app.scroll("id:time-pane", 420);
    await shot("03j-spread");
    // the same program run twice: before and after
    await app.press("id:compare-open");
    const run = app.menu()?.map((m) => m.label).find((l) => l.startsWith("curl"));
    if (run) {
      await app.chooseMenuItem(run);
      await app.waitFor("id:run-change", { timeout: 10000 });
      await shot("03h-runs");
      await app.press("id:compare-back");
    } else if (app.menu()) await app.closeMenu();

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

    await app.keyboard.press("mod+shift+a");
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
    await app.keyboard.press("mod+k");
    await app.waitFor("id:palette-command");
    await shot("06-palette");
    await app.type("id:palette-command", "ver");
    await shot("06b-palette-search");
    // a whole tour of the app: minutes on a slow runner, not seconds
  }, 300000);
}
