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
        "obs.rename" => vec![f(
            "config",
            "# Spark: the S3A magic committer (Hadoop 3.3+), no rename\nspark.hadoop.fs.s3a.committer.name=magic\nspark.sql.sources.commitProtocolClass=org.apache.spark.internal.io.cloud.PathOutputCommitProtocol\nspark.sql.parquet.output.committer.class=org.apache.spark.internal.io.cloud.BindingParquetOutputCommitter",
        )],
        "obs.markers" => vec![f(
            "config",
            "# keep markers: no delete after each file written beneath one (Hadoop 3.3.1+)\nspark.hadoop.fs.s3a.directory.marker.retention=keep",
        )],
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

/// The language of an SDK as [`crate::callers::sdk`] names it (`boto3
/// 1.44`): what picks the fix shown for it.
pub fn language(sdk: &str) -> Option<&'static str> {
    let name = sdk.split_whitespace().next().unwrap_or("");
    Some(match name {
        "boto3" | "botocore" | "aws-cli" | "s3fs" => "python",
        "aws-sdk-java" | "hadoop-aws" | "hadoop" => "java",
        "aws-sdk-go" | "aws-sdk-go-v2" => "go",
        "aws-sdk-js" => "js",
        _ => return None,
    })
}

/// Of a finding's fixes, the one for the SDK that sent its requests: a
/// store's or an engine's setting whatever the SDK; none for an SDK sonde
/// has no fix in the language of.
pub fn for_sdk<'a>(fixes: &'a [Fix], sdk: Option<&str>) -> Option<&'a Fix> {
    let lang = sdk.and_then(language);
    fixes
        .iter()
        .find(|f| f.sdk == "config")
        .or_else(|| fixes.iter().find(|f| Some(f.sdk) == lang))
}

/// A session after acting on the findings that name what they save.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatIf {
    /// Before and after.
    pub requests: (usize, usize),
    pub waiting_ms: (f64, f64),
    pub cost: (f64, f64),
    pub currency: String,
    /// The findings it counts, and how many findings there are (all that are
    /// listed with it): only those that name what they save are counted.
    pub findings: usize,
    pub of_findings: usize,
    /// The kinds of the findings counted (`obs.hot`), as they are named.
    pub counted: Vec<String>,
}

