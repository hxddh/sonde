# 17.0, the verdict stream: notes for the changelog and the decisions

M0's Rust parts and M7's four scenarios. Lines for CHANGELOG.md, in its voice, then decisions to number.

## Changelog lines

### Added

- **Four trial scenarios**, each chosen from what the clean run did. `skew`: 403 RequestTimeTooSkewed on each
  request's first attempt, for a program that signs its requests; it is owed a retry signed again. `complete-500`:
  CompleteMultipartUpload completes in the store and its first answer is a 500; retrying and taking NoSuchUpload
  as done, or reading the object, survives; reporting a failure breaks; starting the upload again degrades.
  `slow-first-byte`: the first download is held 10 s; a program whose timeout cuts it short and asks again
  survives, one that waits it out is degraded and told to give each attempt a timeout. `stale-list`: a listing
  leaves out each key the run wrote, once; a program that lists again (or keeps its own list) survives, one that
  goes on without a key it wrote breaks, one that writes it again is degraded. `stale-list` is also a project
  fault in `sonde.toml`. See [docs/trials.md](../trials.md).
- A request a fault holds (a delay) and the program stops waiting for is recorded as given up
  (`x-sonde-given-up`), so its retry is read as any failure's.
- A write whose answer was lost, not sent again, whose key the program then read is "read back": it asked the store,
  which had it. Neither retried nor dropped.

### Changed

- **One retry rule, by status and code**: trials, episodes and signals ask one function, and it reads S3's code
  as well as the status: `SlowDown`, `RequestTimeout` (a 400) and `RequestTimeTooSkewed` (a 403) owe a retry
  whatever status they come with. sonde's own client sends a 403 RequestTimeTooSkewed again.
- `lost-race` is offered only where a 412 means a commit was lost: a create in a table's log, or a write with
  `If-Match`. A create of a key of its own (`_SUCCESS`, a lock) that the store made, taken by the program as made,
  is owed nothing ("taken as made"). A race never run again says what is true of both kinds: "no commit followed it
  and nothing read back what won".
- The advice for a request given up names the codes as well as the statuses; a skewed clock's, a stall's and a
  stale listing's have their own.

### Fixed

- **With `{trial}` in a key that also holds `clean` or the scenario's name as a word, a program that never retries
  was "survived" again** (`etl-clean/clean/state`, `etl-$SONDE_TRIAL/error.json` under `error`, a cut body under
  `…/clean/big`). A trial's request is now the clean run's that is the same as written, or the same with the
  scenario's name in some of the clean run's places, and nothing else different.
- A `400 RequestTimeout` scenario was "none a retry is owed for", and a program that dropped the write survived.
- A writer of `_SUCCESS` with `If-None-Match: *` that took a 412 as done was "broke · the commit was never made",
  though sonde had let the write through.
- On Windows a trial from the app hung at its start when the pseudo-console's cursor question came in two reads.
- `show` drew sparklines for a run under a second or of fewer than 20 requests, as 16.0 said it did not.

## Decisions to number

