//! Les **propriétés** de l'alpha-beta — les tests que la suite ne
//! couvrait pas (revue du 2026-10-23).
//!
//! # Pourquoi ce fichier existe
//!
//! La revue a trouvé des bugs que **rien** ne pouvait voir :
//!
//! * la quiescence rendait la **valeur statique** d'un **pat** au lieu
//!   de 0, quand la fenêtre était étroite (le test existant ne
//!   cherchait le pat qu'à la racine, où la fenêtre est large) ;
//! * la table de transposition rendait la valeur **différente** selon
//!   qu'elle était purgée ou non — et le test qui prétendait le
//!   vérifier comparait **deux recherches neuves** (donc deux tables
//!   neuves) : il ne vérifiait **rien** ;
//! * l'alpha-beta et le MCTS **n'évaluaient pas la même position** : la
//!   règle de répétition (2 occurrences contre 3) et le matériel
//!   insuffisant divergeaient — ce qui rompt le contrat de la phase AB
//!   (« les deux algorithmes voient la même évaluation »).
//!
//! Ces bugs sont tous de la famille « **un invariant existe, mais
//! personne ne l'écrit** ». Ce fichier écrit les invariants.
//!
//! # Le style
//!
//! Chaque test énonce une **propriété** du moteur, pas un cas
//! particulier : « la valeur d'une position terminale ne dépend pas de
//! la fenêtre », « la table de transposition ne change pas la valeur »,
//! « le PVS rend la même valeur qu'une fenêtre pleine ». Une propriété
//! qui tient attrape toute une famille de bugs, là où un cas unique
//! n'attrape que lui.

mod common;

use common::flip_vertically;
use mychess::ab::{self, Limits, Options, Tt};
use mychess::movegen::{self, MoveList};
use mychess::position::Position;
use mychess::types::GenKind;
use std::sync::atomic::AtomicBool;

/// Un corpus de positions (les « golden » du dépôt + quelques finales).
const CORPUS: &[&str] = &[
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    "k7/8/KQ6/8/8/8/8/8 w - - 0 1",
    "8/8/8/4k3/8/8/8/K6Q w - - 0 1",
    "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1",
];

/// La recherche, à profondeur fixée, avec des options explicites.
fn search(fen: &str, depth: u32, opts: Options) -> ab::Outcome {
    let pos = Position::from_fen(fen).expect("valid fen");
    let limits = Limits {
        max_depth: depth,
        movetime_ms: None,
        nodes: u64::MAX,
    };
    ab::Search::with_options(pos, vec![], limits, opts).run(&AtomicBool::new(false))
}

/// La recherche sur une **position** (et non un FEN).
fn search_pos(pos: &Position, depth: u32, opts: Options) -> ab::Outcome {
    let limits = Limits {
        max_depth: depth,
        movetime_ms: None,
        nodes: u64::MAX,
    };
    ab::Search::with_options(*pos, vec![pos.zobrist], limits, opts).run(&AtomicBool::new(false))
}

/// Les options par défaut, avec une brique activée seule.
fn with(flag: fn(&mut Options)) -> Options {
    let mut o = Options::default();
    flag(&mut o);
    o
}

// ---------------------------------------------------------------------------
// 1. La valeur d'une position TERMINALE ne dépend pas de la fenêtre.
// ---------------------------------------------------------------------------

