//! Differential tests: fast legal movegen vs the naive `refgen` oracle, plus
//! incremental-Zobrist validation, over thousands of self-play positions.

use mychess::movegen::{MoveList, generate};
use mychess::position::Position;
use mychess::refgen;
use mychess::types::GenKind;

fn next_rand(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

fn key_set(list: &MoveList) -> Vec<u16> {
    let mut v: Vec<u16> = list.iter().map(|m| m.0).collect();
    v.sort_unstable();
    v
}

#[test]
fn movegen_matches_refgen_selfplay() {
    let mut state = 0x1234_5678_9ABC_DEF0u64;
    for game in 0..60 {
        let mut pos = Position::startpos();
        for ply in 0..80 {
            // Zobrist incremental integrity.
            assert_eq!(
                pos.zobrist,
                pos.recompute_zobrist(),
                "game {game} ply {ply} hash"
            );

            let mut fast = MoveList::new();
            generate(&pos, &mut fast, GenKind::All);
            let slow = refgen::legal_moves(&pos);

            let a = key_set(&fast);
            let b = key_set(&slow);
            assert_eq!(a, b, "game {game} ply {ply} moves at {pos:?}");

            // Captures-only must be a subset with identical encoding.
            let mut caps = MoveList::new();
            generate(&pos, &mut caps, GenKind::CapturesOnly);
            let cap_set = key_set(&caps);
            for m in &cap_set {
                assert!(a.contains(m), "captures-only emitted non-capture {m:#x}");
            }

            if a.is_empty() {
                break;
            }
            let pick = (next_rand(&mut state) as usize) % a.len();
            let mv = mychess::types::Move(a[pick]);
            let u = pos.make(mv);
            assert_eq!(pos.zobrist, pos.recompute_zobrist(), "post-make hash");
            pos.unmake(mv, u);
            assert_eq!(pos.zobrist, pos.recompute_zobrist(), "post-unmake hash");

            let u = pos.make(mv);
            let _ = u;
        }
    }
}

#[test]
fn refgen_perft_matches_fast_shallow() {
    let fens = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
    ];
    for fen in fens {
        for d in 1..=3 {
            let mut a = Position::from_fen(fen).unwrap();
            let mut b = Position::from_fen(fen).unwrap();
            assert_eq!(
                mychess::perft::perft(&mut a, d),
                refgen::perft(&mut b, d),
                "{fen} d{d}"
            );
        }
    }
}
