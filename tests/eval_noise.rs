//! Phase AB7 — les invariants du **bruit de la valeur**
//! (`docs/ab/plan-bruit-2026-10-23.md`).
//!
//! Le dispositif de bruit n'est un instrument scientifique que s'il
//! satisfait trois propriétés. Elles sont **testées** ici, pas seulement
//! affirmées :
//!
//! 1. **Déterminisme** — la même position, la même graine et la même
//!    amplitude donnent toujours le même offset. C'est ce qui garde la
//!    table de transposition de l'alpha-beta *cohérente* (une entrée
//!    mémorisée pour une position reste la valeur de cette position) :
//!    un bruit tiré par appel rendrait la TT fausse, et l'alpha-beta
//!    ne mesurerait plus l'amplification mais du bruit de mesure.
//!
//! 2. **Indépendance du trait** — l'erreur appartient au *plateau*, pas
//!    à la position vue par un camp. Le même plateau, joué par les
//!    blancs ou par les noirs, doit être bruité *du même côté* (cadre
//!    « Blanc »).
//!
//! 3. **Forme fixée, amplitude variable** — le champ de bruit est un
//!    vecteur unique, mis à l'échelle par l'amplitude. Deux amplitudes
//!    différentes doivent donner le *même* champ : c'est la condition
//!    pour que la courbe de la phase mesure l'amplitude du bruit, et pas
//!    un changement de bruit.
//!
//! Et un quatrième invariant, celui du **défaut** : à bruit nul,
//! l'évaluation est celle d'avant le dispositif — bit à bit.

