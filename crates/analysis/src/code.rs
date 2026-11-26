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

/// Whether a path's text names a place on this disk, before anything looks
/// at it: on Windows only a drive's path (`C:\…`, or verbatim `\\?\C:\…`),
/// never a device or NT path (`\\.\…`, `\\?\GLOBALROOT\…`, `\??\…`), a
/// share or a path relative to the drive or folder sonde is in; elsewhere
/// an absolute path, not `//host`.
fn local_text(path: &str, windows: bool) -> bool {
    if remote(path) {
        return false;
    }
    if !windows {
        return path.starts_with('/');
    }
    let p = path.replace('/', "\\");
    let drive = p.strip_prefix("\\\\?\\").unwrap_or(&p).as_bytes();
    drive.len() >= 3 && drive[0].is_ascii_alphabetic() && drive[1] == b':' && drive[2] == b'\\'
}

/// Whether a file a run names (a caller's place) may be looked for as it
/// is written: relative (to a [`local_dir`]), or a path on this disk.
pub fn local_file(file: &str) -> bool {
    !remote(file) && (!Path::new(file).is_absolute() || local_text(file, cfg!(windows)))
}

/// The folder a run says it ran in (`cwd` of its header), when it is a
/// folder on this machine: the one way anything reads a run's folder, lists
/// it or runs git in it. A run's file can come from anyone, so a share, a
/// device or NT path, a relative path, or a folder that turns out to be on
/// another machine once resolved (a drive mapped to a share) is none.
/// The path is given back as the run wrote it.
pub fn local_dir(cwd: &Path) -> Option<PathBuf> {
    let windows = cfg!(windows);
    if !local_text(&cwd.to_string_lossy(), windows) {
        return None;
    }
    let canonical = cwd.canonicalize().ok()?;
    (local_text(&canonical.to_string_lossy(), windows) && canonical.is_dir())
        .then(|| cwd.to_path_buf())
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
    let windows = cfg!(windows);
    if !local_text(&dir.to_string_lossy(), windows) || !local_file(file) {
        return Err(Refused::Remote);
    }
    let root = dir
        .canonicalize()
        .map_err(|e| Refused::NoFolder(dir.to_path_buf(), e.to_string()))?;
    // (a drive letter can be a share's: what it resolves to says)
    if !local_text(&root.to_string_lossy(), windows) {
        return Err(Refused::Remote);
    }
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

    #[test]
    fn a_folder_is_local_only_by_a_drive_or_an_absolute_path() {
        // Windows: a drive's path, plain or verbatim, and nothing else
        for ok in [r"C:\work", "c:/work", r"\\?\C:\work", r"D:\"] {
            assert!(local_text(ok, true), "{ok}");
        }
        for no in [
            r"\\host\share",
            "//host/share",
            r"\\?\UNC\host\share",
            r"\\?\GLOBALROOT\Device\Mup\host\share",
            r"\\?\Volume{0b1c}\work",
            r"\\.\C:\work",
            r"\\.\pipe\x",
            r"\??\C:\work",
            r"\??\UNC\host\share",
            r"\work",
            "C:work",
            "work",
            "",
        ] {
            assert!(!local_text(no, true), "{no}");
        }
        // elsewhere: an absolute path, not `//host`
        assert!(local_text("/home/me/work", false));
        for no in ["//host/share", r"\\host\share", "work", ".", ""] {
            assert!(!local_text(no, false), "{no}");
        }
        // a caller's file: relative to the folder, or on this disk
        assert!(local_file("etl/load.py"));
        assert!(!local_file("//host/share/load.py"));
        assert!(!local_file(r"\\host\share\load.py"));
        assert!(!local_file(r"\\?\UNC\host\share\load.py"));
    }

    #[test]
    fn local_dir_is_a_folder_on_this_machine_as_the_run_wrote_it() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(local_dir(dir.path()).as_deref(), Some(dir.path()));
        // not there, not a folder, relative: none
        assert_eq!(local_dir(&dir.path().join("gone")), None);
        std::fs::write(dir.path().join("f.py"), "").unwrap();
        assert_eq!(local_dir(&dir.path().join("f.py")), None);
        assert_eq!(local_dir(Path::new(".")), None);
        assert_eq!(local_dir(Path::new("")), None);
        // another machine's: refused before it is looked at
        assert_eq!(local_dir(Path::new("//host/share")), None);
        assert_eq!(local_dir(Path::new(r"\\host\share")), None);
    }

    #[cfg(windows)]
    #[test]
    fn a_device_or_nt_path_is_no_folder_on_windows() {
        for no in [
            r"\\?\GLOBALROOT\Device\Mup\localhost\C$",
            r"\\.\C:\",
            r"\??\C:\",
            r"\\localhost\C$",
            r"\\?\UNC\localhost\C$",
            r"\Windows",
        ] {
            assert_eq!(local_dir(Path::new(no)), None, "{no}");
            assert_eq!(find(Path::new(no), "a.py"), Err(Refused::Remote), "{no}");
        }
        let dir = tempfile::tempdir().unwrap();
        let verbatim = dir.path().canonicalize().unwrap();
        assert!(local_dir(&verbatim).is_some());
        // a file named by a device path is not looked at either
        assert_eq!(
            find(&verbatim, r"\\?\GLOBALROOT\Device\Mup\localhost\C$\a.py"),
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
