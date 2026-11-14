# The session file (`.sonde`)

A session is the file a recording, replay, import or trial leaves. Any later sonde of the same format version reads
it. This page is its contract ([contract.md](contract.md)); `crates/proxy/src/session.rs` implements it and its test
`the_format_as_its_spec_says` checks it.

## The file

- UTF-8 text, one JSON object per line, lines ending in `\n` (a `\r` before it is ignored).
- Plain while written; gzip-compressed in place when finished (one or more gzip members). Readers detect gzip by
  the first two bytes (`1f 8b`), not the name: both are `.sonde`. `.sonde.gz` is read the same way.
- Line 1 is the **header**; every other line is a **record** or a **log line**. Empty lines are ignored.
- Records are in the order requests finished, not by `id` or `time`.
- Limits: 32 MiB per line, 64 GiB per file decompressed; beyond either it is not a session.
- A torn last line (recorder killed mid-write) is skipped. An unreadable line anywhere else makes the file
  unreadable.

## The header

```json
{"format":"sonde-session","version":2,"sonde":"20.0.0","created":"2026-11-12T10:00:00.123Z","endpoint":"prod","url":"https://s3.eu-west-1.amazonaws.com/","region":"eu-west-1","command":"python etl.py","cwd":"/home/me/etl","git":{"commit":"4f1c2e9…","branch":"main","dirty":false}}
```

| Field | | |
|---|---|---|
| `format` | required | Always `"sonde-session"`. Anything else is not a session. |
| `version` | required | The format's version, an integer: `2` (`1` before 20.0). |
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

## A record

One HTTP exchange, redacted before writing: signatures, keys and tokens are masked in the URL, query and headers.

| Field | | |
|---|---|---|
| `id` | required | An integer, unique within a recording. Files joined from two recordings repeat ids; readers renumber. |
| `time` | required | When the request began, RFC 3339. |
| `endpoint` | required | The endpoint's id. |
| `kind` | required | The S3 call, as the API reference spells it (`PutObject`, `ListObjectsV2`), or `Unknown`. Format 1 named it `operation`. |
| `method` | required | The HTTP method. |
| `url` | required | The URL as sent, signatures masked; may be empty (an import, a lean reading). |
| `query` | required | `[[name, value], …]`, decoded, masked like the URL. |
| `latencyMs` | required | From the request's arrival to the response's end, in ms (a number). |
| `retries` | required | Attempts before this one, as the client said. |
| `requestHeaders`, `responseHeaders` | required | `[{ "name", "value" }, …]`, names in the case they were sent. |
| `bucket`, `key` | optional, null | The bucket and the key. |
| `status` | optional, null | The HTTP status; null when no answer came. |
| `requestId`, `hostId` | optional, null | The store's ids. |
| `requestBytes`, `responseBytes` | optional, null | Body sizes, when known. |
| `errorBody` | optional, null | An error's body, up to 64 KiB, as the store sent it (or sonde's reason, for a request it refused). |
| `requestBody` | optional | A control request's body (DeleteObjects, a lifecycle rule, CompleteMultipartUpload), up to 256 KiB; never object data. |
| `requestBodyTruncated` | optional | `true`: the body was longer than what is kept. |
| `transportError` | optional, null | Why no answer came (DNS, TLS, a timeout). |
| `region` | optional, null | The region the request was signed for. |
| `job` | optional, null | The job (or session tag) it belongs to. |
| `firstByteMs` | optional | Until the response's first byte, in ms. |
| `requester` | optional | Who sent it, as the store's log names them (an import). |
| `injected` | optional | The fault scenario that answered it, delayed it or changed its answer (a trial). |

"Optional" fields may be missing and "null" fields may be `null`; both mean "not known".

Response headers sonde adds are part of the format:

| Header | Meaning |
|---|---|
| `x-sonde-proxy: refused` | the policy refused the request |
| `x-sonde-fault` | a trial scenario acted on it |
| `x-sonde-store-status` | what the store answered under a fault or refusal |
| `x-sonde-cut-at` | where a cut body stopped |

## A log line

```json
{"log":{"time":"2026-11-12T10:00:01.500Z","text":"loading day 2\r\n"}}
```

A piece of the program's output (escape codes included) and when it arrived. A line starting `{"log":` is a log line.

## Versions

- The format is version `2`, written since 20.0: a record's S3 call is `kind` (version `1`, written by 1.0 to 19.x,
  named it `operation`; nothing else differs). 20.0 reads both.
- **Within a version**, fields may be added to the header, records or log lines. Readers skip unknown fields and
  treat a missing optional field as not known. Required fields are never removed, renamed, retyped or redefined.
- **A new version** comes only with a change an older reader would misread (a field removed, renamed or redefined,
  a line kind it cannot skip, another encoding), in a major release ([contract.md](contract.md)).
- A sonde reads every version up to its own and refuses a newer one by name, never partially:
  `written by sonde 21.0.0, format 3; this sonde (20.0.0) reads up to format 2: update sonde to read it`
  (`a newer sonde` when the header has no `sonde`).
- A sessions folder's `.index.jsonl` (each session's counts, trial verdicts, exit code) is sonde's cache and state,
  not part of this contract; an unreadable cache is rebuilt.
