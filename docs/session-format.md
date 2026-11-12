# The session file (`.sonde`)

A session is what a recording, a replay, an import or a trial leaves: one file, readable by any later sonde of the
same format version. This page is its contract ([contract.md](contract.md)); `crates/proxy/src/session.rs` writes
and reads it, and its test `the_format_as_its_spec_says` holds it to this page.

## The file

- UTF-8 text, one JSON object per line, lines ending in `\n` (a `\r` before it is ignored).
- Plain while it is written; a finished session is gzip-compressed in place (one or more gzip members). A reader
  tells them apart by the first two bytes (`1f 8b`), never by the name: both are `.sonde`. `.sonde.gz` is read the
  same way.
- The first line is the **header**. Every other line is a **record** or a **log line**. Empty lines are ignored.
- Lines are appended as requests finish: records are in the order they finished, not by `id` or `time`.
- A line is at most 32 MiB; a file at most 64 GiB once decompressed. Past either it is not a session.
- A last line cut short (the recorder was killed mid-write) is skipped. A line that does not read anywhere else
  makes the file unreadable: it is not a session sonde wrote.

## The header

```json
{"format":"sonde-session","version":1,"sonde":"17.0.0","created":"2026-11-12T10:00:00.123Z","endpoint":"prod","url":"https://s3.eu-west-1.amazonaws.com/","region":"eu-west-1","command":"python etl.py","cwd":"/home/me/etl","git":{"commit":"4f1c2e9…","branch":"main","dirty":false}}
```

| Field | | |
|---|---|---|
| `format` | required | Always `"sonde-session"`. Anything else is not a session. |
| `version` | required | The format's version, an integer: `1`. |
| `created` | required | When the recording began, RFC 3339 in UTC. |
| `endpoint` | required | The endpoint's id (`adhoc` for `--url`). Never its credentials. |
| `url` | required | The endpoint's URL. |
| `region` | required | The signing region; may be empty. |
| `sonde` | optional | The version of the sonde that wrote it (written since 17.0). |
| `command` | optional | The command sonde ran; `replay of …` or `imported from …` for those. |
| `name` | optional | The program's name (`[program] name`, `--name`): its runs and baseline go by it. |
| `cwd` | optional | The folder the command ran in. |
| `git` | optional | `{ "commit", "branch"?, "dirty" }`: the commit the code was at. |
| `scenario` | optional | A trial's fault scenario. |
| `trialOf` | optional | The file name of the clean run a trial is of. |
| `wholeSeconds` | optional | `true`: request times are to the second (an imported log), spread over it. |

## A record

One HTTP exchange with the store, redacted before it was written: signatures, keys and tokens are masked in the
URL, the query and the headers.

| Field | | |
|---|---|---|
| `id` | required | An integer, unique within a recording. A file joined from two recordings repeats them; a reader numbers them afresh. |
| `time` | required | When the request began, RFC 3339. |
| `endpoint` | required | The endpoint's id. |
| `operation` | required | The S3 operation, as the API reference spells it (`PutObject`, `ListObjectsV2`), or `Unknown`. |
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

"Optional" fields may be missing; "null" fields may be `null`; both mean "not known". Response headers sonde adds to
say what it did — `x-sonde-proxy: refused` (the policy refused it), `x-sonde-fault` (a scenario's doing),
`x-sonde-store-status` (what the store answered under a fault or a refusal), `x-sonde-cut-at` — are part of the
format.

## A log line

```json
{"log":{"time":"2026-11-12T10:00:01.500Z","text":"loading day 2\r\n"}}
```

A piece of the program's output (escape codes included), when it arrived. A line starting `{"log":` is a log line.

## Versions

- The format is at version `1`, and every sonde since 1.0 writes it.
- **Within a version**, a field may be added to the header, a record or a log line. A reader passes over a field it
  does not know, and treats a missing optional field as not known. A required field is never removed, renamed, or
  given another type or meaning within a version.
- **A new version** is made only by a change an older reader would read wrongly: a field removed, renamed or given
  another meaning, another line kind it could not skip, another encoding. It is a major release of sonde
  ([contract.md](contract.md)).
- A reader refuses a version newer than it knows, by name: `written by sonde 18.0.0, format 2; this sonde (17.0.0)
  reads up to format 1: update sonde to read it` (`a newer sonde` when the header does not say). It never reads such
  a file as far as it goes. A sonde reads every version up to its own.
- Files beside a session (`<name>.summary.json` counts, `<name>.trials.json` verdicts, `<name>.exit`) are caches and
  sonde's own state: not part of this contract. A cache that does not read is made again.
