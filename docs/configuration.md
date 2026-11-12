# Configuration

sonde reads `config.toml` from its configuration directory: settings, prices and endpoints.

| | Linux | macOS |
|---|---|---|
| Configuration | `$SONDE_CONFIG_DIR`, else `$XDG_CONFIG_HOME/sonde`, else `~/.config/sonde` | `$SONDE_CONFIG_DIR`, else `$XDG_CONFIG_HOME/sonde`, else `~/Library/Application Support/sonde` |
| Data (request log, scans, probe runs, audit log, sessions) | `$SONDE_DATA_DIR`, else `~/.local/share/sonde` | `$SONDE_DATA_DIR`, else `~/Library/Application Support/sonde` |
| Exports | `<data>/exports` unless `exports-dir` | same |
| Downloads | `~/Downloads/sonde` unless `downloads-dir` | same |

The CLI also takes `--config-dir` and `--data-dir`.

## Loading and saving

- Written atomically, mode `0600` on Unix (it may hold static keys).
- Unknown or duplicate endpoint ids and bad URLs are refused on load.
- A file that does not load is never overwritten: the app starts with no endpoints, shows why, and refuses saves
  until the file is fixed and reloaded.
- A save rereads the file and applies only what changed, so edits made meanwhile (by hand or `sonde endpoints`) are
  kept; comments are not.
- Concurrent saves take turns (`config.toml.lock`).

## Example

```toml
[settings]
read-only = true            # sonde's own destructive operations refused (default)
record-policy = "no-delete" # what a program Run records may do: no-delete (default), read-only, allow
concurrency = 16            # requests in flight per job
rate-limit = 50.0           # requests per second per job; omit for none
reqlog-capacity = 5000      # records kept in the request log
reqlog-persist = false      # also keep them in <data>/requests.sqlite
theme = "system"            # system, light, dark
language = "auto"           # auto, en, zh
stale-upload-days = 7       # uploads older than this are flagged
name-callers = false        # runs name the code that sent each request (Python, Node.js, Java)
# exports-dir = "/srv/sonde/exports"
# downloads-dir = "/srv/sonde/downloads"

[settings.prices]           # for multipart, capacity and request costs
default-per-gib-month = 0.023
currency = "USD"
per-1000-writes = 0.005     # PUT, COPY, POST, LIST
per-1000-reads = 0.0004     # GET, HEAD and the rest (DELETE is free)
[settings.prices.by-class]  # per GiB-month
STANDARD = 0.023
STANDARD_IA = 0.0125
GLACIER = 0.0036
DEEP_ARCHIVE = 0.00099

[[endpoint]]
id = "prod"                                    # letters, digits, - _ .
name = "Production (eu-west-1)"
url = "https://s3.eu-west-1.amazonaws.com"
region = "eu-west-1"
vendor = "aws"                                 # aws r2 minio ceph-rgw aliyun-oss tigris backblaze-b2 wasabi gcs-xml other
path-style = false
credentials = { kind = "profile", name = "prod" }

[[endpoint]]
id = "lab"
name = "Lab MinIO"
url = "https://minio.lab:9000"
region = "us-east-1"
vendor = "minio"
path-style = true
ca-file = "/etc/ssl/lab-ca.pem"                # extra trusted CAs (PEM bundle)
buckets = ["scratch"]                          # shown when the credentials cannot ListBuckets
sign-payload = true                            # false: UNSIGNED-PAYLOAD
credentials = { kind = "env", access-key-id-var = "LAB_KEY", secret-access-key-var = "LAB_SECRET" }
```

`credentials.kind`: `default`, `profile`, `env`, `imds`, `web-identity`, `process`, `static`, `anonymous`
([credentials](credentials.md)).

## One-off endpoint

`sonde --url <URL> [--region R] [--path-style] [--ca-file F]` adds an unsaved endpoint `adhoc` with the default
credential chain. `--rate` and `--concurrency` are not saved either. A configured endpoint named `adhoc` (or `demo`,
while the demo runs) is hidden for that run, not replaced.
