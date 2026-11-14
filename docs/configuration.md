# Configuration

Two files, each setting in one of them:

- **`config.toml`** holds what sonde keeps for you: endpoints, prices, sonde's own pace and the app's settings. It is
  in sonde's configuration directory and goes with you, not with a project.
- **`sonde.toml`** holds what a project says about its program: its name, its budget, how it is recorded, its fault
  scenarios. It is in the project's folder (or one above it, up to the repository's root), committed with the code.

| Setting | File | Key |
|---|---|---|
| Endpoints and their credentials | `config.toml` | `[[endpoint]]` |
| Prices (storage, requests) | `config.toml` | `[settings.prices]` |
| sonde's own requests in flight and per second | `config.toml` | `[settings] concurrency`, `rate-limit` |
| Theme, contrast, language | `config.toml` | `[settings] theme`, `high-contrast`, `language` |
| Where exports and downloads go | `config.toml` | `[settings] exports-dir`, `downloads-dir` |
| The program's name | `sonde.toml` | `[program] name` |
| Its budget and baseline | `sonde.toml` | `[check]` |
| What a recorded program may do | `sonde.toml` | `[record] policy` |
| Naming the code that sent each request | `sonde.toml` | `[record] name-callers` |
| Serving TLS to the program | `sonde.toml` | `[record] tls` |
| Its fault scenarios | `sonde.toml` | `[[scenario]]` |

A flag wins over either file (`--policy`, `--no-callers`, `--tls`, `--name`, `check`'s limits). Both files are
in the contract ([contract](contract.md)). Before 20.0, `record-policy` and `name-callers` were `config.toml`
settings; they are no longer read there.

## config.toml

| | Linux | macOS |
|---|---|---|
| Configuration | `$SONDE_CONFIG_DIR`, else `$XDG_CONFIG_HOME/sonde`, else `~/.config/sonde` | `$SONDE_CONFIG_DIR`, else `$XDG_CONFIG_HOME/sonde`, else `~/Library/Application Support/sonde` |
| Data (scans, probe runs, audit log, sessions) | `$SONDE_DATA_DIR`, else `~/.local/share/sonde` | `$SONDE_DATA_DIR`, else `~/Library/Application Support/sonde` |
| Exports | `<data>/exports` unless `exports-dir` | same |
| Downloads | `~/Downloads/sonde` unless `downloads-dir` | same |

The CLI also takes `--config-dir` and `--data-dir`.

- Written atomically, mode `0600` on Unix (it may hold static keys).
- Unknown or duplicate endpoint ids and bad URLs are refused on load; other unknown keys are passed over.
- A file that does not load is never overwritten: the app starts with no endpoints, shows why, and refuses saves
  until the file is fixed and reloaded.
- A save rereads the file and applies only what changed, so edits made meanwhile (by hand or `sonde endpoints`) are
  kept; comments are not.
- Concurrent saves take turns (`config.toml.lock`).

```toml
[settings]
concurrency = 16            # sonde's own requests in flight per job (not in the app)
rate-limit = 50.0           # sonde's own requests per second per job; omit for none (not in the app)
theme = "system"            # system, light, dark
high-contrast = false
language = "auto"           # auto, en, zh
# exports-dir = "/srv/sonde/exports"
# downloads-dir = "/srv/sonde/downloads"

[settings.prices]           # for multipart, capacity and request costs
default-per-gib-month = 0.023
currency = "USD"
per-1000-writes = 0.005     # PUT, COPY, POST, LIST
per-1000-reads = 0.0004     # GET, HEAD and the rest (DELETE is free)
[settings.prices.by-class]  # per GiB-month (not in the app)
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

sonde's own destructive acts are each a dry run, done on a confirmation and kept in the audit log; uploads are
stale past 7 days.

### One-off endpoint

`sonde --url <URL> [--region R] [--path-style] [--ca-file F]` adds an unsaved endpoint `adhoc` with the default
credential chain. `--rate` and `--concurrency` are not saved either. A configured endpoint named `adhoc` (or `demo`,
while the demo runs) is hidden for that run, not replaced.

## sonde.toml

Read by `run`, `check`, `proxy` and the app's Run from the folder they run in or the nearest one above it, up to the
repository's root (`check --config FILE` names another). Every key is checked: an unknown key, a policy that is not
one of three or a scenario sonde cannot run is an error (exit 64), in the CLI and the app alike.

```toml
[program]
name = "nightly-etl"      # runs, baseline, trial verdicts go by it, whatever the arguments

[check]                   # `sonde check`: cli.md
max-requests = 5000
max-errors = 0
baseline = "auto"

[record]
policy = "no-delete"      # what the program may do: no-delete (default), read-only, allow
name-callers = true       # name the code that sent each request (Python, Node.js, Java); false: off
tls = false               # also serve TLS, with a certificate the program is told to trust

[[scenario]]              # trials.md
name = "flaky-wal"
fault = "503 SlowDown"
on = ["PutObject"]
```

- `[program]`: without a name, a program is its command with what changes between runs left out ([cli](cli.md)).
- `[check]`: every key in [cli.md](cli.md#sonde-check).
- `[record] policy`: what each policy refuses is in [recording](recording.md#what-a-recorded-program-may-do). In the
  app, Run's own choice starts at `no-delete`; the project's policy may narrow it, never widen it.
- `[[scenario]]`: [trials](trials.md#a-projects-own-scenarios).