/// What acting on the findings would leave, measured and not estimated: the
/// requests the findings name are taken out of the session, and the session
/// without them is reckoned again. The waiting is the time with a request
/// in flight among those left (a request that ran beside another saves
/// nothing by going), the cost is the price of those left. A finding that
/// saves time without removing a request (requests sent at once) takes its
/// own figure off, when none of its requests is among those removed. A
/// finding that does not name its requests is not counted.
pub fn what_if(records: &[Record], profile: &Profile, findings: &[Observation]) -> Option<WhatIf> {
    let saving: Vec<&Observation> = findings
        .iter()
        .filter(|o| o.saves_requests > 0 || o.saves_ms > 0.0)
        .collect();
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
    // never the whole session: a request each finding keeps is among the ids
    // it does not name. Those left are added up where they are, not copied
    let left = || records.iter().filter(|r| !gone.contains(&r.id));
    let mut waiting = crate::profile::busy_ms_of(left());
    for o in saving.iter().filter(|o| o.saves_requests == 0) {
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
    if counted.is_empty() {
        return None;
    }
    let cost: f64 = left()
        .map(|r| crate::profile::price(r, &profile.prices))
        .sum();
    Some(WhatIf {
        requests: (records.len(), left().count()),
        waiting_ms: (profile.busy_ms, waiting.clamp(0.0, profile.busy_ms)),
        cost: (profile.cost.amount, cost.min(profile.cost.amount)),
        currency: profile.cost.currency.clone(),
        findings: counted.len(),
        of_findings: findings.len(),
        counted: counted
            .iter()
            .filter_map(|o| o.phrases.first().map(|p| p.key.to_string()))
            .collect(),
    })
}

impl WhatIf {
    /// The line `sonde show` closes its findings with.
    pub fn line(&self) -> String {
        use crate::program::money_text;
        use crate::signals::{count_text, ms_text};
        let which = if self.findings < self.of_findings {
            // which ones, when not all: the reader cannot guess
            let mut kinds: Vec<&str> = self.counted.iter().map(|k| kind_name(k)).collect();
            kinds.dedup();
            format!(
                "{} of {} findings ({})",
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
        let mut parts = Vec::new();
        if self.requests.0 != self.requests.1 {
            parts.push(format!(
                "{} → {} requests",
                count_text(self.requests.0),
                count_text(self.requests.1)
            ));
        }
        if ms_text(self.waiting_ms.0) != ms_text(self.waiting_ms.1) {
            parts.push(format!(
                "waiting {} → {}",
                ms_text(self.waiting_ms.0),
                ms_text(self.waiting_ms.1)
            ));
        }
        let (before, after) = (
            money_text(self.cost.0, &self.currency),
            money_text(self.cost.1, &self.currency),
        );
        if before != after {
            parts.push(format!("cost {before} → {after}"));
        }
        format!("acting on {which}: {}", parts.join(" · "))
    }
}

/// A finding's kind in a few words, for naming which were counted.
fn kind_name(key: &str) -> &'static str {
    match key {
        "obs.hot" => "a key read again and again",
        "obs.rename" => "a commit by rename",
        "obs.markers" => "directory markers deleted",
        "obs.listed" => "a prefix listed again and again",
        "obs.headget" => "HEAD before GET",
        "obs.small" => "small files uploaded in parts",
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
        // an SDK with no fix in its language, or none detected: none shown
        assert_eq!(pick(Some("object_store 0.14")), None);
        assert_eq!(pick(None), None);
        // an engine's setting, whatever sent it
        assert_eq!(
            for_sdk(&for_key("obs.rename"), Some("hadoop 3.3")).map(|f| f.sdk),
            Some("config")
        );
    }

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
        let w = what_if(&rs, &p, &obs).unwrap();
        assert_eq!(w.requests, (20, 6));
        assert!(w.waiting_ms.1 < w.waiting_ms.0 && w.cost.1 < w.cost.0);
        // 20 reads of 10 ms each, 50 ms apart: the 14 gone took 140 ms
        assert_eq!(w.waiting_ms, (200.0, 60.0));
    }

    #[test]
    fn three_quick_heads_gone_take_their_own_time_and_price_and_no_more() {
        let prices = RequestPrices {
            per_1000_writes: 5.0,
            per_1000_reads: 0.4,
            per_1000_lists: None,
            currency: "USD".into(),
        };
        let mut rs = Vec::new();
        for i in 0..3u64 {
            let at = 1_700_000_000_000 + i as i64 * 400;
            let r = |id, at, method: &str, op: &str, ms| Record {
                id,
                time: DateTime::from_timestamp_millis(at).unwrap(),
                method: method.into(),
                operation: op.into(),
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
        let w = what_if(&rs, &p, &obs).unwrap();
        assert_eq!(w.requests, (9, 6));
        // 195 ms of waiting, of which the three HEADs were 6: not a third of it
        assert_eq!(w.waiting_ms, (195.0, 189.0));
        // three reads' price off: 3 × 0.0004 of 0.0174
        assert!((w.cost.0 - 0.0174).abs() < 1e-9, "{:?}", w.cost);
        assert!((w.cost.1 - 0.0162).abs() < 1e-9, "{:?}", w.cost);
        // and it is the same on every reading
        assert_eq!(what_if(&rs, &p, &obs), Some(w.clone()));
        assert_eq!(
            w.line(),
            "acting on 1 finding: 9 → 6 requests · waiting 195 ms → 189 ms",
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
                operation: "GetObject".into(),
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
        let w = what_if(&rs, &p, &obs).unwrap();
        assert_eq!((w.findings, w.of_findings), (1, 2));
        assert_eq!(w.requests, (20, 1));
        assert!(
            w.line().starts_with(
                "acting on 1 of 2 findings (a key read again and again): 20 → 1 requests"
            ),
            "{}",
            w.line()
        );
        // nothing that can be counted: nothing is said
        assert_eq!(what_if(&rs, &p, &[vague]), None);
    }
}
