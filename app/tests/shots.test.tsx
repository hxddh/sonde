// Screenshots of every place in its key states, on a real window
// (`nsh test tests/shots.test.tsx --target linux` under a display): wide and
// at the least window size, light and dark, English and Chinese. Written to
// $SONDE_SHOTS (default ./shots), one PNG per state, for review and for the
// visual record CI keeps of each build.

import { type App, open, test } from "nsh/test";
import { runAction } from "./helpers";

const dir = (globalThis as any).process?.env?.SONDE_SHOTS ?? "shots";

/** The tour's program in the shell Run starts it in: sh, or cmd.exe on
 * Windows. The recording endpoint is in AWS_ENDPOINT_URL either way. */
function command(cmd: boolean, marker?: string) {
  const url = cmd ? "%AWS_ENDPOINT_URL%" : "$AWS_ENDPOINT_URL";
  const sink = cmd ? "NUL" : "/dev/null";
  // signed with the keys sonde gives the program: its proxy refuses anything else
  const keys = cmd ? "%AWS_ACCESS_KEY_ID%:%AWS_SECRET_ACCESS_KEY%" : "$AWS_ACCESS_KEY_ID:$AWS_SECRET_ACCESS_KEY";
  const curl = `curl -s --aws-sigv4 aws:amz:us-east-1:s3 -u ${keys}`;
  const get = (k: string) => `${curl} -o ${sink} ${url}/demo/${k} && echo fetched ${k}`;
  // small objects, one of 3 MiB (its first byte against the rest of it), a key that is not
  // there (a miss) and a bucket that is not (a failure: the errors chart has something to draw)
  // a pause first: a trial of it runs long enough to be seen running
  const pause = cmd ? "ping -n 3 127.0.0.1 >NUL" : "sleep 1.5";
  // its config read ten times (a finding: a key read again and again)
  // (with a marker, sh's form reads it ten times more on the first run and not again on the next: the same command,
  // changed behaviour, as a program fixed between two runs — what "Since the run before" is about, the finding fixed)
  const again = cmd
    ? `(for /L %i in (1,1,10) do @${curl} -o ${sink} ${url}/demo/config/app.yaml)`
    : marker
      ? `(if [ -e ${marker} ]; then n=0; else n=10; touch ${marker}; fi; for i in $(seq $n); do ${curl} -o ${sink} ${url}/demo/config/app.yaml; done)`
      : `for i in 1 2 3 4 5 6 7 8 9 10; do ${curl} -o ${sink} ${url}/demo/config/app.yaml; done`;
  // a write whose refusal curl never hears (no -f): under faults it breaks
  const put = `${curl} -X PUT --data hello -o ${sink} ${url}/demo/tour/notes.txt && echo wrote notes.txt`;
  return [pause, ...["README.md", "config/app.yaml", "images/logo.png", "backups/db-2026-10-01.tar.zst"].map(get), again, put, `${curl} -o ${sink} ${url}/demo/missing.txt`, `${curl} -o ${sink} ${url}/no-such-bucket/a.txt`, `${curl} -o ${sink} ${url}/no-such-bucket/b.txt`, "echo done"].join(" && ");
}

