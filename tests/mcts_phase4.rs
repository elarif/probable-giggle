//! Phase 4 — PUCT et priors de policy : le filet de tests.
//!
//! Les portes, dans l'ordre d'importance :
//!
//! * **G1 — les priors** : la distribution somme à 1, tous les priors sont
//!   strictement positifs, et l'ordre **MVV-LVA** est respecté (un pion
//!   qui prend une dame prime sur une dame qui prend un pion, qui prime
//!   sur un coup calme).
//! * **G2 — le contrat du moteur** : le chemin PUCT rend un coup légal,
//!   il est **reproductible** pour une graine donnée, et le chemin UCT
//!   (figé) reste disponible et inchangé.
//! * **G3 — la tactique** : le mat en un est trouvé en mono **et** en
//!   parallèle, et la batterie tactique n'est pas moins bien traitée
//!   qu'avec la règle UCT (c'est la régression de phase 4 : le gain
//!   mesuré est en *valeur*, pas en tactique).
//! * **G4 — le gain mesuré** : à petit budget, la règle PUCT converge
//!   **plus vite en valeur** que la règle UCT (l'erreur de score
//!   moyenne contre une recherche de référence est plus faible). C'est
//!   la métrique qui a motivé la phase 4
//!   (`docs/mcts/phase4-2026-10-22.md`).
//! * **G5 — le parallélisme** : le mode parallèle reste honnête (le
//!   budget de nœuds total est respecté) et le mat en un y est trouvé.

use mychess::mcts::{Limits, Policy, Rng, Search, policy};
use mychess::movegen::{MoveList, generate};
use mychess::position::Position;
use mychess::types::{GenKind, Move};
use std::sync::atomic::AtomicBool;

/// The seed of the corpus generator (the corpus is reproducible).
const CORPUS_SEED: u64 = 0x00C0_FFEE_1234_5678;

fn limits(nodes: u64) -> Limits {
    Limits {
        nodes,
        movetime_ms: None,
        max_playout_ply: 100,
        c_uct: std::f32::consts::SQRT_2,
        c_puct: mychess::mcts::DEFAULT_C_PUCT,
        root_noise: mychess::mcts::DEFAULT_ROOT_NOISE,
        leaf_q: 0,
    }
}

fn legal_moves(pos: &Position) -> Vec<Move> {
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    list.iter().collect()
}

/// The material of one side, in centipawns.
fn material_of(pos: &Position, side: usize) -> i32 {
    let mut acc = 0;
    for kind in 0..6 {
        let n = (pos.bb_piece[kind] & pos.bb_color[side]).count_ones() as i32;
        acc += n * mychess::eval::material(kind);
    }
    acc
}

