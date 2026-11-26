# The run file (`.sonde`)

A run is the file a recording, replay, import or trial leaves. Any later sonde of the same format version reads
it. This page is its contract ([contract.md](../contract.md)); `crates/analysis/src/run.rs` implements it and its test
`the_format_as_its_spec_says` checks it.

## The file

- UTF-8 text, one JSON object per line, lines ending in `\n` (a `\r` before it is ignored).
- Plain while written; gzip-compressed in place when finished (one or more gzip members). Readers detect gzip by
  the first two bytes (`1f 8b`), not the name: both are `.sonde`. `.sonde.gz` is read the same way.
- Line 1 is the **header**; every other line is a **record** or a **log line**. Empty lines are ignored.
- Records are in the order requests finished, not by `id` or `time`.
- Limits: 32 MiB per line, 64 GiB per file decompressed; beyond either it is not a run.
- A torn last line (recorder killed mid-write) is skipped. An unreadable line anywhere else makes the file
  unreadable.

## The header

```json
{"format":"sonde-run","version":2,"sonde":"20.0.0","created":"2026-11-12T10:00:00.123Z","endpoint":"prod","url":"https://s3.eu-west-1.amazonaws.com/","region":"eu-west-1","command":"python etl.py","cwd":"/home/me/etl","git":{"commit":"4f1c2e9…","branch":"main","dirty":false}}
```

| Field | | |
|---|---|---|
| `format` | required | Always `"sonde-run"`. Anything else is not a run. |
| `version` | required | The format's version, an integer: `2`. |
| `created` | required | When the recording began, RFC 3339 in UTC. |
| `endpoint` | required | The endpoint's id (`adhoc` for `--url`). Never its credentials. |
| `url` | required | The endpoint's URL. |
| `region` | required | The signing region; may be empty. |
| `sonde` | optional | The sonde version that wrote it. |
| `command` | optional | The command sonde ran; `replay of …` or `imported from …` for those. |
| `name` | optional | The program's name (`[program] name`, `--name`): its runs and baseline go by it. |
| `cwd` | optional | The folder the command ran in. |
| `git` | optional | `{ "commit", "branch"?, "dirty" }`: the commit the code was at. |
| `scenario` | optional | A trial's fault scenario. |
| `trialOf` | optional | The file name of the clean run a trial is of. |
| `wholeSeconds` | optional | `true`: request times are to the second (an imported log), spread over it. |
| `pathStyle` | optional | `true`: a store named for the run alone (`--url`) was reached path-style (`--path-style`). Since 33.0. |
| `caFile` | optional | The file of that store's own authorities (`--ca-file`): its path, absolute, never its content. Since 33.0. |
| `tls` | optional | `true`: the program was served TLS (`--tls`, `[record] tls`). Since 33.0. |
| `policy` | optional | The [policy](../recording.md#what-a-recorded-program-may-do) it was recorded under: `allow`, `no-delete` or `read-only`. Since 33.0. |

The last four say how the run was recorded, for `sonde show`'s `verify` to say it again; a run without them (one
written before 33.0) is read as before. No header field holds a credential.

## A record

One HTTP exchange, redacted before writing: signatures, keys and tokens are masked in the URL, query and headers.

