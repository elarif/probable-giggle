//! UCI protocol net through the REAL binary (fastchess pre-flight):
//! handshake, position replay, go/bestmove, stop bestmove safety.
//! Reader thread forwards every engine line to a channel (order kept, no
//! lost-buffer issues); `wait_for` INCLUDES the tagged line in the result.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};

struct UciSession {
    child: Child,
    rx: mpsc::Receiver<String>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl UciSession {
    fn start() -> UciSession {
        let bin = env!("CARGO_BIN_EXE_mychess");
        let mut child = Command::new(bin)
            .arg("uci")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn mychess uci");
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        let r = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if tx.send(line.unwrap_or_default()).is_err() {
                    break;
                }
            }
        });
        UciSession {
            child,
            rx,
            reader: Some(r),
        }
    }
    fn send(&mut self, line: &str) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
    }
    /// Drain engine lines until one starts with `tag`; ALL seen lines
    /// (including the tagged one) are returned.
    fn wait_for(&self, tag: &str, timeout_ms: u64) -> Vec<String> {
        let t0 = std::time::Instant::now();
        let mut out_lines = Vec::new();
        loop {
            match self.rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(l) => {
                    if l.starts_with(tag) {
                        out_lines.push(l);
                        return out_lines;
                    }
                    out_lines.push(l);
                }
                Err(RecvTimeoutError::Timeout) => {
                    if t0.elapsed().as_millis() > timeout_ms as u128 {
                        panic!("timeout waiting for `{tag}` (saw {out_lines:?})");
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("engine stream closed before `{tag}` (saw {out_lines:?})");
                }
            }
        }
    }
}

impl Drop for UciSession {
    fn drop(&mut self) {
        if let Some(w) = self.child.stdin.as_mut() {
            let _ = writeln!(w, "quit");
            let _ = w.flush();
        }
        let _ = self.child.wait();
        drop(self.reader.take());
    }
}

#[test]
fn uci_handshake_engine_ready() {
    let mut s = UciSession::start();
    s.send("uci");
    let lines = s.wait_for("uciok", 5000);
    assert!(
        lines.iter().any(|l| l.starts_with("id name mychess-mcts")),
        "engine id line missing"
    );
    s.send("isready");
    let got = s.wait_for("readyok", 5000);
    got.iter().for_each(|_| ());
}

#[test]
fn uci_go_nodes_returns_real_bestmove() {
    let mut s = UciSession::start();
    s.send("uci");
    let _ = s.wait_for("uciok", 5000);
    s.send("position startpos moves e2e4 e7e5");
    s.send("isready");
    let _ = s.wait_for("readyok", 30000);
    s.send("go nodes 2000");
    let lines = s.window_for_bestmove();
    let bm = lines
        .iter()
        .find(|l| l.starts_with("bestmove"))
        .and_then(|l| l.split_whitespace().nth(1))
        .expect("bestmove line");
    assert_eq!(bm.len(), 4, "bestmove token shape: {bm}");
    assert_ne!(bm, "0000", "non-terminal position must move");
    // legality: token must be one of the root moves for the replayed position
    let mut pos = mychess::Position::startpos();
    for u in ["e2e4", "e7e5"] {
        let mut list = mychess::movegen::MoveList::new();
        mychess::movegen::generate(&pos, &mut list, mychess::types::GenKind::All);
        for i in 0..list.len() {
            if list.get(i).uci() == u {
                pos.make(list.get(i));
            }
        }
    }
    let mut list = mychess::movegen::MoveList::new();
    mychess::movegen::generate(&pos, &mut list, mychess::types::GenKind::All);
    let known: Vec<String> = (0..list.len()).map(|i| list.get(i).uci()).collect();
    assert!(
        known.iter().any(|u| u == bm),
        "bestmove {bm} illegal (legal: {known:?})"
    );
}

#[test]
fn uci_stop_answers_with_bestmove() {
    let mut s = UciSession::start();
    s.send("uci");
    let _ = s.wait_for("uciok", 5000);
    s.send("position startpos");
    s.send("go infinite");
    std::thread::sleep(std::time::Duration::from_millis(120));
    s.send("stop");
    let lines = s.window_for_bestmove();
    let bm = lines
        .iter()
        .find(|l| l.starts_with("bestmove"))
        .and_then(|l| l.split_whitespace().nth(1))
        .expect("bestmove after stop");
    assert_eq!(bm.len(), 4, "bestmove token shape after stop: {bm}");
}

impl UciSession {
    /// Collect lines until `bestmove` with a generous bound (30 s).
    fn window_for_bestmove(&self) -> Vec<String> {
        self.wait_for("bestmove", 30000)
    }
}
