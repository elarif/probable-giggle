//! Phase AB — bench de l'alpha-beta (le débit de nœuds).
//!
//! Le MCTS a son bench (`benches/mcts.rs`) : le nombre de simulations
//! par seconde est la métrique qui compte pour le moteur. Ici, la
//! métrique est le **débit de nœuds** de l'alpha-beta, mesuré sur une
//! recherche à profondeur fixée (le temps, lui, dépend de la position
//! et de l'ordre des coups).
//!
//! Le bench est aussi le **contrat de performance** de la phase AB : une
//! régression de débit (un `MoveList` réalloué par nœud, une table PV
//! mal copiée, …) doit se voir ici, pas dans un match.
//!
//! Le débit est rapporté en `nps` (nœuds par seconde) : le nombre de
//! nœuds de la recherche est mesuré une fois, et Criterion rapporte
//! alors `elements/s`.
//!
//! Les positions sont deux positions de référence : l'ouverture du
//! livre ECO (peu de pièces, arbre étroit) et un milieu de partie
//! (beaucoup de coups, des captures) — c'est là que le débit se dégrade.

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use mychess::ab::{Limits, Search};
use mychess::position::Position;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

/// La position initiale (ouverture : peu de pièces, arbre étroit).
const ECO_FIRST: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
/// Une position de milieu de partie (beaucoup de coups, des captures).
const MIDGAME: &str = "r1bq1rk1/pp2ppbp/2np1np1/8/2BNP3/2N1B3/PPP2PPP/R2Q1RK1 w - - 0 10";

fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("ab");

    for (label, fen) in [("eco", ECO_FIRST), ("midgame", MIDGAME)] {
        let pos = Position::from_fen(fen).expect("valid fen");
        let hist = vec![pos.zobrist];
        for depth in [5u32, 7u32] {
            let limits = Limits {
                max_depth: depth,
                movetime_ms: None,
                nodes: u64::MAX,
            };
            // Le nombre de noeuds est mesure une fois : c'est lui qui
            // donne l'element/s rapporte par Criterion.
            let probe = Search::new(pos, hist.clone(), limits).run(&AtomicBool::new(false));
            let nodes = probe.nodes.max(1);
            let mut b = group.benchmark_function(format!("nps/{label}_d{depth}"), |b| {
                b.iter_custom(|iters| {
                    let start = Instant::now();
                    for _ in 0..iters {
                        let search = Search::new(pos, hist.clone(), limits);
                        let o = search.run(&AtomicBool::new(false));
                        std::hint::black_box(o.nodes);
                    }
                    start.elapsed()
                })
            });
            b.throughput(Throughput::Elements(nodes));
            b.iter(Duration::from_secs(1));
        }
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
