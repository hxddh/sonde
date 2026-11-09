//! What to change in the program, by SDK: each finding a few lines of code
//! that act on it (the change, not a whole program), and what acting on
//! every finding would leave of a session (`what_if`).

use reqlog::Record;
use serde::Serialize;

use crate::episode::Observation;
use crate::profile::Profile;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fix {
    /// `python`, `java`, `go`, `js`, or `config` for a store's setting.
    pub sdk: &'static str,
    pub code: &'static str,
}

const fn f(sdk: &'static str, code: &'static str) -> Fix {
    Fix { sdk, code }
}

/// The changes that act on a finding, by its phrase key.
pub fn for_key(key: &str) -> Vec<Fix> {
    match key {
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
                "// GET alone: its response carries what HEAD would\nout, err := s3c.GetObject(ctx, &s3.GetObjectInput{Bucket: &b, Key: &k})\nsize := out.ContentLength",
            ),
            f(
                "js",
                "// GET alone: its response carries what HEAD would\nconst obj = await s3.send(new GetObjectCommand({ Bucket: b, Key: k }));\nconst size = obj.ContentLength;",
            ),
        ],
        "obs.hot" => vec![
            f(
                "python",
                "# read it once, keep it for the run\nfrom functools import lru_cache\n@lru_cache(maxsize=None)\ndef read(key):\n    return s3.get_object(Bucket=b, Key=key)[\"Body\"].read()",
            ),
            f(
                "java",
                "// read it once, keep it for the run\nMap<String, byte[]> cache = new ConcurrentHashMap<>();\nbyte[] read(String key) { return cache.computeIfAbsent(key, k -> s3.getObjectAsBytes(r -> r.bucket(b).key(k)).asByteArray()); }",
            ),
            f(
                "go",
                "// read it once, keep it for the run\nvar cache sync.Map\nif v, ok := cache.Load(k); ok { return v.([]byte), nil }",
            ),
            f(
                "js",
                "// read it once, keep it for the run\nconst cache = new Map();\nconst read = (k) => cache.get(k) ?? cache.set(k, fetchObject(k)).get(k);",
            ),
        ],
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
                "// list the prefix once; look keys up in the result\np := s3.NewListObjectsV2Paginator(s3c, &s3.ListObjectsV2Input{Bucket: &b, Prefix: &prefix})",
            ),
            f(
                "js",
                "// list the prefix once; look keys up in the result\nfor await (const page of paginateListObjectsV2({ client: s3 }, { Bucket: b, Prefix: p })) keys.push(...(page.Contents ?? []));",
            ),
        ],
        "obs.small" => vec![
            f(
                "python",
                "# one PUT under the threshold\nfrom boto3.s3.transfer import TransferConfig\ns3.upload_file(path, b, k, Config=TransferConfig(multipart_threshold=64 * 1024 * 1024))",
            ),
            f(
                "java",
                "// one PUT under the threshold\nS3TransferManager.builder().s3Client(S3AsyncClient.crtBuilder().minimumPartSizeInBytes(64L << 20).build()).build();",
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
        "obs.serial" => vec![
            f(
                "python",
                "# eight at once instead of one after another\nfrom concurrent.futures import ThreadPoolExecutor\nwith ThreadPoolExecutor(8) as pool:\n    bodies = list(pool.map(lambda k: s3.get_object(Bucket=b, Key=k)[\"Body\"].read(), keys))",
            ),
            f(
                "java",
                "// eight at once instead of one after another\nList<CompletableFuture<ResponseBytes<GetObjectResponse>>> all = keys.stream()\n    .map(k -> s3Async.getObject(r -> r.bucket(b).key(k), AsyncResponseTransformer.toBytes())).toList();",
            ),
            f(
                "go",
                "// eight at once instead of one after another\ng, ctx := errgroup.WithContext(ctx)\ng.SetLimit(8)\nfor _, k := range keys { k := k; g.Go(func() error { return get(ctx, k) }) }\nerr := g.Wait()",
            ),
            f(
                "js",
                "// eight at once instead of one after another\nconst limit = pLimit(8);\nconst bodies = await Promise.all(keys.map((k) => limit(() => get(k))));",
            ),
        ],
        "obs.throttled" | "obs.throttled.rate" => vec![
            f(
                "python",
                "# fewer at once, and the SDK's adaptive retry\nfrom botocore.config import Config\ns3 = boto3.client(\"s3\", config=Config(retries={\"mode\": \"adaptive\", \"max_attempts\": 10}, max_pool_connections=16))",
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
                "# spread keys over prefixes: a hash before the name\nimport hashlib\nkey = f\"{hashlib.md5(name.encode()).hexdigest()[:2]}/{name}\"",
            ),
            f(
                "java",
                "// spread keys over prefixes: a hash before the name\nString key = String.format(\"%02x/%s\", name.hashCode() & 0xff, name);",
            ),
            f(
                "go",
                "// spread keys over prefixes: a hash before the name\nkey := fmt.Sprintf(\"%02x/%s\", crc32.ChecksumIEEE([]byte(name))&0xff, name)",
            ),
            f(
                "js",
                "// spread keys over prefixes: a hash before the name\nconst key = `${createHash(\"md5\").update(name).digest(\"hex\").slice(0, 2)}/${name}`;",
            ),
        ],
        "obs.unfinished" => vec![f(
            "config",
            "<LifecycleConfiguration><Rule><ID>abort-incomplete-mpu</ID><Filter><Prefix></Prefix></Filter><Status>Enabled</Status>\n  <AbortIncompleteMultipartUpload><DaysAfterInitiation>7</DaysAfterInitiation></AbortIncompleteMultipartUpload>\n</Rule></LifecycleConfiguration>",
        )],
        "obs.retried" => vec![
            f(
                "python",
                "# a longer read timeout: retries resend the whole request\nfrom botocore.config import Config\ns3 = boto3.client(\"s3\", config=Config(read_timeout=60, connect_timeout=5))",
            ),
            f(
                "java",
                "// a longer read timeout: retries resend the whole request\nS3Client.builder().httpClientBuilder(ApacheHttpClient.builder().socketTimeout(Duration.ofSeconds(60))).build();",
            ),
            f(
                "go",
                "// a longer read timeout: retries resend the whole request\ns3.NewFromConfig(cfg, func(o *s3.Options) { o.HTTPClient = &http.Client{Timeout: 60 * time.Second} })",
            ),
            f(
                "js",
                "// a longer read timeout: retries resend the whole request\nnew S3Client({ requestHandler: new NodeHttpHandler({ requestTimeout: 60_000 }) })",
            ),
        ],
        "obs.slow" => vec![
            f(
                "python",
                "# large objects in ranges at once, not one long GET\nfrom boto3.s3.transfer import TransferConfig\ns3.download_file(b, k, path, Config=TransferConfig(multipart_chunksize=8 * 2**20, max_concurrency=8))",
            ),
            f(
                "java",
                "// large objects in ranges at once, not one long GET\nS3TransferManager tm = S3TransferManager.builder().s3Client(S3AsyncClient.crtCreate()).build();\ntm.downloadFile(d -> d.getObjectRequest(r -> r.bucket(b).key(k)).destination(path)).completionFuture().join();",
            ),
            f(
                "go",
                "// large objects in ranges at once, not one long GET\nd := manager.NewDownloader(s3c, func(d *manager.Downloader) { d.PartSize = 8 << 20; d.Concurrency = 8 })\n_, err := d.Download(ctx, file, &s3.GetObjectInput{Bucket: &b, Key: &k})",
            ),
            f(
                "js",
                "// large objects in ranges at once, not one long GET\nconst part = (i) => s3.send(new GetObjectCommand({ Bucket: b, Key: k, Range: `bytes=${i * size}-${(i + 1) * size - 1}` }));\nconst parts = await Promise.all([...Array(n).keys()].map(part));",
            ),
        ],
        "obs.races" => vec![
            f(
                "python",
                "# the write lost a race: read what won, never overwrite it blindly\ntry:\n    s3.put_object(Bucket=b, Key=k, Body=body, IfNoneMatch=\"*\")\nexcept s3.exceptions.ClientError as e:\n    if e.response[\"Error\"][\"Code\"] != \"PreconditionFailed\":\n        raise\n    won = s3.get_object(Bucket=b, Key=k)",
            ),
            f(
                "java",
                "// the write lost a race: read what won, never overwrite it blindly\ntry { s3.putObject(r -> r.bucket(b).key(k).ifNoneMatch(\"*\"), body); }\ncatch (S3Exception e) { if (e.statusCode() != 412) throw e; won = s3.getObject(r -> r.bucket(b).key(k)); }",
            ),
            f(
                "go",
                "// the write lost a race: read what won, never overwrite it blindly\n_, err := s3c.PutObject(ctx, &s3.PutObjectInput{Bucket: &b, Key: &k, Body: body, IfNoneMatch: aws.String(\"*\")})\nvar re *awshttp.ResponseError\nif errors.As(err, &re) && re.HTTPStatusCode() == 412 { /* read what won */ }",
            ),
            f(
                "js",
                "// the write lost a race: read what won, never overwrite it blindly\ntry { await s3.send(new PutObjectCommand({ Bucket: b, Key: k, Body: body, IfNoneMatch: \"*\" })); }\ncatch (e) { if (e.$metadata?.httpStatusCode !== 412) throw e; /* read what won */ }",
            ),
        ],
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
        _ => Vec::new(),
    }
}

