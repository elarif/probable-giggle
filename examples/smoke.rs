//! Dev smoke for the MCTS v1 (no timing display — correctness only).
//! Usage: smoke [nodes] [seed]

use mychess::mcts::{Limits, Search};
use mychess::position::Position;
use std::hint::black_box;
use std::sync::atomic::AtomicBool;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let nodes: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(400);
    let seed: u64 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x1F2E_3D4C_5B6A_7988);
    let stop = AtomicBool::new(false);
    let search = Search::new(
        black_box(Position::startpos()),
        vec![Position::startpos().zobrist],
        Limits {
            nodes,
            movetime_ms: None,
            max_playout_ply: 100,
            c_uct: std::f32::consts::SQRT_2,
        },
        seed,
    );
    let o = search.run(&stop);
    let best_str = if o.best == mychess::types::Move::NONE {
        "0000".to_string()
    } else {
        o.best.uci()
    };
    println!(
        "best={best_str} nodes={} score_cp={} elapsed_ms={}",
        o.nodes, o.score_cp, o.elapsed_ms
    );
}
