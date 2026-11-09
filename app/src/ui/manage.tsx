// Looking after sessions: a title of the user's, removing one, sharing one
// as a web page. Each is in the session's context menu and its ⋯ menu.

import { Clipboard, effect, openUrl, signal, Text, untracked, View } from "nsh";
import { AlertDialog } from "nsh/base";
import { t } from "../i18n";
import { api } from "../native";
import { notify, report, sessionsChanged } from "../store";
import type { SessionInfo } from "../types";
import { Button, Field, Modal, Title } from "./kit";
import { programOf } from "./sessionview";
import { toast } from "./toast";

/** What a session is called: the user's title, else its program, else its endpoint. */
/** A path's last part, with either separator (Windows' too). */
export const fileName = (path: string) => path.split(/[\\/]/).pop() || path;

export const sessionName = (s: Pick<SessionInfo, "title" | "command" | "endpoint">) => s.title || programOf(s.command) || s.endpoint;

/** The session being renamed, while the dialog is open. */
const renaming = signal<SessionInfo | null>(null);
export const rename = (s: SessionInfo) => renaming.set(s);

/** The dialog that gives a session a title (an empty one takes it away). */
export function RenameDialog() {
  const draft = signal("");
  // each opening starts from the session's present name
  effect(() => {
    const s = renaming();
    if (s) untracked(() => draft.set(s.title ?? programOf(s.command) ?? ""));
  });
  const save = () => {
    const s = renaming();
    if (!s) return;
    try {
      api.sessions.rename(s.path, draft());
      renaming.set(null);
      sessionsChanged();
    } catch (e) {
      report(e);
    }
  };
  return (
    <Modal open={renaming() !== null} onClose={() => renaming.set(null)} size="narrow" testID="rename-dialog">
      <Title>{t("sessions.renameTitle")}</Title>
      <Field label={t("sessions.renameLabel")} value={draft()} onChange={(v) => draft.set(v)} onSubmit={save} placeholder={renaming() ? (programOf(renaming()!.command) ?? renaming()!.endpoint) : ""} testID="rename-input" />
      <View class="flex-row justify-end gap-2">
        <Button variant="ghost" label={t("common.cancel")} onPress={() => renaming.set(null)} />
        <Button variant="primary" label={t("common.save")} onPress={save} testID="rename-save" />
      </View>
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
      onAction: () => void api.sessions.reveal(r.dir).catch(() => Clipboard.writeText(r.dir).then(() => notify(t("common.copied")))),
      duration: 10000,
    });
  } catch (e) {
    // a build without the viewer: the command that makes the page
    // quoted: a path with spaces is one argument
    const command = `sonde share --serve "${path.replace(/(["\\$`])/g, "\\$1")}"`;
    void Clipboard.writeText(command).then(() => notify(t("share.command")), report);
    if (!String(e instanceof Error ? e.message : e).includes("no web viewer")) report(e);
  }
}
