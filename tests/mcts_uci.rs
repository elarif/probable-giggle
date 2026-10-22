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

/// Phase 3: the `Threads` option must be advertised and accepted.
#[test]
fn uci_advertises_threads_option() {
    let mut s = UciSession::start();
    s.send("uci");
    let lines = s.wait_for("uciok", 5000);
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("option name Threads type spin")),
        "the Threads option must be advertised: {lines:?}"
    );
}

/// Phase 3: `setoption name Threads value 4` then `go nodes N` must return a
/// legal bestmove, and the `info` line must report the thread count.
#[test]
fn uci_threads_option_runs_a_parallel_search() {
    let mut s = UciSession::start();
    s.send("uci");
    let _ = s.wait_for("uciok", 5000);
    s.send("setoption name Threads value 4");
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
    // The info line must report the thread count used.
    assert!(
        lines.iter().any(|l| l.contains("threads 4")),
        "the info line must report the thread count: {lines:?}"
    );
    // Legality of the returned move.
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

/// Phase 4: the `Policy`, `Cpuct` and `RootNoise` options must be
/// advertised, and `setoption name Policy value uct` must switch the
/// engine to the legacy rule — the `info` line reports the policy in
/// force.
#[test]
fn uci_advertises_the_phase4_options() {
    let mut s = UciSession::start();
    s.send("uci");
    let lines = s.wait_for("uciok", 5000);
    for name in ["Policy", "Cpuct", "RootNoise"] {
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with(&format!("option name {name} "))),
            "the {name} option must be advertised: {lines:?}"
        );
    }
}

/// The `Policy` option is honoured: the `info` line names the rule in
/// force, and the search still returns a legal move.
#[test]
fn uci_policy_option_switches_the_selection_rule() {
    for (value, expected) in [("uct", "policy uct"), ("puct", "policy puct")] {
        let mut s = UciSession::start();
        s.send("uci");
        let _ = s.wait_for("uciok", 5000);
        s.send(&format!("setoption name Policy value {value}"));
        s.send("position startpos");
        s.send("isready");
        let _ = s.wait_for("readyok", 30000);
        s.send("go nodes 2000");
        let lines = s.window_for_bestmove();
        assert!(
            lines.iter().any(|l| l.contains(expected)),
            "the info line must report the policy ({expected}): {lines:?}"
        );
        let bm = lines
            .iter()
            .find(|l| l.starts_with("bestmove"))
            .and_then(|l| l.split_whitespace().nth(1))
            .expect("bestmove line");
        assert_eq!(bm.len(), 4, "bestmove token shape: {bm}");
        assert_ne!(bm, "0000", "non-terminal position must move");
    }
}

/// Phase 5: the `LeafQ` option must be advertised — it is the knob that
/// makes the tactical resolution of the leaf testable from the outside.
#[test]
fn uci_advertises_the_phase5_option() {
    let mut s = UciSession::start();
    s.send("uci");
    let lines = s.wait_for("uciok", 5000);
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("option name LeafQ type spin")),
        "the LeafQ option must be advertised: {lines:?}"
    );
}

/// Phase 5: `setoption name LeafQ value 2` must be honoured — the `info`
/// line reports the depth in force, and the search still returns a legal
/// move.
#[test]
fn uci_leafq_option_switches_the_leaf_depth() {
    let mut s = UciSession::start();
    s.send("uci");
    let _ = s.wait_for("uciok", 5000);
    s.send("setoption name LeafQ value 2");
    s.send("position startpos moves e2e4 e7e5");
    s.send("isready");
    let _ = s.wait_for("readyok", 30000);
    s.send("go nodes 2000");
    let lines = s.window_for_bestmove();
    assert!(
        lines.iter().any(|l| l.contains("leafq 2")),
        "the info line must report the leaf depth: {lines:?}"
    );
    let bm = lines
        .iter()
        .find(|l| l.starts_with("bestmove"))
        .and_then(|l| l.split_whitespace().nth(1))
        .expect("bestmove line");
    assert_eq!(bm.len(), 4, "bestmove token shape: {bm}");
    assert_ne!(bm, "0000", "non-terminal position must move");
}

/// Phase 5 — **le budget temps est borné par le temps restant**.
///
/// La phase 4 avait un plancher de 50 ms : avec 20 ms de temps restant,
/// le moteur réfléchissait 50 ms et se faisait **flaguer**. Ici, la
/// réponse doit arriver **bien avant** l'échéance.
///
/// La borne est à 30 ms (et non 20) pour absorber la gigue de
/// l'ordonnanceur : le budget interne est de 15 ms, la réponse mesurée
/// est à ~16 ms, et le moteur de la phase 4 (plancher de 50 ms) mettait
/// plus de 50 ms — le test **échouerait** sur le code de la phase 4.
#[test]
fn uci_wtime_is_respected() {
    let mut s = UciSession::start();
    s.send("uci");
    let _ = s.wait_for("uciok", 5000);
    s.send("position startpos");
    s.send("isready");
    let _ = s.wait_for("readyok", 30000);
    let t0 = std::time::Instant::now();
    s.send("go wtime 20");
    let lines = s.window_for_bestmove();
    let elapsed = t0.elapsed().as_millis() as u64;
    assert!(
        elapsed < 30,
        "la reponse doit arriver avant l'echeance : {elapsed} ms pour 20 ms de temps"
    );
    assert!(
        lines.iter().any(|l| l.starts_with("bestmove")),
        "bestmove manquant : {lines:?}"
    );
}
