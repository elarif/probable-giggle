//! Phase 3 bench — the MCTS search itself (not the move generator).
//!
//! The bench measures a **complete search** of a fixed node budget: the
//! number of simulations per second is the metric that matters for the
//! engine, and the phase-2 report already showed that the move generator is
//! only 5-12 % of a simulation.
//!
//! Each benchmark group is reported in `nps` (nodes per second) through
//! criterion's throughput, so the numbers are directly comparable with the
//! `nps` line of the UCI `info` output and with `examples/mcts_probe.rs`.
//!
//! The parallel groups exist to measure the **scaling** of the root-parallel
//! mode. Two regimes must not be mixed:
//!
//! * `nps/split_*` — the same total node budget, split between the workers:
//!   the metric is the **wall time** (the quality per node is *not* the same
//!   as the mono one, see `docs/mcts/phase3-2026-10-22.md`);
//! * `nps/throughput_*` — a fixed wall-clock budget: the metric is the
//!   number of nodes produced in that time.
//!
//! **Phase 4**: the mono group measures the two selection rules side by
//! side (`uct` = the frozen v1 rule, `puct` = the phase-4 rule) in the
//! same session, so the cost of the PUCT selection is measured on the
//! same machine, at the same moment, as the rule it replaces.

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use mychess::mcts::{Limits, Policy, Search};
use mychess::position::Position;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const FENS: &[(&str, &str)] = &[
    (
        "startpos",
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    ),
    (
        "kiwipete",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    ),
];

/// The node budget of one search: big enough to build a real tree, small
/// enough to keep the bench short.
const BUDGET: u64 = 50_000;

fn search(pos: &Position, nodes: u64, threads: usize) -> Search {
    let limits = Limits {
        nodes,
        movetime_ms: None,
        max_playout_ply: 100,
        c_uct: std::f32::consts::SQRT_2,
        c_puct: mychess::mcts::DEFAULT_C_PUCT,
        root_noise: mychess::mcts::DEFAULT_ROOT_NOISE,
    };
    Search::new(*pos, vec![pos.zobrist], limits, 42).with_threads(threads)
}

/// Mono-thread: the two selection rules, side by side.
fn bench_mono(c: &mut Criterion) {
    let mut g = c.benchmark_group("nps");
    g.throughput(Throughput::Elements(BUDGET));
    for (name, fen) in FENS {
        let pos = Position::from_fen(fen).unwrap();
        for (label, policy) in [("uct", Policy::Uct), ("puct", Policy::Puct)] {
            g.bench_function(format!("{label}/{name}"), |b| {
                b.iter(|| {
                    let s = search(&pos, BUDGET, 1).with_policy(policy);
                    let o = s.run(&AtomicBool::new(false));
                    std::hint::black_box(o.best)
                })
            });
        }
    }
    g.finish();
}

/// Root-parallel, same total node budget: the wall time is the metric.
fn bench_parallel_split(c: &mut Criterion) {
    let mut g = c.benchmark_group("nps/split");
    g.throughput(Throughput::Elements(BUDGET));
    for (name, fen) in FENS {
        let pos = Position::from_fen(fen).unwrap();
        for threads in [2usize, 4, 6] {
            g.bench_function(format!("t{threads}/{name}"), |b| {
                b.iter(|| {
                    let s = search(&pos, BUDGET, threads);
                    let o = s.run(&AtomicBool::new(false));
                    std::hint::black_box(o.best)
                })
            });
        }
    }
    g.finish();
}

/// Root-parallel, fixed wall-clock budget: the number of nodes is the metric.
fn bench_parallel_throughput(c: &mut Criterion) {
    let mut g = c.benchmark_group("nps/throughput");
    g.sample_size(10);
    g.warm_up_time(Duration::from_secs(1));
    g.measurement_time(Duration::from_secs(5));
    let pos = Position::startpos();
    for threads in [1usize, 2, 4, 6] {
        g.bench_function(format!("t{threads}/startpos_1s"), |b| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    let limits = Limits {
                        nodes: u64::MAX / 4,
                        movetime_ms: Some(1000),
                        max_playout_ply: 100,
                        c_uct: std::f32::consts::SQRT_2,
                        c_puct: mychess::mcts::DEFAULT_C_PUCT,
                        root_noise: mychess::mcts::DEFAULT_ROOT_NOISE,
                    };
                    let s = Search::new(pos, vec![pos.zobrist], limits, 42).with_threads(threads);
                    let t0 = std::time::Instant::now();
                    let o = s.run(&AtomicBool::new(false));
                    total += t0.elapsed();
                    std::hint::black_box(o.nodes);
                }
                total
            })
        });
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_mono,
    bench_parallel_split,
    bench_parallel_throughput
);
criterion_main!(benches);
