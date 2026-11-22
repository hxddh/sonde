// Updates: is there a newer sonde, and installing it. The desktop app
// replaces itself and restarts, or waits to when sonde quits (Native Shell's
// updater, from the project's GitHub Releases, its notes in the app's
// language); elsewhere — a system install — it links to
// the release.

import { appInfo, effect, openUrl, signal, untracked } from "nsh";
import { type AppUpdate, checkForPackage, checkForUpdate, downloadPackage, downloadUpdate, installAndRestart, installOnQuit, type PackageUpdate, restartToPackage, shellInfo } from "nsh/updater";
import { lang, t } from "../i18n";
import { running } from "../run";
import { runningJobs } from "./activity";
import { report } from "../store";
import { count } from "../format";
import { Button, Notice, Section } from "./kit";

type State =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "current" }
  | { kind: "found"; update: AppUpdate }
  | { kind: "downloading"; update: AppUpdate; share: number }
  // downloaded and checked: it replaces the app when sonde quits
  | { kind: "waiting"; update: AppUpdate }
  // the app's code alone, signed: no new app to install, a restart runs it
  | { kind: "package"; update: PackageUpdate; share: number | null };

/** Asks the Updates section to check now (the palette's "Check for updates"). */
export const checkRequested = signal(0);
let answered = 0;

/** Which install comes first: while a recording or a job runs, the one when sonde quits (a restart now would cut
 * it off); otherwise installing and restarting. */
export const installFirst = (busy: boolean): "quit" | "restart" => (busy ? "quit" : "restart");

/** What a found update says: a newer sonde, or (`shell`: this sonde is already that version, its code updated
 * alone) a newer app around it, never the running version as new. */
export const foundWords = (u: { version: string; shell?: boolean }) => (u.shell ? t("updates.shell") : t("updates.found", { version: u.version }));

export function Updates() {
  const state = signal<State>({ kind: "idle" });
  // what this is: the app as it was built (its name and version are nsh.json's, read with
  // Native Shell's `appInfo`, carried nowhere else) and the Native Shell runtime it runs on
  const app = appInfo();
  const shell = (() => {
    try {
      return shellInfo();
    } catch {
      return null;
    }
  })();
  const check = async () => {
    state.set({ kind: "checking" });
    try {
      // a signed package of sonde's code first: smaller, and nothing to install
      if (shell?.packages) {
        const p = await checkForPackage({ locale: lang() }).catch(() => null);
        if (p) return void state.set({ kind: "package", update: p, share: null });
      }
      const u = await checkForUpdate({ locale: lang() });
      state.set(u ? { kind: "found", update: u } : { kind: "current" });
    } catch (e) {
      state.set({ kind: "idle" });
      report(e);
    }
  };
  // asked from elsewhere: Settings opens on this section, and it checks
  effect(() => {
    const asked = checkRequested();
    if (asked > answered) {
      answered = asked;
      untracked(() => void check());
    }
  });
  // a recording or a job that a restart would cut off
  const busy = () => running() !== null || runningJobs() > 0;
  const install = async (u: AppUpdate, when: "quit" | "restart") => {
    if (!u.canInstall) return void openUrl(u.url);
    state.set({ kind: "downloading", update: u, share: 0 });
    try {
      await downloadUpdate(({ received, total }) => state.set({ kind: "downloading", update: u, share: total ? received / total : 0 }));
      if (when === "restart") return void (await installAndRestart());
      await installOnQuit();
      state.set({ kind: "waiting", update: u });
    } catch (e) {
      state.set({ kind: "found", update: u });
      report(e);
    }
  };
  const s = () => state();
  const applyPackage = async (p: PackageUpdate) => {
    state.set({ kind: "package", update: p, share: 0 });
    try {
      await downloadPackage(({ received, total }) => state.set({ kind: "package", update: p, share: total ? received / total : 0 }));
      await restartToPackage();
    } catch (e) {
      state.set({ kind: "package", update: p, share: null });
      report(e);
    }
  };
  const found = () => (s().kind === "found" || s().kind === "downloading" || s().kind === "waiting" || s().kind === "package" ? (s() as { update: { version: string; shell?: boolean } }).update : null);
  const offered = () => (s().kind === "found" ? (s() as { update: AppUpdate }).update : null);
  // what is known now, in a sentence: and while it downloads, how far
  const text = () => {
    if (s().kind === "current") return t("updates.current");
    if (s().kind === "checking") return t("updates.checking");
    if (found() === null) return t("updates.hint");
    const said = [foundWords(found()!)];
    if (s().kind === "downloading") said.push(t("updates.downloading", { pct: count(Math.round((s() as { share: number }).share * 100)) }));
    if (s().kind === "waiting") said.push(t("updates.waiting"));
    if (offered()?.canInstall && installFirst(busy()) === "quit") said.push(t("updates.busy"));
    const notes = offered()?.notes;
    if (notes) said.push(notes);
    return said.join("\n");
  };
  return (
    <Section plain title={t("updates.title")} description={shell ? t("updates.running", { name: app.name ?? "", version: app.development ? `${app.version ?? ""} ${t("updates.dev")}` : (app.version ?? ""), nsh: shell.nshVersion }) : undefined} testID="updates">
      <Notice tone="info" icon={s().kind === "current" ? "circle-check" : found() !== null ? "download" : undefined} text={text()}>
        <If when={s().kind === "package" ? (s() as { update: PackageUpdate }).update : null}>
          {(p) => <Button small variant="primary" icon="refresh-cw" label={t("updates.restart")} onPress={() => void applyPackage(p())} disabled={(s() as { share: number | null }).share !== null} testID="updates-package" />}
        </If>
        <If when={offered()} else={s().kind === "package" || s().kind === "waiting" ? undefined : <Button small variant="outline" icon="refresh-cw" label={t("updates.check")} onPress={check} disabled={s().kind === "checking" || s().kind === "downloading"} testID="updates-check" />}>
          {(u) => (
            <If when={u().canInstall} else={<Button small variant="primary" icon="download" label={t("updates.download")} onPress={() => install(u(), "restart")} testID="updates-install" />}>
              {/* both installs; the one that cuts nothing off comes first while a recording or a job runs */}
              <Button small variant={installFirst(busy()) === "restart" ? "primary" : "outline"} icon="refresh-cw" label={t("updates.install")} onPress={() => install(u(), "restart")} testID="updates-install" />
              <Button small variant={installFirst(busy()) === "quit" ? "primary" : "outline"} icon="download" label={t("updates.onQuit")} onPress={() => install(u(), "quit")} testID="updates-on-quit" />
            </If>
          )}
        </If>
      </Notice>
    </Section>
  );
}
