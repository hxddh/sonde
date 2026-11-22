# Configuration

Two files, each setting in one of them:

- **`config.toml`** holds what sonde keeps for you: endpoints, prices, sonde's own pace and the app's settings. It is
  in sonde's configuration directory and goes with you, not with a project.
- **`sonde.toml`** holds what a project says about its program: its name, its budget, how it is recorded, its fault
  scenarios. It is in the project's folder (or one above it, up to the repository's root), committed with the code.

| Setting | File | Key |
|---|---|---|
| Endpoints and their credentials | `config.toml` | `[[endpoint]]` |
| Prices (storage, requests, bytes out) | `config.toml` | `[settings.prices]` |
| Whether a run's bytes out leave the store's region | `config.toml` | `[[endpoint]] egress` |
| Theme, language | `config.toml` | `[settings] theme`, `language` |
| The program's name | `sonde.toml` | `[program] name` |
| Its budget and baseline | `sonde.toml` | `[check]` |
| What a recorded program may do | `sonde.toml` | `[record] policy` |
| Naming the code that sent each request | `sonde.toml` | `[record] name-callers` |
| Serving TLS to the program | `sonde.toml` | `[record] tls` |
| Its fault scenarios | `sonde.toml` | `[[scenario]]` |

A flag wins over either file (`--policy`, `--no-callers`, `--tls`, `--name`, `check`'s limits). Both files are
in the contract ([contract](contract.md)).

## config.toml

| | Linux | macOS | Windows |
|---|---|---|---|
| Configuration | `$SONDE_CONFIG_DIR`, else `$XDG_CONFIG_HOME/sonde`, else `~/.config/sonde` | `$SONDE_CONFIG_DIR`, else `$XDG_CONFIG_HOME/sonde`, else `~/Library/Application Support/sonde` | `%SONDE_CONFIG_DIR%`, else `%XDG_CONFIG_HOME%\sonde`, else `%APPDATA%\sonde` |
| Data (runs, scans, audit log) | `$SONDE_DATA_DIR`, else `$XDG_DATA_HOME/sonde`, else `~/.local/share/sonde` | `$SONDE_DATA_DIR`, else `~/Library/Application Support/sonde` | `%SONDE_DATA_DIR%`, else `%APPDATA%\sonde` |
| Exports | `<data>/exports` | same | same |
| Downloads | `~/Downloads/sonde` | same | same |

The CLI also takes `--config-dir` and `--data-dir`.

- Written atomically, mode `0600` on Unix (it may hold static keys).
- Unknown or duplicate endpoint ids and bad URLs are refused on load; other unknown keys are passed over.
- A `ca-file` that does not exist is refused when its endpoint is added or changed, and said when it is used.
- A file that does not load is never overwritten: the app starts with no endpoints, shows why, and refuses saves
  until the file is fixed and reloaded.
- A save rereads the file and applies only what changed, so edits made meanwhile (by hand or `sonde endpoints`) are
  kept; comments are not.
- Concurrent saves take turns (`config.toml.lock`).

```toml
[settings]
theme = "system"            # system, light, dark
language = "auto"           # auto, en, zh

[settings.prices]           # for multipart, capacity and request costs
default-per-gib-month = 0.023  # storage: S3 Standard's (other AWS classes at AWS's list prices)
currency = "USD"
per-1000-writes = 0.005     # PUT, COPY, POST, LIST
per-1000-reads = 0.0004     # GET, HEAD and the rest (DELETE is free)
per-gib-out = 0.09          # bytes out of the store's region (egress)

[[endpoint]]
id = "prod"                                    # letters, digits, - _ .
name = "Production (eu-west-1)"
url = "https://s3.eu-west-1.amazonaws.com"
region = "eu-west-1"
vendor = "aws"                                 # aws r2 minio ceph-rgw aliyun-oss tencent-cos tigris backblaze-b2 wasabi gcs-xml other
path-style = false
credentials = { kind = "profile", name = "prod" }
egress = false                                 # read in its region (true: from outside it, billed); unset: not known

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

Request and egress prices left at S3's (as above) are the endpoint's vendor's own list prices, read 2026-11-18: egress
$0.09/GB on AWS (first 10 TB a month), $0.12/GiB on GCS, $0.08/GB on Alibaba OSS (outside the Chinese mainland), none
on R2, Tigris, Wasabi, B2 (up to three times what is stored), MinIO and Ceph; Tencent COS's is S3's until set. A
price you set wins for every endpoint.

### One-off endpoint

`sonde --url <URL> [--region R] [--path-style] [--ca-file F]` adds an unsaved endpoint `adhoc` with the default
credential chain. A configured endpoint named `adhoc` (or `demo`,
while the demo runs) is hidden for that run, not replaced.

## sonde.toml

Read by `run`, `check` and the app's command bar from the folder they run in or the nearest one above it, up to the
repository's root (`check --config FILE` names another). `show` and `share` read it too: `[program] name`
to find the program's newest run, and `show` what `[check] expect` keeps. Every key is checked: an unknown key, a
policy that is not one of three or a scenario sonde cannot run is an error (exit 64), in the CLI and the app alike.

```toml
[program]
name = "nightly-etl"      # runs, baseline and trial verdicts go by it
runs = "etl"              # for the commands that run it (`python3 etl.py 5`); others go by their own
runs-per-month = 720      # how often it runs: cost and savings said a month (cli.md, money a month)

[check]                   # `sonde check`: cli.md
max-requests = 5000
max-errors = 0
max-transfer = "50 GiB"   # bytes the store sent
baseline = "auto"

[check.findings]          # the findings the project knows, by count (`check --init` writes it): more fails
"obs.hot|data/conf.json" = 12

[record]
policy = "no-delete"      # what the program may do: allow (default; a trial: no-delete), no-delete, read-only
name-callers = true       # name the code that sent each request (Python, Node.js, Java); false: off
tls = false               # also serve TLS, with a certificate the program is told to trust

[[scenario]]              # trials.md
name = "flaky-wal"
fault = "503 SlowDown"
on = ["PutObject"]
```

- `[program]`: `name` and `runs` both or neither; without them, a program is what its command runs ([cli](cli.md)).
  `runs-per-month` alone applies to every program run in the folder.
- `[check]`: every key in [cli.md](cli.md#sonde-check).
- `[record] policy`: what each policy refuses is in [recording](recording.md#what-a-recorded-program-may-do). In the
  app, the project's policy may narrow the command bar's own choice, never widen it.
- `[[scenario]]`: [trials](trials.md#a-projects-own-scenarios).
