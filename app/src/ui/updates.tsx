// Updates: is there a newer sonde, and installing it. The desktop app
// replaces itself and restarts (Native Shell's updater, from the project's
// GitHub Releases); elsewhere — a system install, the terminal binary — it
// links to the release.

import { effect, openUrl, signal, untracked } from "nsh";
import { type AppUpdate, checkForPackage, checkForUpdate, downloadPackage, downloadUpdate, installAndRestart, type PackageUpdate, restartToPackage, shellInfo } from "nsh/updater";
import { t } from "../i18n";
import { report } from "../store";
import { Button, Notice, Section } from "./kit";

type State =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "current" }
  | { kind: "found"; update: AppUpdate }
  | { kind: "downloading"; update: AppUpdate; share: number }
  // the app's code alone, signed: no new app to install, a restart runs it
  | { kind: "package"; update: PackageUpdate; share: number | null };

/** Asks the Updates section to check now (the palette's "Check for updates"). */
export const checkRequested = signal(0);
let answered = 0;

export function Updates() {
  const state = signal<State>({ kind: "idle" });
  // what this is: the version installed and the Native Shell runtime it runs on
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
        const p = await checkForPackage().catch(() => null);
        if (p) return void state.set({ kind: "package", update: p, share: null });
      }
      const u = await checkForUpdate();
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
  const install = async (u: AppUpdate) => {
    if (!u.canInstall) return void openUrl(u.url);
    state.set({ kind: "downloading", update: u, share: 0 });
    try {
      await downloadUpdate(({ received, total }) => state.set({ kind: "downloading", update: u, share: total ? received / total : 0 }));
      await installAndRestart();
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
  const found = () => (s().kind === "found" || s().kind === "downloading" || s().kind === "package" ? (s() as { update: { version: string } }).update.version : null);
  // what is known now, in a sentence: and while it downloads, how far
  const text = () => {
    if (s().kind === "current") return t("updates.current");
    if (s().kind === "checking") return t("updates.checking");
    if (found() === null) return t("updates.hint");
    const said = [t("updates.found", { version: found()! })];
    if (s().kind === "downloading") said.push(t("updates.downloading", { pct: String(Math.round((s() as { share: number }).share * 100)) }));
    const notes = s().kind === "found" ? (s() as { update: AppUpdate }).update.notes : null;
    if (notes) said.push(notes);
    return said.join("\n");
  };
  return (
    <Section plain title={t("updates.title")} description={shell ? t("updates.running", { version: shell.version ?? t("updates.dev"), nsh: shell.nshVersion }) : undefined} testID="updates">
      <Notice tone="info" icon={s().kind === "current" ? "circle-check" : found() !== null ? "download" : undefined} text={text()}>
        <If when={s().kind === "package" ? (s() as { update: PackageUpdate }).update : null}>
          {(p) => <Button small variant="primary" icon="refresh-cw" label={t("updates.restart")} onPress={() => void applyPackage(p())} disabled={(s() as { share: number | null }).share !== null} testID="updates-package" />}
        </If>
        <If when={s().kind === "found" ? (s() as { update: AppUpdate }).update : null} else={s().kind === "package" ? undefined : <Button small variant="outline" icon="refresh-cw" label={t("updates.check")} onPress={check} disabled={s().kind === "checking" || s().kind === "downloading"} testID="updates-check" />}>
          {(u) => <Button small variant="primary" icon="download" label={u().canInstall ? t("updates.install") : t("updates.download")} onPress={() => install(u())} testID="updates-install" />}
        </If>
      </Notice>
    </Section>
  );
}
