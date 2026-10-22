//! Phase 5 — **la feuille tactiquement résolue** (quiescence-lite) et le
//! **budget temps** : le filet de tests.
//!
//! Les portes, dans l'ordre d'importance :
//!
//! * **G1 — la parité `leaf_q = 0`** : le chemin par défaut doit être
//!   **bit-à-bit** celui de la phase 4. Le test le vérifie sur la
//!   fonction de feuille (`leaf_value_q(..., 0)` est le même `f32` que
//!   `leaf_value(...)`) et le différentiel de la phase 3 — qui rejoue
//!   l'algorithme v1 — reste vert **sans modification**.
//! * **G2 — la sémantique de `qvalue`** : la valeur est **pour le trait**,
//!   avec **stand-pat** (le trait peut refuser l'échange), la profondeur 0
//!   est l'éval statique, et un mat trouvé dans la chaîne vaut un mat.
//! * **G3 — la terminalité est prioritaire** : un mat, une pat, la règle
//!   des 50 coups, la triple répétition et le matériel insuffisant gardent
//!   leur valeur **exacte**, quelle que soit la profondeur de quiescence.
//! * **G4 — le contrat du moteur** : le coup rendu est légal, la recherche
//!   est **reproductible** à graine donnée, et le budget de nœuds est
//!   **exact** (mono et parallèle).
//! * **G5 — le budget temps** : le budget ne dépasse **jamais** le temps
//!   restant (le plancher de 50 ms de la phase 4 a fait adjuger 158/200
//!   parties en TC 1+0.01).

use mychess::eval::eval_for_mover;
use mychess::mcts::{
    Limits, Policy, Rng, Search, leaf_value, leaf_value_q, qsearch::qvalue, terminal_value,
};
use mychess::movegen::{MoveList, generate};
use mychess::position::Position;
use mychess::types::{GenKind, Move};
use mychess::uci::parse_go;
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

fn fen(s: &str) -> Position {
    Position::from_fen(s).expect("valid FEN")
}