| Field | | |
|---|---|---|
| `id` | required | An integer, unique within a recording. Files joined from two recordings repeat ids; readers renumber. |
| `time` | required | When the request began, RFC 3339. |
| `endpoint` | required | The endpoint's id. |
| `kind` | required | The S3 call, as the API reference spells it (`PutObject`, `ListObjectsV2`), or `Unknown`. Format 1 named it `operation`. |
| `method` | required | The HTTP method. |
| `url` | required | The URL as sent, signatures masked; may be empty (an import, a lean reading). A recording's holds the key as the program wrote it (`a%2Fb`). |
| `query` | required | `[[name, value], …]`, decoded, masked like the URL. |
| `latencyMs` | required | From the request's arrival to the response's end, in ms (a number). |
| `retries` | required | Attempts before this one, as the client said. |
| `requestHeaders`, `responseHeaders` | required | `[{ "name", "value" }, …]`, names in the case they were sent. |
| `bucket`, `key` | optional, null | The bucket and the key, decoded (`a%2Fb` is `a/b`); a recorded key's bytes that are not UTF-8 stay `%XX`. |
| `status` | optional, null | The HTTP status; null when no answer came. |
| `requestId`, `hostId` | optional, null | The store's ids. |
| `requestBytes`, `responseBytes` | optional, null | Body sizes, when known: the bytes that went, for a body cut or left unread. |
| `errorBody` | optional, null | An error's body, up to 64 KiB, as the store sent it (or sonde's reason, for a request it refused). |
| `requestBody` | optional | A control request's body (DeleteObjects, a lifecycle rule, CompleteMultipartUpload), up to 256 KiB; never object data. |
| `requestBodyTruncated` | optional | `true`: the body was longer than what is kept. |
| `transportError` | optional, null | Why no whole answer came (DNS, TLS, a timeout, a body cut, the program stopped waiting). |
| `region` | optional, null | The region the request was signed for. |
| `job` | optional, null | The job (or run tag) it belongs to. |
| `firstByteMs` | optional | Until the response's first byte, in ms. |
| `requester` | optional | Who sent it, as the store's log names them (an import). |
| `injected` | optional | The fault scenario that answered it, delayed it or changed its answer (a trial). |

"Optional" fields may be missing and "null" fields may be `null`; both mean "not known".

Headers sonde adds to a record are part of the format:

| Header | Of the | Meaning |
|---|---|---|
| `x-sonde-proxy: refused` | response | sonde refused the request (the policy, or one it cannot record) |
| `x-sonde-policy` | response | the policy that refused it (`no-delete`, `read-only`) |
| `x-sonde-fault` | response | the trial scenario that acted on it |
| `x-sonde-store-status` | response | what the store answered under a fault |
| `x-sonde-cut-at` | response | where a cut body stopped, in bytes |
| `x-sonde-hidden` | response | a key a stale listing left out, once per key |
| `x-sonde-given-up` | response | the program stopped waiting for an answer a fault held (closed its connection) |
| `x-sonde-keys` | response | how many keys a listing listed (its body is not kept) |
| `x-sonde-note` | request | what sonde did to it (`presigned URL, re-signed by sonde`) |
| `x-sonde-body-sha256` | request | the hash of a control request's body as sent (the kept body is cut and redacted) |
| `x-sonde-caller` | request | the place in the program's code that sent it (`file:line function`), from the program or sonde's hook |

## A log line

```json
{"log":{"time":"2026-11-12T10:00:01.500Z","text":"loading day 2\r\n"}}
```

A piece of the program's output (escape codes included) and when it arrived. A line starting `{"log":` is a log line.

## Versions

- The format is version `2`, written since 20.0.
- **Within a version**, fields may be added to the header, records or log lines. Readers skip unknown fields and
  treat a missing optional field as not known. Required fields are never removed, renamed, retyped or redefined.
- **A new version** comes only with a change an older reader would misread (a field removed, renamed or redefined,
  a line kind it cannot skip, another encoding), in a major release ([contract.md](../contract.md)).
- A sonde reads its own version only and refuses any other by name, never partially:
  `written by sonde 32.0.0, format 3; this sonde (24.0.0) reads format 2: update sonde to read it`
  (`another sonde` when the header has no `sonde`; an older format is refused the same, without the advice).
- A runs folder's `.index.jsonl` (each run's counts, exit code, trial verdicts with where each broke, `where`: `kind`,
  `key`, `caller`, and its `advice`: `key`, `args`) is sonde's cache and state, not part of this contract; an
  unreadable cache is rebuilt.
