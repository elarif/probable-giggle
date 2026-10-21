//! Comparaison directe des deux generateurs et des deux perfts.
//!
//! Criterion, pas de seuil absolu : on lit des **ratios** dans une meme
//! session, machine au repos.

use criterion::{Criterion, criterion_group, criterion_main};
use mychess::gct::generate::generate as gct_generate;
use mychess::gct::perft::perft as gct_perft;
use mychess::movegen::{MoveList, count_moves, generate};
use mychess::perft::perft;
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

fn bench_generate(c: &mut Criterion) {
    let mut g = c.benchmark_group("generate");
    for (name, fen) in FENS {
        let pos = Position::from_fen(fen).unwrap();
        // `MoveList::len` est un champ prive : on construit une liste neuve a
        // chaque iteration. C'est exactement ce que fait un appelant reel, et
        // cela inclut donc le cout de `MoveList::new()` (~15 ns), identique
        // pour les deux implementations — le ratio reste juste.
        g.bench_function(format!("gct/{name}"), |b| {
            b.iter(|| {
                let mut l = MoveList::new();
                gct_generate(&pos, &mut l)
            });
        });
        g.bench_function(format!("movegen/{name}"), |b| {
            b.iter(|| {
                let mut l = MoveList::new();
                generate(&pos, &mut l, GenKind::All);
                l.len()
            });
        });
    }
    g.finish();
}

/// `gct` n'a **pas** d'equivalent de `count_moves` : son `generate` construit
/// une `MoveList` et renvoie l'indice d'append, il ne compte pas. Les bras
/// `gct` de ce groupe etaient donc byte-identiques a ceux de `generate` : huit
/// mesures dupliquees, sans information. L'asymetrie elle-meme est le fait a
/// retenir — au bord d'un perft, `movegen` appelle `count_moves` la ou `gct`
/// construit une liste — et c'est ce que mesure le ratio de perft du rapport
/// (`docs/movegen/gct-benchmark-2026-10-21.md`, sections « Lecture » et
/// « Verdict ») : il compare deux strategies de feuille, pas deux couts de
/// generation.
fn bench_count(c: &mut Criterion) {
    let mut g = c.benchmark_group("count");
    for (name, fen) in FENS {
        let pos = Position::from_fen(fen).unwrap();
        g.bench_function(format!("movegen_count_moves/{name}"), |b| {
            b.iter(|| count_moves(&pos))
        });
    }
    g.finish();
}

fn bench_perft(c: &mut Criterion) {
    let mut g = c.benchmark_group("perft");
    let startpos = Position::from_fen(FENS[0].1).unwrap();
    let kiwipete = Position::from_fen(FENS[1].1).unwrap();
    g.bench_function("gct/startpos_d4", |b| {
        b.iter(|| {
            let mut p = startpos;
            gct_perft(&mut p, 4)
        });
    });
    g.bench_function("movegen/startpos_d4", |b| {
        b.iter(|| {
            let mut p = startpos;
            perft(&mut p, 4)
        });
    });
    g.bench_function("gct/kiwipete_d3", |b| {
        b.iter(|| {
            let mut p = kiwipete;
            gct_perft(&mut p, 3)
        });
    });
    g.bench_function("movegen/kiwipete_d3", |b| {
        b.iter(|| {
            let mut p = kiwipete;
            perft(&mut p, 3)
        });
    });
    g.finish();
}

criterion_group!(benches, bench_generate, bench_count, bench_perft);
criterion_main!(benches);
