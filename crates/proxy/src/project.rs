//! A project's `sonde.toml`: its budget (`[check]`), its program's name
//! (`[program]`), how it is recorded (`[record]`) and its fault scenarios
//! (`[[scenario]]`). One reading for the CLI and the app, strict: a key
//! sonde does not know is an error in both, never passed over in one.
//! docs/configuration.md names every key.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::Policy;
use crate::fault::Scenario;
use crate::program::Budget;

/// The `[check]` table: a run's budget, kept with the code it is about
/// (`max-waiting` and `max-p95` in ms, or "2s"; `baseline` a session,
/// `auto`, or an https URL of one).
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Check {
    pub max_requests: Option<usize>,
    pub max_waiting: Option<toml::Value>,
    pub max_cost: Option<f64>,
    pub max_errors: Option<usize>,
    pub max_p95: Option<toml::Value>,
    /// Findings kept on purpose: never a new warning.
    #[serde(default)]
    pub expect: Vec<String>,
    pub tolerance: Option<f64>,
    #[serde(default)]
    pub no_new_warnings: bool,
    pub baseline: Option<String>,
}

impl Check {
    /// `max-waiting` in ms.
    pub fn max_waiting_ms(&self) -> Result<Option<f64>, String> {
        self.max_waiting
            .as_ref()
            .map(|v| duration_ms("max-waiting", v))
            .transpose()
    }

    /// `max-p95` in ms.
    pub fn max_p95_ms(&self) -> Result<Option<f64>, String> {
        self.max_p95
            .as_ref()
            .map(|v| duration_ms("max-p95", v))
            .transpose()
    }

    /// The limits a run is held to alone (no baseline: no tolerance).
    pub fn budget(&self) -> Result<Budget, String> {
        Ok(Budget {
            max_requests: self.max_requests,
            max_waiting_ms: self.max_waiting_ms()?,
            max_cost: self.max_cost,
            max_errors: self.max_errors,
            max_p95_ms: self.max_p95_ms()?,
            expect: self.expect.clone(),
            tolerance: None,
            no_new_warnings: false,
        })
    }
}

/// The `[program]` table: the name its runs and its baseline go by.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Program {
    pub name: Option<String>,
}

/// The `[record]` table: what a recording of the project lets through
/// (`allow`, `no-delete`, `read-only`).
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Record {
    pub policy: Option<String>,
}

/// A fault scenario of the project's own, a `[[scenario]]`:
/// `fault = "503 SlowDown"` (or `500`, `lost-answer`, `cut-body`,
/// `lost-race`, `delay 200ms`), `on` the operations it is for, `prefix` the
/// keys, `times` the attempts at each request that meet it (1 unless said;
/// 0 is every attempt).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ScenarioTable {
    pub name: String,
    pub fault: String,
    #[serde(default)]
    pub on: Vec<String>,
    #[serde(default)]
    pub prefix: String,
    pub times: Option<u32>,
}

impl ScenarioTable {
    pub fn scenario(&self) -> Result<Scenario, String> {
        crate::fault::project_scenario(&self.name, &self.fault, &self.on, &self.prefix, self.times)
    }
}

/// `sonde.toml` as written.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct File {
    #[serde(default)]
    pub check: Check,
    #[serde(default)]
    pub program: Program,
    #[serde(default)]
    pub record: Record,
    #[serde(default)]
    pub scenario: Vec<ScenarioTable>,
}

/// `sonde.toml`, read and checked.
#[derive(Debug, Default, Clone)]
pub struct Project {
    pub path: Option<PathBuf>,
    pub file: File,
    pub scenarios: Vec<Scenario>,
    /// `[record] policy`, when it says one.
    pub policy: Option<Policy>,
}

/// `sonde.toml` in `from` or a folder above it, to the repository's root.
pub fn find(from: &Path) -> Option<PathBuf> {
    let mut dir = from.to_path_buf();
    loop {
        let f = dir.join("sonde.toml");
        if f.is_file() {
            return Some(f);
        }
        if dir.join(".git").exists() || !dir.pop() {
            return None;
        }
    }
}

/// Reads and checks `sonde.toml`'s text: every key known, every scenario
/// one sonde can run, the program's name not empty, the policy one of three.
/// Durations are [`Check::budget`]'s to read: a run is not refused for its
/// budget, only `check` is.
pub fn parse(text: &str) -> Result<Project, String> {
    let file: File = toml::from_str(text).map_err(|e| {
        let hint = if text.contains("[[scenarios]]") {
            "\na scenario is a [[scenario]] table, one for each"
        } else {
            ""
        };
        format!("{}{hint}", e.to_string().trim_end())
    })?;
    crate::fault::project_program_name(file.program.name.as_deref())?;
    let scenarios =
        crate::fault::project_scenarios(file.scenario.iter().map(ScenarioTable::scenario))?;
    let policy = file
        .record
        .policy
        .as_deref()
        .map(|p| {
            Policy::named(p).ok_or_else(|| {
                format!("[record] policy = {p:?}: one of allow, no-delete, read-only")
            })
        })
        .transpose()?;
    Ok(Project {
        path: None,
        file,
        scenarios,
        policy,
    })
}

