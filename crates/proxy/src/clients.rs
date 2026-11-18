//! Clients that do not read `AWS_ENDPOINT_URL`, pointed at the recording
//! by their own means, as [`crate::jvm`] points the JVM engines.
//!
//! - rclone reads a backend's options from `RCLONE_S3_*`, and they win over
//!   a remote's config file (a remote's own connection string still wins):
//!   every S3 remote, named or `:s3:`, goes through sonde.
//! - The DuckDB CLI reads no variable for its endpoint (its `s3_*` settings
//!   come from SQL; it takes the keys and region from `AWS_*` itself):
//!   `sonde run -- duckdb …` is given an init file (`-init`) of `SET s3_*`
//!   statements, after what `~/.duckdbrc` says (which `-init` replaces),
//!   unless the command names its own.

/// rclone's S3 backend options for the recording: endpoint, keys, region
/// and path-style addressing (a local endpoint has no bucket names).
pub fn rclone_env(url: &str, region: &str, key: &str, secret: &str) -> Vec<(String, String)> {
    [
        ("RCLONE_S3_ENDPOINT", url),
        ("RCLONE_S3_ACCESS_KEY_ID", key),
        ("RCLONE_S3_SECRET_ACCESS_KEY", secret),
        ("RCLONE_S3_REGION", region),
        ("RCLONE_S3_FORCE_PATH_STYLE", "true"),
        // the keys above, not the machine's or the instance's
        ("RCLONE_S3_ENV_AUTH", "false"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// Whether a command's program is the DuckDB CLI.
pub fn is_duckdb(program: &str) -> bool {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    name == "duckdb"
}

/// The SQL a DuckDB CLI starts with: `rc` (what `~/.duckdbrc` says, which
/// `-init` stands in for), then sonde's `s3_*` settings for `endpoint`
/// (plain HTTP: `http://host:port`). No key: DuckDB reads
/// `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY` and `AWS_REGION` itself.
pub fn duckdb_init(endpoint: &str, rc: Option<&str>) -> String {
    let host = endpoint
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');
    let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let mut sql = String::new();
    if let Some(rc) = rc.filter(|r| !r.trim().is_empty()) {
        sql.push_str(rc.trim_end());
        sql.push('\n');
    }
    sql.push_str("-- sonde run: S3 goes through the recording\n");
    for (k, v) in [
        ("s3_endpoint", quote(host)),
        ("s3_url_style", quote("path")),
        ("s3_use_ssl", "false".into()),
    ] {
        sql.push_str(&format!("SET {k}={v};\n"));
    }
    sql
}

/// The DuckDB CLI's command with `-init <init>` before its own arguments;
/// none for another program, or a command that names its own init file.
pub fn duckdb_with_init(command: &[String], init: &str) -> Option<Vec<String>> {
    let (program, rest) = command.split_first()?;
    if !is_duckdb(program) || rest.iter().any(|a| a == "-init") {
        return None;
    }
    let mut out = vec![program.clone(), "-init".into(), init.into()];
    out.extend(rest.iter().cloned());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn rclone_is_given_the_endpoint_and_keys_for_every_s3_remote() {
        let env = rclone_env("http://127.0.0.1:9000", "eu-west-1", "SONDEK", "s3cr3t");
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("RCLONE_S3_ENDPOINT"), Some("http://127.0.0.1:9000"));
        assert_eq!(get("RCLONE_S3_ACCESS_KEY_ID"), Some("SONDEK"));
        assert_eq!(get("RCLONE_S3_SECRET_ACCESS_KEY"), Some("s3cr3t"));
        assert_eq!(get("RCLONE_S3_REGION"), Some("eu-west-1"));
        assert_eq!(get("RCLONE_S3_FORCE_PATH_STYLE"), Some("true"));
        assert_eq!(get("RCLONE_S3_ENV_AUTH"), Some("false"));
    }

    #[test]
    fn duckdb_is_known_by_its_files_name() {
        assert!(is_duckdb("duckdb"));
        assert!(is_duckdb("/usr/local/bin/duckdb"));
        assert!(is_duckdb("C:\\tools\\duckdb.exe"));
        assert!(!is_duckdb("python3"));
        assert!(!is_duckdb("duckdb-wasm"));
    }

    #[test]
    fn duckdb_starts_with_sondes_settings_after_its_own_rc() {
        let sql = duckdb_init("http://127.0.0.1:9000", Some("SET threads=2;\n"));
        assert!(sql.starts_with("SET threads=2;\n"), "{sql}");
        assert!(sql.contains("SET s3_endpoint='127.0.0.1:9000';"), "{sql}");
        assert!(sql.contains("SET s3_url_style='path';"));
        assert!(sql.contains("SET s3_use_ssl=false;"));
        // no key on file: DuckDB reads the environment's
        assert!(!sql.contains("key"), "{sql}");
        // the settings come after the rc, so they win
        assert!(sql.find("threads").unwrap() < sql.find("s3_endpoint").unwrap());
        assert!(duckdb_init("http://h:1", None).starts_with("--"));
        // quoted as SQL quotes
        assert!(duckdb_init("http://it's:1", None).contains("'it''s:1'"));
    }

    #[test]
    fn duckdb_is_given_its_init_file_unless_it_names_one() {
        assert_eq!(
            duckdb_with_init(&words("duckdb db.duckdb -c select"), "/x/init.sql").unwrap(),
            words("duckdb -init /x/init.sql db.duckdb -c select")
        );
        assert!(duckdb_with_init(&words("duckdb -init mine.sql"), "/x").is_none());
        assert!(duckdb_with_init(&words("python3 q.py"), "/x").is_none());
        assert!(duckdb_with_init(&[], "/x").is_none());
    }
}
