# 14.0 reviewed, 15.0 proposed

A review of 14.0.0 after its release, by readers who did not write it: the Rust for bugs (the released binary run
against a fake store), the app's code against its own standard, the product used as a newcomer would use it (curl,
boto3 and pylance programs against the built-in store), Native Shell at its newest commit, and every screen looked
at on a real window. Then what 15.0 should be. 14.0's own review is [review-13.0.md](review-13.0.md).

## What 14.0 got wrong

### Trials: the verdict cannot be trusted yet

14.0's one new claim is "sonde says whether it survived". Checked on real programs it was right most of the time
(boto3 with and without retries, curl with and without `--retry`, multipart, pylance), and wrong in ways that matter:

| | Where | What |
|---|---|---|
| High | `proxy/trial.rs` `judge` | **A program that never retries can be "survived".** Any later request with the same method and key is taken as the retry of a failed one. A program that writes `state`, polls, and writes `state` again has its lost first write read as "retried, after 313 ms" — the wait is its own sleep. Confirmed by running it. |
| High | `proxy/fault.rs`, `trial.rs` | **Requests without a key share one identity.** Two `DeleteObjects` batches are one request: only the first meets the fault, the second is read as its retry, and the first batch's objects are never deleted. The same for listings paged by `start-after` or a key marker. Confirmed. |
| High | `app` `Sessions.tsx`, `sonde-nsh` | **A trial's session cannot be opened in the app.** A press on a trial's row does nothing in the main window; in a terminal or a session's own window the newest ordinary session opens in its place, silently. Trials live in `sessions/trials/`, which the app's list does not read. Read end to end, not pressed. |
| Medium | `cmd/record.rs` `check` | `sonde check` ignores a broken trial unless a budget is also given (64, "nothing to check"); `-f json` has no trials and says `passed: false` with every verdict ok; `-f markdown` and `-f github` say "over budget". |
| Medium | `cmd/record.rs` `run` | A later run without `--trial` keeps the old verdicts: `check` fails a fixed program. Trials of an earlier trial stay in `trials/` for ever. |
| Medium | `cmd/record.rs` | **Ctrl-C does not stop a trial**: the remaining scenarios run, and the interrupted one is saved as "broke … exited 130", which `check` then fails. A trial gets no stdin, so a program fed by a pipe "breaks". |
| Medium | `episode.rs`, `trial.rs` | A lost answer on CompleteMultipartUpload is read as an upload left unfinished and as "written twice" when the store refused the second attempt (NoSuchUpload); the operation is titled "aborted" though the object is whole. The line never says why the program broke. |
| Medium | `trial.rs` | "Jittered" is said of a fixed 1 s, 2 s, 4 s backoff, and "in step" of boto3's jittered one: waits of different attempts are pooled. A lost-answer write that is not retried is counted among the reads. A 404 scenario is judged as if a retry were owed. `slow` prints a verdict with no figure. |
| Medium | `trial.rs` `remove` | Deleting a run deletes whatever path its `.trials.json` names (an unchecked string in a file that can arrive with a shared session), and for good, while the run itself goes to the trash. |
| Low | several | `{trial}` is rewritten in runs that are not trials; a scenario's `on = ["PutObjekt"]` is accepted and "untested" passes CI; `[[scenarios]]` is ignored in silence; the injected-fault finding is printed last, after "the store asked for fewer requests", which contradicts it. |

No test runs `sonde run --trial` end to end, and the unit tests have one request per identity, which is why none of
the above failed one.

### Elsewhere

