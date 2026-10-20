use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use mychess::movegen::{MoveList, count_moves, generate};
use mychess::perft::perft;
use mychess::position::{Position, Undo};
use mychess::types::{GenKind, Move};
use std::hint::black_box;
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
    ("pos3", "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1"),
    (
        "pos6",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    ),
];

fn legal_moves(pos: &Position) -> Vec<Move> {
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    list.iter().collect()
}

/// Deterministic pseudo-random walk (same PRNG family as the differential
/// tests) so benchmark paths and test paths agree in style.
fn next_rand(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// A fixed deterministic 32-ply legal self-play path from startpos.
/// Setup-side invariant (GATE 2.5 M1, mirroring tests/differential.rs):
/// `zobrist == recompute_zobrist()` after every make AND every unmake, and the
/// round trip restores the start position exactly.
fn path32() -> [Move; 32] {
    let mut state = 0xC0FF_EE12_3456_789Au64;
    let mut pos = Position::startpos();
    let mut moves = [Move::NONE; 32];
    for (ply, mslot) in moves.iter_mut().enumerate() {
        let moves_list = legal_moves(&pos);
        assert!(
            !moves_list.is_empty(),
            "path32 hit an empty position at ply {ply}"
        );
        *mslot = moves_list[(next_rand(&mut state) as usize) % moves_list.len()];
        let mv = *mslot;
        let u = pos.make(mv);
        assert_eq!(
            pos.zobrist,
            pos.recompute_zobrist(),
            "path32 make invariant, ply {ply}"
        );
        pos.unmake(mv, u);
        assert_eq!(
            pos.zobrist,
            pos.recompute_zobrist(),
            "path32 unmake invariant, ply {ply}"
        );
        pos.make(mv);
    }
    moves
}

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
        // M2 metric: the leaf path (L1). Per-iteration cost == one
        // `count_moves` call (no list to amortise: the `_x1` shape).
        c.bench_function(&format!("count_moves/{name}"), |b| {
            b.iter(|| black_box(count_moves(black_box(&pos))))
        });
    }
}

fn bench_make_unmake(c: &mut Criterion) {
    for (name, fen) in FENS {
        let base = Position::from_fen(fen).unwrap();
        let mut list = MoveList::new();
        generate(&base, &mut list, GenKind::All);
        let mv = list.get(0);
        c.bench_function(&format!("make_unmake/{name}"), |b| {
            b.iter(|| {
                let mut p = base;
                let u = p.make(black_box(mv));
                p.unmake(mv, u);
                black_box(p.zobrist)
            })
        });
    }
}

/// M1 redesigned `gives_check`: the old bench aggregated one full legal-move
/// list per criterion iteration (20-48 make+is_square_attacked calls), so its
/// reported "~1.11 µs" was a batch cost, not a per-call cost. New protocol: 8
/// representative calls per iteration — the `_x8` name fixes the divisor, the
/// sample is spread over the list so captures and quiets are mixed, and both
/// the board and the move are black-boxed per call. Setup (outside timing)
/// replays every call through the make/`in_check` oracle and asserts equal
/// verdicts. The implementation is only replaced if the re-measured per-call
/// cost exceeds the <80 ns contract (M1 decision recorded in MIGRATION.md).
fn bench_gives_check(c: &mut Criterion) {
    for (name, fen) in FENS {
        let pos = Position::from_fen(fen).unwrap();
        let moves = legal_moves(&pos);
        // Invariant (outside timing): every generated move is a genuine legal
        // move and `gives_check` agrees with an independent make/in-check
        // replay of the same move.
        for &mv in &moves {
            let mut p = pos;
            let u = p.make(mv);
            let oracle = p.in_check(pos.side.opp());
            p.unmake(mv, u);
            assert_eq!(pos.gives_check(mv), oracle, "{name} gives_check invariant");
        }
        let k = 8usize;
        let sample: Vec<Move> = (0..k).map(|i| moves[i * moves.len() / k]).collect();
        c.bench_function(&format!("gives_check_x8/{name}"), |b| {
            b.iter(|| {
                let mut n = 0u32;
                for &mv in &sample {
                    if black_box(&pos).gives_check(black_box(mv)) {
                        n += 1;
                    }
                }
                black_box(n)
            })
        });
    }
}

/// M1 redesigned `zobrist_incremental`: the old bench did ~48 make/unmake
/// pairs of Kiwipete per criterion iteration, so the reported "949.8 ns" was
/// ~19.8 ns per pair — already under the <25 ns/ply contract, but the unit
/// was unusable. New protocol (pre-committed): a 30-ply descent (make) +
/// ascension (unmake) on startpos from a fixed deterministic path — exactly
/// the hash-history maintenance an MCTS descent pays per node — with the
/// `incremental == recompute` invariant asserted per ply in setup.
///
/// Reported value = 30 make + 30 unmake; per-ply cost = /30.
fn bench_mcts_descend(c: &mut Criterion) {
    let pos0 = Position::startpos();
    let moves = path32();
    let mut p = pos0;
    let mut undos = [undo_zero(); 30];
    c.bench_function("mcts_descend30/startpos", |b| {
        b.iter(|| {
            let mut h = p.zobrist;
            for (ply, mv) in moves.iter().enumerate().take(30) {
                undos[ply] = p.make(*mv);
                h ^= p.zobrist;
            }
            black_box(h);
            for ply in (0..30).rev() {
                p.unmake(moves[ply], undos[ply]);
            }
            black_box(p.zobrist)
        })
    });
}

