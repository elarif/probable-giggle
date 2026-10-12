//! Perft: fast bulk-counting perft, detailed counters, divide, parallel.

use crate::movegen::{attackers_to, count_moves, generate, MoveList};
use crate::position::Position;
use crate::types::*;
use rayon::prelude::*;

/// Bulk-counting perft (single-threaded). Leaves at depth 1 are counted via
/// `count_moves` (popcount over target bitboards, L1), i.e. no per-leaf
/// make/unmake and no `MoveList` materialisation.
pub fn perft(pos: &mut Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    if depth == 1 {
        return count_moves(pos);
    }
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    let mut nodes = 0;
    for mv in list.iter() {
        let u = pos.make(mv);
        nodes += perft(pos, depth - 1);
        pos.unmake(mv, u);
    }
    nodes
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct PerftCounts {
    pub nodes: u64,
    pub captures: u64,
    pub ep: u64,
    pub castles: u64,
    pub promotions: u64,
    pub checks: u64,
    pub discovery_checks: u64,
    pub double_checks: u64,
    pub mates: u64,
}

impl PerftCounts {
    fn add(&mut self, o: &PerftCounts) {
        self.nodes += o.nodes;
        self.captures += o.captures;
        self.ep += o.ep;
        self.castles += o.castles;
        self.promotions += o.promotions;
        self.checks += o.checks;
        self.discovery_checks += o.discovery_checks;
        self.double_checks += o.double_checks;
        self.mates += o.mates;
    }
}

/// Exhaustive perft with per-category counters (counted at the leaves).
pub fn perft_detailed(pos: &mut Position, depth: u32) -> PerftCounts {
    let mut c = PerftCounts::default();
    if depth == 0 {
        c.nodes = 1;
        return c;
    }
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    let us = pos.side;
    for mv in list.iter() {
        let is_cap = mv.is_capture();
        let is_ep = mv.is_ep();
        let is_castle = mv.is_castle();
        let is_promo = mv.is_promotion();
        let u = pos.make(mv);
        if depth == 1 {
            c.nodes += 1;
            if is_cap {
                c.captures += 1;
            }
            if is_ep {
                c.ep += 1;
            }
            if is_castle {
                c.castles += 1;
            }
            if is_promo {
                c.promotions += 1;
            }
            let opp = us.opp();
            if pos.in_check(opp) {
                c.checks += 1;
                let att = attackers_to(pos, pos.king_sq(opp), us);
                if att.count_ones() >= 2 {
                    c.double_checks += 1;
                } else if att != (1u64 << mv.to()) {
                    c.discovery_checks += 1;
                }
                let mut replies = MoveList::new();
                generate(pos, &mut replies, GenKind::All);
                if replies.is_empty() {
                    c.mates += 1;
                }
            }
        } else {
            let sub = perft_detailed(pos, depth - 1);
            c.add(&sub);
        }
        pos.unmake(mv, u);
    }
    c
}

fn perft_seq(pos: &mut Position, depth: u32) -> u64 {
    perft(pos, depth)
}

fn perft_par(pos: &Position, depth: u32, split: u32) -> u64 {
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    if depth == 1 {
        return list.len() as u64;
    }
    if split > 0 && list.len() >= 2 {
        let moves: Vec<Move> = list.iter().collect();
        moves
            .par_iter()
            .map(|&mv| {
                let mut p = *pos;
                let u = p.make(mv);
                let n = perft_par(&p, depth - 1, split - 1);
                p.unmake(mv, u);
                n
            })
            .sum()
    } else {
        let mut p = *pos;
        let mut n = 0;
        for mv in list.iter() {
            let u = p.make(mv);
            n += perft_seq(&mut p, depth - 1);
            p.unmake(mv, u);
        }
        n
    }
}

/// Parallel perft: divide at the root and recurse via rayon for the first
/// `SPLIT_DEPTH` plies to keep the ~20 root tasks from starving the pool.
pub fn perft_parallel(pos: &Position, depth: u32, threads: usize) -> u64 {
    if depth <= 1 {
        let mut p = *pos;
        return perft(&mut p, depth);
    }
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .build()
        .expect("thread pool");
    pool.install(|| perft_par(pos, depth, 2))
}

/// `divide`: one line per root move.
pub fn divide(pos: &mut Position, depth: u32) -> u64 {
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    let mut total = 0;
    for mv in list.iter() {
        let u = pos.make(mv);
        let n = if depth <= 1 { 1 } else { perft(pos, depth - 1) };
        pos.unmake(mv, u);
        println!("{}: {}", mv.uci(), n);
        total += n;
    }
    println!("total: {total}");
    total
}

/// `divide --depth2`: root moves then each reply, with counts.
pub fn divide2(pos: &mut Position, depth: u32) -> u64 {
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    let mut total = 0;
    for mv in list.iter() {
        let u = pos.make(mv);
        let mut sub = MoveList::new();
        generate(pos, &mut sub, GenKind::All);
        let sub_total: u64 = if depth <= 2 {
            sub.len() as u64
        } else {
            let mut t = 0;
            for mv2 in sub.iter() {
                let u2 = pos.make(mv2);
                let n = perft(pos, depth - 2);
                pos.unmake(mv2, u2);
                println!("{} {}: {}", mv.uci(), mv2.uci(), n);
                t += n;
            }
            t
        };
        pos.unmake(mv, u);
        println!("{}: {}", mv.uci(), sub_total);
        total += sub_total;
    }
    println!("total: {total}");
    total
}
