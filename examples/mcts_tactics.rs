//! Phase 4 tactics probe — **at a constant node budget**, how many forced
//! tactical shots does each selection rule still find?
//!
//! The phase-3 quality probe only counted **mates in one**. That metric is
//! too coarse to separate two selection rules (both find the three mates
//! of the battery). This probe uses a wider **tactics battery** whose
//! solution is computed by an **oracle**, not by a human annotation:
//!
//! * a **mate** — the move leaves the opponent with no legal reply and in
//!   check;
//! * a **safe material win** — the move captures or promotes, gains at
//!   least 300 centipions of material, and the opponent has **no** legal
//!   reply that recaptures the moved piece on its new square (the win is
//!   not refuted).
//!
//! The engine's best move is then scored: hit (the move is in the winning
//! set) or miss. The probe prints the **hit rate** per configuration, and
//! both selection rules (`uct`, `puct`) run **in the same session**, so
//! the comparison is not polluted by a machine that drifted between runs.
//!
//! ```text
//! cargo run --release --example mcts_tactics -- <nodes> [threads...]
//! ```

use mychess::eval;
use mychess::mcts::{Limits, Policy, Search};
use mychess::movegen::{MoveList, generate};
use mychess::position::Position;
use mychess::types::{GenKind, Move};
use std::sync::atomic::AtomicBool;

/// The tactics battery. Every position holds at least one **forced** win
/// (a mate or a safe material win) — the probe prints the size of the
/// winning set, so a position whose oracle answer is empty is a bug of the
/// battery and shows up immediately.
const BATTERY: &[&str] = &[
    // --- mates in one -----------------------------------------------------
    "6k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1",  // Ra8#
    "7k/8/5KQ1/8/8/8/8/8 w - - 0 1",         // Qg7#
    "3k4/8/3K4/8/8/8/8/6Q1 w - - 0 1",       // Qg8#
    "k7/8/1K6/8/8/8/8/7Q w - - 0 1",         // Qh8#
    "k7/8/KQ6/8/8/8/8/8 w - - 0 1",          // 3 mates
    "6k1/5ppp/8/8/8/8/5PPP/4R1K1 w - - 0 1", // Re8#
    // --- free queen -------------------------------------------------------
    "4k3/8/8/3q4/8/8/8/3RK3 w - - 0 1",  // Rxd5
    "4k3/8/8/3q4/8/8/8/3R1K2 w - - 0 1", // Rxd5
    "4k3/8/8/3q4/8/8/8/3QK3 w - - 0 1",  // Qxd5
    "4k3/8/2q5/1P6/8/8/8/4K3 w - - 0 1", // bxc6
    "4k3/8/8/3q4/5N2/8/8/4K3 w - - 0 1", // Nxd5
    "4k3/8/8/3q4/2P5/8/8/4K3 w - - 0 1", // cxd5
    "r3k3/8/8/3q4/8/8/8/3RK3 w - - 0 1", // Rxd5
    // --- free rook --------------------------------------------------------
    "4k3/8/8/3r4/8/8/8/3RK3 w - - 0 1",  // Rxd5
    "4k3/8/8/3r4/8/8/8/3R1K2 w - - 0 1", // Rxd5
    "4k3/8/8/3r4/5N2/8/8/4K3 w - - 0 1", // Nxd5
    "4k3/8/8/3r4/8/5B2/8/4K3 w - - 0 1", // Bxd5
    // --- free promotion ---------------------------------------------------
    "4k3/1P6/8/8/8/8/8/4K3 w - - 0 1", // b8=Q
    "4k3/2P5/8/8/8/8/8/4K3 w - - 0 1", // c8=Q
    "4k3/P7/8/8/8/8/8/4K3 w - - 0 1",  // a8=Q
    // --- a hanging minor piece --------------------------------------------
    "4k3/8/8/3n4/8/8/8/3RK3 w - - 0 1", // Rxd5 (knight)
    "4k3/8/8/3b4/8/8/8/3RK3 w - - 0 1", // Rxd5 (bishop)
];

fn legal_moves(pos: &Position) -> Vec<Move> {
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    list.iter().collect()
}

