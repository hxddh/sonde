//! Running a program in a pseudo-terminal (POSIX ptys, Windows ConPTY),
//! its output read on a thread and handed over as bytes.

use std::io::{Read, Write};

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use tokio::sync::{mpsc, oneshot};

/// The running program: its terminal's input, size and life.
pub struct Running {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

impl Running {
    pub fn write(&mut self, data: &[u8]) -> bool {
        self.writer
            .write_all(data)
            .and_then(|_| self.writer.flush())
            .is_ok()
    }

    pub fn resize(&self, cols: u16, rows: u16) -> bool {
        self.master
            .resize(PtySize {
                rows: rows.max(1),
                cols: cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .is_ok()
    }

    pub fn kill(&mut self) -> bool {
        self.killer.kill().is_ok()
    }
}

type Spawned = (
    mpsc::UnboundedReceiver<Vec<u8>>,
    oneshot::Receiver<Option<i32>>,
    Running,
);

/// The folder a program runs in: the one asked for, else the home folder.
pub fn folder(cwd: Option<&str>) -> Option<std::path::PathBuf> {
    cwd.filter(|d| !d.trim().is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(Into::into))
        .or_else(|| std::env::var_os("USERPROFILE").map(Into::into))
        .or_else(|| std::env::current_dir().ok())
}

/// Starts `command` through the platform's shell in a new terminal of
/// `cols`×`rows`, with `set` added to and `unset` removed from the
/// environment. Output arrives on the channel, which closes when the
/// program and every process holding its terminal are gone; the exit code
/// on the oneshot.
pub fn spawn(
    command: &str,
    cwd: Option<&str>,
    set: &[(String, String)],
    unset: &[String],
    cols: u16,
    rows: u16,
) -> anyhow::Result<Spawned> {
    let pair = native_pty_system().openpty(PtySize {
        rows: rows.max(1),
        cols: cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut cmd = if cfg!(windows) {
        let mut c = CommandBuilder::new("cmd.exe");
        c.args(["/C", command]);
        c
    } else {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let mut c = CommandBuilder::new(shell);
        c.args(["-c", command]);
        c
    };
    if let Some(d) = folder(cwd) {
        cmd.cwd(d);
    }
    for k in unset {
        cmd.env_remove(k);
    }
    for (k, v) in set {
        cmd.env(k, v);
    }
    cmd.env("TERM", "xterm-256color");
    // line-buffered output, so lines arrive when they are printed
    cmd.env("PYTHONUNBUFFERED", "1");
    let mut child = pair.slave.spawn_command(cmd)?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;
    let killer = child.clone_killer();
    let (out_tx, out_rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if out_tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let (exit_tx, exit_rx) = oneshot::channel();
    std::thread::spawn(move || {
        let code = child.wait().ok().map(|s| s.exit_code() as i32);
        let _ = exit_tx.send(code);
    });
    Ok((
        out_rx,
        exit_rx,
        Running {
            writer,
            master: pair.master,
            killer,
        },
    ))
}

/// Decodes UTF-8 arriving in pieces: a character split across two reads
/// waits for its end.
#[derive(Default)]
pub struct Utf8Stream {
    pending: Vec<u8>,
}

impl Utf8Stream {
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::new();
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(s) => {
                    out.push_str(s);
                    self.pending.clear();
                    return out;
                }
                Err(e) => {
                    let good = e.valid_up_to();
                    out.push_str(std::str::from_utf8(&self.pending[..good]).expect("valid prefix"));
                    match e.error_len() {
                        // an incomplete character at the end: keep it for the next read
                        None => {
                            self.pending.drain(..good);
                            return out;
                        }
                        Some(bad) => {
                            out.push('\u{fffd}');
                            self.pending.drain(..good + bad);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_across_reads() {
        let mut d = Utf8Stream::default();
        let s = "héllo 世界".as_bytes();
        let mut out = String::new();
        for b in s.chunks(1) {
            out.push_str(&d.push(b));
        }
        assert_eq!(out, "héllo 世界");
        assert_eq!(d.push(&[0xff, b'a']), "\u{fffd}a");
    }

    #[tokio::test]
    async fn runs_in_a_terminal_with_the_environment() {
        let (mut rx, exit, _r) = spawn(
            "printf \"$SONDE_T [$UNSET_ME]\"; test -t 1 && echo ' tty'",
            Some("/"),
            &[("SONDE_T".into(), "set".into())],
            &["UNSET_ME".into()],
            80,
            24,
        )
        .unwrap();
        let mut out = String::new();
        let mut d = Utf8Stream::default();
        while let Some(b) = rx.recv().await {
            out.push_str(&d.push(&b));
        }
        assert!(out.contains("set [] tty"), "{out:?}");
        assert_eq!(exit.await.unwrap(), Some(0));
    }
}
