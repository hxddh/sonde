//! The commands a folder likely runs, for Run to propose when its command
//! field is empty: a Makefile's targets, package.json's scripts,
//! pyproject's entry points, the Python files that run as a program (with
//! `spark-submit` for a Spark job), `go run .`, `cargo run`. Read from the
//! folder alone (never its subfolders), quickly; nothing found is nothing
//! proposed.

use std::path::Path;

use serde::Serialize;

/// One command proposed, and the file it was read from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Suggested {
    pub command: String,
    pub from: String,
}

/// At most this many are proposed.
pub const MOST: usize = 6;
/// Of each kind (targets, scripts, files), at most this many.
const EACH: usize = 3;
/// Python files read for whether they run as a program, and how much of each.
const PY_FILES: usize = 40;
const PY_BYTES: u64 = 64 * 1024;

/// The commands `dir` likely runs, the likeliest first; `windows`: as
/// cmd.exe runs them (`python`, not `python3`).
pub fn suggested(dir: &Path, windows: bool) -> Vec<Suggested> {
    let mut out: Vec<Suggested> = Vec::new();
    let mut add = |command: String, from: &str| {
        if out.len() < MOST && !out.iter().any(|s| s.command == command) {
            out.push(Suggested {
                command,
                from: from.to_string(),
            });
        }
    };
    for (c, f) in package_scripts(dir) {
        add(c, &f);
    }
    for (c, f) in pyproject_scripts(dir) {
        add(c, &f);
    }
    for (c, f) in make_targets(dir) {
        add(c, &f);
    }
    for (c, f) in python_programs(dir, windows) {
        add(c, &f);
    }
    if dir.join("go.mod").is_file() {
        add("go run .".into(), "go.mod");
    }
    if read(dir, "Cargo.toml")
        .and_then(|t| t.parse::<toml::Table>().ok())
        .is_some_and(|t| t.contains_key("package"))
    {
        add("cargo run".into(), "Cargo.toml");
    }
    out
}

fn read(dir: &Path, name: &str) -> Option<String> {
    let path = dir.join(name);
    let len = std::fs::metadata(&path).ok()?.len();
    if len > 1024 * 1024 {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Targets that are no run of the program: housekeeping, help.
const NOT_A_RUN: &[&str] = &[
    "clean",
    "distclean",
    "install",
    "uninstall",
    "help",
    "lint",
    "fmt",
    "format",
    "build",
    "prepare",
    "deps",
];

/// `make <target>` for a Makefile's targets, in its order.
fn make_targets(dir: &Path) -> Vec<(String, String)> {
    // the names as the folder spells them: on a file system that ignores case
    // (macOS, Windows) `makefile` would open a `Makefile` and be named wrong
    let names: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    let Some((name, text)) = ["GNUmakefile", "makefile", "Makefile"]
        .iter()
        .filter(|n| names.iter().any(|x| x == *n))
        .find_map(|n| read(dir, n).map(|t| (*n, t)))
    else {
        return vec![];
    };
    let mut targets: Vec<String> = Vec::new();
    for line in text.lines() {
        // a rule starts its line: `name: deps`, not `x := y`, not a recipe
        if line.starts_with(['\t', ' ', '#', '.']) {
            continue;
        }
        let Some((head, rest)) = line.split_once(':') else {
            continue;
        };
        if rest.starts_with('=') || head.contains('=') {
            continue;
        }
        for t in head.split_whitespace() {
            let ok = t
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c));
            if ok && !NOT_A_RUN.contains(&t) && !targets.iter().any(|x| x == t) {
                targets.push(t.to_string());
            }
        }
    }
    targets
        .into_iter()
        .take(EACH)
        .map(|t| (format!("make {t}"), name.to_string()))
        .collect()
}

/// `npm run <script>` (pnpm, yarn or bun where the folder uses it) for
/// package.json's scripts: start and dev first, hooks and housekeeping
/// left out.
fn package_scripts(dir: &Path) -> Vec<(String, String)> {
    let Some(json) =
        read(dir, "package.json").and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    else {
        return vec![];
    };
    let Some(scripts) = json.get("scripts").and_then(|s| s.as_object()) else {
        return vec![];
    };
    let runner = if dir.join("pnpm-lock.yaml").is_file() {
        "pnpm run"
    } else if dir.join("yarn.lock").is_file() {
        "yarn"
    } else if dir.join("bun.lockb").is_file() || dir.join("bun.lock").is_file() {
        "bun run"
    } else {
        "npm run"
    };
    let mut names: Vec<&String> = scripts
        .keys()
        .filter(|n| {
            !NOT_A_RUN.contains(&n.as_str())
                && !n.starts_with("pre")
                && !n.starts_with("post")
                && !n.contains(' ')
        })
        .collect();
    let rank = |n: &str| match n {
        "start" => 0,
        "dev" => 1,
        _ => 2,
    };
    names.sort_by_key(|n| rank(n));
    names
        .into_iter()
        .take(EACH)
        .map(|n| (format!("{runner} {n}"), "package.json".into()))
        .collect()
}

/// pyproject.toml's entry points (`[project.scripts]`, Poetry's), run as
/// the folder's tool runs them (`uv run`, `poetry run`).
fn pyproject_scripts(dir: &Path) -> Vec<(String, String)> {
    let Some(t) = read(dir, "pyproject.toml").and_then(|t| t.parse::<toml::Table>().ok()) else {
        return vec![];
    };
    let at = |path: &[&str]| -> Vec<String> {
        let mut v: &toml::Value = &toml::Value::Table(t.clone());
        for k in path {
            match v.get(k) {
                Some(x) => v = x,
                None => return vec![],
            }
        }
        v.as_table()
            .map(|s| s.keys().cloned().collect())
            .unwrap_or_default()
    };
    let mut names = at(&["project", "scripts"]);
    names.extend(at(&["tool", "poetry", "scripts"]));
    let runner = if dir.join("uv.lock").is_file() {
        "uv run "
    } else if dir.join("poetry.lock").is_file() {
        "poetry run "
    } else {
        ""
    };
    names
        .into_iter()
        .filter(|n| !n.contains(char::is_whitespace))
        .take(EACH)
        .map(|n| (format!("{runner}{n}"), "pyproject.toml".into()))
        .collect()
}

