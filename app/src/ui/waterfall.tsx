// A session's operations as a waterfall: one bar per operation, from when it
// began to when it ended, so what ran together and what waited show.

import { Scroll, View } from "nsh";
import { Bar, Grid, Plot, PlotAxis } from "nsh/base/plot";
import { ms } from "../format";
import type { SessionView } from "../types";

const MAX = 400;
const ROW = 16;

export function Waterfall(props: { view: SessionView; onSelect: (i: number) => void }) {
  const rows = () => {
    const eps = props.view.episodes.slice(0, MAX);
    const t0 = eps.length ? new Date(eps[0].start).getTime() : 0;
    return eps.map((e, i) => ({
      label: `${i + 1}. ${e.title}`,
      start: new Date(e.start).getTime() - t0,
      // a zero-length operation still gets a sliver
      end: Math.max(new Date(e.end).getTime() - t0, new Date(e.start).getTime() - t0 + 0.5),
    }));
  };
  return (
    <Scroll class="flex-1 rounded-(--radius) border border-border" testID="waterfall">
      <View style={{ height: rows().length * ROW + 40 }} class="p-2">
        <Plot
          data={rows()}
          y="label"
          yScale={{ type: "band", paddingInner: 0.25 }}
          xScale={{ type: "linear" }}
          interactive
          accessibilityLabel="Operations over time"
          class="flex-1 text-xs text-muted-foreground"
          onHoverChange={() => {}}
        >
          <Grid />
          <Bar value="end" base="start" class="fill-primary" inactiveOpacity={0.5} />
          <PlotAxis placement="bottom" format={(v) => ms(v)} />
          <PlotAxis placement="left" />
        </Plot>
      </View>
    </Scroll>
  );
}