/// **La valeur d'une position terminale ne dépend pas de la fenêtre.**
///
/// C'est l'invariant que la quiescence violait : la coupe « stand-pat »
/// (`stand >= beta`) était testée **avant** la détection du pat, donc
/// une position de **pat** était rendue à sa **valeur statique** dès
/// que la fenêtre était étroite — et une nulle (0) devenait une
/// position gagnante ou perdante selon la fenêtre.
///
/// Le test passe donc la **même** position terminale dans plusieurs
/// fenêtres, dont une étroite et négative — celle que la negamax
/// construit quand le parent est en difficulté.
#[test]
fn la_valeur_dune_position_terminale_ne_depend_pas_de_la_fenetre() {
    // Le pat : le roi noir n'a aucune case légale et n'est pas en échec.
    let pat = Position::from_fen("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();
    for (alpha, beta) in [
        (-30_000i32, 30_000i32),
        (-1_000, 1_000),
        (0, 1),
        (-1_000, -900),
        (-2_000, -1_999),
        (900, 1_000),
    ] {
        let v = ab::search::quiescence_of(&pat, alpha, beta);
        assert_eq!(
            v, 0,
            "un pat vaut 0, mais la fenêtre ({alpha}, {beta}) a rendu {v}"
        );
    }

    // Le mat : la valeur est un **score de mat**, lui aussi indépendant
    // de la fenêtre (il ne depend que de la distance au mat).
    //
    // C'est le roi **noir maté**, les noirs au trait : la dame blanche
    // en g7 donne l'échec, le roi blanc en g6 défend la dame, et le roi
    // noir n'a **aucune** case légale (g8 et h7 sont couvertes par la
    // dame, g7 est défendue).
    let mated = Position::from_fen("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1").unwrap();
    for (alpha, beta) in [(-30_000i32, 30_000i32), (-1_000, 1_000), (0, 1)] {
        let v = ab::search::quiescence_of(&mated, alpha, beta);
        assert!(
            v <= -ab::MATE + 100,
            "un mat doit rendre un score de mat, pas {v} (fenêtre {alpha}, {beta})"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. La table de transposition ne change PAS la valeur.
// ---------------------------------------------------------------------------

/// **La table de transposition ne change pas la valeur.**
///
/// Le test précédent (`ab_tt_does_not_change_the_score`) comparait
/// **deux recherches neuves** — donc deux tables neuves : il ne
/// vérifiait rien. Ici, la table est **partagée** et **purgée** entre
/// deux recherches : la valeur et le coup doivent être **identiques**.
///
/// C'est l'invariant qui rend la table *accélératrice* et non
/// *perturbatrice* : une entrée de table qui changerait la valeur
/// rendrait les mesures de débit (et de force) **non reproductibles**.
#[test]
fn la_table_de_transposition_ne_change_pas_la_valeur() {
    let mut tt = Tt::new();
    for fen in CORPUS {
        let pos = Position::from_fen(fen).unwrap();
        let limits = Limits {
            max_depth: 5,
            movetime_ms: None,
            nodes: u64::MAX,
        };
        let search = ab::Search::new(pos.clone(), vec![pos.zobrist], limits);

        // 1. la recherche à froid (table neuve)
        let cold = search.run_with_tt(&AtomicBool::new(false), &mut tt);

        // 2. la même recherche, la table **conservée** : elle doit
        //    rendre la même valeur (etconomiser des nœuds).
        let warm = search.run_with_tt(&AtomicBool::new(false), &mut tt);
        assert_eq!(
            cold.score_cp, warm.score_cp,
            "la table a changé la valeur ({fen})"
        );
        assert_eq!(cold.best, warm.best, "la table a changé le coup ({fen})");
        assert!(
            warm.nodes <= cold.nodes,
            "la table chaude a coûté **plus** de nœuds ({fen})"
        );

        // 3. la même recherche, la table **purgée** : la valeur doit
        //    être **identique** à la recherche à froid.
        tt.clear();
        let purged = search.run_with_tt(&AtomicBool::new(false), &mut tt);
        assert_eq!(
            cold.score_cp, purged.score_cp,
            "la table purgée a changé la valeur ({fen})"
        );
        assert_eq!(
            cold.best, purged.best,
            "la table purgée a changé le coup ({fen})"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. Le PVS rend la même valeur qu'une fenêtre pleine.
// ---------------------------------------------------------------------------

/// **Le PVS rend la même valeur qu'une alpha-beta à fenêtre pleine.**
///
/// Le PVS (recherche à fenêtre nulle puis re-recherche) est une
/// **optimisation** : elle doit trouver la **même** valeur, avec
/// **moins** de nœuds. Si les deux divergent, la valeur dépend de
/// l'ordre des coups — c'est le symptôme d'une fenêtre mal transmise.
#[test]
fn le_pvs_equivaut_a_la_fenetre_pleine() {
    for fen in CORPUS {
        let pvs = search(fen, 5, with(|o| o.pvs = true));
        let full = search(fen, 5, with(|o| o.pvs = false));
        assert_eq!(
            pvs.score_cp, full.score_cp,
            "le PVS a changé la valeur ({fen}) : {} contre {}",
            pvs.score_cp, full.score_cp
        );
        // Le **nombre de nœuds** n'est pas un invariant du PVS : la
        // re-recherche en fenêtre pleine peut coûter plus que ce qu'elle
        // économise, sur certaines positions. Ce qui est invariant,
        // c'est la **valeur**.
    }
}

// ---------------------------------------------------------------------------
// 4. La règle de répétition : TROIS occurrences, comme le MCTS.
// ---------------------------------------------------------------------------

/// **La répétition exige trois occurrences** — et le MCTS en fait autant.
///
/// La règle FIDE : la nulle par répétition est **automatique** à la
/// **troisième** occurrence ; à la deuxième, elle est seulement
/// *réclamable*. L'alpha-beta traitait la deuxième comme une nulle
/// automatique : il evaluait `0` une position qui n'était pas Drawing —
/// et il **divergait** du MCTS, ce qui **rompait le contrat** de la
/// phase AB (les deux algorithmes doivent voir la même évaluation).
#[test]
fn la_repetition_exige_trois_occurrences() {
    // La position est visitée **deux** fois dans l'historique : ce
    // n'est **pas** une nulle automatique.
    let pos = Position::from_fen("8/8/8/4k3/8/8/4P3/4K3 w - - 0 1").unwrap();
    let twice = vec![pos.zobrist, pos.zobrist];
    let limits = Limits {
        max_depth: 4,
        movetime_ms: None,
        nodes: u64::MAX,
    };
    let o = ab::Search::new(pos.clone(), twice, limits).run(&AtomicBool::new(false));
    assert!(
        o.score_cp != 0,
        "une position vue **deux** fois ne doit pas être une nulle automatique (score {})",
        o.score_cp
    );

    // La même position vue **trois** fois : la nulle est automatique.
    let thrice = vec![pos.zobrist, pos.zobrist, pos.zobrist];
    let limits = Limits {
        max_depth: 4,
        movetime_ms: None,
        nodes: u64::MAX,
    };
    let o = ab::Search::new(pos.clone(), thrice, limits).run(&AtomicBool::new(false));
    assert_eq!(
        o.score_cp, 0,
        "une position vue **trois** fois est une nulle automatique"
    );
}

// ---------------------------------------------------------------------------
// 5. Le matériel insuffisant est une nulle (comme le MCTS).
// ---------------------------------------------------------------------------

/// **Le matériel insuffisant est une nulle**, dans les DEUX moteurs.
///
/// Roi contre roi : la position est nulle. Le MCTS l'applique
/// (`mcts::terminal_value`), l'alpha-beta non — les deux moteurs
/// n'évaluaient donc pas la même position.
#[test]
fn le_materiel_insuffisant_est_une_nulle() {
    let fen = "8/8/8/4k3/8/8/8/4K3 w - - 0 1";
    let o = search(fen, 6, Options::default());
    assert_eq!(o.score_cp, 0, "roi contre roi est une nulle (alpha-beta)");

    // Le MCTS doit rendre la même chose.
    let pos = Position::from_fen(fen).unwrap();
    let limits = mychess::mcts::Limits {
        nodes: 2_000,
        movetime_ms: None,
        max_playout_ply: 100,
        c_uct: std::f32::consts::SQRT_2,
        c_puct: mychess::mcts::DEFAULT_C_PUCT,
        root_noise: 0.0,
        leaf_q: 0,
    };
    let m = mychess::mcts::Search::new(pos, vec![], limits, 42).run(&AtomicBool::new(false));
    assert_eq!(
        m.score_cp, 0,
        "roi contre roi est une nulle (MCTS), le moteur a rendu {}",
        m.score_cp
    );
}

// ---------------------------------------------------------------------------
// 6. La symétrie : l'évaluation est anti-symétrique, le score est égal.
// ---------------------------------------------------------------------------

/// Le **retournement vertical** d'un FEN : les rangs sont inversés, les
/// fichiers sont **conservés**, et les couleurs sont **échangées**.
///
/// # Pourquoi ce retournement, et pas la rotation de 180° ?
///
/// L'évaluation lit **la même table** pour les deux camps, dans le cadre
/// de chacun : les blancs lisent `PSTV[case ^ 56]` (le rang inversé), les
/// noirs `PSTV[case]`. La symétrie **exacte** de cette construction est
/// donc le **retournement vertical** (les rangs s'inversent, les
/// **L'évaluation est exactement anti-symétrique sous le retournement
/// vertical.**
///
/// C'est l'invariant **mathématique** du moteur : la valeur « Blanc
/// positif » d'une position et de son miroir sont **exactement**
/// opposées, au centipion près.
///
/// Il attrape une inversion de signe quelque part dans l'évaluation
/// (le genre de bug que la revue a trouvé dans la quiescence) — et il
/// **fixe la convention**, que rien n'écrivait : la symétrie exacte
/// est le retournement vertical **avec** échange des couleurs, pas la
/// rotation de 180°.
#[test]
fn levaluation_est_antisymetrique_sous_le_retournement_vertical() {
    for fen in CORPUS {
        let flipped = flip_vertically(fen);
        let a = Position::from_fen(fen).unwrap();
        let b = Position::from_fen(&flipped).unwrap();
        assert_eq!(
            mychess::eval::eval_white(&a),
            -mychess::eval::eval_white(&b),
            "le retournement vertical ne renverse pas l'évaluation ({fen})"
        );
    }
}

/// **Le score de recherche est identique sur la position et son miroir.**
///
/// Le score rendu par la recherche est **côté trait** (positif = le
/// camp au trait est mieux). Sur le miroir, le camp au trait est
/// l'**autre** camp : le score doit donc être **le même**, pas
/// l'opposé.
///
/// (Écrire `-b` serait l'erreur classique : on confond le score côté
/// trait et la valeur « Blanc positif ».)
///
/// C'est un test de *cohérence globale* : il attrape une inversion de
/// signe quelque part dans la chaîne (évaluation, quiescence, table,
/// PV) — le genre de bug que la revue a trouvé deux fois (le signe de
/// la quiescence, l'échelle du score du MCTS).
#[test]
fn le_score_est_identique_sur_la_position_miroir() {
    for fen in CORPUS {
        let flipped = flip_vertically(fen);
        let a = search(fen, 4, Options::default());
        let b = search(&flipped, 4, Options::default());
        assert_eq!(
            a.score_cp, b.score_cp,
            "le miroir ne rend pas le même score ({fen} : {} contre {})",
            a.score_cp, b.score_cp
        );
    }
}

// ---------------------------------------------------------------------------
// 7. Le coup rendu est toujours légal, sur un corpus tiré au sort.
// ---------------------------------------------------------------------------

/// **Le coup rendu est toujours légal** — sur des positions tirées au
/// sort par auto-jeu, pas seulement sur les cas d'école.
///
/// C'est le contrat UCI (« jamais `0000` sur une position jouable »),
/// vérifié sur un corpus large.
#[test]
fn le_coup_rendu_est_toujours_legal() {
    let mut state = 0xC0FF_EE12_3456_789Au64;
    let mut rng = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for game in 0..20 {
        let mut pos = Position::startpos();
        for _ in 0..40 {
            let mut list = MoveList::new();
            movegen::generate(&pos, &mut list, GenKind::All);
            if list.is_empty() {
                break;
            }
            let o = search_pos(&pos, 4, Options::default());
            let uci = o.best.uci();
            let legal: Vec<String> = (0..list.len()).map(|i| list.get(i).uci()).collect();
            assert!(
                legal.contains(&uci),
                "le coup {uci} n'est pas légal (jeu {game})"
            );
            // on joue un coup **légal** (au hasard) pour avancer
            let mv = list.get((rng() as usize) % list.len());
            // **La position doit réellement avancer** : le test
            // d'origine faisait `make` *puis* `unmake`, ce qui
            // remettait la position à l'identique — le corpus était
            // donc **800 copies de la position initiale**, et la
            // propriété n'était vérifiée que sur *une* position.
            pos.make(mv);
        }
    }
}
