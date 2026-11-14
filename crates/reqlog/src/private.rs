//! Files that hold what a program sent and received (sessions, their caches
//! and verdicts, the request log, the audit log) are their user's alone:
//! made 0600 on Unix, whatever the umask. A file already there keeps its
//! mode. Elsewhere they are made as any file is.
//!
//! A file kept whole ([`replace`]) is written beside itself, synced, and
//! renamed over the one it replaces: a crash leaves the old file or the new
//! one, never half of either.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

/// Options that make a file 0600.
pub fn options() -> OpenOptions {
    #[allow(unused_mut)]
    let mut o = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o
}

/// [`File::create`], 0600.
pub fn create(path: &Path) -> std::io::Result<File> {
    options().write(true).create(true).truncate(true).open(path)
}

/// A file made new, 0600: an error when one is there (`AlreadyExists`).
pub fn create_new(path: &Path) -> std::io::Result<File> {
    options().write(true).create_new(true).open(path)
}

/// [`std::fs::write`], 0600.
pub fn write(path: &Path, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    create(path)?.write_all(bytes.as_ref())
}

/// Replaces `path` with `bytes` durably: see [`replace_with`].
pub fn replace(path: &Path, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    replace_with(path, |f| f.write_all(bytes.as_ref()))
}

/// Replaces `path` with what `fill` writes, durably: into a temporary file
/// of its own beside it (0600; two writers never share one), which is
/// synced, renamed over `path`, and the folder synced where the platform
/// allows. On an error the temporary file is removed and `path` is as it was.
pub fn replace_with(
    path: &Path,
    fill: impl FnOnce(&mut File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let written = create(&tmp)
        .and_then(|mut f| {
            fill(&mut f)?;
            f.sync_all()
        })
        .and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written?;
    sync_dir(path);
    Ok(())
}

/// Syncs the folder `path` is in, so a rename into it survives a crash.
/// Best effort: Windows cannot open a folder to sync it.
fn sync_dir(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        let dir = if dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            dir
        };
        let _ = File::open(dir).and_then(|d| d.sync_all());
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Makes `path` 0600 (a file another library made, such as SQLite's).
pub fn restrict(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn made_for_their_user_alone() {
        let t = tempfile::tempdir().unwrap();
        let mode = |name: &str| {
            std::fs::metadata(t.path().join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        super::write(&t.path().join("a"), b"x").unwrap();
        super::create(&t.path().join("b")).unwrap();
        std::fs::write(t.path().join("c"), b"x").unwrap();
        super::restrict(&t.path().join("c")).unwrap();
        assert_eq!((mode("a"), mode("b"), mode("c")), (0o600, 0o600, 0o600));
    }

    #[test]
    fn a_replaced_file_is_whole_and_leaves_nothing_beside_it() {
        let t = tempfile::tempdir().unwrap();
        let path = t.path().join("kept.json");
        super::replace(&path, b"one").unwrap();
        super::replace(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        let failed = super::replace_with(&path, |_| Err(std::io::Error::other("full")));
        assert!(failed.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"two", "as it was");
        let names: Vec<_> = std::fs::read_dir(t.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["kept.json"]);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