| Topic | Options | Choice, and why |
|---|---|---|
| Matching a trial's request to the clean run's when its key holds a run's name | take out the run's own name on each side (16.0); substitute only where the command's template had `{trial}`; take out both names on both sides; a key fits when the scenario's name stands in some of the clean run's places | **The last, looked up under both names taken out**: `unplace_both` makes a coarse name on each side (both runs' names out), and among the clean run's requests of that coarse name the trial's is the one the same as written, else the one it `fits` (the scenario's name, as a whole word, in some places where the clean run has `clean`, nothing else different). The command's template cannot say where the name went: a program builds keys from `SONDE_TRIAL` any way it likes (`etl-$SONDE_TRIAL/…`, a variable from a config file), and `{trial}` in an argument says nothing of the key's other words. Taking both names out on both sides alone merges keys the program tells apart (`clean/x` and `slowdown/x` in one run), so it is only the lookup, and `fits` decides. As-written first keeps D-114's `error/log` under `error`. |
| Which retry rule | trials by status, episodes by status and code (16.0); one function | **One function, `episode::retryable`**, by status (408, 429, 500, 502, 503, 504), by code (SlowDown, RequestTimeout, RequestTimeTooSkewed) and by no answer; trials ask it for "owed" and "answered" alike. Supersedes D-115's `status_owes_retry`, which is now private to it. A fault sonde answers keeps its code at the head of the record (`Code: …`), and `code_of` reads that only of an injected record. |
| Where lost-race is offered, and a create that loses on a key of its own | offer it everywhere and judge by the program's belief; offer it only on a commit family; both | **Both**: offered for a create in a table's log or any `If-Match` write (an update on what was read is lost if not redone); a create of a key of its own asked for by name is judged "taken as made" when the program neither wrote it over nor failed: the store holds the key, as the program believes. Retrying with the same condition (a real 412) is the same. |
| `skew` | move the program's clock; answer 403 RequestTimeTooSkewed | **Answer it**, on each request's first attempt, like `slowdown`; offered when the clean run signed a request itself (any but a presigned URL's: the `Authorization` header is not in a lean reading, which the app keeps of a large session). The retry is let through, so a client that retries without signing anew passes here; said in the limits. botocore does not retry it (neither the code nor a 403 is among its retries) and is seen to break: that is what a skewed machine does to it. |
| `complete-500` | a fault of its own; `lost-answer` on CompleteMultipartUpload alone | **The lost answer on CompleteMultipartUpload alone**, so it is judged by the rules the lost answer already has: a retry answered NoSuchUpload taken as done survives (the "refused" reason says why when it is not); reading the object instead of retrying survives ("read back", new, for any lost answer); exiting 0 without either breaks (told of a failure, it said all was well); **an upload started again from its first part is degraded, not broke**: the object is right, the cost is every part twice. |
| `slow-first-byte`: what is a stall, how long, how many | every GET held; the first held; a fixed or a project's delay | **One download in all, held 10 s before it is sent on**, so a trial costs 10 s at most; a delay of 5 s or more (`STALL_MS`, a project's too) is a stall that a program is to cut short. Cut short (its connection closed while held, recorded as given up, or another attempt began while it was held) then asked again: survived. Waited out: degraded, not broke: it finished, slowly. The bound is a rule's `most`, the one place a scenario counts by arrival. |
| `stale-list`: what is judged | the listing's use; the key's requests after it | **The key's requests**: for each key left out, its reads and writes in the trial against the clean run's. Read less often: broke (it went on without what it wrote, whatever the exit); written again: degraded; nothing done with it in either run and no listing again: untested, since what the listing was used for is not in the requests. Each key is left out once, by the proxy, from a 2xx, uncompressed listing of 16 MiB or less. |
| Sparkline in `show` for a short run | as wide as its slices; none | **None under a second or under 20 requests** (`signals::FEW`, the app's `FEW`): a handful of requests has no shape over time. The figures stay. |

## Needs of the app (not done here: another stream owns the UI)

- `app/src/trials.ts` `SCENARIOS` lists the 16.0 scenarios for their labels and lines (`scenario.<name>`,
  `scenario.about.<name>` in the locales). The four new ones show their id and Rust's `about` until they are added
  there, with en and zh labels: `skew` (时钟偏差), `complete-500`, `slow-first-byte`, `stale-list`.
- The verdict carries new fields the app may show beside the reasons: `yielded`, `checked`, `reuploaded`, `held`,
  `waited`, `stale`, `relisted`, `missed`, `rewrote`. The reasons and advice are phrases with en and zh templates
  in `app/src/phrases.ts` already (`trial.why.waited`, `trial.why.reuploaded`, `trial.why.rewrote`,
  `trial.why.missed`, `trial.why.staleUnused`, `trial.advice.skew`, `trial.advice.timeout`, `trial.advice.list`;
  `trial.why.unrun` and `trial.advice.retry` reworded).
- The app's sparkline rule (`signals.ts`, under 20 requests) should also take a run under a second, as `show` now
  does.
- A record's `x-sonde-hidden` (a key a stale listing left out) and `x-sonde-given-up` could be said in the
  request's inspector beside "Injected by sonde".
