# Review of 20.0, and 21.0 proposed

Three reviews of 20.0.0 (main at 8d495f9), each checked by running it:
- the interface's 194 Linux pictures, judged against Linear, Raycast and Apple's own apps;
- the app's code against what nsh offers (pinned at f4d68f7, and nsh's main since then);
- the product: every "Done when" of 20.0, run on `--demo` and moto.

## In short

20.0's verdicts mostly hold, but two new findings give wrong advice, and two settings are applied silently. The
interface is about 70% of the way to top-tier. Its structure and consistency are there. Its typography, quiet and
motion are not.

The gap is systemic. Refinement comes from a few tokens and shared components, not from replacing the kit. nsh's
components carry behaviour, not looks, and nsh's own design guide asks every app to keep its own styled layer
(`kit.tsx` is that layer). Wrapping more of nsh removes only about 200–300 lines. What raises the polish is:

- motion and state feedback, which is almost absent;
- one way of doing each thing;
- a newer nsh.

Fixed before the 20.0.0 release (#23): the Windows tour's zh menu, and a `record-policy` left in `config.toml`
that was ignored without a word, so a former read-only recording could write.

## What 20.0 got wrong

### Verdicts and trust (each reproduced)

- **The hot-key fix is wrong when the key is rewritten or missing.** A program that PUTs then GETs
  `state/counter` 15 times is told to `lru_cache` it, which would return stale data. Twelve GETs that each
  answer NoSuchKey get the same advice, but `lru_cache` does not cache exceptions.
- **A changed argument is credited as a fix.** `--limit 30` then `--limit 5` reads
  `↓ less … saved 25 requests`. Every digit run in the command becomes `{n}`, so a smaller workload looks like a
  saving.
- **The budget comes from the wrong place.** `import s3://` applies the current folder's `sonde.toml` budget
  whatever the program. `check FILE` applies `[program]` limits to a run of another program without a word.
- **"Which run" differs between commands.** `check` takes the project's newest run; `show`, `compare` and
  `share` take the newest of any program.
- **A run refused by the policy is not explained.** The summary says `1 × refused by sonde`, but never names
  the request, the policy or `--policy allow`.
- **A failed or interrupted run looks clean in `sessions`.** There is no exit column, and SIGTERM is not said
  as such, though cli.md says it is.
- **`-o` is inconsistent.** `import -o` and `share --html -o` overwrite an existing file; `run -o` refuses.
- **Small things:**
  - `compare` of two imports of one log contradicts itself, and shows a waiting time that `show` says is not
    known.
  - An empty import exits 64, not 65.
  - `check --init` on a 0-request run writes `max-requests = 0`.

### The interface (systemic; picture names in the review notes)

1. **Small text has no leading.** `nsh.json` sets sizes without line heights, so 11–12 pt paragraphs are set
   solid next to airy 13 pt notices.
2. **Colour paints instead of marking.** Rows are tinted, every verdict is a red pill, every Activity row has a
   green "Finished", and tile figures are coloured. Nothing is the focal point.
3. **Borders inside cards.** Tables and key-value grids are boxed inside cards and the inspector. DESIGN.md
   itself forbids this.
4. **Chrome is always on.** Every tile has a chevron, every column a sort glyph, every row a trash or pencil,
   and every chart an info and a download icon.
5. **Headers and dialogs are uneven.**
   - The session header has four labelled buttons plus ⋯.
   - Buckets sections have no title.
   - The trial dialog jumps between three sizes, and its done state has no primary.
   - Dialog widths run from 560 to 880.
6. **No motion or state.**
   - Buttons, rows, cards and the place list have no transitions.
   - The indeterminate `Bar` is a static bar.
   - Loading buttons swap their label, so their width jumps.
   - Figure's popover entrance jumps.
7. **Several ways to do one thing:**
   - five tone vocabularies;
   - three disabled opacities, three dot sizes and three highlight tokens for popup rows;
   - shadows and durations as raw Tailwind;
   - about 110 hand-written text-role class strings.
8. **Local defects:**
   - The histogram prints its sentence twice.
   - Peak chips sit on data.
   - Half-width chart titles wrap.
   - Paths are middle-cut with a double space, or start-cut so the only varying part is hidden.
   - The sticky list header lets rows show through.
   - At the least window the sidebar keeps 272 px and tiles stack one per row.
   - In dark mode the primary button's label is dark.
   - Chinese gets "（ 94% ）" gaps and breaks inside words.

### Health

- **CI does not prove what the docs say.**
  - Only Linux pictures are approved.
  - The tour skips a shot it cannot reach (`if (app.shows(…))`), so a chart lost at the least window passes.
  - PR CI takes 25 minutes; main's run takes 40.
- **Duplication.** The app bridge re-implements trial planning and keeps its own exit store beside
  `.index.jsonl`. CLI modules still carry folded names (`watch.rs` holds `import s3://`, plus `du`, `mpu`,
  `demo`, `replay`).
- **Docs.**
  - `performance.md` has stale lines (19.0's binary, `diff`).
  - The 20.0 break list is said three times.
  - `decisions.md` has 204 rows, many superseded.

## What holds

- Identity v2 and programs keyed by their command: `hour=` and a changed `--date` read as one program, and
  "no change".
- `check` fails a failed run.
- `compare` reads more as more.
- `lost-answer` and `complete-500` are the same on `--demo` and moto.
- Replay leaves no bucket.
- Control characters are escaped everywhere.
- Every JSON output validates against its schema.
- Exit codes are as documented.
- The interface keeps one grammar, one chart system and real colour tokens, and its dark mode is good.

## 21.0

One release (the owner's decisions: upgrade nsh; no default AWS endpoint; no announcement rule). tui, the bucket
tools and probe stay as they are. Every Time chart stays, shown by default. Nothing
new unless it closes a loop.

**1. Advice that is safe to take.**
- A hot key's fix accounts for writes between reads, and gives a different fix for keys that are not found.
- Each finding's fix gets a test that the advice would not break the program.

**2. A loop that counts only real fixes.**
- A program's key leaves out only dates, ids and UUIDs, not every number.
- When the command changed, less, more and fixed say so (`--limit 200 → 20`) and count no saving.

**3. One rule for "which run", and every failure said.**
- `show`, `compare`, `share`, `check` and `import s3://` resolve the program the same way: its name, else this
  folder's program. Another program's `sonde.toml` is never applied.
- A refused request names itself, the policy and the flag that lifts it.
- `sessions` gets an exit column; SIGTERM is said.
- `-o` refuses an existing file everywhere without `--force`.

**4. The interface, refined on nsh.** In this order, with the pictures approved once at the end:
1. **Upgrade nsh** to its current main, in a PR of its own. It brings:
   - popup entrance and exit for Select, Combobox and DatePicker;
   - native menus in the app's language;
   - first-baseline alignment;
   - clamped-text fixes;
   - `quitRequest`;
   - Linux accessibility fixes.

   It needs `"application": true` in `nsh.json`, plus the pins in Cargo.toml and in the workflows.
2. **Tokens.**
   - Line heights for every size; CJK never below 12 pt, with punctuation squeezed and no breaks inside words.
   - Motion (120/180/240/360 ms, one ease) and shadow tokens.
   - One tone table and named text roles.
   - Dark mode: the primary's label and the switch thumb.
   - One selection look and one popup-row highlight.
3. **Motion and state.**
   - Hover and press transitions with a small press sink on Button, IconButton, ListRow, Card and Place.
   - Modal fades in.
   - `Bar` sweeps when indeterminate and glides to new values.
   - Button gets a `loading` state.
   - All of it off under reduce-motion.
4. **Quiet surfaces** (shared components):
   - Colour goes on marks (dot, icon, 2 px rule), not on text or fills; verdicts become quiet text.
   - Tables and key-value lists inside a card are bare, with hairlines only.
   - Chrome shows on hover or selection.
   - One header pattern:
     - the session header has one primary plus ⋯, and its command on one line, cut in the middle;
     - every Buckets section gets the same PageHeader.
   - Dialogs come in two widths and keep their size across states.
   - One rule for cutting paths.
   - At the least window the sidebar narrows to 220 px and tiles are never one per row.
5. **Charts, every one kept.**
   - Chips go off the data.
   - Titles take the card's width.
   - Legends are shorter.
   - Flows are drawn in role colours.
   - The histogram's sentence is said once.
6. **Remove visual noise.**
   - The Objects tiles that repeat the section row.
   - Run's repeated "Requests go to".
   - Columns whose every row holds the same value.
   - The "Finished" pills.
   - Compatibility's compare card before any run exists.

**5. CI that proves it, and less to keep.**
- macOS and Windows pictures approved. A tour shot that cannot be reached fails.
- The app bridge's trial planning and exit store move into `sonde-proxy`.
- CLI modules are named after their commands.
- `performance.md` is remeasured; the 20.0 break list is said once; superseded decisions move behind an index.
- Kit lint: no raw `duration-N`, `shadow-*` or icon `size={N}` outside the kit.

Removed in 21.0, without notice (the owner's decision: a break comes in the major that makes it, and the
contract's announce-one-version-ahead rule goes):
- `operations[].kind` becomes `operations[].shape`, so "kind" means only the S3 call;
- `check --program-exit` goes, since the session keeps the exit.

### Not in 21.0

- New commands or charts.
- A default AWS endpoint without one configured (the owner's decision).
- `endpoints remove` (the app and `config.toml` do it).

### Streams

| Stream | Scope | Files |
|---|---|---|
| A | nsh upgrade, tokens, motion and state | `nsh.json`, `theme.json`, `kit.tsx`, workflows, Cargo |
| B | Quiet surfaces, headers, dialogs, least window | `kit.tsx` (after A), `ui/*`, `screens/*` |
| C | Charts | `ui/plots/*` |
| D | Verdicts, loop, "which run", failures said | `crates/proxy`, `crates/sonde-cli` |
| E | CI proof, dedup, docs | `app/tests`, `crates/sonde-nsh`, `docs` |

A lands first; B and C follow on it. D and E run alongside.

### Done when

- A rewritten or missing hot key gets no cache advice.
- `--limit 200 → 20` reads "the command changed", with no saving.
- `import s3://` from another project's folder uses no budget.
- A refused delete names the request and `--policy allow`.
- `sessions` shows a run's exit.
- No paragraph in the app is set without leading.
- No status colour fills a row's text.
- No table is boxed inside a card.
- Every control has a hover or press state.
- No dialog changes size between its states.
- Every Time chart is shown, at every window size.
- Pictures are approved on Linux, macOS and Windows, and none is skipped.
