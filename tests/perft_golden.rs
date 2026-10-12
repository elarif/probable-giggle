//! Golden perft files (source: https://www.chessprogramming.org/Perft_Results).
//!
//! Tiers:
//!   T1  (CI, fast)      : nodes + available detail columns, depth <= 5
//!   T2  (nightly/opt-in): depth 6..=8 nodes, detail columns depth 6
//!   T3  (opt-in, long)  : startpos depth 9 (parallel)

use mychess::perft::{perft, perft_detailed, perft_parallel, PerftCounts};
use mychess::position::Position;

struct Row {
    depth: u32,
    nodes: u64,
    // captures, ep, castles, promotions, checks, discovery, double, mates
    cols: [Option<u64>; 8],
}

struct Golden {
    fen: String,
    rows: Vec<Row>,
}

fn load(path: &str) -> Golden {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut fen = String::new();
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if fen.is_empty() && line.contains('/') {
            fen = line.to_string();
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        assert!(toks.len() >= 2, "bad golden row: {line}");
        let depth: u32 = toks[0].parse().unwrap();
        let nodes: u64 = toks[1].parse().unwrap();
        let mut cols = [None; 8];
        for (i, c) in cols.iter_mut().enumerate() {
            if let Some(t) = toks.get(2 + i).filter(|t| **t != "-") {
                *c = Some(t.replace('_', "").parse().unwrap());
            }
        }
        rows.push(Row { depth, nodes, cols });
    }
    assert!(!fen.is_empty(), "{path}: no FEN");
    Golden { fen, rows }
}

fn all() -> Vec<Golden> {
    [
        "tests/perft/startpos.txt",
        "tests/perft/kiwipete.txt",
        "tests/perft/pos3.txt",
        "tests/perft/pos4.txt",
        "tests/perft/pos5.txt",
        "tests/perft/pos6.txt",
    ]
    .iter()
    .map(|p| load(p))
    .collect()
}

fn check_detail(g: &Golden, row: &Row) {
    let mut pos = Position::from_fen(&g.fen).unwrap();
    let c: PerftCounts = perft_detailed(&mut pos, row.depth);
    assert_eq!(c.nodes, row.nodes, "{} d{} nodes", g.fen, row.depth);
    let got = [
        c.captures,
        c.ep,
        c.castles,
        c.promotions,
        c.checks,
        c.discovery_checks,
        c.double_checks,
        c.mates,
    ];
    let names = [
        "captures", "ep", "castles", "promotions", "checks", "discovery", "double", "mates",
    ];
    for i in 0..8 {
        if let Some(want) = row.cols[i] {
            assert_eq!(got[i], want, "{} d{} {}", g.fen, row.depth, names[i]);
        }
    }
}

#[test]
fn t1_perft_nodes() {
    // T1 (CI): node totals up to depth 5, bulk-counting.
    for g in all() {
        for row in &g.rows {
            if row.depth > 5 {
                continue;
            }
            let mut pos = Position::from_fen(&g.fen).unwrap();
            assert_eq!(perft(&mut pos, row.depth), row.nodes, "{} d{}", g.fen, row.depth);
        }
    }
}

#[test]
fn t1_details_shallow() {
    // Detailed columns for shallow depths (fast, exhaustive classification).
    for g in all() {
        for row in &g.rows {
            if row.depth > 4 {
                continue;
            }
            if row.cols.iter().all(|c| c.is_none()) {
                continue;
            }
            check_detail(&g, row);
        }
    }
}

#[test]
fn t1_nodes_vs_refgen_small() {
    // Cross-check the fast perft against the naive oracle at shallow depths.
    for g in all() {
        for row in &g.rows {
            if row.depth > 3 {
                continue;
            }
            let mut pos = Position::from_fen(&g.fen).unwrap();
            assert_eq!(perft(&mut pos, row.depth), row.nodes);
            let mut pos2 = Position::from_fen(&g.fen).unwrap();
            assert_eq!(mychess::refgen::perft(&mut pos2, row.depth), row.nodes);
        }
    }
}

#[test]
#[ignore = "T2: depth 6..=8 nodes (minutes, single-thread)"]
fn t2_nodes_deep() {
    for g in all() {
        for row in &g.rows {
            if !(6..=7).contains(&row.depth) {
                continue;
            }
            let mut pos = Position::from_fen(&g.fen).unwrap();
            assert_eq!(perft(&mut pos, row.depth), row.nodes, "{} d{}", g.fen, row.depth);
        }
    }
}

#[test]
#[ignore = "T2: detailed columns at depth 6"]
fn t2_details_depth6() {
    for g in all() {
        for row in &g.rows {
            if row.depth == 6 && row.cols.iter().any(|c| c.is_some()) {
                check_detail(&g, row);
            }
        }
    }
}

#[test]
#[ignore = "T2: parallel nodes for large depths (startpos d8, pos3 d8, pos6 d7)"]
fn t2_parallel_deep() {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    for g in all() {
        for row in &g.rows {
            if !(7..=8).contains(&row.depth) {
                continue;
            }
            let pos = Position::from_fen(&g.fen).unwrap();
            assert_eq!(
                perft_parallel(&pos, row.depth, threads),
                row.nodes,
                "{} d{}",
                g.fen,
                row.depth
            );
        }
    }
}

#[test]
#[ignore = "T3 grand slam: startpos depth 9 (parallel, ~minutes-hours)"]
fn t3_startpos_d9() {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    let pos = Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
    assert_eq!(perft_parallel(&pos, 9, threads), 2439530234167);
}
