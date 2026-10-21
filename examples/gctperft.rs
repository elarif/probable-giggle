//! Mesure du perft `gct` : `cargo run --release --example gctperft -- <fen> <depth>`

use mychess::gct::perft::perft;
use mychess::position::Position;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: gctperft <fen> <depth>");
        std::process::exit(2);
    }
    // La table GCT publie Kiwipete sans demi-coup ni compteur de coups, alors
    // que `Position::from_fen` exige le demi-coup. On complète les deux champs
    // neutres, comme les goldens (`0 1`) : ils n'ont aucun effet sur le perft.
    let mut fen = args[1].clone();
    if fen.split_whitespace().count() == 4 {
        fen.push_str(" 0 1");
    }
    let mut pos = match Position::from_fen(&fen) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("FEN invalide : {e:?}");
            std::process::exit(2);
        }
    };
    let depth: u32 = args[2].parse().expect("depth");
    let t = Instant::now();
    let n = perft(&mut pos, depth);
    let s = t.elapsed().as_secs_f64();
    println!(
        "gct perft({depth}) = {n}  [{:.2}s, {:.1} Mnodes/s]",
        s,
        n as f64 / s / 1e6
    );
}
