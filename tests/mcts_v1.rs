//! MCTS v1 — correctness net (STEP 0). No performance measurement here:
//! the BENCH-FREEZE holds until M4-d9 reconciles; these are legality /
//! terminality / determinism / protocol gates.

use mychess::mcts::{Limits, Rng, Search, terminal_value};
use mychess::movegen::{MoveList, generate};
use mychess::position::Position;
use mychess::types::{GenKind, Move};
use std::sync::atomic::AtomicBool;

fn limits(nodes: u64, _seed: u64) -> Limits {
    Limits {
        nodes,
        movetime_ms: None,
        max_playout_ply: 100,
        c_uct: std::f32::consts::SQRT_2,
    }
}

/// The MCTS plays only legal moves: on every golden position, a short search
/// must return a move present in the legal root list (or 0000 if terminal).
#[test]
fn v1_bestmove_is_legal_on_golden_positions() {
    for fen in [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    ] {
        let pos = Position::from_fen(fen).unwrap();
        let search = Search::new(pos, vec![pos.zobrist], limits(300, 42), 42);
        let o = search.run(&AtomicBool::new(false));
        if o.best == Move::NONE {
            // only acceptable if the position is terminal at the root
            assert!(terminal_value(&pos, &[pos.zobrist]).is_some(), "{fen}");
            continue;
        }
        assert!(
            o.best.uci() != "0000",
            "{fen}: empty bestmove on a non-terminal position"
        );
        let u = o.best.uci();
        let mut found = false;
        let mut list = MoveList::new();
        generate(&pos, &mut list, GenKind::All);
        for i in 0..list.len() {
            if list.get(i).uci() == u {
                found = true;
            }
        }
        assert!(found, "{fen}: bestmove {u} not in the legal root list");
    }
}

/// A1's future home is exercised here as a sign-convention gate: the v1 UCT
/// with random playouts must find a mate-in-1 (KQK corner, two mate candidates;
/// the search must return one of them and report an overwhelming score).
#[test]
fn v1_finds_mate_in_1() {
    let fen = "k7/8/KQ6/8/8/8/8/8 w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    // the set of moves that mate immediately (core oracled)
    let mut mates: Vec<String> = Vec::new();
    {
        let mut list = MoveList::new();
        generate(&pos, &mut list, GenKind::All);
        for i in 0..list.len() {
            let mut p = pos;
            p.make(list.get(i));
            let mut replies = MoveList::new();
            generate(&p, &mut replies, GenKind::All);
            if replies.is_empty() && p.in_check(p.side) {
                mates.push(list.get(i).uci());
            }
        }
    }
    assert!(!mates.is_empty(), "test position must contain a mate in 1");
    let search = Search::new(pos, vec![pos.zobrist], limits(3000, 42), 42);
    let o = search.run(&AtomicBool::new(false));
    let b = o.best.uci();
    assert!(
        mates.contains(&b),
        "mate-in-1 not found: best={b} (candidates {mates:?}), score={}",
        o.score_cp
    );
    assert!(
        o.score_cp >= 90,
        "mated line must score ~+100cp, got {}",
        o.score_cp
    );
}

