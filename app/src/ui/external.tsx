// A command from a run sonde did not record here (a file opened, dropped
// or linked to), asked about before it runs. The run's file is anyone's:
// its command and its folder are whatever its author wrote, and Run again
// or a trial would start them through the user's shell. So they are shown
// first, whole and exactly as they would run (no value hidden, nothing cut,
// every character that would not show as itself written out), and nothing
// starts without a yes to them. Rust holds the bridge to the same: it
// starts such a run's command only with the command and folder confirmed
// here, to the character.

import { Text, View } from "nsh";
import { exactText } from "../command";
import { t } from "../i18n";
import type { Confirmed } from "../types";
import { Actions, Button, Labeled, Modal, ModalBody, Muted, Notice, TEXT, Title } from "./kit";

/** A command and the folder it runs in, exactly: every line of it, wrapped, never cut or scrubbed; what would not
 * show as itself (a line break, an escape, a character that reorders the text) as its code point, and said so. */
export function ExactCommand(props: { command: string; cwd: string | null; endpoint?: string | null }) {
  const command = () => exactText(props.command);
  const cwd = () => (props.cwd ? exactText(props.cwd) : null);
  const written = () => command() !== props.command || (cwd() ?? "") !== (props.cwd ?? "");
  return (
    <View class="gap-3">
      <Labeled label={t("external.command")}>
        {/* (a well, as code has: the whole command, on as many lines as it takes) */}
        <View selectable class="min-w-0 flex-1 rounded-(--radius) bg-muted/60 px-3 py-2">
          <Text testID="exact-command" accessibilityLabel={command()} class={TEXT.code}>
            {command()}
          </Text>
        </View>
      </Labeled>
      <Labeled label={t("external.cwd")}>
        <View selectable class="min-w-0 flex-1">
          <Text testID="exact-cwd" class={TEXT.code}>
            {cwd() ?? t("run.cwdHome")}
          </Text>
        </View>
      </Labeled>
      <If when={props.endpoint}>
        {(e) => (
          <Labeled label={t("common.endpoint")}>
            <Text testID="exact-endpoint" class={TEXT.code}>
              {exactText(e())}
            </Text>
          </Labeled>
        )}
      </If>
      <If when={written()}>
        <View testID="exact-unseen">
          <Muted>{t("external.unseen")}</Muted>
        </View>
      </If>
    </View>
  );
}

/** Run again, of a run from elsewhere: what would run, and Cancel or the yes to exactly that. */
export function ConfirmRunDialog(props: { ask: Confirmed; endpoint?: string | null; onClose: () => void; onConfirm: () => void }) {
  return (
    <Modal open size="wide" onClose={props.onClose} testID="external-run" describe={t("external.text")}>
      <Title>{t("external.title")}</Title>
      <ModalBody>
        <Notice testID="external-run-why" tone="warn" text={t("external.text")} />
        <ExactCommand command={props.ask.command} cwd={props.ask.cwd} endpoint={props.endpoint} />
      </ModalBody>
      <Actions>
        <Button variant="ghost" label={t("common.cancel")} onPress={props.onClose} testID="external-run-cancel" />
        <Button variant="primary" icon="play" label={t("external.run")} onPress={props.onConfirm} testID="external-run-start" />
      </Actions>
    </Modal>
  );
}
