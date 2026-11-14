# Superseded decisions

Decisions a later one replaced, moved from [decisions.md](../decisions.md) (which keeps those in force), in their
order.

| # | Decision | Options | Choice and reason | Superseded by |
|---|---|---|---|---|
| D-002 | Native Shell version | crates.io 0.4.0; a git commit | **`longbridge/nsh` tag `v0.5.0`, commit `f4d68f7`**, pinned by full hash in `Cargo.toml` (`rev`) and CI (`NSH_REV`): the CLI must match the crates it links. | D-207 |
| D-011 | Plugin views in the macOS app | a custom AppKit host; degrade | **Degrade**: views opened in `sonde tui`. | D-031 |
| D-012 | Plugin capability policy | trust the package; check | **Check at install and on open**: only `sonde.read.*`. | D-031 |
| D-030 | Navigation | nine sections; a few places and a palette | **Four places and a command palette** (⌘K, `:` in the terminal). | D-040 |
| D-044 | Compatibility | a Buckets tab; from the endpoints | **Its own window, from the endpoints**: it runs on several endpoints for minutes. | D-187 |
| D-062 | What a program is | a given name; the command | **The command, spaces collapsed** (named later: D-110): nothing to set up. Baselines in `baselines.json`. | D-110, then D-199 |
| D-076 | Activity's requests | an inspector; a session | **Both**: an inspector, and "Open as a session". | D-190 |
| D-082 | Production from logs | a daemon; `watch --logs` | **`sonde watch --logs`**: each pass a run with a baseline and `check`. | D-169 |
| D-091 | The Time pane | charts stacked; a board | **A board** under one timeline: nine stacked sections were a scroll, not a view. | D-186 |
| D-110 | A program's identity | the command line; a name | **`[program] name` or `--name`, else the command**: no existing baseline is lost. | D-199 |
| D-115 | Which answers owe a retry | each its own list; one | **One function** (`status_owes_retry`: 408, 429, 500, 502, 503, 504); 501 owes none. | D-126 |
| D-140 | Session format version | 2; 1 | **1**, with an optional `sonde` field; a newer format is refused by name. | D-200 |
| D-155 | Timed tests | every push; nightly | **Nightly**; a missed memory budget runs once more. | D-205 |
| D-168 | A folded command | remove; alias for one version | **An alias for 18.0** that says its new form; gone in 19.0. | D-192 |
| D-193 | A finding across runs | its text; a normalised identity | **A normalised identity** (numbers, ids and dates stripped from keys and paths), so a fix is seen as one. | D-198 |
| D-195 | CI | every OS each change; Linux, others nightly | **Linux per change, macOS/Windows/Intel nightly and at release**; the release reuses CI's builds. | D-203 |
| D-197 | Breaking in 20.0 | announce in 19.x; break now | **Break now**, by the owner's decision; 20.0 reads format 1; the announcement rule holds from 20.0. | D-206 |
