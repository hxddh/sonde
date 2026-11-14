// Group by: the operations table split by one dimension — kind of request,
// prefix, size, the program's line, SDK or status — each group in the one
// table of groups the Time pane has by kind (`GroupTable`), and one sentence
// on where the slow and the costly requests are. Read off the interface's
// thread (`Sessions.split`). A kind of request chosen narrows the operations
// to it.

import { resource, Text, View } from "nsh";
import { t } from "../i18n";
import { say } from "../phrases";
import { api } from "../native";
import type { Split, SplitBy } from "../types";
import { Icon } from "./icon";
import { Await, Choice } from "./kit";
import { GroupTable, groupRows } from "./profile";

export type GroupBy = SplitBy | "none";
export const GROUP_BY: GroupBy[] = ["none", "kind", "prefix", "size", "caller", "sdk", "status"];

/** The one Group by control: each operation (none), or the operations split by a dimension. */
export function GroupByChoice(props: { value: GroupBy; onChange: (v: GroupBy) => void }) {
  return <Choice inline label={t("split.groupBy")} testID="group-by" value={props.value} items={GROUP_BY.map((v) => ({ value: v, title: t(`split.by.${v}`) }))} onChange={(v) => props.onChange(v as GroupBy)} />;
}

/** The split: its sentence, then a row a group. By kind of request, a row
 * pressed is that kind's operations (`onKind`). */
export function SplitTable(props: { path: string; by: SplitBy; onKind?: (kind: string) => void }) {
  const split = resource(
    () => [props.path, props.by] as const,
    ([p, by]): Promise<Split> => api.sessions.split(p, by),
  );
  // a kind's row (not the rest folded together) narrows the operations to it
  const kindOf = (s: Split, name: string | null) => (name === null ? null : (s.rows.find((r) => !r.other && (r.key || "—") === name)?.key ?? null));
  return (
    <Await loading={split.loading() && !split()} error={split.error()} retry={() => split.refetch()} testID="split">
      <If when={split()}>
        {(s) => (
          <View class="min-h-0 flex-1 gap-2" testID="split-view">
            {/* where the slow and the costly requests are, in a sentence */}
            <If when={say(s().phrases, s().text)}>
              {(line) => (
                <View class="flex-row items-start gap-2 px-1">
                  <View class="min-h-5 justify-center">
                    <Icon name="sparkles" />
                  </View>
                  <Text testID="split-sentence" class="min-w-0 flex-1 text-body leading-5 text-foreground">{line()}</Text>
                </View>
              )}
            </If>
            <GroupTable
              testID="split-table"
              rows={groupRows(s())}
              title={s().by === "kind" ? t("column.kind") : t(`split.by.${s().by}`)}
              currency={s().currency}
              max={14}
              onSelect={
                s().by === "kind" && props.onKind
                  ? (name) => {
                      const k = kindOf(s(), name);
                      if (k) props.onKind!(k);
                    }
                  : undefined
              }
            />
          </View>
        )}
      </If>
    </Await>
  );
}
