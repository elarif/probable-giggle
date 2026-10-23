//! Static evaluation (STEP 0/A1 surface): material + symmetric PST.
//!
//! This module is deliberately simple, fast and modifiable (the phase-0
//! contract): material 100/300/300/500/900 + PeSTO-lite symmetric piece-square
//! tables (Michniewski's "Simplified Evaluation Function", documented). In
//! MCTS v1 the leaf value comes from random playouts (result in {-1,0,1});
//! **A1 flips the leaf to `eval_for_mover`** — this module is that hook.
//! C3 will make it tapered; D1 may replace it by a NN (same call shape).
//!
//! Sign convention: `eval_white` returns centipawns from White's perspective
//! (positive = White better); `eval_for_mover` converts to the
//! side-to-move perspective (positive = mover better) — exactly what the
//! MCTS value convention needs (`mcts::Node::sum` is mover-perspective).

use crate::position::Position;
use crate::types::{BISHOP, BLACK, Color, KNIGHT, PAWN, QUEEN, ROOK, WHITE};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// **Le bruit de la valeur** (phase AB7, `docs/ab/plan-bruit-2026-10-23.md`) :
/// l'amplitude, en centipions, d'une erreur de valeur *déterministe*
/// attachée à chaque position.
///
/// C'est le dispositif expérimental de la phase : il permet de mesurer la
/// **réponse** de chaque algorithme de recherche à une valeur bruitée,
/// en tenant tout le reste constant.
///
/// # Ce que le bruit est, et ce qu'il n'est pas
///
/// * Il est une **erreur de la fonction de valeur** : la même position
///   est *toujours* mal évaluée du même côté. Ce n'est **pas** un tirage
///   par appel — c'est ce qui garde la table de transposition de
///   l'alpha-beta *cohérente* (une entrée mémorisée reste la valeur de
///   cette position) et ce qui rend l'expérience reproductible.
/// * Il est **indépendant du trait** : l'erreur appartient au *plateau*,
///   pas à la position vue par un camp. La même position, quel que soit
///   le trait, est donc mal évaluée du même côté (cadre « Blanc »), ce
///   qui est la seule lecture cohérente d'une « erreur d'évaluation ».
/// * Il est **indépendant de l'amplitude** : le champ de bruit est un
///   vecteur unique dans `[-1, +1]`, mis à l'échelle par l'amplitude.
///   Deux amplitudes différentes donnent le **même champ**, de même
///   forme : la courbe de la phase mesure l'amplitude, et rien d'autre.
///
/// Ces trois propriétés sont **testées** (`tests/eval_noise.rs`).
static NOISE_CP: AtomicU32 = AtomicU32::new(0);
static NOISE_SEED: AtomicU64 = AtomicU64::new(0);

/// La graine du champ de bruit par défaut (quand le bruit est actif sans
/// graine explicite). Fixe : le champ de bruit doit être **le même** dans
/// toutes les parties et sur toutes les machines, sinon la mesure n'est
/// pas reproductible.
pub const DEFAULT_NOISE_SEED: u64 = 0x4E4F_4953_4500_0001;

/// L'amplitude du bruit de valeur (0 = bruit nul).
pub fn noise_cp() -> u32 {
    NOISE_CP.load(Ordering::Relaxed)
}

/// La graine du champ de bruit.
pub fn noise_seed() -> u64 {
    NOISE_SEED.load(Ordering::Relaxed)
}

/// Active le bruit de valeur : `cp` centipions d'amplitude, `seed` la
/// graine du hachage (le même couple ⇒ le même champ de bruit).
///
/// `cp == 0` désactive le bruit : l'évaluation est alors **bit-à-bit**
/// celle d'avant le dispositif (le chemin par défaut des phases 3 à 5
/// n'est pas touché).
pub fn set_noise(cp: u32, seed: u64) {
    NOISE_CP.store(cp, Ordering::Relaxed);
    NOISE_SEED.store(seed, Ordering::Relaxed);
}

/// Désactive le bruit de valeur (retour à l'évaluation pure).
pub fn clear_noise() {
    NOISE_CP.store(0, Ordering::Relaxed);
}

