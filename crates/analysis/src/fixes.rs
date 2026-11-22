//! What to change in the program, by SDK: each finding a few lines of code
//! that act on it (the change, not a whole program), and what acting on
//! every finding would leave of a run (`what_if`). An engine's own setting
//! comes first where its requests are an engine's ([`crate::engines`]).

use std::borrow::Cow;

use reqlog::Record;
use serde::Serialize;

use crate::episode::Observation;
use crate::profile::Profile;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fix {
    /// `python`, `java`, `go`, `js`, `rust` (object_store), or `config` for
    /// a store's or an engine's setting.
    pub sdk: &'static str,
    /// The change, its first line a comment that says it.
    pub code: Cow<'static, str>,
    /// The engine's setting it changes, when it is one: what an agent edits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setting: Option<crate::engines::Setting>,
}

const fn f(sdk: &'static str, code: &'static str) -> Fix {
    Fix {
        sdk,
        code: Cow::Borrowed(code),
        setting: None,
    }
}

/// A finding's fixes once it is said another way (`key`): its engine's
/// kept, and the SDKs' only where it had them (its engine's requests had
/// none).
pub fn rekeyed(old: Vec<Fix>, key: &str) -> Vec<Fix> {
    let sdks = old.iter().any(|f| f.sdk != "config");
    let own: Vec<Fix> = old.into_iter().filter(|f| f.setting.is_some()).collect();
    let engine = !own.is_empty();
    let general = for_key(key)
        .into_iter()
        .filter(|f| (sdks || f.sdk == "config") && !(f.sdk == "config" && engine));
    own.into_iter().chain(general).collect()
}

/// The SDK's own setting that turns its retries on, by language (`python`,
/// `java`, `go`, `js`): what a trial advises when a failed request was not
/// sent again. Retries and nothing else: no pool, no adaptive mode.
pub fn retries_on(sdk: &str) -> Option<&'static str> {
    Some(match sdk {
        "python" => {
            "boto3.client(\"s3\", config=Config(retries={\"mode\": \"standard\", \"max_attempts\": 5}))"
        }
        "java" => {
            "S3Client.builder().overrideConfiguration(o -> o.retryStrategy(RetryMode.STANDARD)).build()"
        }
        "go" => "config.LoadDefaultConfig(ctx, config.WithRetryMaxAttempts(5))",
        "js" => "new S3Client({ maxAttempts: 5 })",
        _ => return None,
    })
}