// a long command into a field: pasted where the test runner can paste (quicker than typing it), typed where it
// cannot yet (Native Shell's runner pastes on macOS, iOS, the web and headless; not on a Windows desktop)
async function enter(app: any, id: string, text: string) {
  try {
    await app.paste(id, { text });
  } catch (e) {
    if (!/pasting in tests is not supported/.test(String(e))) throw e;
    await app.type(id, text);
  }
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
    // a screen of charts: each draws itself in (420 ms, data that arrives after the figures its own): shot once drawn
    const charts = async (name: string) => {
      await app.wait(1500);
      await shot(name);
    };
    // The title bar's buttons are reached by their shortcuts: on macOS a
    // test's press in the hidden title bar's area does not reach the view
    // (FINDINGS 12). The sidebar holds the places and their lists at every
    // size: brought back if it was put away.
    // a press that a busy window may let pass (it reads a long list of sessions again): pressed again until it shows
    const pressUntil = async (press: string, shown: string) => {
      for (let i = 0; i < 4; i++) {
        await app.press(press);
        if (await app.waitFor(shown, { timeout: 4000 }).then(() => true, () => false)) return;
      }
      await app.waitFor(shown, { timeout: 4000 });
    };
    // A menu's items, once it is up (a native menu, on macOS, opens a moment after the press that asks for it)
    const menuItems = async () => {
      await app.waitUntil(() => (app.menu()?.length ?? 0) > 0, { timeout: 5000 }).catch(() => {});
      return app.menu()?.map((m) => m.label) ?? [];
    };
    // a menu closed, if one is still up (a native menu may have closed itself in between)
    const closeAnyMenu = async () => {
      if (app.menu()) await app.closeMenu().catch(() => {});
    };
    // The item `label` of the menu `open` shows: asked again once if the menu was gone before it was chosen
    const chooseFrom = async (open: () => Promise<unknown>, label: string) => {
      for (let n = 0; ; n++) {
        await closeAnyMenu();
        await open();
        const items = await menuItems();
        try {
          return await app.chooseMenuItem(label);
        } catch (e) {
          if (n > 0) throw new Error(`choosing ${JSON.stringify(label)} from [${items.join(", ")}]: ${e}`);
          await app.wait(500);
        }
      }
    };
    // an act on the session header, or in its ⋯ menu where the header has no room for it (its label in the tour's language)
    const act = async (id: string, label: string, zh: string) => {
      if (app.shows(`id:${id}`)) return app.press(`id:${id}`);
      await chooseFrom(() => app.press("id:session-more"), v.lang ? zh : label);
    };
    const go = async (area: string) => {
      if (!app.shows(`id:nav-${area}`)) await app.keyboard.press("mod+b");
      await app.press(`id:nav-${area}`);
    };
    // The screens that draw measured figures (a run's times, a trial's run over its clean run, two runs compared, a
    // program's runs) show committed sessions (tests/fixtures/sessions, which scripts/shots.sh puts in the tour's
    // data folder): `./nightly-report.sh` run on two mornings, before its fix and after (with a slowdown trial).
    // The tour's own runs are measured anew each time; their screens show what recording does, not its figures.
    // The committed run after the fix, opened from the list of runs: the oldest are last, so the list is brought to
    // its end, the newer of the program's two rows pressed (the first of the two in the list), and the list brought
    // back to its start.
    // A run opened by its row's name (the first of that name in the list), the list brought down to it and back.
    const openRow = async (name: string, header: string) => {
      await go("sessions");
      if (!app.shows("id:session-list") && !app.shows("id:program-list")) await app.keyboard.press("mod+b");
      if (!app.shows("id:session-list")) await pressUntil("id:sessions-by-runs", "id:session-list");
      for (let n = 0; n < 30 && !app.shows(`id:session-list >> text:${name}`); n++) await app.scroll("id:session-list", 800);
      await app.scroll("id:session-list", 800);
      await app.wait(300);
      // (its header says its command: the session shown is the one pressed; a press the list's settling undid, again)
      const shown = () => app.shows("id:session-summary") && app.texts().some((x) => x.includes(header));
      for (let n = 0; n < 4 && !shown(); n++) {
        await app.press(`id:session-list >> text:${name} >> nth=0`);
        await app.waitUntil(shown, { timeout: 4000 }).catch(() => {});
      }
      await app.waitUntil(shown, { timeout: 10000 });
      if (!app.shows("id:session-list")) await app.keyboard.press("mod+b");
      for (let n = 0; n < 30 && !app.shows("id:session-0"); n++) await app.scroll("id:session-list", -800);
      await app.wait(600);
    };
    const fixture = () => openRow("nightly-report.sh", "./nightly-report.sh");

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
    // recording a program started elsewhere: what it asks before it starts
    if (app.shows("id:record")) {
      await app.press("id:record");
      await app.waitFor("id:record-start");
      await shot("02a-record");
      await app.press("id:record-cancel");
    }
    // which shell Run uses: cmd.exe on Windows (no probe run: it would sit in every list)
    const env = (globalThis as any).process?.env ?? {};
    // (asked of the app: the demo's own sample command, left in the field, is written for the shell Run uses;
    // a test has no process to ask, and its own `platform` is not the window's)
    const sample = String((await app.value("id:run-command")) ?? "");
    const cmd = sample.includes("%AWS_ENDPOINT_URL%") || (globalThis as any).process?.platform === "win32" || env.OS === "Windows_NT" || env.RUNNER_OS === "Windows";
    // the sample said which shell: no second guess is owed
    const sure = sample.includes("AWS_ENDPOINT_URL");
    // (the demo left its own first command in the field: typed after it, the two ran as one)
    await app.clear("id:run-command");
    const marker = `/tmp/sonde-tour-${v.name}-${Date.now()}`;
    await enter(app, "id:run-command", command(cmd, marker));
    // (the button follows the field a frame later)
    await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
    await app.press("id:run-start");
    await app.waitFor("id:run-ended", { timeout: 20000 });
    // (the other shell's form, should the sample not have said and the first have reached no store)
    if (!sure && (await app.wait(1500), app.shows("id:run-no-requests"))) {
      await app.clear("id:run-command");
      await enter(app, "id:run-command", command(!cmd, marker));
      await app.waitUntil(() => app.enabled("id:run-start"), { timeout: 10000 });
      await app.press("id:run-start");
      await app.waitUntil(() => !app.shows("id:run-no-requests"), { timeout: 20000 });
      await app.waitFor("id:run-ended", { timeout: 20000 });
    }
    await shot("02b-ran");
    // run again from the band's one Run, the program fixed meanwhile (its config read once, not ten times):
    // what changed since the run before, the finding fixed with what it saved
    if (!app.shows("id:run-no-requests") && !String((await app.value("id:run-command")) ?? "").includes("%AWS_ENDPOINT_URL%")) {
      await app.press("id:run-again-band");
      await app.waitFor("id:run-status", { timeout: 10000 });
      await app.waitFor("id:run-ended", { timeout: 20000 });
      await app.waitFor("id:since-run", { timeout: 10000 });
      await charts("02bb-since");
    }
    // the same command under faults: said before it runs, followed as it goes, its verdicts
    await runAction(app, "run-try");
    await app.waitFor("id:trial-plan", { timeout: 10000 });
    await shot("02e-trial-plan");
    await app.press("id:trial-start");
    await app.waitFor("id:trial-progress", { timeout: 10000 });
    await app.screenshot(`${dir}/${v.name}-02f-trial-running.png`);
    await app.waitFor("id:trial-elapsed");
    await app.waitUntil(() => !app.shows("id:trial-cancel"), { timeout: 90000 });
    await shot("02g-trial-done");
    // what broke, tried again on a fresh run (after a change, to see it survive): said first, then run
    if (app.shows("id:trial-again-here")) {
      await app.press("id:trial-again-here");
      await app.waitFor("id:trial-again-plan", { timeout: 10000 });
      await shot("02ga-again-plan");
      await app.press("id:trial-start");
      await app.waitFor("id:trial-progress", { timeout: 10000 });
      await app.waitUntil(() => !app.shows("id:trial-cancel"), { timeout: 90000 });
      await shot("02gb-again-done");
    }
    // closed, the run opens on its Runs pane, where the verdicts stay, each with its history
    await app.press("id:trial-close");
    await app.waitFor("id:history-trials", { timeout: 20000 });
    await shot("02h-runs-verdicts");
    // a trial's own session: its notice, the faults' moments on its shape, and the run over its clean run
    // (a long list of verdicts, Chinese's, has it under the fold: brought up, and pressed again)
    // (the trial's session can take longer than a press's wait to open: what is on screen decides the next step)
    for (let n = 0; n < 6 && !app.shows("id:trial-note"); n++) {
      if (app.shows("id:trial-slowdown")) {
        await app.press("id:trial-slowdown");
        await app.waitFor("id:trial-note", { timeout: 5000 }).catch(() => {});
      } else if (app.shows("id:history-pane")) {
        await app.scroll("id:history-pane", 300);
      }
      await app.wait(500);
    }
    await app.waitFor("id:trial-note", { timeout: 20000 });
    await app.wait(600);
    await shot("02i-trial-session");
    // the run under faults over its clean run, on its Time pane: the committed run's slowdown trial, opened as the
    // tour's was, from the run's Runs pane
    await fixture();
    await app.press("id:session-pane-mode-runs");
    await app.waitFor("id:trial-slowdown", { timeout: 10000 });
    await app.press("id:trial-slowdown");
    await app.waitFor("id:trial-note", { timeout: 20000 });
    await app.press("id:session-pane-mode-time");
    await app.waitFor("id:time-pane");
    await app.wait(900);
    await charts("02j-trial-time");
    await app.press("id:trial-open-clean");
    await app.waitFor("id:trial-note", { state: "gone", timeout: 20000 });
    await app.waitFor("id:session-summary", { timeout: 10000 });
    // the program's output lined up with what it did
    if (app.enabled("id:session-pane-mode-aligned")) {
      await app.press("id:session-pane-mode-aligned");
      await app.waitFor("id:output", { timeout: 5000 });
      await shot("02k-output");
    }
    // the run's own session (the committed run, its clean run): what failed over its time, and the first byte against
    // the rest
    await go("sessions");
    await app.waitFor("id:session-summary", { timeout: 10000 });
    await app.press("id:signal-errors");
    await app.waitFor("id:time-pane");
    await app.wait(900);
    await charts("02c-errors");
    await app.press("id:signal-latency");
    await app.wait(700);
    if (app.shows("id:chart-firstByte")) {
      await app.scroll("id:time-pane", 320);
      await charts("02d-first-byte");
    }

    await go("sessions");
    // the programs run here, each over its runs: the committed program's runs beside the list (its run is the one shown)
    await app.press("id:sessions-by-programs");
    await app.waitFor("id:program-list", { timeout: 10000 });
    await app.press("id:session-pane-mode-runs");
    await app.waitFor("id:history-pane", { timeout: 10000 });
    await app.wait(900);
    await shot("03p-programs");
    // (back on Operations: the example opens on the pane the runs were left on, and has no Runs of its own)
    await app.press("id:session-pane-mode-operations");
    await app.wait(500);
    await app.press("id:sessions-by-runs");
    // (the runs' list back first: a press before it settles is undone as it chooses its first run)
    if (!app.shows("id:session-list")) await pressUntil("id:sessions-by-runs", "id:session-list");
    await app.wait(700);
    const wal = v.lang ? "text:查找最新的 WAL：探测了编号 1…128" : "text:looked for the newest WAL: probed ids 1…128";
    // read off the interface's thread: there once its header is (a variant after the first
    // opens on the example already, and reads it again as the list changes: let that settle;
    // the example opened from its row when the list's settling undid the press)
    await app.press("id:open-example");
    await app.wait(900);
    // (a press the list's settling undid: the example opened from its row instead)
    if (!(await app.waitFor(wal, { timeout: 5000 }).then(() => true, () => false))) {
      await openRow("gen-slatedb", "gen-slatedb s3://slate");
      await app.press("id:session-pane-mode-operations");
      await app.waitFor(wal, { timeout: 10000 });
    }
    await shot("03-session");
    await app.press(v.lang ? "text:查找最新的 WAL：探测了编号 1…128" : "text:looked for the newest WAL: probed ids 1…128");
    await app.waitFor("id:inspector");
    // (the operation's chart of its requests draws itself in: shot once drawn)
    await charts("03b-operation");
    // a narrow window's inspector comes in from the edge, its requests under the fold
    if (width < 1000) await app.scroll("id:inspector-scroll", 400);
    await app.press("id:op-request-0");
    await app.waitFor("id:record-detail");
    // (a narrow window's inspector, scrolled to its requests, back at its start: where a drag stops differs run to run)
    if (width < 1000) await app.scroll("id:inspector-scroll", -5000).catch(() => {});
    await shot("03c-request");
    await app.press("id:inspector-close");
    await app.press("id:observation-1");
    await shot("03d-filtered");
    // a slice of the session's shape: the operations running then
    await app.press("id:clear-filter");
    // why slow: the operations split by kind of request, and where the slow ones are, in a sentence
    await app.press(v.lang ? "label:分组" : "label:Group by");
    await app.press(v.lang ? "text:请求类型" : "text:Kind of request");
    await app.waitFor("id:split-table", { timeout: 10000 });
    await shot("03e-group-by");
    await app.press(v.lang ? "label:分组" : "label:Group by");
    await app.press(v.lang ? "text:逐个操作" : "text:Each operation");
    await app.waitFor("id:operations");
    if (app.shows("id:session-shape")) {
      await app.hover("id:session-shape");
      await app.press("id:session-shape");
      await shot("03f-slice");
    }
    await app.press("id:session-pane-mode-time");
    await app.waitFor("id:time-pane");
    // the board's charts draw themselves in: the picture is of them drawn
    await app.wait(900);
    await charts("03g-time");
    // the slice chosen on the strip is the Time pane's too (its one chip): let go, the charts below are of the whole run
    if (app.shows("id:clear-filter")) await app.press("id:clear-filter");
    // further down the Time pane, a window at a time: every request and each
    // kind over time; pushback and the distribution; time spent and chains;
    // limits, heat and costs; the flows
    await app.scroll("id:time-pane", 300);
    await charts("03h-requests");
    await app.scroll("id:time-pane", 400);
    await charts("03i-distribution");
    await app.scroll("id:time-pane", 400);
    await charts("03j-spent");
    await app.scroll("id:time-pane", 400);
    await charts("03k-prefixes");
    // to the board's end: where requests went
    await app.scroll("id:time-pane", 4000);
    // the flows on demand, pressed last: the shots above show the pane as it opens
    if (app.shows("id:flows-show")) await app.press("id:flows-show").catch(() => {});
    await charts("03l-flows");
    // a signal pressed in the header brings its chart into view: latency's bands, then bytes each way
    await app.press("id:signal-latency");
    await app.wait(700);
    await charts("03m-latency");
    // the folded header holds the first three figures: bytes is pressed from the header unfolded
    if (!app.shows("id:signal-bytes")) {
      await app.scroll("id:time-pane", -10000);
      await app.wait(500);
    }
    if (app.shows("id:signal-bytes")) {
      await app.press("id:signal-bytes");
      await app.wait(700);
      await charts("03n-bytes");
    }
    await app.press("id:signal-errors");
    await app.wait(700);
    await charts("03o-errors");
    // the same program run twice, before and after its fix: the committed run after it against the one before
    await fixture();
    // (by "Since the run before"'s Compare where it is shown: the same two runs, and no menu opened from a menu, which
    // macOS's test bridge lists but cannot choose from)
    let run: string | undefined = "the run before";
    if (app.shows("id:since-compare")) await app.press("id:since-compare");
    else {
      await act("compare-open", "Compare…", "对比…");
      run = (await menuItems()).find((l) => l.includes("nightly-report"));
      if (run) await chooseFrom(() => act("compare-open", "Compare…", "对比…"), run);
    }
    if (run) {
      await app.waitFor("id:run-change", { timeout: 10000 });
      // (each run's signals and the charts arrive after the figures: a picture taken at once showed "Not timed" over empty charts)
      await app.wait(1500);
      await charts("03h-runs");
      await app.press("id:compare-back");
    } else await closeAnyMenu();

    // what a replay on another store would send, said before it runs
    await app.waitFor("id:session-more", { timeout: 10000 });
    await act("replay-open", "Replay on…", "重放到…");
    await app.waitFor("id:replay-plan", { timeout: 10000 });
    await shot("03q-replay-plan");
    await app.press("id:replay-cancel");
    await app.waitFor("id:replay-plan", { state: "gone" });

    await go("store");
    // the buckets are listed from the store: there when the listing is in
    await app.waitFor("id:bucket-demo", { timeout: 10000 });
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
    // a change to the store: the plan, and the typed confirmation
    await app.press("id:plan-abort");
    await app.waitFor("id:confirm-dialog");
    await shot("04e-confirm");
    await app.press("id:confirm-cancel");
    await app.waitFor("id:confirm-dialog", { state: "gone" });
    // a key's versions over time: the next section in Buckets' one row
    await app.press("id:store-section-versions");
    await app.waitFor("id:screen-versions");
    await app.type("id:version-key", "config/app.yaml");
    await app.press("id:load-versions");
    await app.waitFor("id:timeline", { timeout: 10000 });
    await shot("04f-versions");
    // why the bucket is this big
    await app.press("id:store-section-capacity");
    await app.waitFor("id:du-findings", { timeout: 15000 });
    await app.wait(600);
    await charts("04g-capacity");
    // a table format's layout, decoded (from the objects' tile, which says one was recognised)
    await app.press("id:store-section-browse");
    if (!app.shows("id:bucket-demo-layouts")) await app.keyboard.press("mod+b");
    await app.press("id:bucket-demo-layouts");
    await app.waitFor("id:overview-layouts", { timeout: 15000 });
    await app.press("id:overview-layouts");
    await app.waitFor("id:analyze-slatedb", { timeout: 15000 });
    await app.press("id:analyze-slatedb");
    await app.waitFor("id:layout-report", { timeout: 15000 });
    await shot("04h-layouts");
    // the endpoint's compatibility: the last of Buckets' sections
    await app.press("id:store-section-probes");
    await app.waitFor("id:screen-probes");
    await shot("04i-compat");
    await app.press("id:store-section-browse");

    await app.keyboard.press("mod+shift+a");
    await app.waitFor("id:activity");
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
    await app.press("id:add-endpoint");
    await app.waitFor("id:endpoint-form");
    // the form is a card in place: there once its foot is
    await app.waitFor("id:ep-save");
    await new Promise((r) => setTimeout(r, 500));
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
