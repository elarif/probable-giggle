//! Vidage de la recherche alpha-beta (diagnostic temporaire).
//!
//! ```text
//! cargo run --release --example ab_dump -- "<fen>" [profondeur]
//! ```

use mychess::ab::{self, Limits};
use mychess::position::Position;
use std::sync::atomic::AtomicBool;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fen = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "8/8/8/4k3/8/8/8/K6Q w - - 0 1".to_string());
    let max = args.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(6);
    let pos = Position::from_fen(&fen).unwrap();
    let hist = vec![pos.zobrist];
    for d in 1..=max {
        let limits = Limits {
            max_depth: d,
            movetime_ms: None,
            nodes: u64::MAX,
        };
        let o = ab::Search::new(pos, hist.clone(), limits).run(&AtomicBool::new(false));
        let pv: Vec<String> = o.pv.iter().map(|m| m.uci()).collect();
        println!(
            "profondeur {d} : score {} cp, coup {}, noeuds {}, pv [{}]",
            o.score_cp,
            o.best.uci(),
            o.nodes,
            pv.join(" ")
        );
    }
}
