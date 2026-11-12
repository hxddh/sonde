// Group by: the operations table split by one dimension — kind of request,
// prefix, size, the program's line, SDK or status — each group with the
// table's figures, and one sentence on where the slow and the costly
// requests are. Read off the interface's thread (`Sessions.split`).

import { resource, Text, View } from "nsh";
import { cost, count, duration, percent } from "../format";
import { t } from "../i18n";
import { say } from "../phrases";
import { api } from "../native";
import type { Split, SplitBy } from "../types";
import { Icon } from "./icon";
import { Await, Choice, SimpleTable } from "./kit";

export type GroupBy = SplitBy | "none";
export const GROUP_BY: GroupBy[] = ["none", "operation", "prefix", "size", "caller", "sdk", "status"];

/** The one Group by control: each operation (none), or the operations split by a dimension. */
export function GroupByChoice(props: { value: GroupBy; onChange: (v: GroupBy) => void }) {
  return <Choice inline label={t("split.groupBy")} testID="group-by" value={props.value} items={GROUP_BY.map((v) => ({ value: v, title: t(`split.by.${v}`) }))} onChange={(v) => props.onChange(v as GroupBy)} />;
}

/** The split: its sentence, then a row a group with the operations table's figures. */
export function SplitTable(props: { path: string; by: SplitBy }) {
  const split = resource(
    () => [props.path, props.by] as const,
    ([p, by]): Promise<Split> => api.sessions.split(p, by),
  );
  const ms = (v: number) => duration(v);
  const rows = (s: Split) =>
    s.rows.map((r) => [
      r.other ? t("split.other") : r.key || "—",
      count(r.requests),
      r.errors ? `${count(r.errors)} (${percent(r.errorShare)})` : "0",
      ms(r.p50Ms),
      ms(r.p95Ms),
      ms(r.p99Ms),
      percent(r.share),
      cost(r.cost, s.currency),
    ]);
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
            <SimpleTable
              testID="split-table"
              columns={[t(`split.by.${s().by}`), t("column.requests"), t("figure.errors"), "p50", "p95", "p99", t("profile.waiting"), t("profile.cost")]}
              rows={rows(s())}
              end={[1, 2, 3, 4, 5, 6, 7]}
              fill={0}
              max={14}
              mono={false}
            />
          </View>
        )}
      </If>
    </Await>
  );
}
