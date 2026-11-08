// A session in time: a bar per operation from its start to its end, in its
// outcome's color, against the time from the session's start (nsh Plot: the
// pointer, the tooltip and the bars' fading run natively). A slice picked on
// the session's shape narrows it to the operations running then, and to
// that time; a press on a row chooses its operation, as the table's do.

import { Scroll, Text, View } from "nsh";
import { bytes } from "../format";
import { t } from "../i18n";
import { episodeTitle } from "../phrases";
import type { SessionView } from "../types";
import { type GanttRow, Gantt, offset } from "./plots";
import { duration, episodeTone, span } from "./sessionview";

const time = (iso: string) => new Date(iso).getTime();

/** At most this many bars: a longer session is narrowed by a slice of its shape. */
const MAX_ROWS = 1500;

export function Timeline(props: { view: SessionView; rows?: number[]; selected: number | null; onSelect: (i: number) => void; window?: [number, number] | null }) {
  const start = () => span(props.view)[0];
  const shown = () => (props.rows ?? props.view.episodes.map((_, i) => i)).slice(0, MAX_ROWS);
  const rows = (): GanttRow[] =>
    shown().map((i) => {
      const e = props.view.episodes[i];
      const tone = episodeTone(e);
      return {
        i,
        title: episodeTitle(e),
        start: time(e.start) - start(),
        // a bar too short to see still shows
        end: Math.max(time(e.end) - start(), time(e.start) - start() + 0.4),
        tone: tone === "refused" ? "server" : tone,
        detail: `${t("sessions.requestsN", { n: e.records.length })} · ↑${bytes(e.bytesUp)} ↓${bytes(e.bytesDown)}`,
      };
    });
  const domain = (): [number, number] => props.window ?? [0, Math.max(span(props.view)[1] - start(), 1)];
  const label = () => t("timeline.ganttText", { n: rows().length, total: offset(span(props.view)[1] - start()) });
  return (
    <View testID="timeline" class="min-h-0 flex-1 overflow-hidden rounded-(--radius-card) border border-border bg-card">
      <View class="h-9 flex-row items-center gap-3 border-b border-border px-3">
        <Text class="flex-1 text-small text-muted-foreground">{props.window ? t("timeline.window", { a: offset(domain()[0]), b: offset(domain()[1]) }) : t("timeline.pickSlice")}</Text>
        <If when={(props.rows ?? props.view.episodes).length > MAX_ROWS}>
          <Text class="text-small text-muted-foreground">{t("timeline.first", { n: MAX_ROWS })}</Text>
        </If>
      </View>
      <Scroll class="min-h-0 flex-1" contentClass="pb-3">
        <Gantt rows={rows()} domain={domain()} selected={props.selected} onSelect={props.onSelect} label={label()} />
      </Scroll>
    </View>
  );
}

export { duration };