/// The folder's Python files that run as a program (`if __name__ ==
/// "__main__"`, or a `main.py`): `python3 x.py`, or `spark-submit x.py`
/// for one that starts Spark. `main.py` first, then by name.
fn python_programs(dir: &Path, windows: bool) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut files: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| {
            n.ends_with(".py")
                && !n.starts_with(['.', '_'])
                && !n.starts_with("test_")
                && n != "setup.py"
                && n != "conftest.py"
        })
        .collect();
    files.sort_by_key(|n| (n != "main.py", n.clone()));
    let python = if windows { "python" } else { "python3" };
    let mut out = Vec::new();
    for name in files.into_iter().take(PY_FILES) {
        let Ok(f) = std::fs::File::open(dir.join(&name)) else {
            continue;
        };
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut std::io::Read::take(f, PY_BYTES), &mut text);
        let runs = name == "main.py"
            || text.contains("__name__ == \"__main__\"")
            || text.contains("__name__ == '__main__'");
        if !runs {
            continue;
        }
        let quoted = if name.contains(' ') {
            format!("\"{name}\"")
        } else {
            name.clone()
        };
        let command = if text.contains("SparkSession") || text.contains("SparkContext") {
            format!("spark-submit {quoted}")
        } else {
            format!("{python} {quoted}")
        };
        out.push((command, name));
        if out.len() == EACH {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (n, t) in files {
            std::fs::write(d.path().join(n), t).unwrap();
        }
        d
    }

    fn commands(d: &tempfile::TempDir, windows: bool) -> Vec<String> {
        suggested(d.path(), windows)
            .into_iter()
            .map(|s| s.command)
            .collect()
    }

    #[test]
    fn an_empty_folder_proposes_nothing() {
        let d = folder(&[("README.md", "# hi"), ("notes.py", "x = 1\n")]);
        assert!(suggested(d.path(), false).is_empty());
        assert!(suggested(Path::new("/no/such/folder"), false).is_empty());
    }

    #[test]
    fn a_makefiles_targets_in_its_order_without_housekeeping() {
        let d = folder(&[(
            "Makefile",
            ".PHONY: etl clean\nVAR := x\nCC=gcc\netl: deps\n\tpython3 etl.py\nclean:\n\trm -rf out\nreport load: etl\n%.o: %.c\n",
        )]);
        assert_eq!(
            commands(&d, false),
            ["make etl", "make report", "make load"]
        );
        assert_eq!(suggested(d.path(), false)[0].from, "Makefile");
    }

    #[test]
    fn package_scripts_start_first_by_the_folders_runner() {
        let pkg = r#"{"scripts": {"build": "tsc", "prestart": "x", "seed": "node seed.js", "start": "node index.js", "dev": "nodemon"}}"#;
        let d = folder(&[("package.json", pkg)]);
        assert_eq!(
            commands(&d, false),
            ["npm run start", "npm run dev", "npm run seed"]
        );
        let d = folder(&[("package.json", pkg), ("pnpm-lock.yaml", "")]);
        assert_eq!(commands(&d, false)[0], "pnpm run start");
        // not JSON: nothing, quietly
        let d = folder(&[("package.json", "{")]);
        assert!(commands(&d, false).is_empty());
    }

    #[test]
    fn pyproject_entry_points_and_python_programs() {
        let d = folder(&[
            (
                "pyproject.toml",
                "[project]\nname = \"etl\"\n[project.scripts]\netl-load = \"etl.cli:main\"\n",
            ),
            ("uv.lock", ""),
            (
                "job.py",
                "import boto3\nif __name__ == \"__main__\":\n    main()\n",
            ),
            ("lib.py", "def f(): pass\n"),
            (
                "spark_job.py",
                "from pyspark.sql import SparkSession\nif __name__ == '__main__':\n    run()\n",
            ),
            ("test_job.py", "if __name__ == \"__main__\": pass\n"),
        ]);
        assert_eq!(
            commands(&d, false),
            [
                "uv run etl-load",
                "python3 job.py",
                "spark-submit spark_job.py"
            ]
        );
        // cmd.exe has `python`
        assert_eq!(commands(&d, true)[1], "python job.py");
    }

    #[test]
    fn go_and_cargo_projects() {
        let d = folder(&[("go.mod", "module x\n")]);
        assert_eq!(commands(&d, false), ["go run ."]);
        let d = folder(&[("Cargo.toml", "[package]\nname = \"x\"\n")]);
        assert_eq!(commands(&d, false), ["cargo run"]);
        // a workspace alone runs nothing by itself
        let d = folder(&[("Cargo.toml", "[workspace]\nmembers = []\n")]);
        assert!(commands(&d, false).is_empty());
    }

    #[test]
    fn at_most_a_few() {
        let d = folder(&[
            ("Makefile", "a:\nb:\nc:\nd:\n"),
            (
                "package.json",
                r#"{"scripts": {"x": "1", "y": "2", "z": "3", "w": "4"}}"#,
            ),
            ("go.mod", "module x\n"),
        ]);
        let c = commands(&d, false);
        assert_eq!(c.len(), MOST);
        assert_eq!(c.iter().filter(|c| c.starts_with("make")).count(), 3);
    }
}
