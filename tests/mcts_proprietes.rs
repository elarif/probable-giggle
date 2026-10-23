//! Les **propriétés** du MCTS — les tests que la suite ne couvrait pas
//! (revue du 2026-10-23).
//!
//! # Pourquoi ce fichier existe
//!
//! L'audit a trouvé un bug que **rien** ne pouvait voir : le score UCI
//! du MCTS était **dix fois trop petit**. Le pont `Q ↔ centipions`
//! multipliait par `100` là où le pont officiel vaut `1/1000`. Le
//! moteur annonçait `score cp 90` pour une position que l'alpha-beta
//! notait `895`.
//!
//! C'est la famille « une **échelle** qui n'est écrite nulle part » : le
//! code est correct au sens de Rust, il rend *un* nombre, mais ce
//! nombre n'est pas dans l'échelle du protocole. Aucun test ne pouvait
//! le voir, parce que tous les tests comparaient le MCTS à **lui-même**.
//!
//! Ce fichier écrit donc les invariants :
//!
//! * le score du MCTS est dans l'**échelle des centipions**, la même
//!   que celle de l'alpha-beta (le contrat de la phase AB) ;
//! * la terminalité est **exacte** : `-1` pour un mat, `0` pour un pat,
//!   une nulle par les Fifty Moves, la répétition ou le matériel
//!   insuffisant ;
//! * le MCTS est **déterministe** à graine fixée ;
//! * le **budget de nœuds** est respecté, en mono et en parallèle ;
//! * la feuille du MCTS et celle de l'alpha-beta voient **exactement**
//!   la même valeur.

mod common;

use common::flip_vertically;
use mychess::mcts::{self, Limits, Policy};
use mychess::position::Position;
use std::sync::atomic::AtomicBool;

/// Les limites du MCTS : un budget de nœuds, sans bruit de racine
/// (le MCTS est alors **déterministe**).
fn limits(nodes: u64) -> Limits {
    Limits {
        nodes,
        movetime_ms: None,
        max_playout_ply: 100,
        c_uct: std::f32::consts::SQRT_2,
        c_puct: mcts::DEFAULT_C_PUCT,
        root_noise: 0.0,
        leaf_q: 0,
    }
}

/// Le MCTS sur une position.
fn search(pos: Position, nodes: u64, seed: u64) -> mcts::Outcome {
    mcts::Search::new(pos, vec![], limits(nodes), seed)
        .with_policy(Policy::Puct)
        .run(&AtomicBool::new(false))
}

// ---------------------------------------------------------------------------
// 1. Le score est dans l'échelle des centipions.
// ---------------------------------------------------------------------------

/// **Le score du MCTS est dans l'échelle des centipions** — la même
/// que celle de l'alpha-beta.
///
/// Le pont `Q ↔ cp` est un **nom** (`EVAL_Q_CP_SCALE = 1/1000`) : une
/// unité de Q vaut 1 000 centipions. Le bug corrigé le 2026-10-23
/// multipliait par 100 au lieu de diviser par le pont : le moteur
/// annonçait `score cp 90` pour une position valant 895.
///
/// Le test est **relatif** — il compare le MCTS à l'alpha-beta, qui
/// est la référence de l'échelle — et il est **robuste** : le MCTS
/// Approximation de l'alpha-beta n'a pas à être exactement égal, mais il
/// doit être dans le **même ordre de grandeur**, et surtout dans la
/// **même unité**.
#[test]
fn le_score_du_mcts_est_dans_leche_echelle_des_centipions() {
    // K + Q contre K : la position vaut ~895 cp pour l'évaluation
    // statique (côté trait).
    let fen = "8/8/8/4k3/8/8/8/K6Q w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    let statique = mychess::eval::eval_for_mover(&pos);
    assert!(
        statique > 500,
        "la position de référence n'est plus gagnante : {statique}"
    );

    let m = search(pos.clone(), 20_000, 42);
    assert!(
        m.score_cp > 400,
        "le score du MCTS ({}) n'est pas dans l'échelle des centipions \
         (l'évaluation statique vaut {statique})",
        m.score_cp
    );
    assert!(
        m.score_cp <= 1000,
        "le score du MCTS ({}) dépasse l'échelle (la valeur statique \
         vaut {statique})",
        m.score_cp
    );

    // L'alpha-beta, sur la même position, rend la même **unité**.
    let ab = mychess::ab::Search::new(
        pos,
        vec![],
        mychess::ab::Limits {
            max_depth: 4,
            movetime_ms: None,
            nodes: u64::MAX,
        },
    )
    .run(&AtomicBool::new(false));
    assert!(
        ab.score_cp > 400 && ab.score_cp <= 1000,
        "l'alpha-beta rend {} : l'échelle de référence a changé",
        ab.score_cp
    );
    // Et les deux moteurs sont **comparables** : pas un facteur 10.
    let ratio = m.score_cp as f64 / ab.score_cp as f64;
    assert!(
        (0.5..2.0).contains(&ratio),
        "le MCTS ({}) et l'alpha-beta ({}) ne sont pas dans la même échelle",
        m.score_cp,
        ab.score_cp
    );
}

