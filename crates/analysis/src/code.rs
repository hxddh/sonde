//! The file a run's code place names (`etl/load.py:212 read_manifest`),
//! found on this machine. The place and the folder the run says it ran in
//! are both text from the run's file, which may be someone else's: a file is
//! "of the run" only when it really is inside that folder (links followed),
//! is a regular file, and is of a type on the list. The app opens such a
//! file ([`find`]); `sonde show` reads one line of it ([`line`]).

use std::path::{Path, PathBuf};

/// Program source: what a caller's place can name, and all [`line`] reads.
const SOURCE: &[&str] = &[
    "py", "js", "ts", "tsx", "jsx", "mjs", "cjs", "java", "kt", "go", "rs", "rb", "c", "h", "cpp",
    "cs", "scala",
];

/// Text an editor shows, opened by a press beside [`SOURCE`]; never read
/// into `show`'s output (settings and data are where secrets are kept).
const TEXT: &[&str] = &["sql", "toml", "yaml", "yml", "json", "md", "txt", "xml"];

/// Of [`SOURCE`], what Windows runs when opened (Windows Script Host, an
/// installed Python or Ruby): shown there, not opened.
const RUNS_ON_WINDOWS: &[&str] = &["js", "py", "rb"];

/// Whether a press may open a file of this extension: source and text,
/// which the system shows in an editor. Anything else (programs, scripts,
/// shortcuts, installers, a file with no extension) is not on the list.
pub fn opens(ext: &str, windows: bool) -> bool {
    let e = ext.to_ascii_lowercase();
    let e = e.as_str();
    (SOURCE.contains(&e) || TEXT.contains(&e)) && !(windows && RUNS_ON_WINDOWS.contains(&e))
}

/// Whether a line of a file of this extension may be read out: source only.
fn reads(ext: &str) -> bool {
    SOURCE.contains(&ext.to_ascii_lowercase().as_str())
}

/// A path on another machine (`\\host\share`, `//host/share`,
/// `\\?\UNC\…`): even looking at it reaches out to that host.
pub fn remote(path: &str) -> bool {
    let p = path.replace('/', "\\");
    match p.strip_prefix("\\\\") {
        // `\\?\C:\…` and `\\.\…` are this machine's, unless they spell UNC
        Some(rest) => match rest
            .strip_prefix("?\\")
            .or_else(|| rest.strip_prefix(".\\"))
        {
            Some(local) => local.to_ascii_uppercase().starts_with("UNC\\"),
            None => true,
        },
        None => false,
    }
}

/// A file a place names, as it is on this machine (canonical).
#[derive(Debug, PartialEq)]
pub struct Found {
    pub path: PathBuf,
    /// A regular file inside the folder the run ran in.
    pub inside: bool,
}

/// Why a place names nothing to look at.
#[derive(Debug, PartialEq)]
pub enum Refused {
    /// The folder or the file is on another machine: not looked at.
    Remote,
    /// The folder the run says it ran in is not there (the folder, why).
    NoFolder(PathBuf, String),
    /// The file is not there (as it was looked for).
    Missing(PathBuf),
}

/// The file `file` names for a run that ran in `dir`, relative to it or
/// absolute. `dir` and `file` both come from the run file: a folder on
/// another machine is refused before anything touches the path
/// (canonicalizing it would contact the host), and a folder that is a whole
/// disk is no folder to be "inside" of.
pub fn find(dir: &Path, file: &str) -> Result<Found, Refused> {
    if remote(&dir.to_string_lossy()) || remote(file) {
        return Err(Refused::Remote);
    }
    let root = dir
        .canonicalize()
        .map_err(|e| Refused::NoFolder(dir.to_path_buf(), e.to_string()))?;
    // a canonical root on Windows is verbatim (`\\?\C:\…`), where `/` is no
    // separator: a place written `etl/load.py` is joined as `etl\load.py`
    let file = if cfg!(windows) {
        std::borrow::Cow::Owned(file.replace('/', "\\"))
    } else {
        std::borrow::Cow::Borrowed(file)
    };
    let candidate = Path::new(file.as_ref());
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let path = joined
        .canonicalize()
        .map_err(|_| Refused::Missing(joined))?;
    // (a device, a pipe or a folder is no file to open or read)
    let regular = path.metadata().map(|m| m.is_file()).unwrap_or(false);
    let inside = root.parent().is_some() && path.starts_with(&root) && regular;
    Ok(Found { path, inside })
}

