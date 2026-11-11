// Looking after sessions: a title of the user's, removing one, sharing one
// as a web page. Each is in the session's context menu and its ⋯ menu.

import { effect, openUrl, signal, untracked } from "nsh";
import { AlertDialog } from "nsh/base";
import { t } from "../i18n";
import { api } from "../native";
import { forgetRun } from "../run";
import { copy, notify, report, sessionsChanged } from "../store";
import type { SessionInfo } from "../types";
import { Actions, Button, Field, Modal, Title } from "./kit";
import { programOf } from "./sessionview";
import { toast } from "./toast";

/** A path's last part, with either separator (Windows' too). */
export const fileName = (path: string) => path.split(/[\\/]/).pop() || path;

/** What a session is called: the user's title, else its program, else its endpoint. */
export const sessionName = (s: Pick<SessionInfo, "title" | "command" | "endpoint">) => s.title || programOf(s.command) || s.endpoint;

/** The session being renamed, while the dialog is open, and the window that
 * asked (a session's own window, by its path; null: the main window). */
const asked = signal<{ session: SessionInfo; window: string | null } | null>(null);
export const rename = (s: SessionInfo, window: string | null = null) => asked.set({ session: s, window });

/** The dialog that gives a session a title (an empty one takes it away):
 * one per window, open only in the window that asked. */
export function RenameDialog(props: { window?: string }) {
  const renaming = () => {
    const a = asked();
    return a && a.window === (props.window ?? null) ? a.session : null;
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
      // an empty title takes the user's away: the session is called by its program again
      api.sessions.rename(s.path, draft().trim());
      asked.set(null);
      sessionsChanged();
    } catch (e) {
      report(e);
    }
  };
  return (
    <Modal open={renaming() !== null} onClose={() => asked.set(null)} size="narrow" testID="rename-dialog">
      <Title>{t("sessions.renameTitle")}</Title>
      <Field label={t("sessions.renameLabel")} value={draft()} onChange={(v) => draft.set(v)} onSubmit={save} placeholder={renaming() ? (programOf(renaming()!.command) ?? renaming()!.endpoint) : ""} testID="rename-input" />
      <Actions>
        <Button variant="ghost" label={t("common.cancel")} onPress={() => asked.set(null)} />
        <Button variant="primary" label={t("common.save")} onPress={save} testID="rename-save" />
      </Actions>
    </Modal>
  );
}

/** Asks, then removes a session's file. Resolves whether it went. */
export async function removeSession(s: SessionInfo, removed?: () => void): Promise<boolean> {
  const { action } = await AlertDialog.show({
    title: t("sessions.removeTitle", { name: sessionName(s) }),
    description: t("sessions.removeText", { n: s.requests }),
    actions: [
      { id: "cancel", label: t("common.cancel"), role: "cancel" },
      { id: "remove", label: t("sessions.remove"), role: "destructive" },
    ],
  });
  if (action !== "remove") return false;
  try {
    await api.sessions.remove(s.path);
    // Run no longer offers to open or run again what is gone
    forgetRun(s.path);
    removed?.();
    sessionsChanged();
    notify(t("sessions.removed", { name: sessionName(s) }));
    return true;
  } catch (e) {
    report(e);
    return false;
  }
}

/** Writes a session as a web page, opens it in the browser, and says where it is. */
export async function shareSession(path: string) {
  try {
    const r = await api.sessions.share(path);
    void openUrl(r.url);
    toast({
      id: "shared",
      kind: "success",
      title: t("share.ready"),
      description: t("share.where", { dir: r.dir }),
      // the page's folder in the file manager, where it is a press away
      action: t("share.showFolder"),
      onAction: () => void api.sessions.reveal(r.dir).catch(() => copy(r.dir)),
      duration: 10000,
    });
  } catch (e) {
    // a build without the viewer: the command that makes the page
    // quoted: a path with spaces is one argument
    const command = `sonde share --serve "${path.replace(/(["\\$`])/g, "\\$1")}"`;
    copy(command, t("share.command"));
    if (!`${e instanceof Error ? e.message : e}`.includes("no web viewer")) report(e);
  }
}