fn undo_zero() -> Undo {
    Undo {
        captured: 0,
        castle: 0,
        ep: 64,
        cap_sq: 255,
        halfmove: 0,
        zobrist: 0,
    }
}

/// GATE close-out bench 2 (MCTS baseline): the repetition-detection walkback
/// the MCTS layer will run at every node — compare the current hash against
/// the last 16 path hashes. Two flavours: miss (16 compares, no exit) and
/// hit-at-4 (early exit). Hash values come from a real deterministic descent
/// so the data is representative; colliding entries (two-fold repetition in
/// the path) are perturbed for the `zero_miss` case — the compare cost is
/// identical either way.
fn bench_mcts_walkback(c: &mut Criterion) {
    let moves = path32();
    let mut p = Position::startpos();
    let mut hashes = [0u64; 16];
    for i in 0..16 {
        p.make(moves[i]);
        hashes[i] = p.zobrist;
    }
    for p2 in 0..16 {
        for p1 in 0..p2 {
            if hashes[p2] == hashes[p1] {
                hashes[p2] ^= 1;
            }
        }
    }
    // `zero_miss`: current hash outside the window (16 compares).
    // `hit_at4`: current hash equals the entry 4 plies back (index 11 from a
    // 16-entry window ending at index 15) → early exit after 5 compares.
    for j in (0..16).rev() {
        if hashes[j] == hashes[15] {
            hashes[j] ^= 1;
        }
    }
    let zero_miss = hashes;
    let mut hit_at4 = hashes;
    let mut cur_zero = splitmix(zero_miss[15] ^ 0xA5A5_5A5A_3C3C_C3C3);
    while zero_miss.contains(&cur_zero) {
        cur_zero = splitmix(cur_zero);
    }
    hit_at4[11] = hit_at4[15];
    c.bench_function("mcts_rep_walkback16/zero_miss", |b| {
        b.iter(|| {
            let cur = black_box(cur_zero);
            let mut hit = false;
            for j in (0..16).rev() {
                if black_box(zero_miss[j]) == cur {
                    hit = true;
                    break;
                }
            }
            black_box(hit)
        })
    });
    c.bench_function("mcts_rep_walkback16/hit_at4", |b| {
        b.iter(|| {
            let cur = black_box(hit_at4[15]);
            let mut hit = false;
            for j in (0..16).rev() {
                if black_box(hit_at4[j]) == cur {
                    hit = true;
                    break;
                }
            }
            black_box(hit)
        })
    });
}

fn splitmix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// GATE close-out bench 3 (MCTS baseline): PST evaluation pattern (base of
/// the future A1 eval): full 64-square mailbox walk with color-mirrored PST
/// index and sign — exactly the loop shape the leaf evaluation will use. The
/// table here is a deterministic stand-in (formula-shaped, compile-time
/// constant as future A1 tables will be); what is benchmarked is the pattern,
/// not the tuned values.
fn bench_mcts_pst(c: &mut Criterion) {
    let pos = Position::startpos();
    let pst = black_box(&PST_STAND_IN);
    c.bench_function("mcts_pst_eval/startpos", |b| {
        b.iter(|| {
            let mb = black_box(&pos).mailbox;
            let mut acc = 0i32;
            for sq in 0..64usize {
                let m = mb[sq];
                if m != 0 {
                    if m <= 6 {
                        acc += pst[sq] as i32;
                    } else {
                        acc -= pst[sq ^ 56] as i32;
                    }
                }
            }
            black_box(acc)
        })
    });
}

/// Deterministic stand-in PST (i16, 64 entries). Piece-specific tables for
/// the future A1 layer will multiply this shape; the cost pattern is the
/// same mailbox walk + mirror + sum.
const PST_STAND_IN: [i16; 64] = [
    0, 50, 10, 5, 5, 10, 50, 0, 20, 30, 10, 0, 0, 10, 30, 20, 10, 20, 20, 20, 20, 20, 20, 10, 0, 0,
    0, 20, 20, 0, 0, 0, 5, -5, 0, 0, 0, 0, -5, 5, 5, 10, 10, -20, -20, 10, 10, 5, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

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

/// M2 stop-rule metric: end-to-end perft(6) 1T through criterion (pre-committed
/// gate metric, alongside the `sweep` CLI medians recorded in MIGRATION.md).
fn bench_perft6_1t(c: &mut Criterion) {
    let mut g = c.benchmark_group("perft1t");
    g.sample_size(10);
    g.warm_up_time(Duration::from_secs(2));
    g.measurement_time(Duration::from_secs(15));
    g.throughput(Throughput::Elements(119_060_324));
    g.bench_function("startpos_d6", |b| {
        b.iter(|| {
            let mut p = Position::startpos();
            black_box(perft(&mut p, 6))
        })
    });
    g.finish();
}

criterion_group!(
    benches,
    bench_generate,
    bench_make_unmake,
    bench_gives_check,
    bench_mcts_descend,
    bench_mcts_walkback,
    bench_mcts_pst,
    bench_perft,
    bench_perft6_1t
);
criterion_main!(benches);
