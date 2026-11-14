// A request of a session in the inspector: the request in full, the
// commands that redo it (aws s3api, curl without a signature), and its key's
// history in the session — every request to that key, whatever its
// operation, each a press away.

import { resource, Text, View } from "nsh";
import { clock, count, duration } from "../format";
import { t } from "../i18n";
import { api } from "../native";
import { copy } from "../store";
import type { KeyRequest, RequestRecord } from "../types";
import { Icon } from "./icon";
import { Button, Card, Disclosure, ICON, ListRow, Muted, WIDTH, TEXT } from "./kit";
import { RecordView } from "./record";
import { StatusMark } from "./sessionview";

/** How many of a key's requests the inspector lists around the one shown. */
const AROUND = 12;

export function RecordDetail(props: { path: string; record: RequestRecord; onBack?: () => void; /** another request of the key pressed */ onRecord: (id: number) => void }) {
  const r = () => props.record;
  const copyAs = (kind: "cli" | "curl") => {
    const text = api.sessions.copy(props.path, r().id, kind);
    if (text) copy(text);
  };
  return (
    <RecordView
      record={r()}
      onBack={props.onBack}
      actions={
        <>
          <Button small variant="outline" icon="copy" label={t("requests.copyCli")} onPress={() => copyAs("cli")} testID="copy-cli" />
          <Button small variant="outline" icon="copy" label={t("requests.copyCurl")} onPress={() => copyAs("curl")} testID="copy-curl" />
        </>
      }
      more={
        <If when={r().key}>
          <KeyHistory path={props.path} bucket={r().bucket ?? ""} keyName={r().key!} at={r().id} onRecord={props.onRecord} />
        </If>
      }
    />
  );
}

/** Every request of the record's key in the session, in time order, the one shown marked: what
 * happened to the key before and after it (written, read back, deleted, read again). Said only
 * when the key had more than this one. */
function KeyHistory(props: { path: string; bucket: string; keyName: string; at: number; onRecord: (id: number) => void }) {
  const list = resource(
    () => [props.path, props.bucket, props.keyName] as const,
    async ([p, b, k]): Promise<KeyRequest[]> => {
      try {
        return await api.sessions.keyHistory(p, b, k);
      } catch {
        return [];
      }
    },
  );
  // a window of them around the one shown
  const shown = () => {
    const all = list() ?? [];
    const i = Math.max(0, all.findIndex((x) => x.id === props.at));
    const from = Math.max(0, Math.min(i - Math.floor(AROUND / 2), all.length - AROUND));
    return { rows: all.slice(from, from + AROUND), from, total: all.length };
  };
  return (
    <If when={(list()?.length ?? 0) > 1}>
      <Disclosure open title={t("keyHistory.title")} count={list()!.length} testID="key-history">
        <Card pad="none">
          <Each of={shown().rows} key={(x) => `${x.id}`}>
            {(x, i) => (
              <ListRow
                divided
                first={i() === 0}
                testID={`key-request-${shown().from + i()}`}
                label={`${x().kind} ${clock(x().time, "ms")}`}
                selected={x().id === props.at}
                disabled={x().id === props.at}
                onPress={() => props.onRecord(x().id)}
                trailing={
                  // its time, then (another request than the one shown) the chevron, at the row's end
                  <>
                    <Text class={`shrink-0 tabular-nums ${TEXT.note}`}>{duration(x().latencyMs)}</Text>
                    {/* (the one shown keeps the chevron's room, so the times stand in one column) */}
                    <View class={x().id === props.at ? "opacity-0" : ""}>
                      <Icon name="chevron-right" size={ICON.small} />
                    </View>
                  </>
                }
              >
                <View class={WIDTH.status}>
                  <StatusMark status={x().status} injected={x().injected} />
                </View>
                <Text class={`min-w-0 flex-1 ${TEXT.small} line-clamp-1`}>{x().kind}</Text>
                <Text class={`shrink-0 ${TEXT.codeQuiet}`}>{clock(x().time, "ms")}</Text>
              </ListRow>
            )}
          </Each>
        </Card>
        <If when={shown().total > shown().rows.length}>
          <Muted class={TEXT.captionSize}>{t("keyHistory.more", { n: count(shown().rows.length), of: count(shown().total) })}</Muted>
        </If>
      </Disclosure>
    </If>
  );
}