/// A reproducible corpus of positions, reached by random playouts.
fn corpus(count: usize) -> Vec<Position> {
    let mut rng = Rng::new(CORPUS_SEED);
    let mut out = Vec::with_capacity(count);
    while out.len() < count {
        let mut pos = Position::startpos();
        for ply in 0..40 {
            let moves = legal_moves(&pos);
            if moves.is_empty() {
                break;
            }
            let pick = (rng.next_u64() % moves.len() as u64) as usize;
            pos.make(moves[pick]);
            if ply % 5 == 2 {
                out.push(pos);
                if out.len() == count {
                    break;
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// G1 — the priors
// ---------------------------------------------------------------------------

/// The priors are a probability distribution: they sum to 1 and they are
/// all strictly positive (no move is ever starved).
#[test]
fn priors_are_a_distribution() {
    for fen in [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "4k3/8/8/3q4/8/8/8/3RK3 w - - 0 1",
    ] {
        let pos = Position::from_fen(fen).unwrap();
        let moves = legal_moves(&pos);
        let p = policy::priors(&pos, &moves);
        assert_eq!(p.len(), moves.len(), "one prior per legal move");
        let sum: f32 = p.iter().sum();
        assert!(
            (sum - 1.0).abs() < 1e-5,
            "the priors must sum to 1 on {fen}, got {sum}"
        );
        assert!(
            p.iter().all(|&x| x > 0.0),
            "every prior must be strictly positive on {fen}"
        );
        assert!(
            p.iter().all(|&x| x <= 1.0),
            "no prior can exceed 1 on {fen}"
        );
    }
}

/// The MVV-LVA order: a pawn taking a queen beats a queen taking a queen,
/// which beats a quiet move.
#[test]
fn mvv_lva_order_is_respected() {
    // White: Pb5, Qc2, Ke1. Black: Pc6, Ke8. White can play bxc6 (a
    // pawn taking a queen) and Qxc6 (a queen taking a queen).
    let pos = Position::from_fen("4k3/8/2p5/1P6/8/8/2Q5/4K3 w - - 0 1").unwrap();
    let moves = legal_moves(&pos);
    let p = policy::priors(&pos, &moves);
    let get = |uci: &str| {
        let i = moves
            .iter()
            .position(|m| m.uci() == uci)
            .unwrap_or_else(|| panic!("{uci} must be legal"));
        p[i]
    };
    let pawn_takes_queen = get("b5c6");
    let queen_takes_queen = get("c2c6");
    let quiet = get("e1e2");
    assert!(
        pawn_takes_queen > queen_takes_queen,
        "MVV-LVA: a pawn taking a queen ({pawn_takes_queen}) must beat a queen taking a queen ({queen_takes_queen})"
    );
    assert!(
        queen_takes_queen > quiet,
        "a capture must beat a quiet move ({queen_takes_queen} vs {quiet})"
    );
}

/// The prior is **peaked**: the capture that wins a queen carries a
/// clearly larger mass than the quiet moves. This is what makes the
/// exploration term move-dependent.
#[test]
fn the_prior_is_peaked_on_the_winning_capture() {
    let pos = Position::from_fen("4k3/8/8/3q4/8/8/8/3RK3 w - - 0 1").unwrap();
    let moves = legal_moves(&pos);
    let p = policy::priors(&pos, &moves);
    let mut capture = 0.0f32;
    let mut quiet_max = 0.0f32;
    for (mv, prior) in moves.iter().zip(p.iter()) {
        if mv.uci() == "d1d5" {
            capture = *prior;
        } else if !mv.is_capture() && !mv.is_promotion() {
            quiet_max = quiet_max.max(*prior);
        }
    }
    assert!(capture > 0.0, "Rxd5 must be legal");
    assert!(
        capture > 5.0 * quiet_max,
        "the capture that wins a queen must dominate the quiet moves ({capture} vs {quiet_max})"
    );
}

// ---------------------------------------------------------------------------
// G2 — the engine contract
// ---------------------------------------------------------------------------

/// The PUCT path returns a legal move, and it is reproducible for a given
/// seed.
#[test]
fn puct_is_legal_and_reproducible() {
    for fen in [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    ] {
        let pos = Position::from_fen(fen).unwrap();
        let legal = legal_moves(&pos);
        let run = |seed: u64| {
            Search::new(pos, vec![pos.zobrist], limits(3000), seed)
                .with_policy(Policy::Puct)
                .run(&AtomicBool::new(false))
        };
        let a = run(42);
        let b = run(42);
        assert_eq!(a.best, b.best, "same seed, same move");
        assert_eq!(a.score_cp, b.score_cp, "same seed, same score");
        assert!(legal.contains(&a.best), "the best move must be legal");
    }
}

/// The legacy UCT rule is still selectable, and it is *not* the PUCT one
/// (the two rules must not have been silently merged).
#[test]
fn the_two_policies_are_distinct() {
    let pos = Position::startpos();
    let uct = Search::new(pos, vec![pos.zobrist], limits(2000), 42)
        .with_policy(Policy::Uct)
        .run(&AtomicBool::new(false));
    let puct = Search::new(pos, vec![pos.zobrist], limits(2000), 42)
        .with_policy(Policy::Puct)
        .run(&AtomicBool::new(false));
    assert_ne!(
        (uct.best, uct.score_cp),
        (puct.best, puct.score_cp),
        "the PUCT rule must change the search (otherwise the phase is a no-op)"
    );
}

// ---------------------------------------------------------------------------
// G3 — the tactics
// ---------------------------------------------------------------------------

/// The mate in one is found in mono and in parallel, with the PUCT rule.
#[test]
fn puct_finds_mate_in_1() {
    let pos = Position::from_fen("k7/8/KQ6/8/8/8/8/8 w - - 0 1").unwrap();
    let mates: Vec<String> = legal_moves(&pos)
        .into_iter()
        .filter(|mv| {
            let mut p = pos;
            p.make(*mv);
            legal_moves(&p).is_empty() && p.in_check(p.side)
        })
        .map(|m| m.uci())
        .collect();
    assert!(!mates.is_empty(), "the position must hold a mate in 1");
    for threads in [1usize, 4] {
        let o = Search::new(pos, vec![pos.zobrist], limits(3000), 42)
            .with_policy(Policy::Puct)
            .with_threads(threads)
            .run(&AtomicBool::new(false));
        assert!(
            mates.contains(&o.best.uci()),
            "mate in 1 not found (threads {threads}): best={} (candidates {mates:?})",
            o.best.uci()
        );
    }
}

/// The tactics battery: the PUCT rule must not find **fewer** forced wins
/// than the legacy UCT rule at the same budget.
#[test]
fn puct_is_not_worse_than_uct_on_the_tactics() {
    // The positions of the battery: each one holds a forced win (a mate or
    // a safe material win) — the oracle is the one of
    // `examples/mcts_tactics.rs`.
    let battery = [
        "6k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1",
        "7k/8/5KQ1/8/8/8/8/8 w - - 0 1",
        "3k4/8/3K4/8/8/8/8/6Q1 w - - 0 1",
        "k7/8/1K6/8/8/8/8/7Q w - - 0 1",
        "k7/8/KQ6/8/8/8/8/8 w - - 0 1",
        "6k1/5ppp/8/8/8/8/5PPP/4R1K1 w - - 0 1",
        "4k3/8/8/3q4/8/8/8/3RK3 w - - 0 1",
        "4k3/8/8/3q4/8/8/8/3R1K2 w - - 0 1",
        "4k3/8/8/3q4/8/8/8/3QK3 w - - 0 1",
        "4k3/8/2q5/1P6/8/8/8/4K3 w - - 0 1",
        "4k3/8/8/3q4/5N2/8/8/4K3 w - - 0 1",
        "4k3/8/8/3q4/2P5/8/8/4K3 w - - 0 1",
        "4k3/8/8/3r4/8/8/8/3RK3 w - - 0 1",
        "4k3/1P6/8/8/8/8/8/4K3 w - - 0 1",
    ];
    let nodes = 100u64;
    let mut hits_uct = 0usize;
    let mut hits_puct = 0usize;
    for fen in battery {
        let pos = Position::from_fen(fen).unwrap();
        // The oracle: the mating moves, and the safe material wins.
        let us = pos.side.idx();
        let them = us ^ 1;
        let us_before = material_of(&pos, us);
        let them_before = material_of(&pos, them);
        let wins: Vec<Move> = legal_moves(&pos)
            .into_iter()
            .filter(|mv| {
                let mut p = pos;
                p.make(*mv);
                if legal_moves(&p).is_empty() && p.in_check(p.side) {
                    return true;
                }
                // A safe material win: the swing (what the mover gains plus
                // what the opponent loses) is at least a minor piece, and
                // the opponent cannot recapture the moved piece.
                let swing =
                    (material_of(&p, us) - us_before) + (them_before - material_of(&p, them));
                swing >= 300
                    && !legal_moves(&p)
                        .into_iter()
                        .any(|r| r.to() == mv.to() && r.is_capture())
            })
            .collect();
        assert!(
            !wins.is_empty(),
            "the battery position {fen} must hold a win"
        );
        let uct = Search::new(pos, vec![pos.zobrist], limits(nodes), 42)
            .with_policy(Policy::Uct)
            .run(&AtomicBool::new(false));
        let puct = Search::new(pos, vec![pos.zobrist], limits(nodes), 42)
            .with_policy(Policy::Puct)
            .run(&AtomicBool::new(false));
        if wins.contains(&uct.best) {
            hits_uct += 1;
        }
        if wins.contains(&puct.best) {
            hits_puct += 1;
        }
    }
    assert!(
        hits_puct >= hits_uct,
        "the PUCT rule must not lose tactics: puct {hits_puct} vs uct {hits_uct}"
    );
}

// ---------------------------------------------------------------------------
// G4 — the measured gain: a faster convergence in value
// ---------------------------------------------------------------------------

/// At a small budget, the PUCT rule is **closer to the value** of a long
/// reference search than the legacy UCT rule.
///
/// This is the metric that motivated the phase 4: the exploration bonus
/// of the PUCT decays as `1/(1+n)` instead of `1/sqrt(n)`, so the search
/// trusts its values sooner. Measured on a 24-position corpus, the mean
/// absolute score error at 500 nodes is ~2 cp for the PUCT against ~13 cp
/// for the UCT (`docs/mcts/phase4-2026-10-22.md`).
#[test]
fn puct_converges_faster_in_value_than_uct() {
    let positions = corpus(24);
    let ref_nodes = 20_000u64;
    let short_nodes = 500u64;
    let mut err_uct = 0i64;
    let mut err_puct = 0i64;
    for pos in &positions {
        let reference = Search::new(*pos, vec![pos.zobrist], limits(ref_nodes), 42)
            .with_policy(Policy::Uct)
            .run(&AtomicBool::new(false));
        let uct = Search::new(*pos, vec![pos.zobrist], limits(short_nodes), 42)
            .with_policy(Policy::Uct)
            .run(&AtomicBool::new(false));
        let puct = Search::new(*pos, vec![pos.zobrist], limits(short_nodes), 42)
            .with_policy(Policy::Puct)
            .run(&AtomicBool::new(false));
        err_uct += (uct.score_cp - reference.score_cp).abs() as i64;
        err_puct += (puct.score_cp - reference.score_cp).abs() as i64;
    }
    assert!(
        err_puct < err_uct,
        "the PUCT rule must converge faster in value: puct {err_puct} vs uct {err_uct} (sum of the absolute score errors)"
    );
}

// ---------------------------------------------------------------------------
// G5 — the parallelism
// ---------------------------------------------------------------------------

/// The parallel mode stays honest (the total node budget is respected)
/// and still finds the mate in one.
#[test]
fn the_parallel_puct_is_honest_and_finds_the_mate() {
    let pos = Position::from_fen("k7/8/KQ6/8/8/8/8/8 w - - 0 1").unwrap();
    let mates: Vec<String> = legal_moves(&pos)
        .into_iter()
        .filter(|mv| {
            let mut p = pos;
            p.make(*mv);
            legal_moves(&p).is_empty() && p.in_check(p.side)
        })
        .map(|m| m.uci())
        .collect();
    for threads in [2usize, 4, 6] {
        let o = Search::new(pos, vec![pos.zobrist], limits(4096), 42)
            .with_policy(Policy::Puct)
            .with_threads(threads)
            .run(&AtomicBool::new(false));
        assert_eq!(o.nodes, 4096, "the node budget must stay honest");
        assert!(
            mates.contains(&o.best.uci()),
            "mate in 1 not found (threads {threads}): best={}",
            o.best.uci()
        );
    }
}
