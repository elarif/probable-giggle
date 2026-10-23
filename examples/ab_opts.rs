//! Vidage des scores par configuration d'options (diagnostic temporaire).
//!
//! ```text
//! cargo run --release --example ab_opts -- "<fen>" [profondeur]
//! ```

use mychess::ab::{self, Limits, Options};
use mychess::position::Position;
use std::sync::atomic::AtomicBool;

fn run(fen: &str, depth: u32, opts: Options, label: &str) {
    let pos = Position::from_fen(fen).unwrap();
    let hist = vec![pos.zobrist];
    let limits = Limits {
        max_depth: depth,
        movetime_ms: None,
        nodes: u64::MAX,
    };
    let o = ab::Search::with_options(pos, hist, limits, opts).run(&AtomicBool::new(false));
    let pv: Vec<String> = o.pv.iter().map(|m| m.uci()).collect();
    println!(
        "{label:<12} score {:>6} cp, coup {}, noeuds {:>7}, pv [{}]",
        o.score_cp,
        o.best.uci(),
        o.nodes,
        pv.join(" ")
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fen = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "4k3/8/8/3q4/4P3/8/8/4K3 w - - 0 1".to_string());
    let depth = args.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(4);
    let d = Options::default();
    run(&fen, depth, d, "defaut");
    run(
        &fen,
        depth,
        Options {
            razoring: true,
            ..d
        },
        "razoring",
    );
    run(
        &fen,
        depth,
        Options {
            futility: true,
            ..d
        },
        "futility",
    );
    run(
        &fen,
        depth,
        Options {
            lmr: true,
            ..d
        },
        "lmr",
    );
    run(
        &fen,
        depth,
        Options {
            null_move: true,
            ..d
        },
        "null-move",
    );
}