// ---------------------------------------------------------------------------
// 2. La terminalité est exacte.
// ---------------------------------------------------------------------------

/// **La terminalité du MCTS est exacte** : `-1` pour un mat, `0` pour
/// un pat, `0` pour une nulle par les Fifty Moves, la répétition ou le
/// matériel insuffisant, et `None` pour une position normale.
///
/// C'est le contrat de la feuille (A1) : la terminalité est décidée
/// **avant** l'évaluation, et elle n'est **jamais** remplacée par
/// l'évaluation statique.
#[test]
fn la_terminalite_du_mcts_est_exacte() {
    // Un mat : le roi noir est maté.
    let mated = Position::from_fen("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1").unwrap();
    assert_eq!(
        mcts::terminal_value(&mated, &[]),
        Some(-1.0),
        "un mat doit valoir -1"
    );

    // Un pat : aucune case légale, pas d'échec.
    let pat = Position::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
    assert_eq!(
        mcts::terminal_value(&pat, &[]),
        Some(0.0),
        "un pat doit valoir 0"
    );

    // Matériel insuffisant : roi contre roi.
    let kvk = Position::from_fen("8/8/8/4k3/8/8/8/4K3 w - - 0 1").unwrap();
    assert_eq!(
        mcts::terminal_value(&kvk, &[]),
        Some(0.0),
        "le matériel insuffisant doit valoir 0"
    );

    // Les Fifty Moves.
    let mut fifty = Position::from_fen("8/8/8/4k3/8/8/4P3/4K3 w - - 100 60").unwrap();
    fifty.halfmove = 100;
    assert_eq!(
        mcts::terminal_value(&fifty, &[]),
        Some(0.0),
        "la règle des Fifty Moves doit valoir 0"
    );

    // La répétition **à trois** occurrences.
    //
    // La fenêtre de répétition est **bornée par l'horloge des Fifty
    // Moves** : une position ne peut se répéter que si aucune capture ni
    // aucun coup de pion n'a eu lieu depuis — c'est exactement ce que
    // compte le compteur `halfmove`. Une position à `halfmove = 0`
    // (le cas d'un FEN neuf) n'a donc **aucune** répétition possible.
    let pos = Position::from_fen("8/8/8/4k3/8/8/4P3/4K3 w - - 20 60").unwrap();
    let deux = vec![pos.zobrist, pos.zobrist];
    let trois = vec![pos.zobrist, pos.zobrist, pos.zobrist];
    assert_eq!(
        mcts::terminal_value(&pos, &deux),
        None,
        "une position vue deux fois n'est pas une nulle automatique"
    );
    assert_eq!(
        mcts::terminal_value(&pos, &trois),
        Some(0.0),
        "une position vue trois fois est une nulle automatique"
    );

    // Et la fenêtre : à `halfmove = 0`, aucune répétition n'est
    // possible (aucun coup de pion ni capture depuis la dernière).
    let recent = Position::from_fen("8/8/8/4k3/8/8/4P3/4K3 w - - 0 1").unwrap();
    assert_eq!(
        mcts::terminal_value(&recent, &trois),
        None,
        "la répétition ne peut pas dater d'avant le dernier coup de pion"
    );

    // Une position normale n'est pas terminale.
    let normale = Position::startpos();
    assert_eq!(
        mcts::terminal_value(&normale, &[]),
        None,
        "la position initiale n'est pas terminale"
    );
}

// ---------------------------------------------------------------------------
// 3. Le MCTS est déterministe à graine fixée.
// ---------------------------------------------------------------------------

