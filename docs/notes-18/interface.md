# 18.0 — the interface stream

What this stream changed in the app (plan §5 "Store tools in one place", §7 "Refined, and kept so", and the app's
part of §1), for the release's changelog and decision log. The second UI wave wires the other streams' features
(Group by, verdicts across runs, the fix loop's saving) into what is here.

## Changelog lines

### Changed

- **Run shows the result.** When a run ends, Run says what it did in one sentence, then its top three findings, each
  with what acting on it saves, and the one figure for acting on them all. A finding pressed opens the session on
  it, its fix open.
- **One "if you act on these" figure.** The session's headline says what the run did and where its time went; the
  second what-if it carried ("Acting on the first finding: …") is gone. What acting on the findings saves is said
  once, beside them.
- **The Time pane reads in one order** — Traffic, Latency, Errors, Where — with every chart kept and shown. Each
  chart's title is the question it answers ("Where did the time go?", "Which requests were slow?"), in English
  and Chinese. The table by operation opens Latency.
- **Every Time chart follows the selection.** The stretch picked on the timeline zooms the charts over time, and
  every other chart (the table by operation, sizes, where the time went, the latency histogram, first byte, the
  answers, prefixes, cost, flows) is reckoned again from the requests that started in it, and says which stretch.
  A trial over its clean run and the chains of requests zoom with it too.
- **Buckets holds the store's tools.** Objects, Size, Incomplete uploads, Versions, Layouts and Compatibility are
  one row of sections in Buckets (as the terminal already had them), not sheets over the objects and a window of
  their own. The tiles over the objects lead to the same sections.
- **A finding leads to its fix in the store.** "Incomplete uploads that were never completed or aborted" opens
  Buckets › Incomplete uploads on the bucket the requests went to.
- **A compatibility run outlives its page.** Leaving the section no longer stops the probes; back there, it shows
  how far they got, then the matrix. Its jobs are in Activity, where they can be stopped.
- The incomplete uploads' table says how old each upload is and no longer the day it began, so it fits the page at
  the least window.
- **The heat maps read in dark.** The prefix and latency heat maps ramp between tones of their own (`heat-low`,
  `heat-high`), checked against the card in light and dark: a cell of one request no longer disappears into the
  card.
- **A bar of none says 0.** Where a pair of bars has one with a count (sizes sent and received) or a histogram has an
  empty bucket between full ones, the empty one says "0" on a sliver of a bar.
- **The screenshot tour gates a merge.** CI compares each platform's tour with the last green tour of the default
  branch; a screen that changed past 0.5% of its pixels (font smoothing aside), or is new, must match its approved
  picture in `app/shots.approved.json`, or the job fails. Approving is running `scripts/shots-diff.py approve` on the
  run's screenshots and committing the file.

### Removed

- The Compatibility window and its "Check compatibility…" button in Settings › Endpoints: Compatibility is a
  section of Buckets.
- The bucket's context menu entries for its tools (they were a second door to the same sections), and the sheets
  over the objects.

### Fixed

- The middle of a name cut in its middle shows a gap on Linux: a Native Shell limit, recorded as FINDINGS 24.

## Decisions

| # | Topic | Options | Chosen |
|---|---|---|---|
| D-1xx | What Run shows after a run | the operations only; a link to the session; the findings | **Its top three findings**, with their savings and the one what-if; a press opens the finding in the session. |
| D-1xx | The what-if in a session | headline and findings bar; one | **One, beside the findings**: the headline says what the run did. |
| D-1xx | The Time pane's charts | ~8 by default and "more"; all | **All, shown**, in one reading order (Traffic, Latency, Errors, Where), each titled by its question. |
| D-1xx | A chart not over time, under a stretch | whole session; recomputed in JS from samples; recomputed in Rust | **Recomputed in Rust** (`sessions.stretch`, the requests that started in it): samples are capped, prefixes and costs are not in them. A shared page cannot, and says "of the whole session". |
| D-1xx | Store tools | tiles opening sheets, a window, context menus; sections | **One row of sections in Buckets** (desktop and terminal alike); tiles and findings lead to them. Supersedes D-044's window for Compatibility. |
| D-1xx | A compatibility run left behind | stopped with its page; kept | **Kept**: it takes minutes; its jobs are in Activity. |
| D-1xx | Interaction | per screen; one grammar | **Select → inspect → act**: one selection marked once, one inspector, every act also in the palette or on a key (DESIGN.md). |
| D-1xx | Heat maps' ramp | `bar-track` → `chart-1`; tones of their own | **`heat-low` → `heat-high`**, held by lint-charts against the card in light and dark. |
| D-1xx | A bar of 0 | unsaid; a label without a bar; a sliver | **A sliver** (`ZERO_SLIVER`) under its "0": Native Shell draws no label for a bar of no value. |
| D-1xx | Approving a visual change | a person looks at the artifact; a committed baseline | **A committed baseline of fingerprints** (`app/shots.approved.json`, a 40×25 grid of greys per picture): a change past 0.5% that does not match it fails CI. |
