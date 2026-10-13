//! Root-split PARALLEL perft_detailed runner (GATE 2.5 → phase MCTS bridging).
//!
//! M4 (startpos d9 `--detailed`) was launched single-threaded: `perft_detailed`
//! has no rayon path, so d9 (~2.44e12 leaves, ~25-30 ns/leaf) would run ~20 h
//! serial. This example mirrors `perft::perft_parallel`'s structure but
//! accumulates `PerftCounts` instead of node counts — NO core modification:
//! it consumes `perft::perft_detailed` and the frozen core via the public API
//! only. Counter semantics stay identical by construction (same recursion,
//! counters summed over independent root subtrees, validated against the
//! serial routine and the golden columns before any long run).
//!
//! Usage: `mychess-d9detailed <fen> <depth> <threads> [split]`
//! Prints the same table shape as `mychess perft <fen> <depth> --detailed`.

use mychess::movegen::{MoveList, generate};
use mychess::perft::{PerftCounts, perft_detailed};
use mychess::position::Position;
use mychess::types::{GenKind, Move};
use rayon::prelude::*;
use std::time::Instant;

fn merge_counts(mut a: PerftCounts, b: PerftCounts) -> PerftCounts {
    a.nodes += b.nodes;
    a.captures += b.captures;
    a.ep += b.ep;
    a.castles += b.castles;
    a.promotions += b.promotions;
    a.checks += b.checks;
    a.discovery_checks += b.discovery_checks;
    a.double_checks += b.double_checks;
    a.mates += b.mates;
    a
}

fn counts_par(pos: &Position, depth: u32, split: u32) -> PerftCounts {
    if depth == 0 {
        return PerftCounts {
            nodes: 1,
            ..PerftCounts::default()
        };
    }
    if depth == 1 || split == 0 {
        let mut p = *pos;
        return perft_detailed(&mut p, depth);
    }
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    let moves: Vec<Move> = list.iter().collect();
    moves
        .par_iter()
        .map(|&mv| {
            let mut p = *pos;
            let u = p.make(mv);
            let sub = counts_par(&p, depth - 1, split - 1);
            p.unmake(mv, u);
            sub
        })
        .reduce(PerftCounts::default, merge_counts)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: d9detailed <fen> <depth> <threads> [split]");
        std::process::exit(2);
    }
    let fen = args[1].clone();
    let depth: u32 = args[2].parse().expect("depth");
    let threads: usize = args[3].parse().expect("threads");
    let split: u32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(2);
    eprintln!("[{}]", mychess::slider::describe());
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .build()
        .unwrap();
    pool.install(|| {
        let pos = Position::from_fen(&fen).unwrap();
        let t = Instant::now();
        let c = counts_par(&pos, depth, split);
        let s = t.elapsed().as_secs_f64();
        println!("depth {depth}");
        println!("nodes            {:>18}", c.nodes);
        println!("captures         {:>18}", c.captures);
        println!("ep               {:>18}", c.ep);
        println!("castles          {:>18}", c.castles);
        println!("promotions       {:>18}", c.promotions);
        println!("checks           {:>18}", c.checks);
        println!("discovery_checks {:>18}", c.discovery_checks);
        println!("double_checks    {:>18}", c.double_checks);
        println!("mates            {:>18}", c.mates);
        println!("[{:.2}s, {:.1} Mnodes/s]", s, c.nodes as f64 / s / 1e6);
    });
}