| | Where | What |
|---|---|---|
| Medium | `proxy/lib.rs` | **The proxy still forwards keys with `.` or `..` segments to another key.** 14.0 refused them in sonde's own requests only; the changelog said "refused". `GET /b/a/../g1` reaches the store as `GET /b/g1`. Confirmed. |
| Medium | `proxy/replay.rs` | **A replay leaves what it wrote**: "Removed 0 objects under rp1/" with four objects (and 11 MiB) still there; the built-in store's bucket is versioned. Confirmed. |
| Medium | `proxy/fixes.rs` | **"Acting on N findings" is arithmetic that cannot be**: removing three 2 ms HEADs takes waiting from 189 ms to 132 ms (70% of everything); one session printed "→ 62 ms" and "→ 14 ms" on two readings. It is on every summary. |
| Medium | CLI | `compare`, `replay` and `diff` do not take a session's name (the docs say every command does); "newest" is a replay, an import or a failed run as readily as the last run; a program is its whole command line, so a changed argument or interpreter path is "another program" with no baseline. |
| Medium | app | A bucket's sheet keeps the old endpoint's figures under the new bucket's name when the endpoint changes beneath it (Multipart's "Abort uploads older than…" among them). A live session's pane is rebuilt each second: sort, scroll and zoom are lost. The sidebar toggle does nothing while a place holds the sidebar open, and takes effect later. |
| Medium | app, zh | Sentences written in Rust are shown in English under Chinese: capacity findings, the lifecycle verdict, layout facts, probe titles and summaries, plan summaries, job progress, the credentials line. |
| Low | several | Prefix folding walks a lone deep folder up level by level and counts it as several; a Delta table under a path with `/data/` loses its files; a folder import follows a symlink to itself; `--log-out` on `run` writes an empty file; `check --init` writes a time limit one identical run in two will fail; `-f` help lists formats the command refuses; every subcommand's help is half global options. |

## The interface against its own standard

**None of the eleven leftovers 13.0's review counted was fixed in 14.0**; 14.0 fixed behaviour, not sameness.
Counted again:

| | 13.0 | Today |
|---|---|---|
| Renderings of a figure (the standard says two) | 5 | 5 |
| Headings drawn by hand | 11 sites, 6 looks | 13 sites, 7 looks |
| Pressable rows | 3 | 6, and 5 static rows in 3 paddings |
| The chart frame | the Time pane only | the Time pane only: Capacity, Compare, the trends, the waterfall and Run's live panel have their own |
| Layout thresholds (the standard names two) | 6 | 5 values at 7 sites |
| Names for one duration | 4 | 5 |
| Locale keys nothing uses | about 40 | 48 |
| The shared page | the old layout | the old layout |

And what is newly counted:

- **Spacing off the scale** (4, 8, 12, 16, 24) about a hundred times: 6 pt 42, 2 pt 33, 20 pt 15, 10 pt 11. The lint
  catches two values. The page gutter is 20 in five places and 16 in the sheets.
- **Charts are 24 one-offs**: five row pitches where the standard says one, three left edges, three treemap
  heights, four legend gaps, square and round swatches, two fades, two hover outlines; Flows, the trends and the
  distribution comparison have no tooltip.
- **Eight icon sizes; three control heights** (Run's command field is the third, drawn by hand); four forms of
  "nothing here".
- **On the screens**: with the inspector open nothing reflows — the header's figures wrap, the summary is cut
  mid-word, the Took column is clipped, a finding's subject is cut to `slate/db/gc/man…` while its saving keeps its
  width; a bucket's four tiles become three and one. Chart labels are still cut (`GetObject, not fou…`,
  `Chain 2 · 18 reque…`). An inspector mixes boxed pairs and loose text. Compatibility has its title twice. The
  welcome page names three engines of four and does not mention trials. The sessions list shows five identical
  `curl` rows.

## Native Shell at its newest commit

The newest commit is `744d950` (100 commits and 14 hours after the pin). **Nothing in it lets sonde delete a chart
or a kit piece**: Plot changed by one doc comment (still no press, selection, brush, zoom or log scale), and the
table, sheet, terminal and tooltip have no commit.

Of 105 exported components, about 33 are a styled wrap of one export, about 65 have nothing upstream (all 24
charts), and 5 small ones repeat what the pinned commit already has: about 80 lines of 7,150. The kit is already
thin; the room to own less is in owning *fewer kinds* of the same thing, which is the section above.

Worth taking:

