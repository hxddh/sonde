//! A run's money said a month: an imported log's at the rate of its own
//! span, a program's run times `sonde.toml`'s `[program] runs-per-month`.
//! Neither: none, and money is said for the run alone.

use serde::Serialize;

use crate::run::RunHeader;

/// A month, in seconds (30 days).
pub const MONTH_SECONDS: f64 = 30.0 * 86_400.0;

/// The shortest log projected to a month: a minute.
const SHORTEST_SPAN: f64 = 60.0;

/// What a month of this run is: `times` this run (a log's span, or
/// `[program] runs-per-month`), and what it costs.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Month {
    /// How many of this run a month is.
    pub times: f64,
    /// `log` (an imported log, at the rate of its span) or `runs`
    /// (`[program] runs-per-month`).
    pub from: &'static str,
    /// A month's cost at that rate.
    pub cost: f64,
    pub currency: String,
}

impl Month {
    /// `amount` (of this run) a month.
    pub fn of(&self, amount: f64) -> f64 {
        amount * self.times
    }
}

/// A month of the run `header` names, `seconds` long and costing `cost`.
pub fn month(header: &RunHeader, seconds: f64, cost: f64, currency: &str) -> Option<Month> {
    let (times, from) =
        if header.command.as_deref().is_some_and(|c| {
            c.starts_with("imported from ") || c.starts_with(crate::program::LOGS_OF)
        }) {
            (seconds >= SHORTEST_SPAN).then(|| (MONTH_SECONDS / seconds, "log"))?
        } else if header.is_recording() {
            (runs_per_month(header)?, "runs")
        } else {
            return None;
        };
    Some(Month {
        times,
        from,
        cost: cost * times,
        currency: currency.to_string(),
    })
}

/// `[program] runs-per-month` of the `sonde.toml` where the run's program
/// ran, when it is for this program (`runs` names its command, or no
/// `runs` is given).
fn runs_per_month(header: &RunHeader) -> Option<f64> {
    let cwd = crate::code::local_dir(std::path::Path::new(header.cwd.as_deref()?))?;
    let project = crate::project::of(&cwd).ok()??;
    let program = &project.file.program;
    let n = program.runs_per_month?;
    let mine = match program.runs.as_deref() {
        None => true,
        Some(runs) => {
            crate::program::program_key(header.command.as_deref()).as_deref() == Some(runs.trim())
        }
    };
    mine.then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(command: &str, cwd: Option<&std::path::Path>) -> RunHeader {
        let mut h: RunHeader = serde_json::from_value(serde_json::json!({
            "format": "sonde", "version": 1, "created": "2026-11-14T10:00:00Z",
            "endpoint": "aws", "url": "https://s3.amazonaws.com", "region": "us-east-1",
            "command": command,
        }))
        .unwrap();
        h.cwd = cwd.map(|d| d.to_string_lossy().into_owned());
        h
    }

    #[test]
    fn an_import_is_a_month_at_the_rate_of_its_span() {
        let h = header("imported from access.log", None);
        let m = month(&h, 3600.0, 0.05, "USD").unwrap();
        assert_eq!((m.times, m.from), (720.0, "log"));
        assert!((m.cost - 36.0).abs() < 1e-9);
        // a log of seconds says no month
        assert!(month(&h, 10.0, 0.05, "USD").is_none());
    }

    #[test]
    fn a_run_is_a_month_by_runs_per_month_and_none_without_it() {
        let dir = tempfile::tempdir().unwrap();
        let h = header("python3 etl.py", Some(dir.path()));
        assert!(month(&h, 2.0, 0.01, "USD").is_none());
        std::fs::write(
            dir.path().join("sonde.toml"),
            "[program]\nruns-per-month = 200\n",
        )
        .unwrap();
        let m = month(&h, 2.0, 0.01, "USD").unwrap();
        assert_eq!((m.times, m.from), (200.0, "runs"));
        // for another program's command, when `runs` names one
        std::fs::write(
            dir.path().join("sonde.toml"),
            "[program]\nname = \"etl\"\nruns = \"etl\"\nruns-per-month = 30.5\n",
        )
        .unwrap();
        assert_eq!(month(&h, 2.0, 0.01, "USD").unwrap().times, 30.5);
        let other = header("python3 load.py", Some(dir.path()));
        assert!(month(&other, 2.0, 0.01, "USD").is_none());
    }
}
