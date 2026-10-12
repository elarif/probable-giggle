use criterion::{criterion_group, criterion_main, Criterion};
use mychess::movegen::{generate, MoveList};
use mychess::perft::perft;
use mychess::position::Position;
use mychess::types::GenKind;
use std::hint::black_box;

const FENS: &[(&str, &str)] = &[
    ("startpos", "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"),
    ("kiwipete", "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1"),
    ("pos3", "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1"),
    ("pos6", "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10"),
];

fn bench_generate(c: &mut Criterion) {
    for (name, fen) in FENS {
        let pos = Position::from_fen(fen).unwrap();
        c.bench_function(&format!("generate_all/{name}"), |b| {
            b.iter(|| {
                let mut list = MoveList::new();
                generate(black_box(&pos), &mut list, GenKind::All);
                black_box(list.len())
            })
        });
        c.bench_function(&format!("generate_captures/{name}"), |b| {
            b.iter(|| {
                let mut list = MoveList::new();
                generate(black_box(&pos), &mut list, GenKind::CapturesOnly);
                black_box(list.len())
            })
        });
    }
}

fn bench_make_unmake(c: &mut Criterion) {
    for (name, fen) in FENS {
        let base = Position::from_fen(fen).unwrap();
        let mut tmp = base;
        let mut list = MoveList::new();
        generate(&tmp, &mut list, GenKind::All);
        let mv = list.get(0);
        c.bench_function(&format!("make_unmake/{name}"), |b| {
            b.iter(|| {
                let mut p = base;
                let u = p.make(black_box(mv));
                p.unmake(mv, u);
                black_box(p.zobrist)
            })
        });
        let _ = &mut tmp;
    }
}

fn bench_gives_check(c: &mut Criterion) {
    for (name, fen) in FENS {
        let pos = Position::from_fen(fen).unwrap();
        let mut list = MoveList::new();
        generate(&pos, &mut list, GenKind::All);
        let moves: Vec<_> = list.iter().collect();
        c.bench_function(&format!("gives_check/{name}"), |b| {
            b.iter(|| {
                let mut n = 0u32;
                for &mv in &moves {
                    if black_box(&pos).gives_check(mv) {
                        n += 1;
                    }
                }
                black_box(n)
            })
        });
    }
}

fn bench_zobrist(c: &mut Criterion) {
    let pos = Position::from_fen(FENS[1].1).unwrap();
    let mut list = MoveList::new();
    generate(&pos, &mut list, GenKind::All);
    let moves: Vec<_> = list.iter().collect();
    c.bench_function("zobrist_incremental", |b| {
        b.iter(|| {
            let mut p = pos;
            let mut h = 0u64;
            for &mv in &moves {
                let u = p.make(mv);
                h ^= p.zobrist;
                p.unmake(mv, u);
            }
            black_box(h)
        })
    });
}

fn bench_perft(c: &mut Criterion) {
    let kiwipete = Position::from_fen(FENS[1].1).unwrap();
    c.bench_function("perft_bulk/kiwipete_d2_2039", |b| {
        b.iter(|| {
            let mut p = kiwipete;
            black_box(perft(&mut p, 2))
        })
    });
    c.bench_function("perft_bulk/kiwipete_d3_97862", |b| {
        b.iter(|| {
            let mut p = kiwipete;
            black_box(perft(&mut p, 3))
        })
    });
}

criterion_group!(
    benches,
    bench_generate,
    bench_make_unmake,
    bench_gives_check,
    bench_zobrist,
    bench_perft
);
criterion_main!(benches);