| | Use |
|---|---|
| `focus-visible:` as keyboard-only focus | the focus ring stops showing after a click |
| `ViewRef.reveal` | a chart press scrolls its operation into view (replaces `scrollTo({ y: 1e6 })`) |
| `open(…, { modules })` in tests | the failure states (a listing that fails, a store that is gone) get tests without a store |
| `singleInstance`, `secondInstance` | a second launch or a `sonde://` link focuses the running app |
| Packaged windows read `nsh.json`'s `window` | probably ends FINDINGS #3's workaround |
| `Image` with a tint; `textOverflow: "ellipsis-middle"`; `RadioGroup`; `Table`; `Input` `disabled` | the icon's Canvas, `middle()`, `Options`, the boxed `KeyValue` and a stale entry of `nsh-issues.md` |

Risks of moving the pin: borders now draw under children, Tabs and Collapsible are retimed, Select opens below its
trigger on Linux and Windows (every screenshot moves), and Windows text inputs were rewritten.

## What cannot be seen

The Time pane has ten charts, and still lacks what an observability reader looks for first. From the newcomer's
list, checked against the code:

| Wanted | Today |
|---|---|
| Bytes a second, up and down, over time | recorded, never drawn |
| Time to first byte against the whole exchange | recorded per request, shown nowhere |
| Percentiles over time (p50, p95, p99 as bands) | whole-run p50 and p95 only; no p99 |
| Errors and retries over time, by status | counts only |
| Request size distribution; time against size (MiB/s) | no |
| Cost by operation | a total only |
| Per-prefix rate against S3's limit | the app's treemap colour; not in the CLI |
| A trial against its clean run | two numbers in a line |

## 15.0

**One reading of a run: four signals, one chart system, one way to draw each thing — and a verdict that is right.**

14.0 added a capability and 13.0 added charts; neither made the product *one*. 15.0 adds no place and no command.

| | What | Size |
|---|---|---|
| **M0: the verdict** (ship as 14.0.1) | A retry is counted against the clean run: an identity the clean run sent *n* times and the trial answered successfully fewer than *n* times was not retried. Keyless requests are told apart by their body and paging. A lost answer knows what the store did. Jitter is judged within one attempt number. Ctrl-C ends the trial and writes nothing; `check` judges trials alone; a new run clears old verdicts. Every row of "what 14.0 got wrong" above, each with a test that runs `sonde run --trial` on a small program. Dot-segment keys refused in the proxy; a replay removes versions; "acting on" subtracts the requests it names. | M |
| **M1: four signals** | A run's header becomes **rate, errors, latency, bytes** (and in flight): each a figure with its line over the session, each opening its chart. New in the analysis and in `sonde show`: bytes a second each way; p50/p95/p99 over time; first byte against the rest; errors and retries by status over time (injected ones marked); size distribution and time against size; cost by operation. | L |
| **M2: one chart system** | One frame, one left edge, one row pitch, one legend, one tooltip, one empty state for all 24 charts, Capacity's, Compare's and the shared page's included, enforced by a lint that counts them. Every chart has a tooltip and, where it shows requests, a press that filters to them. A trial is drawn against its clean run on the same axes. | L |
| **M3: one of each** | The eleven counts to their standard, each with a lint so they stay: two figures, the kit's heading and row only, spacing on the scale, two control heights, three icon sizes, one empty state. A pane that narrows reflows by rule (figures first, then columns by rank, never a clipped column). Rust's sentences become phrases, so Chinese is whole. | L |
| **M4: trials in the app** | A trial's session opens, is marked as one, and its injected requests are marked; Run has "…then under faults", with the scenarios it will run said before it runs. A program has a name of its own (`sonde.toml`, or the command without its arguments), so its baseline survives a changed argument. | M |
| **M5: Native Shell** | The pin moved to the newest commit; the six adoptions above; tests of failure states with faked modules. Three requests upstream (Plot press, log scale, null values) that would remove about 300 lines of workaround. | S |

What goes, so that it is not more: 48 locale keys; the dead code the audit names; four of five duration functions;
three of five figure renderings; the second tooltip; the header's six loose figures (the four signals replace
them); the estimate in "acting on N findings" wherever it cannot be measured.

**Not in 15.0**: a new place or dashboard; more engines than recordings bring; production daemons; the cut-short
body and lost-race scenarios (after the verdict is right).

The risk, named: M2 and M3 touch every screen. They are done screen by screen behind the counting lints, each
screen's pictures compared before and after, and the pin moves last (M5), so that a moved pixel has one cause.
