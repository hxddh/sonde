# Plugins

A plugin is a TSX view — a decoder lens or a probe view — delivered as a signed Native Shell package
(`.nshpkg`). sonde verifies it, runs it in an engine of its own, and gives it read-only data of the bucket and
prefix the user opened it on. The example is `plugins/delta-lens`, a Delta Lake `_delta_log` lens.

## What a plugin can do

Nothing but call the host. Its `nsh.json` declares exactly that:

```json
{
  "name": "Delta Lake lens",
  "id": "dev.sonde.plugins.delta",
  "version": "1.0.0",
  "build": 1,
  "capabilities": { "host": ["sonde.read.context", "sonde.read.list", "sonde.read.get"] }
}
```

sonde refuses to install a package that declares any other capability with a grant (network, files, processes,
clipboard, web views, …), or host calls outside `sonde.read.*`. The host answers only these calls:

| Call | Data | Answer |
|---|---|---|
| `sonde.read.context` | — | `{ bucket, prefix, vendor, sonde }` (no endpoint URL, no credentials) |
| `sonde.read.list` | `{ prefix?, limit? }` (≤ 10 000) | `[{ key, size, lastModified, etag }]` under the context prefix |
| `sonde.read.head` | `{ key }` | `{ size, etag, metadata }` |
| `sonde.read.get` | `{ key, start?, end? }` (≤ 8 MiB) | `{ base64, size }` |
| `sonde.read.layout` | — | the layouts detected under the context prefix |

Keys outside the context prefix (or with `..`) are refused. Calls are made by sonde's Rust core with the
endpoint's credentials, which the plugin never sees.

## Write one

```sh
nsh init my-lens --template ts     # then set id, version, build and capabilities as above
cd my-lens && nsh check && nsh test
```

Test it against a host the test plays (`open(entry, { host: { handlers } })`), as `plugins/delta-lens/App.test.tsx`
does: the handlers answer `sonde.read.*` with fixture data, and `app.host.calls()` shows what the plugin asked.

## Sign and publish

```sh
nsh keygen keys/root && nsh keygen keys/release                       # root stays offline
nsh certify --root keys/root.secret --key keys/release.pub --apps 'dev.sonde.plugins.*' -o keys/release.cert
nsh publish my-lens/App.tsx --key keys/release.secret --cert keys/release.cert -o my-lens.nshpkg
nsh verify my-lens.nshpkg --root keys/root.pub
```

`scripts/sign-plugin.sh <plugin> <keys> <out.nshpkg>` does the same. Raise `build` for every release: an
installed build refuses older ones (rollback protection).

## Install, trust, revoke

```sh
sonde plugin trust "ACME" keys/root.pub      # or Plugins › Trusted roots in the app
sonde plugin install my-lens.nshpkg
sonde plugin list
nsh revoke --root keys/root.secret --sequence 1 --package <sha256> -o revocations.json
sonde plugin revoke revocations.json         # revoked plugins are disabled and cannot be enabled
```

Verification checks the release key's certificate against a trusted root, revocations, the signature, the app id,
every file's hash, the minimum runtime and the build (`nsh-package`). It runs again each time a plugin opens.

## Open

In the terminal UI: Plugins, choose the bucket and prefix, Open. The plugin takes the screen in its own engine;
F12 closes it. The macOS runner `nsh package` builds cannot host a second engine (FINDINGS.md #3): the app lists,
installs, trusts and revokes plugins, and says that their views open in `sonde tui`.
