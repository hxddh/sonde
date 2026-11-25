// Looking after runs: a title of the user's, removing one, sharing one
// as a web page. Each is in the run's context menu and its ⋯ menu.

import { effect, signal, untracked } from "nsh";
import { t } from "../i18n";
import { api } from "../native";
import { forgetRun } from "../run";
import { copy, notify, report, runsChanged } from "../store";
import type { RunInfo } from "../types";
import { Actions, Button, Field, Modal, Title, confirmAlert } from "./kit";
import { runName } from "./runlist";
import { programOf } from "./runview";
import { toast } from "./toast";

/** A path's last part, with either separator (Windows' too). */
export const fileName = (path: string) => path.split(/[\\/]/).pop() || path;

export { runName } from "./runlist";

/** The run being renamed, while the dialog is open, and the window that
 * asked (a run's own window, by its path; null: the main window). */
const asked = signal<{ run: RunInfo; window: string | null } | null>(null);
export const rename = (s: RunInfo, window: string | null = null) => asked.set({ run: s, window });

/** The dialog that gives a run a title (an empty one takes it away):
 * one per window, open only in the window that asked. */
export function RenameDialog(props: { window?: string }) {
  const renaming = () => {
    const a = asked();
    return a && a.window === (props.window ?? null) ? a.run : null;
  };
  const draft = signal("");
  // each opening starts from the title the user gave (none: empty, the
  // program's name as the placeholder), so Save unchanged changes nothing
  effect(() => {
    const s = renaming();
    if (s) untracked(() => draft.set(s.title ?? ""));
  });
  const save = () => {
    const s = renaming();
    if (!s) return;
    try {
      // an empty title takes the user's away: the run is called by its program again
      api.runs.rename(s.path, draft().trim());
      asked.set(null);
      runsChanged();
    } catch (e) {
      report(e);
    }
  };
  return (
    <Modal open={renaming() !== null} onClose={() => asked.set(null)} testID="rename-dialog">
      <Title>{t("runs.renameTitle")}</Title>
      <Field label={t("runs.renameLabel")} value={draft()} onChange={(v) => draft.set(v)} onSubmit={save} placeholder={renaming() ? (programOf(renaming()!.command) ?? renaming()!.endpoint) : ""} testID="rename-input" />
      <Actions>
        <Button variant="ghost" label={t("common.cancel")} onPress={() => asked.set(null)} />
        <Button variant="primary" label={t("common.save")} onPress={save} testID="rename-save" />
      </Actions>
    </Modal>
  );
}

/** Asks, then removes a run's file. Resolves whether it went. */
export async function removeRun(s: RunInfo, removed?: () => void): Promise<boolean> {
  if (!(await confirmAlert({ title: t("runs.removeTitle", { name: runName(s) }), text: t("runs.removeText", { n: s.requests }), commit: t("runs.remove") }))) return false;
  try {
    await api.runs.remove(s.path);
    // Run no longer offers to open or run again what is gone
    forgetRun(s.path);
    removed?.();
    runsChanged();
    notify(t("runs.removed", { name: runName(s) }));
    return true;
  } catch (e) {
    report(e);
    return false;
  }
}

/** Writes a run as one web page, opens it in the browser, and says where the file is. */
export async function shareRun(path: string) {
  try {
    const r = await api.runs.share(path);
    // the system opens it (openUrl opens no file: address)
    api.runs.openPage(r.file).catch(report);
    toast({
      id: "shared",
      kind: "success",
      title: t("share.ready"),
      description: t("share.where", { file: r.file }),
      // the page in the file manager, where it is a press away
      action: t("share.showFile"),
      onAction: () => void api.runs.reveal(r.file).catch(() => copy(r.file)),
      duration: 10000,
    });
  } catch (e) {
    // a build without the viewer: the command that makes the page
    // quoted: a path with spaces is one argument
    const command = `sonde share "${path.replace(/(["\\$`])/g, "\\$1")}"`;
    copy(command, t("share.command"));
    if (!`${e instanceof Error ? e.message : e}`.includes("no web viewer")) report(e);
  }
}
