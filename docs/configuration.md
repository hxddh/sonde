# Configuration

sonde reads `config.toml` from its configuration directory:

| | Linux | macOS |
|---|---|---|
| Configuration | `$SONDE_CONFIG_DIR`, else `$XDG_CONFIG_HOME/sonde`, else `~/.config/sonde` | `$SONDE_CONFIG_DIR`, else `$XDG_CONFIG_HOME/sonde`, else `~/Library/Application Support/sonde` |
| Data (request log, scans, probe runs, audit log, plugins) | `$SONDE_DATA_DIR`, else `~/.local/share/sonde` | `$SONDE_DATA_DIR`, else `~/Library/Application Support/sonde` |
| Exports | `<data>/exports` unless `exports-dir` | same |
| Downloads | `~/Downloads/sonde` unless `downloads-dir` | same |

The CLI takes `--config-dir` and `--data-dir` too. The file is written atomically, mode `0600` on Unix (it may hold
static keys for local test servers). Unknown endpoint ids, duplicate ids and bad URLs are refused when it loads.

```toml
[settings]
read-only = true          # destructive operations refused while on (default)
concurrency = 16          # requests in flight per job
rate-limit = 50.0         # requests per second per job; omit for none
reqlog-capacity = 5000    # records kept in the request log
reqlog-persist = false    # also keep them in <data>/requests.sqlite
theme = "system"          # system, light, dark
language = "auto"         # auto, en, zh
stale-upload-days = 7     # uploads older than this are flagged
# exports-dir = "/srv/sonde/exports"
# downloads-dir = "/srv/sonde/downloads"

[settings.prices]          # per GiB-month, for multipart and capacity costs
default-per-gib-month = 0.023
currency = "USD"
[settings.prices.by-class]
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

`credentials.kind`: `default`, `profile`, `env`, `imds`, `web-identity`, `process`, `static`, `anonymous` —
see [credentials.md](credentials.md).

`sonde --url <URL> [--region R] [--path-style] [--ca-file F]` adds an endpoint `adhoc` for one run, with the
default credential chain; it is not saved.
