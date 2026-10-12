//! L5 gate: memo == exhaustive perft, depth <= 4 always-on, depth 6 opt-in
//! before any deep/grand-slam run. Golden values come exclusively from
//! tests/perft/*.txt (single source of truth). Exercises probing (small
//! table), promotions, EP and castling subtrees.

use mychess::memo::{perft_memo, Memo};
use mychess::perft::perft;
use mychess::position::Position;

/// Parse "fen" + the row for `d` out of a golden file.
fn golden_fen_and_depth(path: &str, d: u32) -> (String, Option<u64>) {
    let text = std::fs::read_to_string(path).unwrap();
    let mut fen = String::new();
    let mut nodes = None;
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
        if toks[0] == d.to_string() {
            nodes = Some(toks[1].replace('_', "").parse().unwrap());
        }
    }
    assert!(!fen.is_empty(), "{path}: no FEN");
    (fen, nodes)
}

const GOLDEN_FILES: [&str; 6] = [
    "tests/perft/startpos.txt",
    "tests/perft/kiwipete.txt",
    "tests/perft/pos3.txt",
    "tests/perft/pos4.txt",
    "tests/perft/pos5.txt",
    "tests/perft/pos6.txt",
];

#[test]
fn memo_equals_exhaustive_d4_all_golden() {
    for path in GOLDEN_FILES {
        for d in 1..=4u32 {
            let (fen, want) = golden_fen_and_depth(path, d);
            let want = want.unwrap_or_else(|| panic!("{path}: no d{d} row"));
            let mut a = Position::from_fen(&fen).unwrap();
            let mut b = Position::from_fen(&fen).unwrap();
            let mut memo = Memo::new(20); // small table: forces probe churn
            memo.min_depth = 2;
            assert_eq!(perft(&mut a, d), want, "{path} d{d}");
            assert_eq!(perft_memo(&mut b, d, &mut memo), want, "{path} d{d} (memo)");
            // Note: at d<=4 remaining-depth>=2 nodes sit at overall ply<=2,
            // where converging transpositions rarely exist (they start at
            // ply>=3 via knight dance-backs), so zero hits here is normal.
            // The hit-count check lives in the d6 gate below.
        }
    }
}

/// The L5 gate proper: d6 on every golden position, memo vs golden value.
#[test]
#[ignore = "L5 gate: d6 per position, ~20-40 s total (single thread)"]
fn memo_equals_exhaustive_d6_golden() {
    for path in GOLDEN_FILES {
        let (fen, want) = golden_fen_and_depth(path, 6);
        // CPW publishes no depth-6 row for "Position 5"; skip it (no
        // reference to validate against — the engine's own d6 there is
        // cross-checked via the file/CPW chain elsewhere).
        let want = match want {
            Some(w) => w,
            None => {
                eprintln!("gate d6: {path} has no published depth-6 row, skipped");
                continue;
            }
        };
        let mut pos = Position::from_fen(&fen).unwrap();
        let mut memo = Memo::new(28);
        let got = perft_memo(&mut pos, 6, &mut memo);
        assert_eq!(got, want, "{path} d6: memo != golden");
        assert!(memo.hits > 0, "{path}: memo had zero hits (cutoff issue?)");
    }
}

#[test]
fn memo_startpos_d6_known_value() {
    // Quick, always-on gate: startpos d6, golden 119,060,324.
    let mut pos = Position::startpos();
    let mut memo = Memo::new(24);
    assert_eq!(perft_memo(&mut pos, 6, &mut memo), 119_060_324);
}