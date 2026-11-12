//! Files that hold what a program sent and received (sessions, their caches
//! and verdicts, the request log, the audit log) are their user's alone:
//! made 0600 on Unix, whatever the umask. A file already there keeps its
//! mode. Elsewhere they are made as any file is.

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

/// [`std::fs::write`], 0600.
pub fn write(path: &Path, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    create(path)?.write_all(bytes.as_ref())
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
}
