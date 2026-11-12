//! The JVM engines' own way of being pointed at an endpoint. Hadoop's s3a
//! (Spark, Flink, Hive) reads its endpoint from its configuration, not from
//! AWS_ENDPOINT_URL, and a `-Dfs.s3a.*` system property is not its
//! configuration: the engine's command line is. `sonde run` writes it there
//! for the engines it knows by name, before the program's own arguments, so
//! that what the program says itself still has the last word.
//!
//! No key goes on a command line (every user of the machine can read one):
//! the credentials provider is told to read the environment, where sonde
//! has put them.

/// An engine whose command line takes Hadoop's configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// spark-submit, spark-shell, pyspark, spark-sql, sparkR: `--conf spark.hadoop.*`.
    Spark,
    /// `flink run` and `flink run-application`: `-D<key>=<value>`.
    Flink,
}

/// The provider that reads AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY.
/// Hadoop 3.4 (AWS SDK v2) maps the v1 name to its own.
const ENV_PROVIDER: &str = "com.amazonaws.auth.EnvironmentVariableCredentialsProvider";

/// The engine a program is, by its file's name.
pub fn engine(program: &str) -> Option<Engine> {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let name = name
        .strip_suffix(".cmd")
        .or_else(|| name.strip_suffix(".sh"))
        .unwrap_or(name);
    match name {
        "spark-submit" | "spark-shell" | "pyspark" | "spark-sql" | "sparkR" => Some(Engine::Spark),
        "flink" => Some(Engine::Flink),
        _ => None,
    }
}

