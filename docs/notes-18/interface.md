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

### Added (the second wave: the other streams' features, wired in)

- **A finding's state across runs.** A run's findings are marked against the program's run before — *new*,
  *back*, *less* (*open* is left unmarked: it says nothing new). Beside them, "Since the run before": requests,
  waiting and errors before → after, and each finding *fixed* or *back*, a fixed one with what it saved, measured,
  against the predicted. In Run after a run (beside its top findings) and in a session's header; its Compare opens
  the two runs.
- **Compare says what changed of the findings**: fixed, less (with how it read before), back, new, each with its
  measured saving; the earlier run is always "before", whichever was opened.
- **Try again what broke.** From the trial's dialog once it ended, and from the Runs pane: the program runs
  afresh, then only the scenarios that broke, degraded or were refused; the fresh run opens on its Runs pane.
- Each scenario says its history: "Broke on Nov 12, survived since". A verdict the recording policy refused has
  a tone of its own (`status-refused`), never a failure's or a degraded run's.
- "Try under faults…" in a session's header too.
- **Group by** on the operations: one control (each operation, kind of request, prefix, size, code line, SDK,
  status) splits the table into groups with its figures (requests, errors, p50, p95, p99, waiting, cost) and one
  sentence on where the slow and costly requests are. A finding or a slice chosen shows the operations again.
- **A request's key history** in the inspector: every request of its key in the session, the one shown marked,
  another a press away.
- The SDK that sent a run's requests, in its header ("from boto3 1.34").
- A narrow session header keeps the command's room: Try, Replay and Compare by their icons (their words in their
  tooltips). Run says what acting on a finding saves under its words, so the words are never cut to nothing.
- The tour's program makes a finding (its config read ten times) and breaks under faults (a write curl never
  retries); the tour tries again what broke and groups the operations.

### Removed

- The Compatibility window and its "Check compatibility…" button in Settings › Endpoints: Compatibility is a
  section of Buckets.
- **Activity › Requests**, sonde's own requests: its list, export, "Open as a session", and every "Show request"
  (the error toasts, a connection test, an object, a probe). An error says itself: what went wrong, the store's
  code and status, the request id to quote. A session's requests stay in its inspector. Activity is jobs and
  changes.
- The bucket's context menu entries for its tools (they were a second door to the same sections), and the sheets
  over the objects.

### Fixed

- A shared note (a conditional create on every SlateDB write) was said nowhere once it stopped being a finding:
  it is each operation's own note again.
- Compare read a run against the run before it backwards when opened from the later run (Run again).

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
| D-1xx | A finding's state in a session | a badge on every finding; only the telling ones; a column | **New, back, less on the finding; fixed and back beside them, once** ("Since the run before"); open unmarked: a badge on every row is noise. |
| D-1xx | Where Group by lives | a pane; a tab; a control on the operations | **One control beside the operations' filter**: the same place, split; no new pane. |
| D-1xx | sonde's own requests | a list in Activity; nowhere | **Nowhere**: an error says itself (code, status, request id); a session's requests are in its inspector. |
| D-1xx | Compare's order | as opened; earlier first | **Earlier first**: "before → after" reads true from Run again, Since, and the menu alike. |
