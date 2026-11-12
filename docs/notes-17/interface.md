# 17.0: the interface

Changelog lines and decisions for the merge. Rows answered: [review-16.0](../review-16.0.md) § The interface.

## Changelog

### Added

- **A budget on its chart**: `sonde.toml`'s `max-p95` on latency, `max-errors` on the error rate, `max-waiting`
  on the runs' waiting, each a dashed line and a chip; the chart's sentence says how far the run is.
- **A stretch of time in a link**: `sonde://…?range=120-480` opens a session's Time pane over it; a shared page
  keeps it in `#range=`. Still two presses.
- **A chart's rows as CSV**, from a button on each Time chart, into the exports folder.
- The tour makes the README's pictures (`SONDE_README=1 scripts/shots.sh`), with the trial plan, a trial over
  its clean run and a finding's advice.
- Names for the four new scenarios in both languages.

### Changed

- The trial dialog from Run opens a finished scenario's session. Its body scrolls; its buttons stay in view.
- A run under a second draws no sparkline.
- The trial's dial is 1 to 5. A screen reader hears each verdict. Fewer requests than the clean run read
  "1 of the clean run's 7 requests".
- Chinese: a broke trial is 未通过.
- The findings' room ends on a whole finding and says how many are in view.
- Free width goes to the column that would be cut; keys and names are cut in the middle; a tile's figure wraps.
- Compare: a legend for the dumbbells, `~` on percentiles of few, "p95" once, treemap figures on a chip.
- Charts: sizes say their counts and the scale only zero; the peak is a chip on its line; Sankey names clear of
  their bars; legends name only what is drawn, a dashed line as a dash.
- Run: 4xx errors in the client's tone; one primary. Dark: a selected segment is lighter than its track. The
  palette's commands are mono after `$ `.
- Paged tables re-ask a failed range, keep a live view past row 500, and read a new run again.

### Lints

- `lint-kit` reads `App.tsx`; fails a literal column width (`COLUMN`), `maxHeight` (`SIZES`), pane size
  (`PANE`), and a hand-made pressable.
- `lint-layout` fails a path column cut at its end.
- `lint-charts`: labelled bars have a zero-only scale, and only they; `TIME_TICK` is the time axis's alone.

## Decisions

| Topic | Options | Choice and why |
|---|---|---|
| Chinese for a broke trial | 失败; 未通过; 中断 | **未通过**: a trial is passed or not; 中断 says it stopped, which it need not. |
| A short run's sparkline | under 20 requests; under a second or 20 | **Both**: a line over half a second is noise beside a total (D-122). |
| The findings' bound | fixed height; whole findings | **Whole findings, said**: no row cut, the count of those in view. |
| A budget's source | the check's verdicts; `sonde.toml` | **`sonde.toml`** via `sonde_proxy::project` (`Sessions.budget`). `max-errors` as a share of the run; `max-waiting` over runs. |
| A range in a link | slice indices; ms | **ms**: slices are the reader's, times the session's. |
| A chart's CSV | clipboard; save dialog; exports folder | **Exports folder**: Native Shell has no save dialog. |
| Widths in pages | file constants; the kit's `COLUMN`, `PANE` | **The kit's**: one name each, read by the layout lint. |
| One primary on Run | Run; Run again | **Run, then Run again once a run ended**. |
| A dialog taller than the window | fixed scroll height; body scrolls | **`ModalBody`**: the dialog is at most the window, its body scrolls, its actions stay. |
