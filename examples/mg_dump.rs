//! Vidage des listes de coups par mode (diagnostic temporaire).
//!
//! ```text
//! cargo run --release --example mg_dump -- "<fen>"
//! ```

use mychess::movegen::{MoveList, generate};
use mychess::position::Position;
use mychess::types::GenKind;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fen = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "8/8/8/4k3/8/8/8/K6Q w - - 0 1".to_string());
    let pos = Position::from_fen(&fen).unwrap();
    for (name, kind) in [
        ("All", GenKind::All),
        ("CapturesOnly", GenKind::CapturesOnly),
        ("QuietsOnly", GenKind::QuietsOnly),
    ] {
        let mut l = MoveList::new();
        generate(&pos, &mut l, kind);
        let moves: Vec<String> = l.iter().map(|m| m.uci()).collect();
        println!("{name} ({}) : {} moves", l.len(), moves.join(" "));
    }
}
