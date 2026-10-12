//! M0 micro-benchmark: slider attack lookups, `pext` path vs `rays` path.
//!
//! Run an A/B pair with the same binary:
//!
//! ```sh
//! MYCHESS_SLIDER=pext cargo bench --bench slider
//! MYCHESS_SLIDER=rays cargo bench --bench slider
//! ```
//!
//! Results are kept separate by the criterion group label (see MIGRATION.md).

use criterion::{criterion_group, criterion_main, Criterion};
use mychess::slider::{self, SliderPath};
use std::hint::black_box;

/// Fixed workload: 64 squares x a spread of occupancies (empty, sparse,
/// dense, blocky). 384 lookups per iteration.
const OCCS: [u64; 6] = [
    0,
    0x0000_0181_8181_8100,
    u64::MAX,
    0x8142_2418_1824_4281,
    0x0000_0000_0000_00FF,
    0x8080_8080_8080_8080,
];

fn setup_path() {
    match std::env::var("MYCHESS_SLIDER").unwrap_or_default().to_ascii_lowercase().as_str() {
        "pext" => {
            slider::force_path(SliderPath::Pext);
            assert_eq!(slider::path_name(), "pext", "BMI2 required for this A/B leg");
        }
        "fallback" => slider::force_path(SliderPath::Fallback),
        _ => {} // machine default (auto)
    }
}

fn bench_lookups(c: &mut Criterion) {
    setup_path();
    let label = slider::path_name();
    let id = |n: &str| format!("{n}/{label}");

    c.bench_function(&id("rook_attack_x384"), |b| {
        b.iter(|| {
            let mut acc = 0u64;
            for sq in 0..64u8 {
                for &occ in &OCCS {
                    acc ^= slider::rook_attacks(black_box(sq), black_box(occ));
                }
            }
            black_box(acc)
        })
    });

    c.bench_function(&id("bishop_attack_x384"), |b| {
        b.iter(|| {
            let mut acc = 0u64;
            for sq in 0..64u8 {
                for &occ in &OCCS {
                    acc ^= slider::bishop_attacks(black_box(sq), black_box(occ));
                }
            }
            black_box(acc)
        })
    });

    c.bench_function(&id("queen_attack_x384"), |b| {
        b.iter(|| {
            let mut acc = 0u64;
            for sq in 0..64u8 {
                for &occ in &OCCS {
                    acc ^= slider::queen_attacks(black_box(sq), black_box(occ));
                }
            }
            black_box(acc)
        })
    });

    // Realistic mix (what movegen does: attacks_by + attackers_to patterns)
    // sampled from real positions.
    let fens = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    ];
    let mut work: Vec<(u8, u64)> = Vec::new();
    for fen in fens {
        let pos = mychess::position::Position::from_fen(fen).unwrap();
        let occ = pos.occ();
        let sliders = pos.bb_piece[mychess::types::BISHOP]
            | pos.bb_piece[mychess::types::ROOK]
            | pos.bb_piece[mychess::types::QUEEN];
        let mut a = sliders;
        while a != 0 {
            let sq = a.trailing_zeros() as u8;
            a &= a - 1;
            work.push((sq, occ));
        }
    }
    c.bench_function(&id("attackers_mix_real_pos"), |b| {
        b.iter(|| {
            let mut acc = 0u64;
            for &(sq, occ) in &work {
                acc ^= slider::rook_attacks(black_box(sq), black_box(occ));
                acc ^= slider::bishop_attacks(black_box(sq), black_box(occ));
            }
            black_box(acc)
        })
    });
}

criterion_group!(benches, bench_lookups);
criterion_main!(benches);