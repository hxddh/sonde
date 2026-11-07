// A request in full, with the commands that redo it (aws s3api, curl
// without a signature): the inspector's view in Activity and in Sessions.

import { Clipboard } from "nsh";
import { t } from "../i18n";
import { api } from "../native";
import { notify, report } from "../store";
import type { RequestRecord } from "../types";
import { Button } from "../ui/kit";
import { RecordView } from "../ui/record";

export function RecordDetail(props: { record: RequestRecord; onBack?: () => void; copyAs?: (kind: "cli" | "curl") => string | null }) {
  const r = () => props.record;
  const copy = (text: string | null) => {
    if (!text) return;
    Clipboard.writeText(text).then(() => notify(t("common.copied")), report);
  };
  return (
    <RecordView
      record={r()}
      onBack={props.onBack}
      actions={
        <>
          <Button small variant="outline" icon="copy" label={t("requests.copyCli")} onPress={() => copy(props.copyAs ? props.copyAs("cli") : api.copyAsAwsCli(r().id))} testID="copy-cli" />
          <Button small variant="outline" icon="copy" label={t("requests.copyCurl")} onPress={() => copy(props.copyAs ? props.copyAs("curl") : api.copyAsCurl(r().id))} testID="copy-curl" />
        </>
      }
    />
  );
}
