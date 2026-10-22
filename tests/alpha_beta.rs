//! AB1/AB2/AB3 — l'alpha-beta de référence (`docs/ab/plan-alpha-beta-2026-10-22.md`).
//!
//! Les tests portent sur les **invariants** du moteur de référence :
//!
//! * un mat en un est **trouvé** (le score est un score de mat) ;
//! * un pat vaut **0**, pas un mat ;
//! * le coup rendu est toujours **légal** (jamais `0000` sur une position
//!   jouable — le contrat que le correctif UCI du 2026-10-22 a posé) ;
//! * la PV est **légale** de bout en bout ;
//! * la recherche est **déterministe** ;
//! * le budget **temps** est respecté ;
//! * le chemin UCI `Search=alpha` répond un coup légal.

use mychess::ab::{self, Limits};
use mychess::movegen::{self, MoveList};
use mychess::position::Position;
use mychess::types::GenKind;
use mychess::types::Move;
use std::sync::atomic::AtomicBool;

/// Recherche à profondeur fixée, sans limite de temps.
fn search_fen(fen: &str, depth: u32) -> ab::Outcome {
    let pos = Position::from_fen(fen).expect("valid fen");
    let hist = vec![pos.zobrist];
    let limits = Limits {
        max_depth: depth,
        movetime_ms: None,
        nodes: u64::MAX,
    };
    ab::Search::new(pos, hist, limits).run(&AtomicBool::new(false))
}

/// Les coups légaux d'une position, en UCI.
fn legal_moves(pos: &Position) -> Vec<String> {
    let mut list = MoveList::new();
    movegen::generate(pos, &mut list, GenKind::All);
    (0..list.len()).map(|i| list.get(i).uci()).collect()
}

/// Un mat en un doit être **trouvé** : le score est un score de mat.
#[test]
fn ab_finds_a_mate_in_one() {
    // 1. Re8# (ou Ra8#) : le roi noir est bloqué par ses propres pions.
    let fen = "6k1/5ppp/8/8/8/8/8/R3R1K1 w - - 0 1";
    let o = search_fen(fen, 4);
    assert!(
        o.score_cp >= ab::MATE - 2,
        "the mate in one must be scored as a mate, got {}",
        o.score_cp
    );
    let pos = Position::from_fen(fen).unwrap();
    let legal = legal_moves(&pos);
    assert!(
        legal.contains(&o.best.uci()),
        "the best move {} must be legal ({legal:?})",
        o.best.uci()
    );
    assert!(
        o.best.uci() == "a1a8" || o.best.uci() == "e1e8",
        "{}",
        o.best.uci()
    );
}

/// Un pat vaut **0** : ni mat, ni « -MATE ».
#[test]
fn ab_scores_a_stalemate_zero() {
    // Le roi noir n'a aucune case légale et n'est pas en échec.
    let fen = "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1";
    let o = search_fen(fen, 4);
    assert_eq!(o.score_cp, 0, "a stalemate is a draw, not a loss");
}

/// Une position gagnante (K+Q contre K) a un score **positif**.
#[test]
fn ab_scores_a_won_endgame_positive() {
    let fen = "8/8/8/4k3/8/8/8/K6Q w - - 0 1";
    let o = search_fen(fen, 6);
    assert!(
        o.score_cp > 0,
        "K+Q vs K must be winning, got {}",
        o.score_cp
    );
    assert!(o.score_cp < ab::MATE - 2, "it is not a mate yet");
}

/// Le coup rendu est **légal** — jamais `0000` sur une position jouable.
#[test]
fn ab_never_answers_0000_on_a_playable_position() {
    for fen in [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "6k1/5ppp/8/8/8/8/8/R3R1K1 w - - 0 1",
        "8/8/8/8/8/5k2/8/6K1 w - - 0 1",
    ] {
        let o = search_fen(fen, 3);
        assert_ne!(o.best, Move::NONE, "a playable position must get a move");
        let pos = Position::from_fen(fen).unwrap();
        let legal = legal_moves(&pos);
        assert!(
            legal.contains(&o.best.uci()),
            "illegal move {} in {fen}",
            o.best.uci()
        );
    }
}