fn legal_moves(pos: &Position) -> Vec<Move> {
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    list.iter().collect()
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
// G1 — la parité `leaf_q = 0` (le chemin par défaut est bit-à-bit)
// ---------------------------------------------------------------------------

/// `leaf_q = 0` doit être **exactement** la feuille de la phase 4 : pas
/// « presque », **les mêmes bits**.
#[test]
fn leaf_q_zero_is_the_phase4_leaf_bit_for_bit() {
    for pos in corpus(200) {
        let hist = [pos.zobrist];
        let a = leaf_value(&pos, &hist);
        let b = leaf_value_q(&pos, &hist, 0);
        assert_eq!(
            a.to_bits(),
            b.to_bits(),
            "leaf_q=0 doit etre bit-a-bit la feuille de la phase 4"
        );
    }
}

/// Le moteur avec `leaf_q = 0` rend **exactement** le même coup, le même
/// score et le même nombre de nœuds que le moteur par défaut (qui est
/// `leaf_q = 0`) : le chemin phase 5 n'introduit aucune dérive.
#[test]
fn the_default_engine_is_the_phase4_engine() {
    for f in [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    ] {
        let pos = fen(f);
        let hist = vec![pos.zobrist];
        let a = Search::new(pos, hist.clone(), limits(4000), 1234).run(&AtomicBool::new(false));
        let b = Search::new(pos, hist, limits(4000), 1234)
            .with_leaf_q(0)
            .with_policy(Policy::Puct)
            .run(&AtomicBool::new(false));
        assert_eq!(a.best, b.best, "coup different sur {f}");
        assert_eq!(a.score_cp, b.score_cp, "score different sur {f}");
        assert_eq!(a.nodes, b.nodes, "nombre de noeuds different sur {f}");
    }
}

// ---------------------------------------------------------------------------
// G2 — la sémantique de `qvalue`
// ---------------------------------------------------------------------------

/// La profondeur 0 est l'éval statique, et une position calme ne bouge pas.
#[test]
fn qvalue_depth_zero_is_the_static_eval() {
    for pos in corpus(100) {
        assert_eq!(qvalue(&pos, 0), eval_for_mover(&pos));
    }
}

/// Le **stand-pat** : le trait peut toujours refuser l'échange. La valeur
/// de la quiescence n'est donc **jamais pire** que l'éval statique.
#[test]
fn the_stand_pat_never_makes_the_mover_worse() {
    for pos in corpus(300) {
        let base = eval_for_mover(&pos);
        for d in [1u32, 2, 4] {
            let q = qvalue(&pos, d);
            assert!(
                q >= base,
                "le stand-pat doit garder l'eval statique : q{d}={q} contre {base}"
            );
        }
    }
}

/// La valeur est **pour le trait** : un trait qui peut prendre une pièce
/// voit sa valeur monter, quel que soit le camp.
#[test]
fn qvalue_is_the_value_of_the_side_to_move() {
    // Les blancs prennent une dame.
    let white = fen("4k3/8/8/3q4/8/8/8/3QK3 w - - 0 1");
    // Les noirs prennent une dame.
    let black = fen("3q1k2/8/8/8/3Q4/8/8/4K3 b - - 0 1");
    for p in [white, black] {
        let base = eval_for_mover(&p);
        let q = qvalue(&p, 1);
        assert!(
            q > base + 800,
            "la capture gratuite doit etre vue : {q} contre {base}"
        );
    }
}

/// Un échange **mauvais** ne doit pas dégrader la valeur (stand-pat) : la
/// dame adverse reprend la capture.
#[test]
fn a_bad_exchange_is_refused_by_the_stand_pat() {
    let p = fen("3qk3/8/8/3p4/4P3/8/8/4K3 w - - 0 1");
    let base = eval_for_mover(&p);
    let q = qvalue(&p, 2);
    assert_eq!(
        q, base,
        "le stand-pat doit refuser l'echange mauvais : {q} contre {base}"
    );
}

/// Un mat trouvé **dans la chaîne** vaut un mat exact, pas une estimation.
#[test]
fn a_mate_in_the_chain_is_exact() {
    let p = fen("7k/7p/8/8/8/3Q4/8/4K2R w - - 0 1");
    let q = qvalue(&p, 1);
    assert!(
        q >= mychess::mcts::qsearch::MATE_CP,
        "le mat doit etre detecte dans la chaine : {q}"
    );
}

/// La quiescence est **déterministe** : à position et profondeur égales,
/// la valeur est la même (aucun aléa, aucun état global).
#[test]
fn qvalue_is_deterministic() {
    for pos in corpus(100) {
        for d in [1u32, 2, 4] {
            assert_eq!(qvalue(&pos, d), qvalue(&pos, d));
        }
    }
}

// ---------------------------------------------------------------------------
// G3 — la terminalité est prioritaire
// ---------------------------------------------------------------------------

/// Un mat, une pat, la règle des 50 coups, la triple répétition et le
/// matériel insuffisant gardent leur valeur **exacte** : la quiescence ne
/// les remplace **jamais**.
#[test]
fn the_terminality_has_priority_over_the_quiescence() {
    // Mat : le trait est en échec et n'a plus de coup.
    let mated = fen("7k/6Q1/5K2/8/8/8/8/8 b - - 0 1");
    // Pat : le trait n'a plus de coup mais n'est pas en échec.
    let stalemate = fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1");
    // Matériel insuffisant.
    let insufficient = fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1");
    // Règle des 50 coups.
    let fifty = fen("4k3/8/8/8/8/8/8/4K3 w - - 120 60");

    for d in [0u32, 1, 2, 4] {
        assert_eq!(
            leaf_value_q(&mated, &[mated.zobrist], d),
            -1.0,
            "le mat doit rester exact (profondeur {d})"
        );
        assert_eq!(
            leaf_value_q(&stalemate, &[stalemate.zobrist], d),
            0.0,
            "la pat doit rester exacte (profondeur {d})"
        );
        assert_eq!(
            leaf_value_q(&insufficient, &[insufficient.zobrist], d),
            0.0,
            "le materiel insuffisant doit rester nul (profondeur {d})"
        );
        assert_eq!(
            leaf_value_q(&fifty, &[fifty.zobrist], d),
            0.0,
            "la regle des 50 coups doit rester nulle (profondeur {d})"
        );
    }
}

/// La triple répétition est détectée **avant** la quiescence : la
/// position répétée vaut nulle, pas une estimation.
#[test]
fn the_repetition_is_detected_before_the_quiescence() {
    // Trois fois la même position : la dernière apparition est nulle.
    let p = fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1");
    let z = p.zobrist;
    let hist = vec![z, z, z];
    assert_eq!(leaf_value_q(&p, &hist, 0), 0.0);
    assert_eq!(leaf_value_q(&p, &hist, 4), 0.0);
}

/// Le chemin de terminaison exporté reste cohérent avec la feuille.
#[test]
fn the_terminal_value_is_unchanged_by_the_quiescence() {
    let mated = fen("7k/6Q1/5K2/8/8/8/8/8 b - - 0 1");
    assert_eq!(terminal_value(&mated, &[mated.zobrist]), Some(-1.0));
    assert_eq!(leaf_value_q(&mated, &[mated.zobrist], 4), -1.0);
}

// ---------------------------------------------------------------------------
// G4 — le contrat du moteur
// ---------------------------------------------------------------------------

/// Le coup rendu est **légal** et la recherche est **reproductible** à
/// graine donnée, avec la quiescence active.
#[test]
fn a_search_with_leaf_q_is_legal_and_reproducible() {
    for f in [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    ] {
        let pos = fen(f);
        let legal = legal_moves(&pos);
        let mut bests = Vec::new();
        for _ in 0..2 {
            let o = Search::new(pos, vec![pos.zobrist], limits(5000), 4242)
                .with_leaf_q(2)
                .run(&AtomicBool::new(false));
            assert_ne!(o.best, Move::NONE, "aucun coup sur {f}");
            assert!(
                legal.contains(&o.best),
                "coup illegal sur {f} : {}",
                o.best.uci()
            );
            bests.push(o.best);
        }
        assert_eq!(bests[0], bests[1], "la recherche doit etre reproductible");
    }
}

/// Le budget de nœuds reste **exact** avec la quiescence active, en mono
/// comme en parallèle : la quiescence ne doit pas faire déborder le
/// budget (une échelle Elo à nœuds fixes resterait comparable).
#[test]
fn the_node_budget_is_exact_with_the_quiescence() {
    let pos = Position::startpos();
    for leaf_q in [0u32, 1, 2, 4] {
        for threads in [1usize, 4] {
            let o = Search::new(pos, vec![pos.zobrist], limits(20_000), 7)
                .with_leaf_q(leaf_q)
                .with_threads(threads)
                .run(&AtomicBool::new(false));
            assert!(
                o.nodes <= 20_000,
                "le budget est depasse : {} noeuds (leaf_q={leaf_q}, threads={threads})",
                o.nodes
            );
            assert!(
                o.nodes > 20_000 * 9 / 10,
                "le budget n'est pas utilise : {} noeuds (leaf_q={leaf_q}, threads={threads})",
                o.nodes
            );
        }
    }
}

/// La quiescence **change** la valeur de la feuille sur une position
/// tactique : c'est le but de la phase. Sans cela, la phase ne ferait
/// rien.
#[test]
fn the_quiescence_changes_the_leaf_value_on_a_tactical_position() {
    // Les blancs peuvent prendre une dame : l'éval statique ne le voit pas.
    let p = fen("4k3/8/8/3q4/8/8/8/3QK3 w - - 0 1");
    let hist = [p.zobrist];
    let static_leaf = leaf_value_q(&p, &hist, 0);
    let quiescent_leaf = leaf_value_q(&p, &hist, 1);
    assert!(
        quiescent_leaf > static_leaf + 0.5,
        "la quiescence doit voir la capture : {quiescent_leaf} contre {static_leaf}"
    );
}

// ---------------------------------------------------------------------------
// G5 — le budget temps
// ---------------------------------------------------------------------------

/// Le budget temps ne doit **jamais** dépasser le temps restant : c'est
/// la correction de la phase 4 (le plancher de 50 ms faisait flaguer
/// 158/200 parties en TC 1+0.01).
#[test]
fn the_time_budget_never_exceeds_the_remaining_time() {
    for (wtime, winc) in [
        (1u128, 0u128),
        (5, 0),
        (20, 0),
        (50, 0),
        (1000, 0),
        (8000, 80),
        (20, 10),
    ] {
        let wt = wtime.to_string();
        let wi = winc.to_string();
        let toks: Vec<&str> = vec!["go", "wtime", &wt, "winc", &wi];
        let g = parse_go(&toks, &mychess::types::Color::White);
        let budget = g.movetime_ms.expect("un budget temps");
        assert!(
            budget <= wtime,
            "le budget {budget} ms depasse le temps restant {wtime} ms"
        );
        assert!(budget >= 1, "le budget ne doit jamais etre nul");
    }
}

/// Le chemin `movetime` reçoit la **même** marge de sécurité.
#[test]
fn the_movetime_budget_keeps_a_margin() {
    for mt in [1u128, 5, 20, 100, 1000] {
        let mt_s = mt.to_string();
        let toks: Vec<&str> = vec!["go", "movetime", &mt_s];
        let g = parse_go(&toks, &mychess::types::Color::White);
        let budget = g.movetime_ms.expect("un budget temps");
        assert!(budget <= mt, "le budget {budget} ms depasse {mt} ms");
        assert!(budget >= 1, "le budget ne doit jamais etre nul");
    }
}

/// Le budget par **nœuds** n'est pas touché par la correction temps.
#[test]
fn the_node_budget_is_untouched() {
    let toks: Vec<&str> = vec!["go", "nodes", "2000"];
    let g = parse_go(&toks, &mychess::types::Color::White);
    assert_eq!(g.nodes, 2000);
    assert_eq!(g.movetime_ms, None);
}
