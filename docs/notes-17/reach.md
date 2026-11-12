# 17.0 notes: the reach of a lake (M4)

## Changelog lines

### Added

- **Spark and Flink are pointed at sonde.** `sonde run -- spark-submit …` (and `spark-shell`, `pyspark`,
  `spark-sql`, `sparkR`) is given `--conf spark.hadoop.fs.s3a.*` before its own arguments, `flink run` and
  `flink run-application` the same as `-Ds3.*`: the endpoint, path style, the region, plain HTTP, and the keys read
  from the environment (none on a command line). A Python program that starts Spark itself reads them from
  `PYSPARK_SUBMIT_ARGS`, in `sonde run` and in the app's Run. An endpoint the command names itself is warned of.
  Recipes for Spark, Hadoop's `core-site.xml`, Flink and Trino's catalogs in
  [docs/recording.md](docs/recording.md).
- **What s3a does, read as such**: a job committed by renaming out of `_temporary/` (FileOutputCommitter) is a
  warning, with the S3A committer settings that write each file once; s3a's directory markers (empty `…/`
  objects) and the deletes they cost are a finding.
- **TLS on the local endpoint**: `--tls`, or `tls = true` under `[record]` in `sonde.toml`. TLS and plain HTTP on
  one port; `AWS_ENDPOINT_URL` is `https://`, and `AWS_CA_BUNDLE`, `SSL_CERT_FILE`, `REQUESTS_CA_BUNDLE`,
  `CURL_CA_BUNDLE` name a bundle of the machine's roots and sonde's, `NODE_EXTRA_CA_CERTS` sonde's alone. The
  certificate is made on the first run by an authority whose key is never kept; its own key is 0600.
- **Hudi and Paimon as operations**: a completed Hudi instant (`commit`, `deltacommit`, `replacecommit`, …, the
  metadata table's said as such, 0.x and 1.x timelines) is a step with its requested and inflight states, its
  markers and its base and log files (`commit 20261112093000123 · 3 files (9.8 KiB)`); a Paimon
  `snapshot/snapshot-<n>` is one with its manifests and bucket files (`snapshot 7 · 1 file`), `LATEST` and
  `schema-<n>` steps of their own.
- A format's leftover deletes (Hudi's markers after a commit) read as `N files deleted`, not as reads.

### Changed

- **S3 Express One Zone is refused with its reason**: `CreateSession` is named, and it and every request to a
  directory bucket (`<name>--<zone>--x-s3`) or with `x-amz-s3session-token` are answered `501 NotImplemented`,
  saying it is not recorded yet, and never reach the store (before, the session's keys came back from the store
  and every request signed with them was refused as signed with a key sonde did not give).
- The hint printed when a program sent nothing no longer suggests `-Dfs.s3a.*`, which Hadoop does not read.

## Decision proposals

| Topic | Options | Choice and why |
|---|---|---|
| Pointing JVM engines at sonde | `JAVA_TOOL_OPTIONS`/`HADOOP_OPTS` `-Dfs.s3a.*`; write a `core-site.xml` and set `HADOOP_CONF_DIR`; the engine's own command line | **The command line, for the engines known by name** (`spark-*`, `flink run`), right after the executable (after the action for Flink), so the command's own arguments still win. `-D` system properties are not Hadoop's configuration; replacing `HADOOP_CONF_DIR` would hide the user's own. `PYSPARK_SUBMIT_ARGS` covers Python that starts Spark itself. Trino's CLI is a client: a recipe only. |
| Keys for s3a | `fs.s3a.access.key`/`secret.key` on the command line; the environment | **The environment**, through `EnvironmentVariableCredentialsProvider` (Hadoop 3.4 maps the v1 name): a command line is readable by every user of the machine, and the forced provider keeps a `spark-defaults.conf` key from signing instead. |
| TLS listener | a second port; TLS only when asked; both on one port | **Both on one port**, by the first byte (a handshake is 22): the SDKs get `https://`, the JVM engines, which trust no bundle a variable names, the plain endpoint beside it. |
| Trust for the child | the certificate alone in `SSL_CERT_FILE` etc.; a bundle | **A bundle of the machine's roots and sonde's authority** for the variables that replace the roots, the authority alone for `NODE_EXTRA_CA_CERTS` (which adds): what else the program reaches is trusted as before. |
| The certificate | a self-signed leaf; an authority kept; an authority forgotten | **An authority whose key is never written**, one leaf for `localhost`, `*.localhost`, `127.0.0.1`, `::1`, about two years, made again near its end: rustls clients refuse a self-signed leaf as a root, and trusting the authority then trusts nothing else. |
| S3 Express One Zone | answer `CreateSession` locally and re-sign upstream with a session sonde holds; pass it through; refuse with a reason | **Refuse with a reason** (501, nothing sent): passing through gives the program the store's session keys, which sonde cannot re-sign; holding sessions of its own per bucket needs `s3express` signing and zonal addressing in s3raw, which cannot be tested without AWS. For 18.0, with an AWS job in CI. |
| Hudi and Paimon commit points | every timeline write; the completed instant / the snapshot | **The completed instant and the snapshot file** (plus `hoodie.properties`, Paimon's `LATEST`/`EARLIEST` and `schema-<n>`, as Iceberg's version hint is): `requested`, `inflight`, markers and manifests fold into the commit; a step counts data files only. |
| Hudi and Paimon in `sonde layout` | decoders now; operations only | **Operations only** for 17.0: a layout lens needs the decoders' facts, phrases and the app's format list; the timeline is listing-only work for 18.0. |
| A rename-based commit | a step; a finding | **A warning finding** (`obs.rename`) counted from copies out of `_temporary/` and the deletes of their sources, its saving those requests, with the magic committer as the fix. |

## Not done, and why

- `sonde layout inspect` for Hudi and Paimon (see above).
- Argument injection in the app's Run (a shell line): only `PYSPARK_SUBMIT_ARGS` there; `--tls` and
  `[record] tls` are the CLI's, the app's recordings stay plain HTTP.
- A Spark run in the `record` CI job (the plan's check): pyspark is ~300 MB. CI wish: a job with
  `pip install pyspark==3.5.*` and `hadoop-aws` writing Parquet through `sonde run -- spark-submit`, asserting the
  `obs.rename` finding and a non-empty session; and one Flink local run.
- The certificate is not refreshed while a proxy runs; Windows' private key file has default ACLs.