/// The material of one side, in centipawns (absolute, not relative).
fn material_of(pos: &Position, side: usize) -> i32 {
    let mut acc = 0;
    for kind in 0..6 {
        let n = (pos.bb_piece[kind] & pos.bb_color[side]).count_ones() as i32;
        acc += n * eval::material(kind);
    }
    acc
}

/// The set of **forced** wins of a position, computed by the oracle.
///
/// * the mating moves;
/// * the safe material wins: the move gains at least `GAIN_MIN`
///   centipions of material and the opponent cannot recapture the moved
///   piece on its new square.
fn winning_moves(pos: &Position) -> Vec<Move> {
    let us = pos.side.idx();
    let them = us ^ 1;
    // The **swing** of the move: what the mover gains (a promotion) plus
    // what the opponent loses (a capture). Capturing a queen does not
    // change the mover's own material, it removes the opponent's; a
    // promotion does the opposite.
    let mover_before = material_of(pos, us);
    let them_before = material_of(pos, them);
    let mut out = Vec::new();
    for mv in legal_moves(pos) {
        let mut p = *pos;
        p.make(mv);
        // A mate: no legal reply, and the opponent is in check.
        if legal_moves(&p).is_empty() && p.in_check(p.side) {
            out.push(mv);
            continue;
        }
        // A safe material win: the swing is at least `GAIN_MIN`, and the
        // opponent cannot recapture the moved piece.
        let gain = (material_of(&p, us) - mover_before) + (them_before - material_of(&p, them));
        if gain < GAIN_MIN {
            continue;
        }
        let recaptured = legal_moves(&p)
            .into_iter()
            .any(|r| r.to() == mv.to() && r.is_capture());
        if !recaptured {
            out.push(mv);
        }
    }
    out
}

/// The minimum material gain (centipawns) of a "safe material win".
const GAIN_MIN: i32 = 300;

fn policy_name(p: Policy) -> &'static str {
    match p {
        Policy::Uct => "uct",
        Policy::Puct => "puct",
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let nodes: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10_000);
    let threads: Vec<usize> = args.iter().skip(2).filter_map(|s| s.parse().ok()).collect();
    let threads = if threads.is_empty() { vec![1] } else { threads };

    // The oracle is computed once, before any search.
    let mut positions = Vec::new();
    for fen in BATTERY {
        let pos = Position::from_fen(fen).expect("valid FEN");
        let wins = winning_moves(&pos);
        assert!(
            !wins.is_empty(),
            "the battery position {fen} has no forced win: the FEN is wrong"
        );
        positions.push((fen, pos, wins));
    }

    println!("# nodes budget: {nodes}");
    println!("# fen | #wins | hit rate and score per configuration");
    let mut hits = [[0usize; 8]; 2];
    let mut totals = [[0usize; 8]; 2];
    for (fen, pos, wins) in &positions {
        print!("{fen} | {} |", wins.len());
        for (pi, policy) in [Policy::Uct, Policy::Puct].iter().enumerate() {
            for (ti, t) in threads.iter().enumerate() {
                let limits = Limits {
                    nodes,
                    movetime_ms: None,
                    max_playout_ply: 100,
                    c_uct: std::f32::consts::SQRT_2,
                    c_puct: mychess::mcts::DEFAULT_C_PUCT,
                    root_noise: mychess::mcts::DEFAULT_ROOT_NOISE,
                    leaf_q: 0,
                };
                let search = Search::new(*pos, vec![pos.zobrist], limits, 42)
                    .with_threads(*t)
                    .with_policy(*policy);
                let o = search.run(&AtomicBool::new(false));
                let hit = wins.contains(&o.best);
                hits[pi][ti] += usize::from(hit);
                totals[pi][ti] += 1;
                print!(
                    " {}/{}=T{t}:{}({})",
                    policy_name(*policy),
                    ti,
                    if hit { "HIT" } else { "miss" },
                    o.score_cp
                );
            }
        }
        println!();
    }
    println!("# hit rate per configuration");
    for (pi, policy) in [Policy::Uct, Policy::Puct].iter().enumerate() {
        for (ti, t) in threads.iter().enumerate() {
            println!(
                "# {}/{ti} (threads {t}): {}/{}",
                policy_name(*policy),
                hits[pi][ti],
                totals[pi][ti]
            );
        }
    }
}