/// The changes that act on a finding, by its phrase key.
pub fn for_key(key: &str) -> Vec<Fix> {
    match key {
        "obs.hot" => cached(false),
        "obs.rangeagain" => cached(true),
        "obs.small" => packed(true, false),
        "obs.smallreads" => packed(false, true),
        "obs.serial" => at_once("GET"),
        "obs.headget" => vec![
            f(
                "python",
                "# GET alone: its response carries what HEAD would\nobj = s3.get_object(Bucket=b, Key=k)\nsize, etag = obj[\"ContentLength\"], obj[\"ETag\"]",
            ),
            f(
                "java",
                "// GET alone: its response carries what HEAD would\nResponseInputStream<GetObjectResponse> obj = s3.getObject(r -> r.bucket(b).key(k));\nlong size = obj.response().contentLength();",
            ),
            f(
                "go",
                "// GET alone: its response carries what HEAD would\nout, err := s3c.GetObject(ctx, &s3.GetObjectInput{Bucket: &b, Key: &k})\nsize := aws.ToInt64(out.ContentLength)",
            ),
            f(
                "js",
                "// GET alone: its response carries what HEAD would\nconst obj = await s3.send(new GetObjectCommand({ Bucket: b, Key: k }));\nconst size = obj.ContentLength;",
            ),
            f(
                "rust",
                "// GET alone: its result carries what HEAD would (object_store)\nlet obj = store.get(&path).await?;\nlet (size, etag) = (obj.meta.size, obj.meta.e_tag.clone());",
            ),
        ],
        "obs.rename" => vec![f(
            "config",
            "# Spark: the S3A magic committer (Hadoop 3.3+), no rename\nspark.hadoop.fs.s3a.committer.name=magic\nspark.sql.sources.commitProtocolClass=org.apache.spark.internal.io.cloud.PathOutputCommitProtocol\nspark.sql.parquet.output.committer.class=org.apache.spark.internal.io.cloud.BindingParquetOutputCommitter",
        )],
        "obs.markers" => vec![f(
            "config",
            "# keep markers: no delete after each file written beneath one\n# (every client of the bucket marker-aware: Hadoop 3.2.2+ or 3.3.1+)\nspark.hadoop.fs.s3a.directory.marker.retention=keep",
        )],
        "obs.listed" => vec![
            f(
                "python",
                "# list the prefix once; look keys up in the result\nkeys = {o[\"Key\"] for page in s3.get_paginator(\"list_objects_v2\").paginate(Bucket=b, Prefix=p) for o in page.get(\"Contents\", [])}",
            ),
            f(
                "java",
                "// list the prefix once; look keys up in the result\nSet<String> keys = s3.listObjectsV2Paginator(r -> r.bucket(b).prefix(p)).contents().stream().map(S3Object::key).collect(toSet());",
            ),
            f(
                "go",
                "// list the prefix once; look keys up in the result\nkeys := map[string]bool{}\nfor p := s3.NewListObjectsV2Paginator(s3c, &s3.ListObjectsV2Input{Bucket: &b, Prefix: &prefix}); p.HasMorePages(); {\n    page, err := p.NextPage(ctx)\n    if err != nil { return err }\n    for _, o := range page.Contents { keys[*o.Key] = true }\n}",
            ),
            f(
                "js",
                "// list the prefix once; look keys up in the result\nconst keys = new Set();\nfor await (const page of paginateListObjectsV2({ client: s3 }, { Bucket: b, Prefix: p })) for (const o of page.Contents ?? []) keys.add(o.Key);",
            ),
            f(
                "rust",
                "// list the prefix once; look keys up in the result (object_store)\nlet keys: HashSet<Path> = store.list(Some(&prefix)).map_ok(|m| m.location).try_collect().await?;",
            ),
        ],
        "obs.deletes" => vec![
            f(
                "python",
                "# up to 1,000 keys a request; a key not deleted is in Errors, not raised\nfor i in range(0, len(keys), 1000):\n    out = s3.delete_objects(Bucket=b, Delete={\"Objects\": [{\"Key\": k} for k in keys[i:i + 1000]], \"Quiet\": True})\n    if out.get(\"Errors\"):\n        raise RuntimeError(f\"not deleted: {out['Errors']}\")",
            ),
            f(
                "java",
                "// up to 1,000 keys a request; a key not deleted is in errors(), not thrown\nfor (int i = 0; i < keys.size(); i += 1000) {\n    List<ObjectIdentifier> ids = keys.subList(i, Math.min(i + 1000, keys.size())).stream().map(k -> ObjectIdentifier.builder().key(k).build()).toList();\n    DeleteObjectsResponse out = s3.deleteObjects(r -> r.bucket(b).delete(d -> d.objects(ids).quiet(true)));\n    if (!out.errors().isEmpty()) throw new IllegalStateException(\"not deleted: \" + out.errors());\n}",
            ),
            f(
                "go",
                "// up to 1,000 keys a request; a key not deleted is in out.Errors, not err\nfor i := 0; i < len(keys); i += 1000 {\n    var ids []types.ObjectIdentifier\n    for _, k := range keys[i:min(i+1000, len(keys))] { ids = append(ids, types.ObjectIdentifier{Key: aws.String(k)}) }\n    out, err := s3c.DeleteObjects(ctx, &s3.DeleteObjectsInput{Bucket: &b, Delete: &types.Delete{Objects: ids, Quiet: aws.Bool(true)}})\n    if err != nil { return err }\n    if len(out.Errors) > 0 { return fmt.Errorf(\"%d not deleted, the first %s: %s\", len(out.Errors), aws.ToString(out.Errors[0].Key), aws.ToString(out.Errors[0].Message)) }\n}",
            ),
            f(
                "js",
                "// up to 1,000 keys a request; a key not deleted is in Errors, not thrown\nfor (let i = 0; i < keys.length; i += 1000) {\n  const out = await s3.send(new DeleteObjectsCommand({ Bucket: b, Delete: { Objects: keys.slice(i, i + 1000).map((Key) => ({ Key })), Quiet: true } }));\n  if (out.Errors?.length) throw new Error(`not deleted: ${JSON.stringify(out.Errors)}`);\n}",
            ),
            f(
                "rust",
                "// up to 1,000 keys a request: object_store's S3 sends delete_stream as DeleteObjects\nlet paths = futures::stream::iter(paths.into_iter().map(Ok)).boxed();\nstore.delete_stream(paths).try_collect::<Vec<_>>().await?;",
            ),
        ],
        "obs.rmw" | "obs.rmw.log" => vec![
            f(
                "python",
                "# write back only over what was read: another writer's change fails it (412), not lost\nobj = s3.get_object(Bucket=b, Key=k)\ns3.put_object(Bucket=b, Key=k, Body=changed(obj[\"Body\"].read()), IfMatch=obj[\"ETag\"])  # on 412, read again",
            ),
            f(
                "java",
                "// write back only over what was read: another writer's change fails it (412), not lost\nResponseBytes<GetObjectResponse> obj = s3.getObjectAsBytes(r -> r.bucket(b).key(k));\ns3.putObject(r -> r.bucket(b).key(k).ifMatch(obj.response().eTag()), RequestBody.fromBytes(changed(obj.asByteArray())));",
            ),
            f(
                "go",
                "// write back only over what was read: another writer's change fails it (412), not lost\nobj, err := s3c.GetObject(ctx, &s3.GetObjectInput{Bucket: &b, Key: &k})\n_, err = s3c.PutObject(ctx, &s3.PutObjectInput{Bucket: &b, Key: &k, Body: changed(obj.Body), IfMatch: obj.ETag})",
            ),
            f(
                "js",
                "// write back only over what was read: another writer's change fails it (412), not lost\nconst obj = await s3.send(new GetObjectCommand({ Bucket: b, Key: k }));\nawait s3.send(new PutObjectCommand({ Bucket: b, Key: k, Body: changed(await obj.Body.transformToString()), IfMatch: obj.ETag }));",
            ),
        ],
        "obs.polled" => vec![
            f(
                "python",
                "# back off between tries (1 s, doubling, up to a minute) until a deadline:\n# fewer requests, not less time (it may see the key a wait late); better, an event from its writer\nimport time\nfrom botocore.exceptions import ClientError\nwait, deadline = 1, time.monotonic() + 600\nwhile True:\n    try:\n        s3.head_object(Bucket=b, Key=k)\n        break\n    except ClientError as e:\n        if e.response[\"Error\"][\"Code\"] != \"404\" or time.monotonic() > deadline:\n            raise\n        time.sleep(wait)\n        wait = min(wait * 2, 60)",
            ),
            f(
                "java",
                "// a waiter asks every 5 s, 20 times at most: fewer requests, not less time (it may see the key 5 s late)\ns3.waiter().waitUntilObjectExists(r -> r.bucket(b).key(k));",
            ),
            f(
                "go",
                "// a waiter backs off between tries (5 s, growing to 2 min) for 10 min at most: fewer requests, not less time\nerr := s3.NewObjectExistsWaiter(s3c).Wait(ctx, &s3.HeadObjectInput{Bucket: &b, Key: &k}, 10*time.Minute)",
            ),
            f(
                "js",
                "// a waiter backs off between tries (5 s, growing to 2 min) for 10 min at most: fewer requests, not less time\nawait waitUntilObjectExists({ client: s3, maxWaitTime: 600 }, { Bucket: b, Key: k });",
            ),
        ],
        "obs.small.parts" => vec![
            f(
                "python",
                "# one PUT under the threshold\nfrom boto3.s3.transfer import TransferConfig\ns3.upload_file(path, b, k, Config=TransferConfig(multipart_threshold=64 * 1024 * 1024))",
            ),
            f(
                "java",
                "// one PUT under the threshold\nS3TransferManager.builder().s3Client(S3AsyncClient.crtBuilder().thresholdInBytes(64L << 20).build()).build();",
            ),
            f(
                "go",
                "// one PUT under the threshold\nup := manager.NewUploader(s3c, func(u *manager.Uploader) { u.PartSize = 64 << 20 })",
            ),
            f(
                "js",
                "// one PUT under the threshold\nnew Upload({ client: s3, params, partSize: 64 * 1024 * 1024 })",
            ),
        ],
        "obs.throttled" | "obs.throttled.rate" => vec![
            f(
                "python",
                "# adaptive retries back off on 503 SlowDown\nfrom botocore.config import Config\ns3 = boto3.client(\"s3\", config=Config(retries={\"mode\": \"adaptive\", \"max_attempts\": 10}))",
            ),
            f(
                "java",
                "// adaptive retries back off on 503 SlowDown\nS3Client.builder().overrideConfiguration(o -> o.retryStrategy(AwsRetryStrategy.adaptiveRetryStrategy())).build();",
            ),
            f(
                "go",
                "// adaptive retries back off on 503 SlowDown\ncfg, _ := config.LoadDefaultConfig(ctx, config.WithRetryMode(aws.RetryModeAdaptive))",
            ),
            f(
                "js",
                "// adaptive retries back off on 503 SlowDown\nnew S3Client({ retryMode: \"adaptive\", maxAttempts: 10 })",
            ),
        ],
        "obs.prefixrate.reads" | "obs.prefixrate.writes" => vec![
            f(
                "python",
                "# spread keys over prefixes: a hash before the name (every reader names keys so)\nimport hashlib\nkey = f\"{hashlib.md5(name.encode()).hexdigest()[:2]}/{name}\"",
            ),
            f(
                "java",
                "// spread keys over prefixes: a hash before the name (every reader names keys so)\nString key = String.format(\"%02x/%s\", name.hashCode() & 0xff, name);",
            ),
            f(
                "go",
                "// spread keys over prefixes: a hash before the name (every reader names keys so)\nkey := fmt.Sprintf(\"%02x/%s\", crc32.ChecksumIEEE([]byte(name))&0xff, name)",
            ),
            f(
                "js",
                "// spread keys over prefixes: a hash before the name (every reader names keys so)\nconst key = `${createHash(\"md5\").update(name).digest(\"hex\").slice(0, 2)}/${name}`;",
            ),
        ],
        "obs.unfinished" => vec![f(
            "config",
            "# a lifecycle rule that aborts them, added to the bucket's own rules (a put of\n# the rules replaces them all; this keeps them): a dry run, then --yes\nsonde bucket lifecycle --bucket BUCKET --days 7",
        )],
        "obs.orphans" => vec![
            f(
                "python",
                "# files no commit names: the table's own cleanup removes them\nfrom deltalake import DeltaTable\nDeltaTable(uri).vacuum(retention_hours=168, dry_run=False)",
            ),
            f(
                "java",
                "// files no commit names: the table's own cleanup removes them\nspark.sql(\"CALL catalog.system.remove_orphan_files(table => 'db.events', older_than => TIMESTAMP '2026-01-01 00:00:00')\");",
            ),
        ],
        "obs.wholeread" => vec![
            f(
                "python",
                "# read the ranges used, not the whole object (pyarrow reads a Parquet file's footer, then its columns)
import pyarrow.parquet as pq, pyarrow.fs as pafs
table = pq.read_table(f\"{b}/{k}\", columns=[\"id\", \"ts\"], filesystem=pafs.S3FileSystem())",
            ),
            f(
                "java",
                "// read the ranges used, not the whole object
byte[] part = s3.getObjectAsBytes(r -> r.bucket(b).key(k).range(\"bytes=\" + start + \"-\" + end)).asByteArray();",
            ),
            f(
                "go",
                "// read the ranges used, not the whole object
out, err := s3c.GetObject(ctx, &s3.GetObjectInput{Bucket: &b, Key: &k, Range: aws.String(fmt.Sprintf(\"bytes=%d-%d\", start, end))})",
            ),
            f(
                "js",
                "// read the ranges used, not the whole object
const part = await s3.send(new GetObjectCommand({ Bucket: b, Key: k, Range: `bytes=${start}-${end}` }));",
            ),
        ],
        "obs.crawl" => vec![
            f(
                "python",
                "# one manifest names the files: read it, not every prefix (or the table's metadata: Iceberg, Delta)
import json
files = json.loads(s3.get_object(Bucket=b, Key=f\"{prefix}_manifest.json\")[\"Body\"].read())",
            ),
            f(
                "java",
                "// one manifest names the files: read it, not every prefix (or the table's metadata: Iceberg, Delta)
String manifest = s3.getObjectAsBytes(r -> r.bucket(b).key(prefix + \"_manifest.json\")).asUtf8String();",
            ),
            f(
                "go",
                "// one manifest names the files: read it, not every prefix (or the table's metadata: Iceberg, Delta)
out, err := s3c.GetObject(ctx, &s3.GetObjectInput{Bucket: &b, Key: aws.String(prefix + \"_manifest.json\")})",
            ),
            f(
                "js",
                "// one manifest names the files: read it, not every prefix (or the table's metadata: Iceberg, Delta)
const manifest = JSON.parse(await (await s3.send(new GetObjectCommand({ Bucket: b, Key: `${prefix}_manifest.json` }))).Body.transformToString());",
            ),
        ],
        _ => Vec::new(),
    }
}

/// The changes that act on a finding, from its requests (`rs`): small
/// objects packed as what they are (records as JSONL, media as tar shards),
/// requests sent at once by the operation they are. [`for_key`] for the rest.
pub fn for_finding(key: &str, rs: &[&Record]) -> Vec<Fix> {
    match key {
        "obs.small" => packed(true, media(rs)),
        "obs.smallreads" => packed(false, media(rs)),
        "obs.serial" => at_once(operation(rs)),
        _ => for_key(key),
    }
}

/// Whether most of `rs` are media or other binary files, by their keys'
/// extension: what is packed in tar shards, where records are packed as
/// lines.
fn media(rs: &[&Record]) -> bool {
    const MEDIA: &[&str] = &[
        "jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "heic", "mp3", "wav", "flac",
        "ogg", "mp4", "mov", "avi", "mkv", "webm", "npy", "npz", "pt", "bin", "pdf",
    ];
    let is_media = |r: &&&Record| {
        r.key
            .as_deref()
            .and_then(|k| k.rsplit_once('.'))
            .is_some_and(|(_, x)| MEDIA.contains(&x.to_ascii_lowercase().as_str()))
    };
    rs.iter().filter(is_media).count() * 2 > rs.len()
}

/// The operation most of `rs` are: `GET`, `PUT`, `HEAD` or `DELETE`.
fn operation(rs: &[&Record]) -> &'static str {
    let mut n = [0usize; 4];
    const OPS: [&str; 4] = ["GET", "PUT", "HEAD", "DELETE"];
    for r in rs {
        if let Some(i) = OPS.iter().position(|o| *o == r.method.as_str()) {
            n[i] += 1;
        }
    }
    let most = (0..4).max_by_key(|i| (n[*i], 4 - i)).unwrap_or(0);
    OPS[most]
}

fn owned(sdk: &'static str, code: String) -> Fix {
    Fix {
        sdk,
        code: Cow::Owned(code),
        setting: None,
    }
}

/// Read once and kept for the run: a key (`range` false) or a range of one.
fn cached(range: bool) -> Vec<Fix> {
    let (what, py, java, go, js) = if range {
        (
            "each range once and keep it (a Parquet footer, an index)",
            "def read_range(key, start, end):\n    return s3.get_object(Bucket=b, Key=key, Range=f\"bytes={start}-{end}\")[\"Body\"].read()",
            "byte[] read(String k, String range) { return cache.computeIfAbsent(k + range, x -> s3.getObjectAsBytes(r -> r.bucket(b).key(k).range(range)).asByteArray()); }",
            "k + rng",
            "const read = (k, Range) => cache.get(k + Range) ?? cache.set(k + Range, fetchRange(k, Range)).get(k + Range);",
        )
    } else {
        (
            "it once, keep it for the run",
            "def read(key):\n    return s3.get_object(Bucket=b, Key=key)[\"Body\"].read()",
            "byte[] read(String key) { return cache.computeIfAbsent(key, k -> s3.getObjectAsBytes(r -> r.bucket(b).key(k)).asByteArray()); }",
            "k",
            "const read = (k) => cache.get(k) ?? cache.set(k, fetchObject(k)).get(k);",
        )
    };
    vec![
        owned(
            "python",
            format!(
                "# read {what}\nfrom functools import lru_cache\n@lru_cache(maxsize=1024)\n{py}"
            ),
        ),
        owned(
            "java",
            format!(
                "// read {what}\nMap<String, byte[]> cache = new ConcurrentHashMap<>();\n{java}"
            ),
        ),
        owned(
            "go",
            format!(
                "// read {what}\nvar cache sync.Map\nif v, ok := cache.Load({go}); ok {{ return v.([]byte), nil }}"
            ),
        ),
        owned(
            "js",
            format!("// read {what}\nconst cache = new Map();\n{js}"),
        ),
    ]
}

/// Small objects packed many to one: written so (`write`), or read so; as
/// lines of records (JSONL; Parquet for columns), or as tar shards for media.
fn packed(write: bool, media: bool) -> Vec<Fix> {
    let ext = if media { "tar" } else { "jsonl" };
    let head = match (write, media) {
        (true, false) => {
            "many records in one object (JSONL, or Parquet for columns), not one object each"
        }
        (true, true) => "many files in one tar shard, not one object each",
        (false, false) => "read a batch of records at a time (JSONL), not one object each",
        (false, true) => "read a tar shard of files at a time, not one object each",
    };
    // (python, java, go, js): how a batch is made, or read
    let (py, java, go, js) = match (write, media) {
        (true, false) => (
            "body = \"\\n\".join(json.dumps(e) for e in batch).encode()",
            "String body = batch.stream().map(Object::toString).collect(Collectors.joining(\"\\n\"));",
            "body := bytes.Join(batch, []byte(\"\\n\"))",
            "const Body = batch.map((e) => JSON.stringify(e)).join(\"\\n\");",
        ),
        (true, true) => (
            "body = io.BytesIO()\nwith tarfile.open(fileobj=body, mode=\"w\") as tar:\n    for name, path in batch: tar.add(path, arcname=name)\nbody = body.getvalue()",
            "byte[] body = tarOf(batch); // TarArchiveOutputStream over a ByteArrayOutputStream",
            "var body bytes.Buffer\ntw := tar.NewWriter(&body) // a header and the bytes of each file, then tw.Close()",
            "const Body = tarOf(batch); // tar-stream: one entry a file",
        ),
        (false, false) => (
            "records = [json.loads(line) for line in body.iter_lines()]",
            "List<String> records = new String(body.readAllBytes()).lines().toList();",
            "sc := bufio.NewScanner(out.Body) // a record a line",
            "const records = (await Body.transformToString()).split(\"\\n\").map(JSON.parse);",
        ),
        (false, true) => (
            "tar = tarfile.open(fileobj=body, mode=\"r|\")  # each member a file",
            "TarArchiveInputStream tar = new TarArchiveInputStream(body); // each entry a file",
            "tr := tar.NewReader(out.Body) // each entry a file",
            "const entries = untar(Body); // tar-stream: each entry a file",
        ),
    };
    if write {
        vec![
            owned(
                "python",
                format!(
                    "# {head}\n{py}\ns3.put_object(Bucket=b, Key=f\"{{prefix}}{{batch_id}}.{ext}\", Body=body)"
                ),
            ),
            owned(
                "java",
                format!(
                    "// {head}\n{java}\ns3.putObject(r -> r.bucket(b).key(prefix + batchId + \".{ext}\"), RequestBody.fromBytes(body));"
                ),
            ),
            owned(
                "go",
                format!(
                    "// {head}\n{go}\n_, err := s3c.PutObject(ctx, &s3.PutObjectInput{{Bucket: &b, Key: aws.String(prefix + batchID + \".{ext}\"), Body: bytes.NewReader(body)}})"
                ),
            ),
            owned(
                "js",
                format!(
                    "// {head}\n{js}\nawait s3.send(new PutObjectCommand({{ Bucket: b, Key: `${{prefix}}${{batchId}}.{ext}`, Body }}));"
                ),
            ),
        ]
    } else {
        vec![
            owned(
                "python",
                format!(
                    "# {head}\nbody = s3.get_object(Bucket=b, Key=f\"{{prefix}}{{batch_id}}.{ext}\")[\"Body\"]\n{py}"
                ),
            ),
            owned(
                "java",
                format!(
                    "// {head}\nResponseInputStream<GetObjectResponse> body = s3.getObject(r -> r.bucket(b).key(prefix + batchId + \".{ext}\"));\n{java}"
                ),
            ),
            owned(
                "go",
                format!(
                    "// {head}\nout, err := s3c.GetObject(ctx, &s3.GetObjectInput{{Bucket: &b, Key: aws.String(prefix + batchID + \".{ext}\")}})\n{go}"
                ),
            ),
            owned(
                "js",
                format!(
                    "// {head}\nconst {{ Body }} = await s3.send(new GetObjectCommand({{ Bucket: b, Key: `${{prefix}}${{batchId}}.{ext}` }}));\n{js}"
                ),
            ),
        ]
    }
}

/// Requests sent eight at once instead of one after another, as the
/// operation they are (`GET`, `PUT`, `HEAD`, `DELETE`).
fn at_once(op: &str) -> Vec<Fix> {
    // (python, java's async client, go, js, object_store): one request
    let (py, java, go, js, rust) = match op {
        "PUT" => (
            "s3.put_object(Bucket=b, Key=k, Body=bodies[k])",
            "s3Async.putObject(r -> r.bucket(b).key(k), AsyncRequestBody.fromBytes(bodies.get(k)))",
            "_, err := s3c.PutObject(ctx, &s3.PutObjectInput{Bucket: &b, Key: &k, Body: bytes.NewReader(bodies[k])})\n        return err",
            "PutObjectCommand({ Bucket: b, Key: k, Body: bodies[k] })",
            "store.put(&p, payloads[&p].clone()).await",
        ),
        "HEAD" => (
            "s3.head_object(Bucket=b, Key=k)",
            "s3Async.headObject(r -> r.bucket(b).key(k))",
            "_, err := s3c.HeadObject(ctx, &s3.HeadObjectInput{Bucket: &b, Key: &k})\n        return err",
            "HeadObjectCommand({ Bucket: b, Key: k })",
            "store.head(&p).await",
        ),
        "DELETE" => (
            "s3.delete_object(Bucket=b, Key=k)",
            "s3Async.deleteObject(r -> r.bucket(b).key(k))",
            "_, err := s3c.DeleteObject(ctx, &s3.DeleteObjectInput{Bucket: &b, Key: &k})\n        return err",
            "DeleteObjectCommand({ Bucket: b, Key: k })",
            "store.delete(&p).await",
        ),
        _ => (
            "s3.get_object(Bucket=b, Key=k)[\"Body\"].read()",
            "s3Async.getObject(r -> r.bucket(b).key(k), AsyncResponseTransformer.toBytes())",
            "out, err := s3c.GetObject(ctx, &s3.GetObjectInput{Bucket: &b, Key: &k})\n        if err != nil { return err }\n        defer out.Body.Close()\n        _, err = io.ReadAll(out.Body)\n        return err",
            "GetObjectCommand({ Bucket: b, Key: k })",
            "store.get(&p).await?.bytes().await",
        ),
    };
    let head = "eight at once instead of one after another";
    vec![
        owned(
            "python",
            format!(
                "# {head}\nfrom concurrent.futures import ThreadPoolExecutor\nwith ThreadPoolExecutor(8) as pool:\n    results = list(pool.map(lambda k: {py}, keys))"
            ),
        ),
        owned(
            "java",
            format!(
                "// {head}\nvar all = keys.stream().map(k -> {java}).toList();\nCompletableFuture.allOf(all.toArray(CompletableFuture[]::new)).join();"
            ),
        ),
        owned(
            "go",
            format!(
                "// {head}\ng, ctx := errgroup.WithContext(ctx)\ng.SetLimit(8)\nfor _, k := range keys {{\n    g.Go(func() error {{\n        {go}\n    }})\n}}\nerr := g.Wait()"
            ),
        ),
        owned(
            "js",
            format!(
                "// {head}\nconst limit = pLimit(8);\nconst results = await Promise.all(keys.map((k) => limit(() => s3.send(new {js}))));"
            ),
        ),
        owned(
            "rust",
            format!(
                "// {head} (object_store)\nlet results: Vec<_> = futures::stream::iter(paths).map(|p| async move {{ {rust} }}).buffered(8).try_collect().await?;"
            ),
        ),
    ]
}

/// The language of an SDK as [`sonde_proxy::callers::sdk`] names it (`boto3
/// 1.44`): what picks the fix shown for it. None for the AWS CLI: botocore
/// underneath, and no code of the user's to change (D-258).
pub fn language(sdk: &str) -> Option<&'static str> {
    let name = sdk.split_whitespace().next().unwrap_or("");
    Some(match name {
        "boto3" | "botocore" | "s3fs" => "python",
        "aws-sdk-java" | "hadoop-aws" | "hadoop" => "java",
        "aws-sdk-go" | "aws-sdk-go-v2" => "go",
        "aws-sdk-js" => "js",
        "object_store" => "rust",
        _ => return None,
    })
}

/// Of a finding's fixes, the one for the SDK that sent its requests: an
/// engine's or a store's setting whatever the SDK (an engine's first, as
/// [`crate::engines::apply`] puts it); none for an SDK sonde has no fix in
/// the language of.
pub fn for_sdk<'a>(fixes: &'a [Fix], sdk: Option<&str>) -> Option<&'a Fix> {
    let lang = sdk.and_then(language);
    fixes
        .iter()
        .find(|f| f.sdk == "config")
        .or_else(|| fixes.iter().find(|f| Some(f.sdk) == lang))
}

/// A run after acting on the findings that name what they save.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatIf {
    /// Before and after.
    pub requests: (usize, usize),
    /// The bytes read out of the store, before and after.
    pub bytes_out: (u64, u64),
    /// None when the run's times cannot say it (an imported log's,
    /// to the second).
    pub waiting_ms: Option<(f64, f64)>,
    pub cost: (f64, f64),
    pub currency: String,
    /// The findings it counts, and how many findings there are (all that are
    /// listed with it): only those that name what they save are counted.
    pub findings: usize,
    pub of_findings: usize,
    /// The kinds of the findings counted (`obs.hot`), as they are named.
    pub counted: Vec<String>,
}

/// Prices what each finding would save ([`Observation::saves_cost`]),
/// times it (its own [`what_if`]: the run without its requests, its
/// waiting reckoned again) and orders the findings by it: those that save
/// time first, the most first, then money, then requests; the rest as they
/// were (warnings first, by the requests they bear on). A run whose times
/// are not known (`timed` false) is ordered by money, then requests.
pub fn rank_by_saving(
    findings: &mut [Observation],
    records: &[Record],
    profile: &Profile,
    timed: bool,
) {
    let prices = &profile.prices;
    if findings
        .iter()
        .any(|o| !o.saved_ids.is_empty() || o.saves_bytes > 0)
    {
        let by_id = crate::episode::RecordsById::of(records);
        for o in findings.iter_mut() {
            o.saves_cost = o
                .saved_ids
                .iter()
                .filter_map(|id| by_id.get(id))
                .map(|r| match o.keeps_bytes {
                    // its bytes are still read (and charged), in fewer requests
                    true => crate::profile::request_price(r, prices),
                    false => crate::profile::price(r, prices),
                })
                .sum::<f64>()
                + bytes_only_cost(o, prices);
            // (a sum of small prices, without its float's noise)
            o.saves_cost = (o.saves_cost * 1e9).round() / 1e9;
        }
    }
    if timed {
        for o in findings.iter_mut() {
            // a saving of time alone is reckoned where it is found
            if o.saves_requests > 0
                && let Some(w) = what_if_of(records, profile, &[&*o], 1, true)
                && let Some((before, after)) = w.waiting_ms
            {
                o.saves_ms = ((before - after) * 10.0).round() / 10.0;
            }
            if let Some(s) = time_saved(o.saves_ms, profile.wall_ms) {
                o.saving_text = Some(s.en());
                o.saving = Some(s);
            }
        }
    }
    findings.sort_by(|a, b| {
        saves(b)
            .cmp(&saves(a))
            .then(b.saves_ms.total_cmp(&a.saves_ms))
            .then(b.saves_cost.total_cmp(&a.saves_cost))
            .then(b.saves_requests.cmp(&a.saves_requests))
    });
}

/// Time saved as time of the run: `≈1.9 s of 6.1 s (31%)`; none under a
/// millisecond.
fn time_saved(ms: f64, wall_ms: f64) -> Option<crate::phrase::Phrase> {
    (ms >= 0.5 && wall_ms > 0.0).then(|| {
        crate::phrase::p(
            "obs.savesWall",
            &[
                ("time", sonde_fmt::duration(ms)),
                ("wall", sonde_fmt::duration(wall_ms)),
                (
                    "pct",
                    ((ms * 100.0 / wall_ms).round() as usize)
                        .clamp(1, 100)
                        .to_string(),
                ),
            ],
        )
    })
}

/// Findings that share a root cause, as one: what to change once, and what
/// that saves. Findings are of one diagnosis when they save something and
/// at least half of each one's objects are the other's (600 small objects
/// written one each, read one each, each with a HEAD first).
/// Requests one after another join the diagnosis whose objects or callers
/// most of them are (`and send the other N at once`), or stand alone.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    /// The same diagnosis across runs: made of its findings' identities.
    pub id: String,
    /// Its findings, by their place in the run's findings: the headline
    /// first, the requests one after another (`serial`) last.
    pub findings: Vec<usize>,
    /// Their identities, in the same order: what the fix loop matches.
    pub identities: Vec<String>,
    /// Its findings' requests one after another, by place, when it has them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<usize>,
    /// What acting on it saves, all its findings at once ([`what_if`] of
    /// them together, never their sum): time waited, money, requests.
    #[serde(skip_serializing_if = "is_zero_f")]
    pub saves_ms: f64,
    #[serde(skip_serializing_if = "is_zero_f")]
    pub saves_cost: f64,
    #[serde(skip_serializing_if = "is_zero_n")]
    pub saves_requests: usize,
    /// The saving said: time of the run, money beside it from a cent (a
    /// month's, when the run says its runs a month), else the requests.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub saving: Vec<crate::phrase::Phrase>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub saving_text: String,
    /// Its requests one after another, said as the rest of the change:
    /// `and send the other 87 requests 8 at once`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rest: Option<crate::phrase::Phrase>,
}

fn is_zero_f(v: &f64) -> bool {
    *v == 0.0
}

fn is_zero_n(v: &usize) -> bool {
    *v == 0
}

/// The run's findings as diagnoses ([`Diagnosis`]), in their order; the
/// findings put in that order too, each diagnosis's together. `findings`
/// ranked first ([`rank_by_saving`]).
pub fn diagnose(
    findings: &mut Vec<Observation>,
    episodes: &[crate::episode::Episode],
    records: &[Record],
    profile: &Profile,
    timed: bool,
    month: Option<&crate::month::Month>,
) -> Vec<Diagnosis> {
    use std::collections::{HashMap, HashSet};
    let by_id = crate::episode::RecordsById::of(records);
    let key = |o: &Observation| o.phrases.first().map_or("", |p| p.key);
    // the objects each finding bears on
    let objects: Vec<HashSet<(&str, &str)>> = findings
        .iter()
        .map(|o| {
            o.episodes
                .iter()
                .filter_map(|&i| episodes.get(i))
                .flat_map(|e| e.records.iter().filter_map(|id| by_id.get(id)))
                .filter_map(|r| Some((r.bucket.as_deref()?, r.key.as_deref()?)))
                .collect()
        })
        .collect();
    let joins = |i: usize| saves(&findings[i]) && key(&findings[i]) != "obs.serial";
    // one root, each finding's: the first of those it shares objects with
    let n = findings.len();
    let mut root: Vec<usize> = (0..n).collect();
    fn top(root: &mut [usize], mut i: usize) -> usize {
        while root[i] != i {
            root[i] = root[root[i]];
            i = root[i];
        }
        i
    }
    for a in (0..n).filter(|&a| joins(a)) {
        for b in (a + 1..n).filter(|&b| joins(b)) {
            let (x, y) = (&objects[a], &objects[b]);
            let (small, large) = if x.len() <= y.len() { (x, y) } else { (y, x) };
            // half of each's objects the other's (a key read again among
            // 600 small objects is not their cause)
            if small.is_empty() || small.len() * 2 < large.len() {
                continue;
            }
            let shared = small.iter().filter(|o| large.contains(*o)).count();
            if shared * 2 >= large.len() {
                let (ra, rb) = (top(&mut root, a), top(&mut root, b));
                root[ra.max(rb)] = ra.min(rb);
            }
        }
    }
    // groups in the findings' order (the first of each its headline)
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut group_of: HashMap<usize, usize> = HashMap::new();
    let mut serials: Vec<usize> = Vec::new();
    for (i, o) in findings.iter().enumerate() {
        if key(o) == "obs.serial" {
            serials.push(i);
            continue;
        }
        let r = top(&mut root, i);
        match group_of.get(&r) {
            Some(&g) => groups[g].push(i),
            None => {
                group_of.insert(r, groups.len());
                groups.push(vec![i]);
            }
        }
    }
    // requests one after another: with the diagnosis that owns most of
    // them, by their objects, else by where in the program they were sent
    let mut serial_of: Vec<Option<usize>> = vec![None; groups.len()];
    let saving: Vec<usize> = (0..groups.len())
        .filter(|&g| saves(&findings[groups[g][0]]))
        .collect();
    for s in serials {
        let mut owns = vec![0usize; groups.len()];
        let places: Vec<HashSet<&str>> = groups
            .iter()
            .map(|g| {
                g.iter()
                    .flat_map(|&i| findings[i].callers.iter().map(|c| c.place.as_str()))
                    .collect()
            })
            .collect();
        let timed_rs: Vec<&Record> = findings[s]
            .timed_ids
            .iter()
            .filter_map(|id| by_id.get(id))
            .collect();
        for r in &timed_rs {
            let object = r.bucket.as_deref().zip(r.key.as_deref());
            let caller = sonde_proxy::callers::caller(r);
            let owner = saving
                .iter()
                .copied()
                .find(|&g| {
                    object.is_some_and(|o| groups[g].iter().any(|&i| objects[i].contains(&o)))
                })
                .or_else(|| {
                    let c = caller.as_deref()?;
                    saving.iter().copied().find(|&g| places[g].contains(c))
                });
            if let Some(g) = owner {
                owns[g] += 1;
            }
        }
        match saving
            .iter()
            .copied()
            .max_by_key(|&g| (owns[g], usize::MAX - g))
        {
            Some(g) if owns[g] * 2 >= timed_rs.len().max(1) && serial_of[g].is_none() => {
                serial_of[g] = Some(s);
                groups[g].push(s);
            }
            _ => {
                serial_of.push(None);
                groups.push(vec![s]);
            }
        }
    }
    // what each saves, all of its findings at once
    let mut out: Vec<(Diagnosis, Vec<usize>)> = groups
        .into_iter()
        .zip(serial_of)
        .map(|(g, serial)| {
            let members: Vec<&Observation> = g.iter().map(|&i| &findings[i]).collect();
            let (ms, cost, requests) = match members.as_slice() {
                [one] => (one.saves_ms, one.saves_cost, one.saves_requests),
                _ => match what_if_of(records, profile, &members, members.len(), timed) {
                    Some(w) => (
                        w.waiting_ms
                            .map_or(0.0, |(a, b)| ((a - b) * 10.0).round() / 10.0),
                        ((w.cost.0 - w.cost.1) * 1e9).round() / 1e9,
                        w.requests.0 - w.requests.1,
                    ),
                    None => (0.0, 0.0, 0),
                },
            };
            let mut ids: Vec<&str> = members.iter().map(|o| o.identity.as_str()).collect();
            ids.sort_unstable();
            let mut d = Diagnosis {
                id: diagnosis_id(&ids),
                findings: Vec::new(),
                identities: Vec::new(),
                // (attached: its place is set below, once the findings are in order)
                serial,
                saves_ms: ms,
                saves_cost: cost,
                saves_requests: requests,
                saving: Vec::new(),
                saving_text: String::new(),
                rest: None,
            };
            d.rest = serial.map(|s| {
                crate::phrase::p(
                    "obs.serial.rest",
                    &[
                        ("n", findings[s].impact.to_string()),
                        ("k", crate::profile::CHAIN_CONCURRENCY.to_string()),
                    ],
                )
            });
            d.saving = saving_of(&d, members[0], profile, timed, month, records.len());
            d.saving_text = crate::phrase::en(&d.saving);
            (d, g)
        })
        .collect();
    // the most time saved first, then money, then requests; the rest after
    let some = |d: &Diagnosis| d.saves_ms > 0.0 || d.saves_cost > 0.0 || d.saves_requests > 0;
    out.sort_by(|(a, ga), (b, gb)| {
        some(b)
            .cmp(&some(a))
            .then(b.saves_ms.total_cmp(&a.saves_ms))
            .then(b.saves_cost.total_cmp(&a.saves_cost))
            .then(b.saves_requests.cmp(&a.saves_requests))
            .then(ga[0].cmp(&gb[0]))
    });
    // the findings in the diagnoses' order, each diagnosis's together
    let mut old: Vec<Option<Observation>> =
        std::mem::take(findings).into_iter().map(Some).collect();
    out.into_iter()
        .map(|(mut d, g)| {
            for i in g {
                if let Some(o) = old[i].take() {
                    d.identities.push(o.identity.clone());
                    d.findings.push(findings.len());
                    findings.push(o);
                }
            }
            // attached, it is the last
            if d.serial.is_some() {
                d.serial = d.findings.last().copied();
            }
            d
        })
        .collect()
}

/// A diagnosis's id: its findings' identities (sorted), hashed (FNV-1a):
/// the same findings, the same id, in any run.
fn diagnosis_id(identities: &[&str]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in identities.join("\n").bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("dx-{:012x}", h >> 16)
}

/// A diagnosis's saving said: the time of the run it saves (with no
/// times, the requests; with neither, its headline's saving), and beside it
/// the money from a cent (a month's, when the run says how many a month).
fn saving_of(
    d: &Diagnosis,
    head: &Observation,
    profile: &Profile,
    timed: bool,
    month: Option<&crate::month::Month>,
    total: usize,
) -> Vec<crate::phrase::Phrase> {
    use crate::phrase::p;
    let first = match time_saved(d.saves_ms, profile.wall_ms).filter(|_| timed) {
        Some(t) => Some(t),
        None if d.saves_requests > 0 => Some(p(
            "obs.saves",
            &[
                ("n", d.saves_requests.to_string()),
                ("s", crate::phrase::plural(d.saves_requests)),
                (
                    "pct",
                    crate::episode::share_pct(d.saves_requests, total).to_string(),
                ),
            ],
        )),
        None => head.saving.clone(),
    };
    let mut out: Vec<crate::phrase::Phrase> = first.into_iter().collect();
    let (amount, key, currency) = match month {
        Some(m) => (m.of(d.saves_cost), "obs.savesMonth", m.currency.as_str()),
        None => (
            d.saves_cost,
            "obs.savesMoney",
            profile.cost.currency.as_str(),
        ),
    };
    if !out.is_empty() && amount >= crate::program::CENT {
        out.push(p(
            key,
            &[("money", crate::program::money_text(amount, currency))],
        ));
    }
    out
}

/// Whether acting on a finding saves something it can count: requests,
/// time or bytes.
fn saves(o: &Observation) -> bool {
    o.saves_requests > 0 || o.saves_ms > 0.0 || o.saves_bytes > 0
}

/// What a finding that saves bytes and no request (a read of the ranges
/// used for a whole one) saves, as egress: when the bytes out are charged.
fn bytes_only_cost(o: &Observation, prices: &crate::profile::RequestPrices) -> f64 {
    match prices.egress() {
        crate::profile::Egress::Charged if o.saved_ids.is_empty() => prices.out(o.saves_bytes),
        _ => 0.0,
    }
}

/// What acting on the findings would leave, measured and not estimated: the
/// requests the findings name are taken out of the run, and the run
/// without them is reckoned again. The waiting is the time with a request
/// in flight among those left (a request that ran beside another saves
/// nothing by going), and a request one finding keeps in place of those it
/// saves is never taken away by another's ([`Observation::kept_ids`]). The cost is the price of those left. A finding that
/// saves time without removing a request (requests sent at once) takes its
/// own figure off, when none of its requests is among those removed. A
/// finding whose requests' bytes are read still, in fewer requests
/// ([`Observation::keeps_bytes`]), takes the requests away and not their
/// bytes. A finding that does not name its requests is not counted. A run whose
/// waiting is not known (`timed` false: an imported log's) says none, and
/// counts no finding that saves only time.
pub fn what_if(
    records: &[Record],
    profile: &Profile,
    findings: &[Observation],
    timed: bool,
) -> Option<WhatIf> {
    let all: Vec<&Observation> = findings.iter().collect();
    what_if_of(records, profile, &all, findings.len(), timed)
}

/// [`what_if`] of some findings (`of` listed in all).
fn what_if_of(
    records: &[Record],
    profile: &Profile,
    findings: &[&Observation],
    of: usize,
    timed: bool,
) -> Option<WhatIf> {
    let saving: Vec<&Observation> = findings.iter().copied().filter(|o| saves(o)).collect();
    if saving.is_empty() || records.is_empty() {
        return None;
    }
    let mut gone: std::collections::HashSet<u64> = Default::default();
    let mut counted: Vec<&Observation> = Vec::new();
    for o in saving.iter().filter(|o| o.saves_requests > 0) {
        if o.saved_ids.is_empty() {
            continue;
        }
        gone.extend(o.saved_ids.iter().copied());
        counted.push(o);
    }
    // what one finding keeps in place of what it saves stays, whatever
    // another saves: a cache's first read of a key is not also a HEAD
    // dropped before a GET the cache took away (one read is left, not none)
    for o in &counted {
        for id in &o.kept_ids {
            gone.remove(id);
        }
    }
    // the requests gone whose bytes are read still, in fewer requests (small
    // objects read from shards): unless another finding takes them away
    // with their bytes (a cache)
    let with_bytes: std::collections::HashSet<u64> = counted
        .iter()
        .filter(|o| !o.keeps_bytes)
        .flat_map(|o| o.saved_ids.iter().copied())
        .collect();
    let still_read: std::collections::HashSet<u64> = counted
        .iter()
        .filter(|o| o.keeps_bytes)
        .flat_map(|o| o.saved_ids.iter().copied())
        .filter(|id| gone.contains(id) && !with_bytes.contains(id))
        .collect();
    // never the whole run: a request each finding keeps is among the ids
    // it does not name. Those left are added up where they are, not copied
    let left = || records.iter().filter(|r| !gone.contains(&r.id));
    let moved = || {
        records
            .iter()
            .filter(|r| still_read.contains(&r.id) && crate::profile::reached_store(r))
    };
    let mut waiting = crate::profile::waiting_ms_of(left());
    for o in saving.iter().filter(|o| timed && o.saves_requests == 0) {
        // its figure was reckoned with every request there: with one of
        // them gone it is another figure, and is left out
        if o.timed_ids.is_empty()
            || o.saves_ms > waiting
            || o.timed_ids.iter().any(|id| gone.contains(id))
        {
            continue;
        }
        waiting -= o.saves_ms;
        counted.push(o);
    }
    // a finding that saves bytes alone (ranges read for a whole object):
    // its bytes, and their egress when charged
    let (mut fewer_bytes, mut fewer_cost) = (0u64, 0.0);
    for o in saving
        .iter()
        .filter(|o| o.saves_requests == 0 && o.saves_ms == 0.0 && o.saves_bytes > 0)
    {
        fewer_bytes += o.saves_bytes;
        fewer_cost += bytes_only_cost(o, &profile.prices);
        counted.push(o);
    }
    if counted.is_empty() {
        return None;
    }
    let cost: f64 = left()
        .map(|r| crate::profile::price(r, &profile.prices))
        .chain(moved().map(|r| {
            crate::profile::price(r, &profile.prices)
                - crate::profile::request_price(r, &profile.prices)
        }))
        .sum::<f64>()
        - fewer_cost;
    let bytes_left: u64 = left()
        .chain(moved())
        .filter(|r| crate::profile::reached_store(r))
        .map(crate::profile::out_bytes)
        .sum();
    Some(WhatIf {
        requests: (records.len(), left().count()),
        bytes_out: (
            profile.cost.transfer.bytes,
            bytes_left
                .saturating_sub(fewer_bytes)
                .min(profile.cost.transfer.bytes),
        ),
        waiting_ms: timed.then(|| (profile.waiting_ms, waiting.clamp(0.0, profile.waiting_ms))),
        cost: (profile.cost.amount, cost.clamp(0.0, profile.cost.amount)),
        currency: profile.cost.currency.clone(),
        findings: counted.len(),
        of_findings: of,
        counted: counted
            .iter()
            .filter_map(|o| o.phrases.first().map(|p| p.key.to_string()))
            .collect(),
    })
}

impl WhatIf {
    /// The line `sonde show` closes its findings with: their total, the
    /// cost a month when `month` says one.
    pub fn line(&self, month: Option<&crate::month::Month>) -> String {
        use crate::program::money_text;
        use crate::signals::{count_text, ms_text};
        let which = if self.findings < self.of_findings {
            // which ones, when not all: the reader cannot guess
            let mut kinds: Vec<&str> = self.counted.iter().map(|k| kind_name(k)).collect();
            kinds.dedup();
            format!(
                "{} of {} findings: {}",
                count_text(self.findings),
                count_text(self.of_findings),
                kinds.join(", ")
            )
        } else {
            format!(
                "{} finding{}",
                count_text(self.findings),
                if self.findings == 1 { "" } else { "s" }
            )
        };
        // time first, then money, then requests and bytes
        let mut parts = Vec::new();
        if let Some((before, after)) = self.waiting_ms
            && ms_text(before) != ms_text(after)
        {
            parts.push(format!("waiting {} → {}", ms_text(before), ms_text(after)));
        }
        let (a, b, a_month) = match month {
            Some(m) => (m.of(self.cost.0), m.of(self.cost.1), " a month"),
            None => (self.cost.0, self.cost.1, ""),
        };
        let (before, after) = (money_text(a, &self.currency), money_text(b, &self.currency));
        // under a cent, an amount nobody acts on: not said, nor its share
        if a.max(b) >= crate::program::CENT && before != after {
            parts.push(format!("cost {before} → {after}{a_month}"));
        }
        if self.requests.0 != self.requests.1 {
            parts.push(format!(
                "{} → {} requests",
                count_text(self.requests.0),
                count_text(self.requests.1)
            ));
        }
        let bytes = crate::episode::human_bytes;
        if bytes(self.bytes_out.0) != bytes(self.bytes_out.1) {
            parts.push(format!(
                "read {} → {}",
                bytes(self.bytes_out.0),
                bytes(self.bytes_out.1)
            ));
        }
        format!("in all ({which}): {}", parts.join(" · "))
    }
}

/// A finding's kind in a few words, for naming which were counted.
fn kind_name(key: &str) -> &'static str {
    match key {
        "obs.hot" => "a key read again and again",
        "obs.rename" => "a commit by rename",
        "obs.markers" => "directory markers deleted",
        "obs.listed" => "a prefix listed again and again",
        "obs.rangeagain" => "a range read again",
        "obs.wholeread" => "objects read whole",
        "obs.smallreads" => "small objects read one by one",
        "obs.crawl" => "prefixes listed one by one",
        "obs.headget" => "HEAD before GET",
        "obs.small" | "obs.small.parts" => "small files",
        "obs.deletes" => "objects deleted one at a time",
        "obs.polled" => "a missing key polled",
        "obs.serial" => "requests one after another",
        _ => "another",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::episode::{episodes, observations};
    use crate::profile::{RequestPrices, profile};
    use chrono::DateTime;

    #[test]
    fn the_fix_shown_is_the_detected_sdks_and_a_setting_for_any() {
        let head = for_key("obs.headget");
        let pick = |sdk: Option<&str>| for_sdk(&head, sdk).map(|f| f.sdk);
        assert_eq!(pick(Some("boto3 1.44")), Some("python"));
        assert_eq!(pick(Some("aws-sdk-java 2.21")), Some("java"));
        assert_eq!(pick(Some("aws-sdk-go-v2 1.30")), Some("go"));
        assert_eq!(pick(Some("aws-sdk-js 3.600")), Some("js"));
        // object_store, Rust's: its own calls (an engine on top of it, SlateDB
        // or Lance, gets the engine's setting instead: tests/engines.rs)
        assert_eq!(pick(Some("object_store 0.14")), Some("rust"));
        // an SDK with no fix in its language, or none detected: none shown
        assert_eq!(pick(Some("aws-sdk-cpp 1.11")), None);
        assert_eq!(pick(None), None);
        // the AWS CLI is botocore underneath, and no Python a user writes:
        // boto3's code is no fix for it (D-258)
        assert_eq!(pick(Some("aws-cli 2.15")), None);
        assert_eq!(retries_on(language("aws-cli 2.15").unwrap_or("")), None);
        // an engine's setting, whatever sent it
        assert_eq!(
            for_sdk(&for_key("obs.rename"), Some("hadoop 3.3")).map(|f| f.sdk),
            Some("config")
        );
    }

    #[test]
    fn each_fix_does_what_its_sdk_needs_for_it() {
        let code = |key: &str, sdk: &str| {
            for_key(key)
                .into_iter()
                .find(|f| f.sdk == sdk)
                .unwrap()
                .code
        };
        // a batch delete answers a key it did not delete in its errors, not
        // as an error: every SDK's fix looks
        for (sdk, errors) in [
            ("python", "out.get(\"Errors\")"),
            ("java", "out.errors()"),
            ("go", "out.Errors"),
            ("js", "out.Errors"),
        ] {
            assert!(code("obs.deletes", sdk).contains(errors), "{sdk}");
        }
        // a poll backed off asks fewer times; it is no sooner
        for f in for_key("obs.polled") {
            assert!(
                f.code.contains("fewer requests, not less time"),
                "{}",
                f.code
            );
        }
        // eight GETs at once: each body read and closed (Go's connection is
        // kept only so)
        assert!(at_once("GET")[2].code.contains("defer out.Body.Close()"));
        // one PUT under the threshold: the CRT's threshold, not its part size
        assert!(code("obs.small.parts", "java").contains("thresholdInBytes"));
    }

    #[test]
    fn no_fix_undoes_what_the_program_relies_on() {
        // throttled: the advice is to send fewer, never a larger pool than
        // the SDK's own (boto3's is 10)
        for key in ["obs.throttled", "obs.throttled.rate"] {
            assert!(
                for_key(key)
                    .iter()
                    .all(|f| !f.code.contains("max_pool_connections")),
                "{key}"
            );
        }
        // an upload rule goes beside the bucket's own rules: a put of a
        // lifecycle configuration replaces them all, `bucket lifecycle` keeps
        // them (services::mpu's tests)
        let rule = &for_key("obs.unfinished")[0];
        assert!(
            rule.code.contains("sonde bucket lifecycle"),
            "{}",
            rule.code
        );
        assert!(!rule.code.contains("<LifecycleConfiguration>"));
        // keys named another way, markers kept: what every other reader of
        // the bucket must do the same, said with the change
        assert!(
            for_key("obs.prefixrate.reads")
                .iter()
                .all(|f| f.code.contains("every reader"))
        );
        assert!(for_key("obs.markers")[0].code.contains("every client"));
        // what a cache, a listing kept or requests at once would change is
        // never found: episode's and profile's tests (a key rewritten or
        // missing, a prefix written between listings, a read of a write)
    }

    #[test]
    fn acting_on_a_hot_key_leaves_one_read_and_shows_how() {
        let rs: Vec<Record> = (0..20u64)
            .map(|i| Record {
                id: i + 1,
                time: DateTime::from_timestamp_millis(1_700_000_000_000 + i as i64 * 50).unwrap(),
                method: "GET".into(),
                kind: "GetObject".into(),
                bucket: Some("b".into()),
                key: Some(if i < 15 {
                    "conf.json".into()
                } else {
                    format!("k{i}").into()
                }),
                status: Some(200),
                latency_ms: 10.0,
                ..Default::default()
            })
            .collect();
        let p = profile(&rs, &RequestPrices::default());
        let obs = observations(&rs, &episodes(&rs));
        let hot = obs
            .iter()
            .find(|o| o.phrases[0].key == "obs.hot")
            .expect("a hot key");
        assert_eq!(hot.saves_requests, 14);
        assert!(
            hot.fixes
                .iter()
                .any(|f| f.sdk == "python" && f.code.contains("lru_cache"))
        );
        let w = what_if(&rs, &p, &obs, true).unwrap();
        assert_eq!(w.requests, (20, 6));
        let waiting = w.waiting_ms.unwrap();
        assert!(waiting.1 < waiting.0 && w.cost.1 < w.cost.0);
        // 20 reads of 10 ms each, 50 ms apart: the 14 gone took 140 ms
        assert_eq!(w.waiting_ms, Some((200.0, 60.0)));
    }

    #[test]
    fn three_quick_heads_gone_take_their_own_time_and_price_and_no_more() {
        let prices = RequestPrices {
            per_1000_writes: 5.0,
            per_1000_reads: 0.4,
            per_1000_lists: None,
            currency: "USD".into(),
            ..RequestPrices::default()
        };
        let mut rs = Vec::new();
        for i in 0..3u64 {
            let at = 1_700_000_000_000 + i as i64 * 400;
            let r = |id, at, method: &str, op: &str, ms| Record {
                id,
                time: DateTime::from_timestamp_millis(at).unwrap(),
                method: method.into(),
                kind: op.into(),
                bucket: Some("b".into()),
                key: Some(format!("k{i}").into()),
                status: Some(200),
                latency_ms: ms,
                ..Default::default()
            };
            // a 2 ms HEAD, then the 60 ms GET it was sent before
            rs.push(r(i * 3 + 1, at, "HEAD", "HeadObject", 2.0));
            rs.push(r(i * 3 + 2, at + 10, "GET", "GetObject", 60.0));
            // and a slow write, most of the cost
            let mut put = r(i * 3 + 3, at + 100, "PUT", "PutObject", 3.0);
            put.key = Some(format!("out{i}").into());
            rs.push(put);
        }
        let p = profile(&rs, &prices);
        let obs = observations(&rs, &episodes(&rs));
        assert!(obs.iter().any(|o| o.phrases[0].key == "obs.headget"));
        let w = what_if(&rs, &p, &obs, true).unwrap();
        assert_eq!(w.requests, (9, 6));
        // 195 ms of waiting, of which the three HEADs were 6: not a third of it
        assert_eq!(w.waiting_ms, Some((195.0, 189.0)));
        // three reads' price off: 3 × 0.0004 of 0.0174
        assert!((w.cost.0 - 0.0174).abs() < 1e-9, "{:?}", w.cost);
        assert!((w.cost.1 - 0.0162).abs() < 1e-9, "{:?}", w.cost);
        // and it is the same on every reading
        assert_eq!(what_if(&rs, &p, &obs, true), Some(w.clone()));
        assert_eq!(
            w.line(None),
            "in all (1 finding): waiting 195 ms → 189 ms · 9 → 6 requests",
            "a cost that reads the same before and after is not printed as a change"
        );
    }

    #[test]
    fn a_finding_that_names_no_request_is_left_out_and_the_line_says_which_count() {
        let rs: Vec<Record> = (0..20u64)
            .map(|i| Record {
                id: i + 1,
                time: DateTime::from_timestamp_millis(1_700_000_000_000 + i as i64 * 50).unwrap(),
                method: "GET".into(),
                kind: "GetObject".into(),
                bucket: Some("b".into()),
                key: Some("conf.json".into()),
                status: Some(200),
                latency_ms: 10.0,
                ..Default::default()
            })
            .collect();
        let p = profile(&rs, &RequestPrices::default());
        let mut obs = observations(&rs, &episodes(&rs));
        let mut vague = obs
            .iter()
            .find(|o| o.phrases[0].key == "obs.hot")
            .unwrap()
            .clone();
        vague.saved_ids.clear();
        vague.phrases[0].key = "obs.listed";
        obs.push(vague.clone());
        let w = what_if(&rs, &p, &obs, true).unwrap();
        assert_eq!((w.findings, w.of_findings), (1, 2));
        assert_eq!(w.requests, (20, 1));
        assert!(
            w.line(None).starts_with(
                "in all (1 of 2 findings: a key read again and again): waiting 200 ms → 10 ms · 20 → 1 requests"
            ),
            "{}",
            w.line(None)
        );
        // nothing that can be counted: nothing is said
        assert_eq!(what_if(&rs, &p, &[vague], true), None);
    }
}
