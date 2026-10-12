//! M0 net: explicitly exercise BOTH slider dispatch paths on every machine.
//!
//! The golden and differential suites above run on whatever path runtime
//! dispatch picks; here we force `pext` and `rays` in turn over the same
//! T1 net (golden nodes d<=5, detailed columns d<=4, refgen cross-check)
//! and require bit-identical results. A faulty PEXT table (or fallback) is
//! caught here before it can poison measurements or the future AI layer.

use mychess::perft::{perft, perft_detailed};
use mychess::position::Position;
use mychess::slider::{self, SliderPath};
use std::sync::{Mutex, MutexGuard, OnceLock};

// Tests in one binary run in parallel threads; flip the global dispatch under
// a lock so each test observes exactly the path it forced.
fn lock() -> MutexGuard<'static, ()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(())).lock().unwrap()
}

#[derive(Clone, Copy, PartialEq, Eq)]
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

fn goldens() -> Vec<Golden> {
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

/// The paths to test: rays always; pext when BMI2 hardware is present.
fn paths() -> Vec<(&'static str, SliderPath)> {
    let mut v = vec![("fallback", SliderPath::Fallback)];
    if std::is_x86_feature_detected!("bmi2") {
        v.push(("pext", SliderPath::Pext));
    }
    v
}

#[test]
fn attack_tables_match_ray_walk_both_paths() {
    let _l = lock();
    let occs = [
        0u64,
        0x00FF_0000_0000_FF00,
        0x8142_2418_1824_4281,
        u64::MAX,
        0x0000_0000_0081_0000,
        0xFFFF_FFFF_FFFF_F000,
        0x0000_0000_0000_00FF,
        0x8080_8080_8080_8080,
    ];
    for path in paths() {
        let label = path.0;
        slider::force_path(path.1);
        assert!(slider::_selftest(), "selftest failed on path {label}");
        for sq in 0..64u8 {
            for &occ in &occs {
                let atts = mychess::slider::queen_attacks(sq, occ);
                let pop = atts.count_ones();
                // sanity: full board attack set is bounded
                assert!(pop <= 27, "{label} sq {sq} occ {occ:#x} attacks {pop}");
            }
        }
    }
    // cross-path equality on a fixed workload
    let mut per_path_results: Vec<Vec<u64>> = Vec::new();
    for path in paths() {
        slider::force_path(path.1);
        let mut sig = Vec::new();
        for sq in 0..64u8 {
            for &occ in &occs {
                sig.push(mychess::slider::rook_attacks(sq, occ));
                sig.push(mychess::slider::bishop_attacks(sq, occ));
            }
        }
        per_path_results.push(sig);
    }
    let want = &per_path_results[0];
    for (i, sig) in per_path_results.iter().enumerate().skip(1) {
        assert_eq!(want, sig, "slider path {} differs from rays", paths()[i].0);
    }
}

#[test]
fn perft_nodes_d5_both_paths() {
    let _l = lock();
    let g = goldens();
    let mut results: Vec<Vec<(&str, u32, u64)>> = Vec::new();
    for (label, path) in paths() {
        slider::force_path(path);
        let mut run = Vec::new();
        for gi in &g {
            for row in &gi.rows {
                if row.depth > 5 {
                    continue;
                }
                let mut pos = Position::from_fen(&gi.fen).unwrap();
                let n = perft(&mut pos, row.depth);
                assert_eq!(n, row.nodes, "{label} {} d{}", gi.fen, row.depth);
                run.push((gi.fen.as_str(), row.depth, n));
            }
        }
        results.push(run);
    }
    for other in results.iter().skip(1) {
        assert_eq!(&results[0], other, "perft results differ across paths");
    }
}

#[test]
fn perft_details_d4_both_paths() {
    let _l = lock();
    let g = goldens();
    let mut results: Vec<Vec<(&str, u32, u64)>> = Vec::new();
    for (label, path) in paths() {
        slider::force_path(path);
        let mut run = Vec::new();
        for gi in &g {
            for row in &gi.rows {
                if row.depth > 4 || row.cols.iter().all(|c| c.is_none()) {
                    continue;
                }
                let mut pos = Position::from_fen(&gi.fen).unwrap();
                let c = perft_detailed(&mut pos, row.depth);
                let got = [
                    c.captures, c.ep, c.castles, c.promotions, c.checks, c.discovery_checks,
                    c.double_checks, c.mates,
                ];
                for (i, want) in row.cols.iter().enumerate() {
                    if let Some(want) = want {
                        assert_eq!(got[i], *want, "{label} {} d{} col{i}", gi.fen, row.depth);
                    }
                }
                run.push((gi.fen.as_str(), row.depth, c.nodes));
            }
        }
        results.push(run);
    }
    for other in results.iter().skip(1) {
        assert_eq!(&results[0], other, "detailed results differ across paths");
    }
}

#[test]
fn refgen_crosscheck_d3_both_paths() {
    let _l = lock();
    for (label, path) in paths() {
        slider::force_path(path);
        for gi in goldens() {
            for row in &gi.rows {
                if row.depth > 3 {
                    continue;
                }
                let mut pos = Position::from_fen(&gi.fen).unwrap();
                assert_eq!(perft(&mut pos, row.depth), row.nodes, "{label} {} d{}", gi.fen, row.depth);
                let mut pos2 = Position::from_fen(&gi.fen).unwrap();
                assert_eq!(
                    mychess::refgen::perft(&mut pos2, row.depth),
                    row.nodes,
                    "{label} refgen {} d{}",
                    gi.fen,
                    row.depth
                );
            }
        }
    }
}

#[test]
fn selfplay_differential_both_paths() {
    // Same spirit as tests/differential.rs (60 games x 80 plies) but run under
    // each forced path so PEXT-indexed generation is move-for-move validated
    // against the refgen oracle.
    let _l = lock();
    for (label, path) in paths() {
        slider::force_path(path);
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for game in 0..60 {
            let mut pos = Position::startpos();
            for ply in 0..80 {
                let fast = mychess::movegen::generate_all(&pos);
                let slow = mychess::refgen::legal_moves(&pos);
                let mut a: Vec<u16> = fast.iter().map(|m| m.0).collect();
                a.sort_unstable();
                let mut b: Vec<u16> = slow.iter().map(|m| m.0).collect();
                b.sort_unstable();
                assert_eq!(a, b, "{label} game {game} ply {ply}");
                if a.is_empty() {
                    break;
                }
                let pick = (mychess_rand(&mut state) as usize) % a.len();
                let mv = mychess::types::Move(a[pick]);
                pos.make(mv);
            }
        }
    }
}

fn mychess_rand(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}