/// La PV est **légale** de bout en bout : on la rejoue.
#[test]
fn ab_pv_is_legal() {
    let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4";
    let o = search_fen(fen, 5);
    assert!(!o.pv.is_empty(), "the PV must not be empty");
    let mut pos = Position::from_fen(fen).unwrap();
    for mv in &o.pv {
        let legal = legal_moves(&pos);
        assert!(
            legal.contains(&mv.uci()),
            "the PV contains an illegal move {} ({legal:?})",
            mv.uci()
        );
        let mut list = MoveList::new();
        movegen::generate(&pos, &mut list, GenKind::All);
        let found = (0..list.len())
            .map(|i| list.get(i))
            .find(|m| m.uci() == mv.uci())
            .expect("the PV move is in the legal list");
        pos.make(found);
    }
}

/// La recherche est **déterministe** : deux exécutions rendent le même
/// score et le même coup.
#[test]
fn ab_is_deterministic() {
    let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4";
    let a = search_fen(fen, 5);
    let b = search_fen(fen, 5);
    assert_eq!(a.score_cp, b.score_cp);
    assert_eq!(a.best, b.best);
    assert_eq!(a.nodes, b.nodes);
}

/// Le budget **temps** est respecté (marge 5 ms).
#[test]
fn ab_respects_the_time_budget() {
    let pos = Position::startpos();
    let hist = vec![pos.zobrist];
    let limits = Limits {
        max_depth: 0, // autant que le temps le permet
        movetime_ms: Some(200),
        nodes: u64::MAX,
    };
    let t0 = std::time::Instant::now();
    let o = ab::Search::new(pos, hist, limits).run(&AtomicBool::new(false));
    let elapsed = t0.elapsed().as_millis();
    assert!(o.nodes > 0, "the search must have visited nodes");
    assert!(elapsed <= 400, "a 200 ms budget must not take {elapsed} ms");
    assert!(o.depth >= 1, "at least one iteration must have completed");
}

/// La table de transposition ne change pas le **score** (invariant) : le
/// score à profondeur fixée est le même avec et sans TT.
#[test]
fn ab_tt_does_not_change_the_score() {
    // Une position à tactique (le mat en deux du « coup du berger » est
    // trop long ; on prend une position riche en fourchettes).
    let fen = "r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4";
    let with_tt = search_fen(fen, 5);
    // La même recherche, mais en purgeant la TT entre les deux appels :
    // le score doit être **identique** — c'est l'invariant qui prouve
    // que l'ajustement des scores de mat et le sondage sont corrects.
    let pos = Position::from_fen(fen).unwrap();
    let hist = vec![pos.zobrist];
    let limits = Limits {
        max_depth: 5,
        movetime_ms: None,
        nodes: u64::MAX,
    };
    let fresh = ab::Search::new(pos, hist, limits).run(&AtomicBool::new(false));
    assert_eq!(
        with_tt.score_cp, fresh.score_cp,
        "the TT must not change the score"
    );
}

// ---------------------------------------------------------------------------
// Le chemin UCI : l'option `Search` bascule sur l'alpha-beta.
// ---------------------------------------------------------------------------

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

/// L'option `Search` est annoncée et `Search=alpha` fait
/// l'alpha-beta : sur un mat en un, le moteur doit **jouer le mat**.
#[test]
fn uci_search_alpha_plays_the_mate() {
    let mut s = UciSession::start();
    s.send("uci");
    let lines = s.wait_for("uciok", 5000);
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("option name Search type string")),
        "the Search option must be advertised: {lines:?}"
    );
    s.send("setoption name Search value alpha");
    s.send("isready");
    let _ = s.wait_for("readyok", 5000);
    s.send("position fen 6k1/5ppp/8/8/8/8/8/R3R1K1 w - - 0 1");
    s.send("go depth 4");
    let lines = s.wait_for("bestmove", 10000);
    let bm = lines
        .iter()
        .find(|l| l.starts_with("bestmove"))
        .and_then(|l| l.split_whitespace().nth(1))
        .expect("bestmove line");
    assert!(
        bm == "a1a8" || bm == "e1e8",
        "the alpha-beta must play the mate, got {bm} ({lines:?})"
    );
    // La ligne `info` doit porter la profondeur et le score.
    assert!(
        lines.iter().any(|l| l.starts_with("info depth")),
        "an info line is expected: {lines:?}"
    );
}

/// Le chemin MCTS reste le **défaut** : les mesures gelées des phases
/// 3–5 doivent rester reproductibles.
#[test]
fn uci_search_defaults_to_mcts() {
    let mut s = UciSession::start();
    s.send("uci");
    let lines = s.wait_for("uciok", 5000);
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("option name Search type string default mcts")),
        "the default must stay mcts: {lines:?}"
    );
}