/// The most of a file [`line`] reads to reach its line: source is small,
/// and a file of one endless line is not read whole.
const MOST: u64 = 8 << 20;

/// Line `line` of the source file `file` of a run that ran in `dir`,
/// trimmed and cut to 200 characters. None unless the file is program
/// source inside that folder ([`find`]): a run from elsewhere names its
/// folder and its files itself, and must not have any other file of this
/// machine read out (`/etc/passwd`, `../../.aws/credentials`, a link out of
/// the folder, a settings file).
pub fn line(dir: Option<&Path>, file: &str, line: u32) -> Option<String> {
    use std::io::{BufRead, Read};
    let found = find(dir?, file).ok().filter(|f| f.inside)?;
    if !found
        .path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(reads)
    {
        return None;
    }
    let f = std::fs::File::open(&found.path).ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let text = std::io::BufReader::new(f.take(MOST))
        .lines()
        .nth(line.checked_sub(1)? as usize)?
        .ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.chars().take(200).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_read_only_from_source_inside_the_folder_the_run_ran_in() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let project = root.join("project");
        std::fs::create_dir_all(project.join("etl")).unwrap();
        std::fs::write(project.join("etl/load.py"), "import boto3\n  read()  \n").unwrap();
        std::fs::write(project.join("settings.json"), "{\"key\": \"SECRET\"}\n").unwrap();
        std::fs::write(project.join("credentials"), "SECRET\n").unwrap();
        std::fs::write(root.join("outside.py"), "SECRET\n").unwrap();
        let at = |file: &str| line(Some(&project), file, 1);
        assert_eq!(at("etl/load.py").as_deref(), Some("import boto3"));
        assert_eq!(
            line(Some(&project), "etl/load.py", 2).as_deref(),
            Some("read()")
        );
        // by its whole path too, when that is inside
        assert!(at(&project.join("etl/load.py").to_string_lossy()).is_some());
        // outside: by an absolute path, by `..`
        assert_eq!(at(&root.join("outside.py").to_string_lossy()), None);
        assert_eq!(at("../outside.py"), None);
        assert_eq!(at("etl/../../outside.py"), None);
        // inside, but no program source: settings, a file with no extension
        assert_eq!(at("settings.json"), None);
        assert_eq!(at("credentials"), None);
        // no folder: nothing is "inside" it; no line 0; a line past the end
        assert_eq!(line(None, "etl/load.py", 1), None);
        assert_eq!(line(Some(&project), "etl/load.py", 0), None);
        assert_eq!(line(Some(&project), "etl/load.py", 9), None);
        // a folder that is the whole disk holds everything: nothing is read
        let top = root.ancestors().last().unwrap();
        assert_eq!(
            line(Some(top), &project.join("etl/load.py").to_string_lossy(), 1),
            None
        );
        // another machine's: refused before the path is looked at
        assert_eq!(at("//host/share/load.py"), None);
        assert_eq!(
            find(Path::new("//host/share"), "load.py"),
            Err(Refused::Remote)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_folder_and_a_device_are_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(root.join("secret.py"), "SECRET\n").unwrap();
        std::os::unix::fs::symlink(root.join("secret.py"), project.join("link.py")).unwrap();
        std::os::unix::fs::symlink(&root, project.join("up")).unwrap();
        std::os::unix::fs::symlink("/dev/zero", project.join("zero.py")).unwrap();
        assert_eq!(line(Some(&project), "link.py", 1), None);
        assert_eq!(line(Some(&project), "up/secret.py", 1), None);
        assert_eq!(line(Some(&project), "zero.py", 1), None);
        assert_eq!(line(Some(&project), "/dev/zero", 1), None);
        // a link that stays inside is the file it names
        std::fs::write(project.join("real.py"), "ok\n").unwrap();
        std::os::unix::fs::symlink(project.join("real.py"), project.join("alias.py")).unwrap();
        assert_eq!(line(Some(&project), "alias.py", 1).as_deref(), Some("ok"));
    }
}
