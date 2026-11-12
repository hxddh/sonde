# 17.0: the interface (M5's rough places, M7's UI)

Notes for the merge: changelog lines in the voice of `[16.0.0]`, and decisions proposed for
`docs/decisions.md`. The rows they answer are in [docs/review-16.0.md](../review-16.0.md) § The interface.

## Changelog lines

### Added

- **A budget drawn on its chart.** Where the session's program has a `[check]` in its project's `sonde.toml`,
  `max-p95` is a dashed line on latency over time, `max-errors` one on the error rate (that many of the run's
  requests), `max-waiting` one on the Runs pane's waiting, each on a chip ("budget 50 ms"); the chart's sentence
  says how far the run is from it.
- **A stretch of time kept in a link.** "Copy link to this stretch of time" puts the Time pane's two slices into a
  `sonde://` link (`?range=120-480`, ms from the session's start); it opens the session on its Time pane over that
  stretch. A shared page keeps its stretch in its address (`#range=120-480`) and opens on it. Still two presses:
  no drag, no wheel.
- **A chart's rows as CSV.** A button at the end of each chart's title on the Time pane (timeline, latency, bytes,
  sizes, errors, error rate) writes what the chart draws, a column per figure in one unit, where exports go.
- The README's pictures are made by the tour (`SONDE_README=1 scripts/shots.sh`), with three new ones: the trial
  plan and its dial, a trial over its clean run, a finding's advice in its row.

### Changed

- **The trial dialog started from Run opens a finished scenario's session**, as its text says and as the Runs
  pane always did.
- **A run under a second, or of fewer than 20 requests, draws no sparkline**: 16.0 said so and drew one under a
  second.
- The trial's dial is 1 to 5, as `--fault slowdown:N` takes. A screen reader hears each verdict as it arrives and
  the trial's end. A trial that sent fewer requests than its clean run says both counts ("1 of the clean run's 7
  requests"), never "0.1× the requests".
- In Chinese a trial the program broke under is 未通过; 失败 stays a failed request, job or budget.
- At the least window the findings' room ends on a finding's edge, never through its text, and says how many are
  in view and that the rest scroll.
- Activity's operation column is as wide as the longest name in it, and the target takes the rest; an upload's
  key and an object's name take the room the figures leave and are cut in the middle, never at the end. A tile's
  figure takes a second line before it is cut.
- Compare: the dumbbells say which dot is which run; a percentile of fewer than 20 requests is marked `~` on
  either side; "p95" is said once; a treemap's figures stand on a chip.
- The sizes chart says each bar's count and its scale writes zero alone; a bar of nothing beside one that has a
  count says 0. A value scale's peak is a chip over its own line. The Sankey's names keep off their own bars. The
  lanes' legend names only the outcomes the session had. A dashed line in a legend is a dash, not a dot. A
  slice's tooltip says what began in it, and on a session's strip that a press shows what ran then.
- Run's band says errors that are all 4xx in the client's tone; Run's command bar keeps the one primary until a
  run's band offers Run again. A selected segment reads lighter than its track in dark. Commands in the palette
  are in mono after `$ `.
- `paged.ts`: a range whose fetch failed is asked again; a live session scrolled past its first 500 rows keeps
  them as it grows; a new run in Run's table, sorted the same and as long, is read again.

### Lints

- `lint-kit` reads `App.tsx` (its two `font-semibold`, a hand-made back and home row are gone), fails a column's
  literal width (the kit's `COLUMN`), a literal `maxHeight` bound (`SIZES`), a pane's literal `minSize`/`maxSize`
  (`PANE`) and a pressable `Text`/`View` by hand.
- `lint-layout` reads the kit's `COLUMN` widths and fails a column of keys, paths, targets or names that its cell
  cuts at the end.
- `lint-charts`: where every bar says its count the scale is `marked` (zero alone), and only there; `TIME_TICK` is
  the time axis's alone (`BUCKET_TICK` for sizes, latencies and counts on a bottom axis).

## Decisions proposed

| Topic | Options | Choice and why |
|---|---|---|
| Chinese for a broke trial | 失败; 未通过; 中断 | **未通过**: a trial is a test the program did or did not pass; 中断 says the run stopped, which a broke trial need not have (it can exit 0 and lose a write). 失败 stays for a request, a job and a budget. In DESIGN's Words. |
| A short run's sparkline | under 20 requests only (16.0's code); under a second or under 20 (16.0's words) | **Both**: a line over 467 ms is a few slices of noise drawn as a shape, beside a figure that is a total. The same rule as the totals (D-122). |
| The findings' room at the least window | a fixed height; whole findings; no bound | **Whole findings, bounded, said**: the bound keeps the operations their rows (16.0's reason), rounded down to the last finding that fits, and the heading says "2 of 5 in view · scroll for more". A finding opened taller than the room keeps the room: its advice scrolls inside it. |
| Where a budget comes from for a chart | the check's verdicts (only with a baseline); `sonde.toml` read by the bridge | **`sonde.toml` from the session's folder**, the file `sonde check` reads, in one small bridge call (`Sessions.budget`) kept apart so the `project` module can replace it. A count budget (`max-errors`) is drawn on the error rate as that share of the run's requests; `max-waiting` is a run's total, so it is drawn over the program's runs, not over one run's time. |
| A range in a link | slice indices; ms from the session's start | **Milliseconds**: slices are the reader's (a live session's change length), times are the session's. D-123 stands: the range is still chosen by two presses. |
| A chart's CSV | the clipboard; a save dialog; the exports folder | **The exports folder**, said in a toast with its path: Native Shell has no save dialog, and a clipboard of 10 000 rows is not a file. The rows are what the chart draws, one unit per column, named in its title. |
| Width literals in pages | file-level constants; the kit's `COLUMN` and `PANE` | **The kit's**: one name per width, as `WIDTH` and `SIZES` already are; the layout lint reads them to check titles and cells fit. File-level constants that are derived (a name's least, the room a table leaves) stay. |
| One primary on Run | the command bar's Run; the band's Run again | **Run until a run has ended, then Run again**: the loop's next step is the band's; Run stays, outlined, for an edited command. |