/// Hadoop's s3a settings for an endpoint: path style (a local endpoint has
/// no bucket names), its scheme said twice (Hadoop 3.3 goes by the flag,
/// 3.4 by the URL), and the keys from the environment.
pub fn s3a_settings(endpoint: &str, region: &str) -> Vec<(String, String)> {
    let tls = endpoint.starts_with("https://");
    [
        ("fs.s3a.endpoint", endpoint.to_string()),
        ("fs.s3a.endpoint.region", region.to_string()),
        ("fs.s3a.path.style.access", "true".to_string()),
        ("fs.s3a.connection.ssl.enabled", tls.to_string()),
        ("fs.s3a.aws.credentials.provider", ENV_PROVIDER.to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// Spark's `--conf` arguments for an endpoint.
pub fn spark_confs(endpoint: &str, region: &str) -> Vec<String> {
    s3a_settings(endpoint, region)
        .into_iter()
        .flat_map(|(k, v)| ["--conf".to_string(), format!("spark.hadoop.{k}={v}")])
        .collect()
}

/// Flink's `-D` arguments: its `s3.` keys are passed on to s3a as
/// `fs.s3a.` by flink-s3-fs-hadoop, and read as they are by flink-s3-fs-presto.
pub fn flink_options(endpoint: &str, region: &str) -> Vec<String> {
    s3a_settings(endpoint, region)
        .into_iter()
        .map(|(k, v)| format!("-Ds3.{}={v}", k.trim_start_matches("fs.s3a.")))
        .collect()
}

/// The command with the engine's settings for `endpoint` written in, and
/// the engine, when its program is one sonde knows. A Flink command other
/// than `run` (`list`, `cancel`) is left as it is.
pub fn with_settings(
    command: &[String],
    endpoint: &str,
    region: &str,
) -> Option<(Engine, Vec<String>)> {
    let (program, rest) = command.split_first()?;
    let e = engine(program)?;
    let mut out = vec![program.clone()];
    match e {
        Engine::Spark => {
            out.extend(spark_confs(endpoint, region));
            out.extend(rest.iter().cloned());
        }
        Engine::Flink => {
            let (action, rest) = rest.split_first()?;
            if action != "run" && action != "run-application" {
                return None;
            }
            out.push(action.clone());
            out.extend(flink_options(endpoint, region));
            out.extend(rest.iter().cloned());
        }
    }
    Some((e, out))
}

/// The arguments of the program's own that say where s3a goes, which then
/// win over sonde's (`--conf spark.hadoop.fs.s3a.endpoint=…`,
/// `-Ds3.endpoint=…`, a bucket's own `fs.s3a.bucket.<b>.endpoint`).
pub fn own_endpoint(command: &[String]) -> Option<&str> {
    command.iter().skip(1).map(String::as_str).find(|a| {
        let setting = a.split_once('=').map_or("", |(k, _)| k);
        setting.ends_with("fs.s3a.endpoint")
            || setting.ends_with("-Ds3.endpoint")
            || (setting.contains("fs.s3a.bucket.") && setting.ends_with(".endpoint"))
    })
}

/// PYSPARK_SUBMIT_ARGS for a Python program that starts Spark itself
/// (`SparkSession.builder…getOrCreate()`): sonde's settings, then what the
/// variable said already, which ends with the shell it starts.
pub fn pyspark_submit_args(existing: Option<&str>, endpoint: &str, region: &str) -> String {
    let ours = spark_confs(endpoint, region).join(" ");
    match existing.map(str::trim).filter(|e| !e.is_empty()) {
        Some(e) => format!("{ours} {e}"),
        None => format!("{ours} pyspark-shell"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn engines_are_known_by_their_files_name() {
        assert_eq!(engine("/opt/spark/bin/spark-submit"), Some(Engine::Spark));
        assert_eq!(engine("pyspark"), Some(Engine::Spark));
        assert_eq!(engine("spark-sql"), Some(Engine::Spark));
        assert_eq!(
            engine("C:\\spark\\bin\\spark-shell.cmd"),
            Some(Engine::Spark)
        );
        assert_eq!(engine("./bin/flink"), Some(Engine::Flink));
        assert_eq!(engine("python"), None);
        assert_eq!(engine("trino"), None);
    }

    #[test]
    fn spark_is_given_its_confs_before_the_programs_own_arguments() {
        let cmd = words("spark-submit --master local[2] job.py s3a://b/out");
        let (e, argv) = with_settings(&cmd, "http://127.0.0.1:9000", "eu-west-1").unwrap();
        assert_eq!(e, Engine::Spark);
        assert_eq!(
            argv,
            words(
                "spark-submit \
                 --conf spark.hadoop.fs.s3a.endpoint=http://127.0.0.1:9000 \
                 --conf spark.hadoop.fs.s3a.endpoint.region=eu-west-1 \
                 --conf spark.hadoop.fs.s3a.path.style.access=true \
                 --conf spark.hadoop.fs.s3a.connection.ssl.enabled=false \
                 --conf spark.hadoop.fs.s3a.aws.credentials.provider=com.amazonaws.auth.EnvironmentVariableCredentialsProvider \
                 --master local[2] job.py s3a://b/out"
            )
        );
        // no key or secret is ever on the command line
        assert!(
            !argv
                .iter()
                .any(|a| a.contains("secret") || a.contains("access.key"))
        );
    }

    #[test]
    fn flink_is_given_its_options_after_run_only() {
        let cmd = words("flink run -c Job job.jar");
        let (e, argv) = with_settings(&cmd, "https://127.0.0.1:9000", "us-east-1").unwrap();
        assert_eq!(e, Engine::Flink);
        assert_eq!(
            &argv[..3],
            &words("flink run -Ds3.endpoint=https://127.0.0.1:9000")[..]
        );
        assert!(argv.contains(&"-Ds3.path.style.access=true".to_string()));
        assert!(argv.contains(&"-Ds3.connection.ssl.enabled=true".to_string()));
        assert_eq!(&argv[argv.len() - 3..], &words("-c Job job.jar")[..]);
        assert!(with_settings(&words("flink list"), "http://x", "r").is_none());
        assert!(with_settings(&words("flink"), "http://x", "r").is_none());
        assert!(with_settings(&words("python x.py"), "http://x", "r").is_none());
    }

    #[test]
    fn an_endpoint_of_the_programs_own_is_seen() {
        let own = words("spark-submit --conf spark.hadoop.fs.s3a.endpoint=http://minio:9000 x.py");
        assert!(own_endpoint(&own).is_some());
        let bucket = words("spark-submit --conf spark.hadoop.fs.s3a.bucket.b.endpoint=x x.py");
        assert!(own_endpoint(&bucket).is_some());
        assert!(own_endpoint(&words("flink run -Ds3.endpoint=x job.jar")).is_some());
        assert!(own_endpoint(&words("spark-submit --conf spark.ui.port=0 x.py")).is_none());
    }

    #[test]
    fn pyspark_submit_args_keep_what_was_there() {
        let fresh = pyspark_submit_args(None, "http://127.0.0.1:1", "r");
        assert!(fresh.starts_with("--conf spark.hadoop.fs.s3a.endpoint=http://127.0.0.1:1 "));
        assert!(fresh.ends_with(" pyspark-shell"));
        let kept = pyspark_submit_args(Some("--packages x pyspark-shell"), "http://h", "r");
        assert!(kept.ends_with(" --packages x pyspark-shell"));
        assert_eq!(kept.matches("pyspark-shell").count(), 1);
    }
}
