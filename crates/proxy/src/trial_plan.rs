//! What a trial of a recorded run would try, said before it runs: its
//! command and folder, the scenarios (the project's own when its
//! `sonde.toml` names some, else those chosen from what the run did), and
//! whether every run would write to the same place. The app plans a trial
//! of a run it shows with it.

use std::path::Path;

use crate::fault::Scenario;
use crate::session::Session;

/// A trial of a run, planned.
#[derive(Debug)]
pub struct Plan {
    /// The run's command, as written.
    pub command: String,
    /// The folder it ran in.
    pub cwd: Option<String>,
    pub scenarios: Vec<Scenario>,
    /// The scenarios are the project's own (`[[scenario]]` in its `sonde.toml`).
    pub project: bool,
    /// The command names no place of its own for each run
    /// ([`names_its_place`]): a trial writes where the clean run wrote.
    pub shares_place: bool,
}

/// Whether a command gives each run of a trial a place of its own, as far
/// as its words say: `{trial}` in it, or SONDE_TRIAL named.
pub fn names_its_place(command: &str) -> bool {
    command.contains("{trial}") || command.contains("SONDE_TRIAL")
}

/// The scenarios a project names: the `[[scenario]]` tables of the
/// `sonde.toml` found from `dir` upwards (to the repository's root), read as
/// `sonde run --trial` reads them ([`crate::project`]). None when there is no
/// such file.
pub fn project_scenarios(dir: &Path) -> Result<Vec<Scenario>, String> {
    Ok(crate::project::of(dir)?
        .map(|p| p.scenarios)
        .unwrap_or_default())
}

/// The plan of a trial of the run `s`; refused for a trial itself, a run of
/// no command, and a run that sent no requests.
pub fn of(s: &Session) -> Result<Plan, String> {
    if s.header.scenario.is_some() {
        return Err("this session is a trial itself: try the run it is a trial of".into());
    }
    let command = s
        .header
        .command
        .clone()
        .filter(|c| crate::program::program_key(Some(c)).is_some())
        .ok_or("only a run of a command can be tried under faults")?;
    if s.records.is_empty() {
        return Err("no trial: the run sent no requests to fail".into());
    }
    let cwd = s.header.cwd.clone();
    let project = match &cwd {
        Some(d) => project_scenarios(Path::new(d))?,
        None => Vec::new(),
    };
    let own = !project.is_empty();
    Ok(Plan {
        shares_place: !names_its_place(&command),
        command,
        cwd,
        scenarios: if own {
            project
        } else {
            crate::trial::choose(&s.records)
        },
        project: own,
    })
}