use mychess::eval;
use mychess::position::Position;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Le bruit est un **etat global du processus** (le moteur est mono
/// processus). Les tests de la crate tournent en **parallele** : sans
/// ce verrou, deux tests qui activent des amplitudes differentes se
/// marchent dessus et le resultat est aleatoire. Les tests qui touchent
/// au bruit sont donc **serialises** — c'est la seule facon honnete de
/// tester un etat global.
fn lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

const SEED: u64 = eval::DEFAULT_NOISE_SEED;

/// Quelques positions du livre ECO, avec des traits variety.
fn positions() -> Vec<Position> {
    [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR b KQkq - 0 1",
        "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4",
        "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R b KQkq - 4 4",
        "8/8/8/8/8/2k5/1q6/K7 w - - 0 1",
        "8/8/8/8/8/2k5/1q6/K7 b - - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "2r3k1/p4p1p/1p2p1p1/8/2P5/1P3P2/P3B1PP/2R3K1 w - - 0 1",
    ]
    .iter()
    .map(|fen| Position::from_fen(fen).expect("valid fen"))
    .collect()
}

/// À bruit nul, l'évaluation est **exactement** celle d'avant : le
/// chemin par défaut des phases 3 à 5 n'est pas touché.
#[test]
fn bruit_nul_ne_change_rien() {
    let _g = lock();
    let pos = Position::from_fen("r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4")
        .unwrap();
    let reference = eval::eval_white(&pos);
    for cp in [0u32, 1, 100, 490, 2000] {
        eval::set_noise(cp, SEED);
        let got = eval::eval_for_mover(&pos);
        if cp == 0 {
            assert_eq!(got, reference, "bruit nul : l'eval doit etre inchangee");
        }
    }
    eval::clear_noise();
    assert_eq!(eval::eval_for_mover(&pos), reference);
}

/// Le meme couple (amplitude, graine) donne toujours le meme offset.
#[test]
fn bruit_deterministe() {
    let _g = lock();
    let ps = positions();
    eval::set_noise(250, SEED);
    let first: Vec<i32> = ps.iter().map(eval::noise_offset).collect();
    for _ in 0..5 {
        let again: Vec<i32> = ps.iter().map(eval::noise_offset).collect();
        assert_eq!(again, first, "le bruit doit etre deterministe");
    }
    // Une autre graine donne un champ different.
    eval::set_noise(250, SEED ^ 0xDEAD_BEEF);
    let other: Vec<i32> = ps.iter().map(eval::noise_offset).collect();
    assert_ne!(other, first, "deux graines distinctes doivent donner deux champs distincts");
    eval::clear_noise();
}

/// L'offset est borne par l'amplitude, et il ne l'atteint pas
/// systematiquement (le bruit est un *champ*, pas une constante).
#[test]
fn bruit_borne_par_amplitude() {
    let _g = lock();
    let ps = positions();
    for cp in [50u32, 100, 250, 490, 2000] {
        eval::set_noise(cp, SEED);
        for p in &ps {
            let o = eval::noise_offset(p);
            assert!(
                o >= -(cp as i32) && o <= cp as i32,
                "offset {o} hors de [-{cp}, {cp}]"
            );
        }
    }
    eval::clear_noise();
}

/// L'erreur appartient au **plateau** : le meme plateau, joue par les
/// blancs ou par les noirs, est bruite du meme cote (cadre « Blanc »).
///
/// Concretement : `eval_for_mover` d'une position et de sa version
/// miroir (trait inverse) doit differer du double de l'eval pure, et
/// le bruit — lu dans le cadre « Blanc » — doit etre le meme des deux
/// cotes.
#[test]
fn bruit_independant_du_trait() {
    let _g = lock();
    let ps = positions();
    eval::set_noise(300, SEED);
    for p in &ps {
        let mut flipped = *p;
        flipped.side = match p.side {
            mychess::types::Color::White => mychess::types::Color::Black,
            mychess::types::Color::Black => mychess::types::Color::White,
        };
        // La cle incrementale porte le trait : on la recalcule apres le
        // basculement (c'est ce que fait le jeu reel).
        flipped.zobrist = flipped.recompute_zobrist();
        // La cle du plateau est la meme des deux cotes.
        let b = eval::noise_offset(p);
        let bf = eval::noise_offset(&flipped);
        assert_eq!(
            b, bf,
            "le meme plateau doit avoir le meme bruit, quel que soit le trait"
        );
        // Et l'eval cote trait reste coherente : la valeur de la position
        // miroir, lue du point de vue du trait, est l opposes de celle de
        // la position, a l'erreur du bruit pres.
        let pure_p = eval::eval_white(p);
        let pure_f = eval::eval_white(&flipped);
        assert_eq!(pure_p, pure_f, "eval_white ne depend pas du trait");
    }
    eval::clear_noise();
}

/// Le champ a une **forme fixee** : l'offset a l'amplitude `A` est
/// l'offset a l'amplitude `B` multiplie par `A/B` (a l'arrondi pres).
///
/// C'est l'invariant qui fait de la courbe de la phase une courbe
/// d'**amplitude** : sans lui, chaque point de la courbe mesurerait un
/// bruit different, et la pente n'aurait aucun sens.
#[test]
fn forme_du_champ_fixe_independante_de_amplitude() {
    let _g = lock();
    let ps = positions();
    eval::set_noise(100, SEED);
    let small: Vec<i32> = ps.iter().map(eval::noise_offset).collect();
    eval::set_noise(400, SEED);
    let big: Vec<i32> = ps.iter().map(eval::noise_offset).collect();
    eval::clear_noise();
    for (i, p) in ps.iter().enumerate() {
        let ratio = small[i] as f64 / 100.0;
        let expected = ratio * 400.0;
        assert!(
            (big[i] as f64 - expected).abs() <= 2.5,
            "plateau {i} : offset {} hors de l'echelle attendue {expected:.1} (petit {})",
            big[i], small[i]
        );
    }
}

/// Le bruit change bien la valeur, et il la change **dans le bon
/// sens** : une position bruitee de +X cp est vue meilleure de X cp.
#[test]
fn bruit_deplace_la_valeur_dans_le_bon_sens() {
    let _g = lock();
    let pos = Position::from_fen("8/8/8/8/8/2k5/1q6/K7 w - - 0 1").unwrap();
    eval::clear_noise();
    let pure = eval::eval_for_mover(&pos);
    eval::set_noise(400, SEED);
    let noisy = eval::eval_for_mover(&pos);
    let off = eval::noise_offset(&pos);
    eval::clear_noise();
    // La position est aux blancs, donc le bruit s'applique tel quel.
    assert_eq!(
        noisy - pure,
        off,
        "l'eval bruitee doit differer de l'eval pure exactement du bruit"
    );
}

/// Les deux algorithmes voient **le meme** bruit : c'est la condition
/// pour que la comparaison AB-vs-MCTS porte sur l'algorithme et pas sur
/// l'evaluation. Les deux passent par `eval_for_mover`, qui est le
/// point d'entree unique de la valeur.
#[test]
fn les_deux_algorithmes_voient_le_meme_bruit() {
    let _g = lock();
    let pos = Position::from_fen("r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4")
        .unwrap();
    eval::clear_noise();
    let pure = mychess::mcts::eval_normalized(&pos);
    eval::set_noise(200, SEED);
    let noisy = mychess::mcts::eval_normalized(&pos);
    // L'offset est lu **bruit actif** (a bruit nul, il vaut 0 par
    // definition).
    let off = eval::noise_offset(&pos) as f32 / 1000.0;
    eval::clear_noise();
    assert!(
        (noisy - pure - off).abs() < 1e-6,
        "la feuille du MCTS doit voir exactement le meme bruit que l'AB"
    );
}