/// A session after acting on every finding that counts a saving: an estimate.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatIf {
    /// Before and after.
    pub requests: (usize, usize),
    pub waiting_ms: (f64, f64),
    pub cost: (f64, f64),
    pub currency: String,
    /// The findings it counts.
    pub findings: usize,
}

/// What acting on the findings would leave: their saved requests (at most
/// nine in ten of the session's: findings can overlap), the waiting those
/// requests took on average, and the time a finding counts itself.
pub fn what_if(records: &[Record], profile: &Profile, findings: &[Observation]) -> Option<WhatIf> {
    let counted: Vec<&Observation> = findings
        .iter()
        .filter(|o| o.saves_requests > 0 || o.saves_ms > 0.0)
        .collect();
    if counted.is_empty() || records.is_empty() {
        return None;
    }
    let total = records.len();
    let saved = counted
        .iter()
        .map(|o| o.saves_requests)
        .sum::<usize>()
        .min(total * 9 / 10);
    let share = saved as f64 / total as f64;
    let waiting_saved = (profile.busy_ms * share + counted.iter().map(|o| o.saves_ms).sum::<f64>())
        .min(profile.busy_ms * 0.95);
    Some(WhatIf {
        requests: (total, total - saved),
        waiting_ms: (profile.busy_ms, profile.busy_ms - waiting_saved),
        cost: (profile.cost.amount, profile.cost.amount * (1.0 - share)),
        currency: profile.cost.currency.clone(),
        findings: counted.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::episode::{episodes, observations};
    use crate::profile::{RequestPrices, profile};
    use chrono::DateTime;

    #[test]
    fn acting_on_a_hot_key_leaves_one_read_and_shows_how() {
        let rs: Vec<Record> = (0..20u64)
            .map(|i| Record {
                id: i + 1,
                time: DateTime::from_timestamp_millis(1_700_000_000_000 + i as i64 * 50).unwrap(),
                method: "GET".into(),
                operation: "GetObject".into(),
                bucket: Some("b".into()),
                key: Some(if i < 15 {
                    "conf.json".into()
                } else {
                    format!("k{i}")
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
        let w = what_if(&rs, &p, &obs).unwrap();
        assert_eq!(w.requests, (20, 6));
        assert!(w.waiting_ms.1 < w.waiting_ms.0 && w.cost.1 < w.cost.0);
    }
}