impl Plan {
    /// The scenarios asked for by name, from the plan's and the built-in
    /// ones (`slowdown:3`: pressed harder, see
    /// [`crate::fault::scenario_asked`]); all of the plan's when none is
    /// named.
    pub fn named(&self, names: &[String]) -> Result<Vec<Scenario>, String> {
        if names.is_empty() {
            return Ok(self.scenarios.clone());
        }
        names
            .iter()
            .map(|n| {
                if let Some(s) = self.scenarios.iter().find(|s| &s.name == n) {
                    return Ok(s.clone());
                }
                // a project's own scenario of the name says its times itself
                let name = n.split_once(':').map_or(n.as_str(), |(name, _)| name);
                if self.project && self.scenarios.iter().any(|s| s.name == name) {
                    return Err(format!(
                        "{n}: {name} is the project's own scenario (sonde.toml says its times)"
                    ));
                }
                crate::fault::scenario_asked(n)?.ok_or_else(|| format!("{n}: no such scenario"))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionHeader;

    fn run(command: &str, cwd: Option<&Path>) -> Session {
        let rec = |id: u64, method: &str, op: &str| reqlog::Record {
            id,
            time: chrono::Utc::now(),
            method: method.into(),
            operation: op.into(),
            bucket: Some("b".into()),
            key: Some(format!("k{id}").into()),
            status: Some(200),
            ..Default::default()
        };
        let mut header = SessionHeader::new("ep", "http://s3", "", Some(command.into()));
        if let Some(d) = cwd {
            header = header.ran_in(d);
        }
        Session {
            header,
            records: vec![rec(1, "PUT", "PutObject"), rec(2, "GET", "GetObject")],
            logs: Vec::new(),
        }
    }

    #[test]
    fn a_plan_is_the_scenarios_chosen_from_what_the_run_did() {
        let s = run("python job.py --out s3://b/x", None);
        let plan = of(&s).unwrap();
        let chosen: Vec<String> = crate::trial::choose(&s.records)
            .into_iter()
            .map(|x| x.name)
            .collect();
        let names: Vec<String> = plan.scenarios.iter().map(|x| x.name.clone()).collect();
        assert_eq!(names, chosen);
        assert!(names.contains(&"lost-answer".to_string()), "{names:?}");
        assert!(!plan.project);
        assert_eq!(plan.command, "python job.py --out s3://b/x");
        assert!(
            plan.shares_place,
            "no {{trial}}: every run writes one place"
        );
        // by name: of the plan's or built in; an unknown one is refused
        assert_eq!(plan.named(&["error".into()]).unwrap().len(), 1);
        assert_eq!(plan.named(&[]).unwrap().len(), names.len());
        assert!(plan.named(&["nope".into()]).is_err());
        // pressed harder: the two that fail a request's first attempt, by a count
        let hard = plan.named(&["slowdown:3".into(), "error".into()]).unwrap();
        assert_eq!(
            (hard[0].name.as_str(), hard[0].rules[0].times),
            ("slowdown", 3)
        );
        assert_eq!(
            hard[0].about,
            "503 SlowDown on each request's first 3 attempts"
        );
        assert_eq!(hard[1].rules[0].times, 1);
        for (spec, says) in [
            ("slowdown:0", "from 1 to 5"),
            ("slowdown:9", "from 1 to 5"),
            ("slowdown:x", "from 1 to 5"),
            ("cut-body:2", "only slowdown and error"),
            ("nope:2", "no such scenario"),
        ] {
            let e = plan.named(&[spec.into()]).unwrap_err();
            assert!(e.contains(says), "{spec}: {e}");
        }
    }

    #[test]
    fn a_trial_a_run_of_no_command_and_a_run_of_no_requests_are_not_tried() {
        let mut t = run("python job.py", None);
        t.header.scenario = Some("slowdown".into());
        assert!(of(&t).unwrap_err().contains("a trial itself"));
        let mut none = run("python job.py", None);
        none.header.command = None;
        assert!(of(&none).unwrap_err().contains("only a run of a command"));
        let mut quiet = run("python job.py", None);
        quiet.records.clear();
        assert!(of(&quiet).unwrap_err().contains("no requests to fail"));
    }

    #[test]
    fn a_command_with_a_place_of_its_own_per_run_does_not_share_it() {
        for command in [
            "python job.py --out s3://b/{trial}/",
            "sh -c 'python job.py --out s3://b/$SONDE_TRIAL/'",
        ] {
            let plan = of(&run(command, None)).unwrap();
            assert!(!plan.shares_place, "{command}");
            // kept as written, to run it again
            assert_eq!(plan.command, command);
        }
    }

    #[test]
    fn a_projects_own_scenarios_are_the_plan_when_its_sonde_toml_names_some() {
        let project = tempfile::tempdir().unwrap();
        let sub = project.path().join("etl");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(project.path().join(".git")).unwrap();
        std::fs::write(
            project.path().join("sonde.toml"),
            "[check]\nmax-requests = 10\n\n[[scenario]]\nname = \"flaky-puts\"\nfault = \"503 SlowDown\"\non = [\"PutObject\"]\nprefix = \"out/\"\ntimes = 2\n\n[[scenario]]\nname = \"lag\"\nfault = \"delay 200ms\"\n",
        )
        .unwrap();
        // found from the folder the command ran in, upwards
        let found = project_scenarios(&sub).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "flaky-puts");
        assert_eq!(found[0].about, "503 SlowDown on PutObject under out/");
        assert_eq!(found[0].rules[0].times, 2);
        assert_eq!(found[1].rules[0].times, 0, "a delay meets every attempt");
        let plan = of(&run("python job.py", Some(&sub))).unwrap();
        assert!(plan.project);
        let names: Vec<_> = plan.scenarios.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["flaky-puts", "lag"]);
        // (its own times are sonde.toml's: not asked for here)
        assert!(
            plan.named(&["flaky-puts:3".into()])
                .unwrap_err()
                .contains("the project's own")
        );
        // a built-in one can still be asked for by name
        assert_eq!(
            plan.named(&["flaky-puts".into(), "error".into()])
                .unwrap()
                .len(),
            2
        );
        // what the command line refuses is refused here: one reading
        for (toml, says) in [
            (
                "[[scenario]]\nname = \"clean\"\nfault = \"500\"\n",
                "run without faults",
            ),
            (
                "[[scenario]]\nname = \"slow\"\nfault = \"500\"\n",
                "one of sonde's own",
            ),
            ("[program]\nname = \"\"\n", "a program's name"),
            (
                "[[scenario]]\nname = \"bad name\"\nfault = \"500\"\n",
                "a name is letters, digits",
            ),
            (
                "[[scenario]]\nname = \"x\"\nfault = \"500\"\nwhen = 1\n",
                "unknown field `when`",
            ),
        ] {
            std::fs::write(project.path().join("sonde.toml"), toml).unwrap();
            let e = project_scenarios(&sub).unwrap_err();
            assert!(e.contains(says), "{e}");
        }
        // and no file above the repository's root is read
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(outside.path().join("repo/.git")).unwrap();
        std::fs::write(outside.path().join("sonde.toml"), "[[scenario]]\n").unwrap();
        assert!(
            project_scenarios(&outside.path().join("repo"))
                .unwrap()
                .is_empty()
        );
    }
}