/// Le finaliseur de SplitMix64 : il assure une **avalanche** complète
/// (un bit qui change en entrée change la moitié des bits en sortie).
/// Sans lui, les positions voisines — dont les clés Zobrist ne diffèrent
/// que de quelques bits — recevraient des bruits voisins, et le champ
/// de bruit aurait une *structure* (corrélée à la position) au lieu
/// d'être un bruit.
#[inline]
fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// La clé Zobrist **sans le trait** : le bruit appartient au plateau.
///
/// La clé de position inclut le trait (il est XORé à chaque coup), on
/// le retire donc pour que le même plateau — joué par les blancs ou par
/// les noirs — soit bruité **du même côté**. C'est la seule lecture
/// cohérente d'une « erreur d'évaluation » : l'erreur est une propriété
/// de la position de l'échiquier, pas de la position vue par un camp.
#[inline]
fn board_key(pos: &Position) -> u64 {
    // `pos.zobrist` inclut le trait XORé quand les noirs sont au trait
    // (cf. `Position::recompute_zobrist`) : le retirer rend la clé
    // insensible au trait.
    if pos.side == Color::Black {
        pos.zobrist ^ crate::zobrist::ZOBRIST.side
    } else {
        pos.zobrist
    }
}

/// Le bruit d'une position : un offset déterministe dans `[-cp, +cp]`,
/// dérivé de la clé du plateau et de la graine du bruit.
///
/// L'offset est exprimé dans le **cadre « Blanc »** (comme `eval_white`)
/// : c'est `eval_for_mover` qui l'applique au trait, en changeant de
/// signe avec lui.
///
/// Le champ est **indépendant de l'amplitude** : l'offset est
/// `round(u · cp)` où `u ∈ [-1, +1]` ne dépend que du plateau et de la
/// graine. Deux amplitudes donnent donc le même champ mis à l'échelle.
pub fn noise_offset(pos: &Position) -> i32 {
    let cp = NOISE_CP.load(Ordering::Relaxed);
    if cp == 0 {
        return 0;
    }
    let h = mix64(board_key(pos) ^ NOISE_SEED.load(Ordering::Relaxed));
    // 53 bits d'entropie ⇒ `u ∈ [-1, +1)` avec une précision de 2^-53 :
    // l'arrondi à l'entier ne déforme pas la loi (le bruit est discret
    // en centipions, comme l'évaluation qu'il perturbe).
    let u = ((h >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0;
    (u * cp as f64).round() as i32
}

pub const MAT_PAWN: i32 = 100;
pub const MAT_KNIGHT: i32 = 300;
pub const MAT_BISHOP: i32 = 300;
pub const MAT_ROOK: i32 = 500;
pub const MAT_QUEEN: i32 = 900;

/// Material value per piece kind (indexed by the bitboard slot order).
const MATERIAL: [i32; 6] = [
    MAT_PAWN, MAT_KNIGHT, MAT_BISHOP, MAT_ROOK, MAT_QUEEN, 0, // KING
];

#[inline(always)]
pub fn material(kind: usize) -> i32 {
    MATERIAL[kind]
}

// Symmetric piece-square tables, written VISUALLY (rank 8 row first). A White
// piece on square `sq` reads row `7 - (sq >> 3)`, file unchanged; a Black
// piece reads row `sq >> 3` (vertical mirror = symmetry).
const PST_PAWN: [i16; 64] = [
    0, 0, 0, 0, 0, 0, 0, 0, 50, 50, 50, 50, 50, 50, 50, 50, 10, 10, 20, 30, 30, 20, 10, 10, 5, 5,
    10, 25, 25, 10, 5, 5, 0, 0, 0, 20, 20, 0, 0, 0, 5, -5, -10, 0, 0, -10, -5, 5, 5, 10, 10, -20,
    -20, 10, 10, 5, 0, 0, 0, 0, 0, 0, 0, 0,
];
const PST_KNIGHT: [i16; 64] = [
    -50, -40, -30, -30, -30, -30, -40, -50, -40, -20, 0, 0, 0, 0, -20, -40, -30, 0, 10, 15, 15, 10,
    0, -30, -30, 5, 15, 20, 20, 15, 5, -30, -30, 0, 15, 20, 20, 15, 0, -30, -30, 5, 10, 15, 15, 10,
    5, -30, -40, -20, 0, 5, 5, 0, -20, -40, -50, -40, -30, -30, -30, -30, -40, -50,
];
const PST_BISHOP: [i16; 64] = [
    -20, -10, -10, -10, -10, -10, -10, -20, -10, 0, 0, 0, 0, 0, 0, -10, -10, 0, 5, 10, 10, 5, 0,
    -10, -10, 5, 5, 10, 10, 5, 5, -10, -10, 0, 10, 10, 10, 10, 0, -10, -10, 10, 10, 10, 10, 10, 10,
    -10, -10, 5, 0, 0, 0, 0, 5, -10, -20, -10, -10, -10, -10, -10, -10, -20,
];
const PST_ROOK: [i16; 64] = [
    0, 0, 0, 0, 0, 0, 0, 0, 5, 10, 10, 10, 10, 10, 10, 5, -5, 0, 0, 0, 0, 0, 0, -5, -5, 0, 0, 0, 0,
    0, 0, -5, -5, 0, 0, 0, 0, 0, 0, -5, -5, 0, 0, 0, 0, 0, 0, -5, -5, 0, 0, 0, 0, 0, 0, -5, 0, 0,
    0, 5, 5, 0, 0, 0,
];
const PST_QUEEN: [i16; 64] = [
    -20, -10, -10, -5, -5, -10, -10, -20, -10, 0, 0, 0, 0, 0, 0, -10, -10, 0, 5, 5, 5, 5, 0, -10,
    -5, 0, 5, 5, 5, 5, 0, -5, 0, 0, 5, 5, 5, 5, 0, -5, -10, 5, 5, 5, 5, 5, 0, -10, -10, 0, 5, 0, 0,
    0, 0, -10, -20, -10, -10, -5, -5, -10, -10, -20,
];
/// King midggame table (C3 may add a tapered endgame twin).
const PST_KING: [i16; 64] = [
    -30, -40, -40, -50, -50, -40, -40, -30, -30, -40, -40, -50, -50, -40, -40, -30, -30, -40, -40,
    -50, -50, -40, -40, -30, -30, -40, -40, -50, -50, -40, -40, -30, -20, -30, -30, -40, -40, -30,
    -30, -20, -10, -20, -20, -20, -20, -20, -20, -10, 20, 20, 0, 0, 0, 0, 20, 20, 20, 30, 10, 0, 0,
    10, 30, 20,
];

/// The piece-square table for a piece kind.
#[inline(always)]
fn pst(kind: usize) -> &'static [i16; 64] {
    match kind {
        PAWN => &PST_PAWN,
        KNIGHT => &PST_KNIGHT,
        BISHOP => &PST_BISHOP,
        ROOK => &PST_ROOK,
        QUEEN => &PST_QUEEN,
        _ => &PST_KING,
    }
}

/// Evaluation in centipawns, **White-positive**.
#[inline]
pub fn eval_white(pos: &Position) -> i32 {
    let mut acc = 0i32;
    for (sq, m) in pos.mailbox.iter().enumerate() {
        if *m != 0 {
            let code = m - 1;
            let kind = (code % 6) as usize;
            // White: visual row 7-rank; Black: mirrored row (symmetric tables).
            let row = if code < 6 { 7 - (sq >> 3) } else { sq >> 3 };
            let i = (row << 3) | (sq & 7);
            let v = material(kind) + pst(kind)[i] as i32;
            if code < 6 {
                acc += v;
            } else {
                acc -= v;
            }
        }
    }
    acc
}

/// **Mover-positive** centipawns (the A1 leaf hook): positive = side to move
/// is better.
///
/// C'est le **point d'entrée unique** de la valeur du projet : la feuille
/// du MCTS et celle de l'alpha-beta passent toutes les deux par ici. C'est
/// la condition pour que le bruit de la phase AB7 (`noise_offset`) soit
/// vu **exactement pareil** par les deux algorithmes.
#[inline(always)]
pub fn eval_for_mover(pos: &Position) -> i32 {
    // Le bruit est une erreur du **plateau**, exprimée dans le cadre
    // « Blanc » : la même position, jouée par les blancs ou par les
    // noirs, est mal évaluée du même côté. Il change donc de signe avec
    // le trait, comme la valeur elle-même.
    let raw = eval_white(pos) + noise_offset(pos);
    if pos.side == Color::White {
        raw
    } else {
        -raw
    }
}

/// Conservative insufficient-material test (v1): no pawns/rooks/queens on
/// board and at most one minor piece per side — K vs K, K+minor vs K are
/// dead positions (draw). K+B/K+N vs K+B/K+N still possible mates are NOT
/// covered; C3/A1 may refine. Deliberately conservative = never converts a
/// forced win into a draw.
#[inline]
pub fn insufficient_material(pos: &Position) -> bool {
    if pos.bb_piece[PAWN] != 0 {
        return false;
    }
    if pos.bb_piece[ROOK] != 0 || pos.bb_piece[QUEEN] != 0 {
        return false; // rook or queen
    }
    let w = (pos.bb_piece[KNIGHT] & pos.bb_color[WHITE]).count_ones()
        + (pos.bb_piece[BISHOP] & pos.bb_color[WHITE]).count_ones();
    let b = (pos.bb_piece[KNIGHT] & pos.bb_color[BLACK]).count_ones()
        + (pos.bb_piece[BISHOP] & pos.bb_color[BLACK]).count_ones();
    w <= 1 && b <= 1
}
