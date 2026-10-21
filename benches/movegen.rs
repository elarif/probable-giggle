//! Coût par appel du générateur de production.
//!
//! Criterion, pas de seuil absolu : on lit des ratios dans une même session.

use criterion::{Criterion, criterion_group, criterion_main};
use mychess::movegen::{MoveList, count_moves, generate};
use mychess::position::Position;
use mychess::types::GenKind;

const FENS: [(&str, &str); 4] = [
    (
        "startpos",
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    ),
    (
        "kiwipete",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    ),
    ("pos3", "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1"),
    (
        "sje",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    ),
];

/// Coût d'une liste neuve, isolé. `black_box` empêche le compilateur de
/// supprimer l'appel : sans lui, `MoveList::new()` sans `memset` se réduit à
/// une constante et la mesure ne veut rien dire.
fn bench_movelist_new(c: &mut Criterion) {
    let mut g = c.benchmark_group("movelist_new");
    g.bench_function("new", |b| {
        b.iter(|| std::hint::black_box(MoveList::new()));
    });
    g.finish();
}

fn bench_generate(c: &mut Criterion) {
    let mut g = c.benchmark_group("generate");
    for (name, fen) in FENS {
        let pos = Position::from_fen(fen).unwrap();
        g.bench_function(format!("all/{name}"), |b| {
            b.iter(|| {
                let mut l = MoveList::new();
                generate(&pos, &mut l, GenKind::All);
                std::hint::black_box(l.len())
            });
        });
        g.bench_function(format!("count/{name}"), |b| b.iter(|| count_moves(&pos)));
    }
    g.finish();
}

criterion_group!(benches, bench_movelist_new, bench_generate);
criterion_main!(benches);
