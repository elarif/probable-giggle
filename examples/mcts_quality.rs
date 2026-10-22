//! Phase 4 quality probe — **at a constant node budget**, what does the
//! selection rule change?
//!
//! The probe measures the same thing as the phase-3 one, but for the two
//! **selection rules** of the engine:
//!
//! * `uct` — the frozen v1 rule (uniform draw among the unvisited
//!   children, UCT among the visited ones);
//! * `puct` — the phase-4 rule (PUCT with the MVV-LVA priors).
//!
//! For each position of the battery, the set of **mating moves** is
//! computed by an oracle (generate the legal moves, play them, and look for
//! a position with no legal reply and the mover in check); the engine is
//! then run with a fixed node budget, in mono and in parallel, and the
//! probe reports the fraction of positions where the engine's best move is
//! a mate, plus the score.
//!
//! Both policies run **in the same session**, so the comparison is not
//! polluted by a machine that drifted between two runs.
//!
//! ```text
//! cargo run --release --example mcts_quality -- <nodes> [threads...] [leafq:d,...]
//! ```
//!
//! `leafq:d` (phase 5) adds the depth of the tactical resolution of the
//! leaf as a configuration dimension (`0` = the static evaluation alone,
//! the phase-4 behaviour).

use mychess::mcts::{Limits, Policy, Search};
use mychess::movegen::{MoveList, generate};
use mychess::position::Position;
use mychess::types::{GenKind, Move};
use std::sync::atomic::AtomicBool;

/// Positions of the battery: a mix of opening positions, middlegames and
/// short tactical shots. Several of them hold a mate in one.
const BATTERY: &[&str] = &[
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
    "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 b - - 0 1",
    "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5Q2/PPPP1PPP/RNB1K1NR w KQkq - 0 1",
    "2rr3k/pp3pp1/1nnqbN1p/3pN3/2pP4/2P3Q1/PPB4P/R4RK1 w - - 0 1",
    "8/8/8/8/8/6k1/6p1/6K1 w - - 0 1",
    "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1",
    "8/5p2/8/2k3P1/p3K3/8/1P6/8 b - - 0 1",
    "6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1",
    "7k/8/KQ6/8/8/8/8/8 w - - 0 1",
    "k7/8/KQ6/8/8/8/8/8 w - - 0 1",
    "8/8/8/8/8/1k6/2q5/K7 w - - 0 1",
];

fn legal_moves(pos: &Position) -> Vec<Move> {
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    list.iter().collect()
}

/// The set of moves that mate immediately (oracle: play the move, look for a
/// position with no legal reply and the side to move in check).
fn mating_moves(pos: &Position) -> Vec<Move> {
    let mut mates = Vec::new();
    for mv in legal_moves(pos) {
        let mut p = *pos;
        p.make(mv);
        if legal_moves(&p).is_empty() && p.in_check(p.side) {
            mates.push(mv);
        }
    }
    mates
}

fn policy_name(p: Policy) -> &'static str {
    match p {
        Policy::Uct => "uct",
        Policy::Puct => "puct",
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let nodes: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(50_000);
    let threads: Vec<usize> = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .map(|t| vec![t])
        .unwrap_or_else(|| vec![1, 2, 4, 6]);
    // Phase 5: the depths of the tactical resolution of the leaf.
    let leaf_qs: Vec<u32> = args
        .iter()
        .find_map(|s| s.strip_prefix("leafq:"))
        .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![0]);

    println!("# nodes budget: {nodes}");
    println!("# fen | mates | score per configuration");
    for fen in BATTERY {
        let pos = Position::from_fen(fen).expect("valid FEN");
        let mates = mating_moves(&pos);
        for policy in [Policy::Uct, Policy::Puct] {
            for leaf_q in &leaf_qs {
                let mut line = format!(
                    "{fen} | {} | {} q{leaf_q}",
                    mates.len(),
                    policy_name(policy)
                );
                for t in &threads {
                    let limits = Limits {
                        nodes,
                        movetime_ms: None,
                        max_playout_ply: 100,
                        c_uct: std::f32::consts::SQRT_2,
                        c_puct: mychess::mcts::DEFAULT_C_PUCT,
                        root_noise: mychess::mcts::DEFAULT_ROOT_NOISE,
                        leaf_q: *leaf_q,
                    };
                    let search = Search::new(pos, vec![pos.zobrist], limits, 42)
                        .with_threads(*t)
                        .with_policy(policy);
                    let o = search.run(&AtomicBool::new(false));
                    let found = !mates.is_empty() && mates.contains(&o.best);
                    line += &format!(
                        " T={t}:{} ({})",
                        if found { "MATE" } else { "----" },
                        o.score_cp
                    );
                }
                println!("{line}");
            }
        }
    }
}