/// Terminality frozen: checkmate (-1 mover view), stalemate (0), 50-move,
/// triple repetition (two shuffle cycles back to startpos), insufficient
/// material (KN vs K).
#[test]
fn v1_terminality_frozen() {
    // Checkmate: black mated, mover value -1.
    let mate_fen = "R6k/8/7K/8/8/8/8/8 b - - 0 1";
    let pos = Position::from_fen(mate_fen).unwrap();
    assert_eq!(
        terminal_value(&pos, &[pos.zobrist]),
        Some(-1.0),
        "checkmate"
    );

    // Stalemate (classic Qf7/Kg6 corner): 0.
    let stale_fen = "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1";
    let pos = Position::from_fen(stale_fen).unwrap();
    assert_eq!(terminal_value(&pos, &[pos.zobrist]), Some(0.0), "stalemate");

    // Triple repetition: replay two reversible 4-ply cycles (Nf3/Nf6/Ng1/Ng8).
    let start = Position::startpos();
    let mut hist: Vec<u64> = vec![start.zobrist];
    let mut p = start;
    for _ in 0..2 {
        for uci in ["g1f3", "g8f6", "f3g1", "f6g8"] {
            let mut list = MoveList::new();
            generate(&p, &mut list, GenKind::All);
            for i in 0..list.len() {
                if list.get(i).uci() == uci {
                    p.make(list.get(i));
                    break;
                }
            }
            hist.push(p.zobrist);
        }
        if p.zobrist == start.zobrist {
            // first cycle: current position == start (2nd occurrence): not a
            // draw yet; second cycle: 3rd occurrence: draw.
        }
    }
    assert_eq!(p.zobrist, start.zobrist, "cycle must return to start");
    // current (3rd occurrence) must draw (mover = white again, halfmove=8).
    assert_eq!(terminal_value(&p, &hist), Some(0.0), "triple repetition");
    // Half-window sanity: 1st cycle (2nd occurrence) must NOT draw.
    let mut p2 = start;
    let mut hist2: Vec<u64> = vec![p2.zobrist];
    for uci in ["g1f3", "g8f6", "f3g1", "f6g8"] {
        let mut list = MoveList::new();
        generate(&p2, &mut list, GenKind::All);
        for i in 0..list.len() {
            if list.get(i).uci() == uci {
                p2.make(list.get(i));
                break;
            }
        }
        hist2.push(p2.zobrist);
    }
    assert_eq!(
        terminal_value(&p2, &hist2),
        None,
        "2nd occurrence is not a draw"
    );

    // Insufficient material: KN vs K with white to move: draw.
    let kn_fen = "8/8/3k4/8/8/8/4N3/4K3 w - - 0 1";
    let pos = Position::from_fen(kn_fen).unwrap();
    assert_eq!(terminal_value(&pos, &[pos.zobrist]), Some(0.0), "KN vs K");

    // 50-move rule: halfmove >= 100 draws (kings d5 / d2: legal, separated).
    let fifty_fen = "8/8/8/3k4/8/8/3K4/8 w - - 100 60";
    let pos = Position::from_fen(fifty_fen).unwrap();
    assert_eq!(terminal_value(&pos, &[pos.zobrist]), Some(0.0), "50-move");
}

/// Determinism: same seed + same limits => identical bestmove and score.
#[test]
fn v1_deterministic_per_seed() {
    let pos = Position::from_fen(
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    )
    .unwrap();
    let run = |seed: u64| {
        let search = Search::new(pos, vec![pos.zobrist], limits(500, seed), seed);
        let o = search.run(&AtomicBool::new(false));
        (o.best.0, o.score_cp, o.nodes)
    };
    assert_eq!(run(7), run(7), "same seed must be fully reproducible");
    assert_ne!(
        run(7),
        run(8),
        "different seeds should differ (almost surely)"
    );
}

/// Eval sanity: white-positive evals, symmetric mirror consistency and the
/// A1 hook (mover-positive).
#[test]
fn eval_material_and_pst_signs() {
    let start = Position::startpos();
    assert_eq!(mychess::eval::eval_white(&start), 0, "startpos is balanced");
    assert_eq!(mychess::eval::eval_for_mover(&start), 0);
    // White up a black queen (both sides keep pawns): material +900, PST ~0.
    let up =
        Position::from_fen("rnb1kbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
    assert!(
        mychess::eval::eval_white(&up) > 880,
        "{}",
        mychess::eval::eval_white(&up)
    );
    // Same position, black to move: mover-positive flips the sign.
    let up_black =
        Position::from_fen("rnb1kbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1").unwrap();
    assert!(mychess::eval::eval_for_mover(&up_black) < -880);
}

/// RNG sanity: splitmix-style deterministic stream, zero-seed safe.
#[test]
fn rng_deterministic() {
    let mut a = Rng::new(1);
    let mut b = Rng::new(1);
    for _ in 0..100 {
        assert_eq!(a.next_u64(), b.next_u64());
    }
}