/// **Le MCTS est déterministe** : la même graine, le même budget, le
/// même coup.
///
/// C'est la condition pour qu'une mesure soit **reproductible** : un
/// MCTS non déterministe rendrait chaque duel, chaque débit et chaque
/// Elo irreproductible — et le bruit de racine (RootNoise) rend le
/// MCTS multi-parties **non** déterministe, ce qui est voulu.
#[test]
fn le_mcts_est_deterministe_a_graine_fixee() {
    let pos =
        Position::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1")
            .unwrap();
    let a = search(pos.clone(), 5_000, 1234);
    let b = search(pos.clone(), 5_000, 1234);
    assert_eq!(a.best, b.best, "le MCTS n'est pas déterministe");
    assert_eq!(a.score_cp, b.score_cp, "le MCTS n'est pas déterministe");
    assert_eq!(a.nodes, b.nodes, "le MCTS n'est pas déterministe");

    // Et une graine **différente** donne (statistiquement) une autre
    // partie : le MCTS explore.
    let c = search(pos, 5_000, 4321);
    assert!(c.nodes == 5_000, "le budget de nœuds n'est pas respecté");
}

// ---------------------------------------------------------------------------
// 4. Le budget de nœuds est respecté, en mono et en parallèle.
// ---------------------------------------------------------------------------

/// **Le budget de nœuds est respecté**, en mono et en parallèle.
///
/// Le budget est le contrat des duels « à nombre de nœuds fixe » : un
/// moteur qui en fait plus (ou moins) rend les mesures incomparables.
#[test]
fn le_budget_de_noeuds_est_respecte() {
    let pos =
        Position::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1")
            .unwrap();
    for threads in [1usize, 2, 4] {
        let o = mcts::Search::new(pos.clone(), vec![], limits(3_000), 7)
            .with_threads(threads)
            .with_policy(Policy::Puct)
            .run(&AtomicBool::new(false));
        assert!(
            o.nodes <= 3_000,
            "le budget de nœuds est dépassé ({threads} threads : {} > 3000)",
            o.nodes
        );
        assert!(
            o.nodes >= 2_900,
            "le budget de nœuds n'a pas été utilisé ({threads} threads : {})",
            o.nodes
        );
    }
}

// ---------------------------------------------------------------------------
// 5. Les deux moteurs voient la même valeur.
// ---------------------------------------------------------------------------

/// **La feuille du MCTS et celle de l'alpha-beta voient exactement la
/// même valeur**.
///
/// C'est le contrat de la phase AB : les deux algorithmes ne doivent
/// differer que par **l'algorithme**, jamais par l'évaluation. Le
/// câblage unique (`nn::leaf_cp`) est le garant ; ce test le verrouille
/// — et il attraperait le jour où quelqu'un câblerait l'évaluation
/// statique *directement* dans l'un des deux.
#[test]
fn les_deux_moteurs_voient_la_meme_valeur() {
    for fen in [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/8/8/4k3/8/8/8/K6Q w - - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    ] {
        let pos = Position::from_fen(fen).unwrap();
        let feuille = mychess::nn::leaf_cp(&pos);
        let normalisee = mcts::eval_normalized(&pos);
        let retour = (normalisee * 1000.0).round() as i32;
        assert!(
            (retour - feuille).abs() <= 1,
            "la feuille du MCTS ({retour}) et celle de l'alpha-beta \
             ({feuille}) ne sont pas la même valeur ({fen})"
        );
    }
}

// ---------------------------------------------------------------------------
// 6. La valeur de feuille est invariante par le miroir.
// ---------------------------------------------------------------------------

/// **La valeur de feuille est la même sur la position et sur son
/// miroir** (retournement vertical + échange des couleurs).
///
/// La valeur de feuille est **côté trait** : sur le miroir, le camp au
/// trait est l'autre camp, donc la valeur — elle — ne change pas.
///
/// C'est un test de *cohérence* de la chaîne d'évaluation : il attrape
/// une inversion de signe (le genre de bug que la revue a trouvé deux
/// fois : le signe de la quiescence, l'échelle du score du MCTS).
#[test]
fn la_valeur_de_feuille_est_invariante_par_le_miroir() {
    for fen in [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/8/8/4k3/8/8/8/K6Q w - - 0 1",
    ] {
        let miroir = flip_vertically(fen);
        let a = Position::from_fen(fen).unwrap();
        let b = Position::from_fen(&miroir).unwrap();
        assert_eq!(
            mychess::nn::leaf_cp(&a),
            mychess::nn::leaf_cp(&b),
            "le miroir ne rend pas la même valeur de feuille ({fen})"
        );
    }
}