/// [`parse`] of the file at `path`; an error names the file.
pub fn read(path: &Path) -> Result<Project, String> {
    let said = |e: String| format!("{}: {e}", path.display());
    let text = std::fs::read_to_string(path).map_err(|e| said(e.to_string()))?;
    let mut p = parse(&text).map_err(said)?;
    p.path = Some(path.to_path_buf());
    Ok(p)
}

/// The project `from` is in: its `sonde.toml`, or none.
pub fn of(from: &Path) -> Result<Option<Project>, String> {
    find(from).map(|f| read(&f)).transpose()
}

/// "250ms", "2s", "1.5 s", or ms as a number.
pub fn duration_text(text: &str) -> Option<f64> {
    let t = text.trim();
    let (n, scale) = if let Some(n) = t.strip_suffix("ms") {
        (n, 1.0)
    } else if let Some(n) = t.strip_suffix('s') {
        (n, 1000.0)
    } else {
        (t, 1.0)
    };
    n.trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| v * scale)
}

/// A duration as `sonde.toml` writes it: ms as a number, or "250ms", "2s".
pub fn duration_ms(name: &str, v: &toml::Value) -> Result<f64, String> {
    match v {
        toml::Value::Integer(n) => Ok(*n as f64),
        toml::Value::Float(f) => Ok(*f),
        toml::Value::String(s) => {
            duration_text(s).ok_or_else(|| format!("{name}: {s:?} is not a duration"))
        }
        other => Err(format!("{name}: {other} is not a duration")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_reads_and_what_sonde_does_not_know_is_refused() {
        let p = parse(
            "[check]\nmax-requests = 10\nmax-waiting = \"2s\"\nmax-p95 = 250\n\n[program]\nname = \"etl\"\n\n[record]\npolicy = \"allow\"\n\n[[scenario]]\nname = \"flaky\"\nfault = \"503 SlowDown\"\non = [\"PutObject\"]\n",
        )
        .unwrap();
        assert_eq!(p.policy, Some(Policy::Allow));
        assert_eq!(p.file.program.name.as_deref(), Some("etl"));
        assert_eq!(p.scenarios.len(), 1);
        let b = p.file.check.budget().unwrap();
        assert_eq!(
            (b.max_requests, b.max_waiting_ms, b.max_p95_ms),
            (Some(10), Some(2000.0), Some(250.0))
        );
        for (text, says) in [
            ("[chek]\n", "chek"),
            ("[record]\npolicy = \"none\"\n", "one of allow"),
            ("[record]\nmode = \"allow\"\n", "mode"),
            ("[[scenarios]]\nname = \"x\"\n", "[[scenario]]"),
            (
                "[[scenario]]\nname = \"x\"\nfault = \"500\"\nwhen = 1\n",
                "when",
            ),
            ("[program]\nname = \"\"\n", "a program's name"),
        ] {
            let e = parse(text).unwrap_err();
            assert!(e.contains(says), "{text}: {e}");
        }
        // a budget's durations are check's to refuse, not a run's
        let p = parse("[check]\nmax-waiting = \"soon\"\n").unwrap();
        assert!(p.file.check.budget().unwrap_err().contains("soon"));
        assert_eq!(parse("").unwrap().policy, None);
    }

    #[test]
    fn found_from_a_folder_upwards_and_not_above_the_repository() {
        let t = tempfile::tempdir().unwrap();
        let sub = t.path().join("repo/etl");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(t.path().join("repo/.git")).unwrap();
        std::fs::write(t.path().join("sonde.toml"), "[[scenario]]\n").unwrap();
        assert!(of(&sub).unwrap().is_none());
        std::fs::write(
            t.path().join("repo/sonde.toml"),
            "[record]\npolicy = \"read-only\"\n",
        )
        .unwrap();
        let p = of(&sub).unwrap().unwrap();
        assert_eq!(p.policy, Some(Policy::ReadOnly));
        assert_eq!(p.path, Some(t.path().join("repo/sonde.toml")));
    }

    #[test]
    fn durations_as_sonde_toml_writes_them() {
        assert_eq!(duration_text("250ms"), Some(250.0));
        assert_eq!(duration_text("1.5 s"), Some(1500.0));
        assert_eq!(duration_text("40"), Some(40.0));
        assert_eq!(duration_text("-1"), None);
        assert_eq!(duration_text("soon"), None);
    }
}
