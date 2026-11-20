//! Port of `host-files/GameLogWatcher.ts`: tails `Client.txt` and emits new
//! lines.
//!
//! Like upstream (`fs.watchFile` with 450ms interval) this polls the file
//! size instead of using inotify/ReadDirectoryChangesW: PoE (and Wine) keep
//! the file open and append to it, which is not reliably reported by
//! change notifications on every filesystem (NTFS via ntfs3/ntfs-3g, network
//! drives, Proton prefixes on other disks), and a stat every 450ms is free.
//!
//! Improvement over upstream: an incomplete trailing line (or a UTF-8
//! sequence split by a read) is buffered until its newline arrives.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use super::paths::{PathEnv, guess_file_location};
use crate::emit::{Emitter, Level};
use crate::types::PlatformEvent;

const POLL_INTERVAL: Duration = Duration::from_millis(450);
const READ_CHUNK: usize = 64 * 1024;

/// Splits a byte stream into trimmed, non-empty lines.
#[derive(Default)]
pub struct LogTail {
    partial: Vec<u8>,
}

impl LogTail {
    pub fn feed(&mut self, data: &[u8]) -> Vec<String> {
        self.partial.extend_from_slice(data);
        let Some(last_nl) = self.partial.iter().rposition(|b| *b == b'\n') else {
            return Vec::new();
        };
        let complete: Vec<u8> = self.partial.drain(..=last_nl).collect();
        String::from_utf8_lossy(&complete)
            .split('\n')
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect()
    }
}

pub(crate) enum LogCmd {
    Restart(Option<PathBuf>),
}

pub(crate) struct GameLogWatcher {
    tx: Sender<LogCmd>,
}

impl GameLogWatcher {
    pub fn spawn(emit: Emitter) -> anyhow::Result<Self> {
        let (tx, rx) = crossbeam_channel::unbounded();
        std::thread::Builder::new().name("rpt-game-log".into()).spawn(move || run(rx, emit))?;
        Ok(Self { tx })
    }

    /// `restart(logFile)`: `None`/empty = auto-detect. No-op if unchanged.
    pub fn restart(&self, path: Option<PathBuf>) {
        let _ = self.tx.send(LogCmd::Restart(path));
    }
}

struct Open {
    file: File,
    offset: u64,
    tail: LogTail,
}

fn run(rx: Receiver<LogCmd>, emit: Emitter) {
    let mut wanted: Option<Option<PathBuf>> = None;
    let mut state: Option<Open> = None;
    loop {
        match rx.recv_timeout(POLL_INTERVAL) {
            Ok(LogCmd::Restart(path)) => {
                let path = path.filter(|p| !p.as_os_str().is_empty());
                if wanted.as_ref() == Some(&path) {
                    continue;
                }
                wanted = Some(path.clone());
                state = None;
                let resolved = match path {
                    Some(p) => p,
                    None => match guess_file_location(PathEnv::from_system().client_log_candidates()) {
                        Some(p) => p,
                        None => continue,
                    },
                };
                match File::open(&resolved).and_then(|f| f.metadata().map(|m| (f, m.len()))) {
                    Ok((file, len)) => {
                        tracing::info!("watching game log {}", resolved.display());
                        state = Some(Open { file, offset: len, tail: LogTail::default() });
                    }
                    Err(_) => emit.log(Level::Error, "GameLogWatcher", "Failed to watch file."),
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }

        if let Some(st) = state.as_mut() {
            let lines = read_to_eof(st);
            if !lines.is_empty() {
                emit.send(PlatformEvent::GameLog { lines });
            }
        }
    }
}

fn read_to_eof(st: &mut Open) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(len) = st.file.metadata().map(|m| m.len()) else { return out };
    if len < st.offset {
        // truncated / rotated: continue from the new end
        st.offset = len;
        st.tail = LogTail::default();
        return out;
    }
    if len == st.offset || st.file.seek(SeekFrom::Start(st.offset)).is_err() {
        return out;
    }
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        match st.file.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                st.offset += n as u64;
                out.extend(st.tail.feed(&buf[..n]));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_splits_and_buffers() {
        let mut t = LogTail::default();
        assert_eq!(t.feed(b"2024/01/01 a\r\n\n  b  \nparti"), vec!["2024/01/01 a", "b"]);
        assert!(t.feed(b"al").is_empty());
        assert_eq!(t.feed(b" line\n"), vec!["partial line"]);
        // multibyte char split across reads
        let s = "Привет\n".as_bytes();
        assert!(t.feed(&s[..3]).is_empty());
        assert_eq!(t.feed(&s[3..]), vec!["Привет"]);
    }

    #[test]
    fn reads_appended_lines() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("Client.txt");
        std::fs::write(&p, "old line\n").unwrap();
        let file = File::open(&p).unwrap();
        let len = file.metadata().unwrap().len();
        let mut st = Open { file, offset: len, tail: LogTail::default() };
        assert!(read_to_eof(&mut st).is_empty());
        use std::io::Write;
        let mut w = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        writeln!(w, "new 1").unwrap();
        write!(w, "new 2").unwrap();
        w.flush().unwrap();
        assert_eq!(read_to_eof(&mut st), vec!["new 1"]);
        writeln!(w).unwrap();
        assert_eq!(read_to_eof(&mut st), vec!["new 2"]);
    }
}
