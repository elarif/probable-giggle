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

/// Le bruit **côté trait** : c'est le bruit tel que la feuille le voit.
///
/// [`noise_offset`] est exprimé dans le cadre « Blanc » (le bruit
/// appartient au plateau, pas au camp au trait) ; la valeur, elle, est
/// côté trait. Cette fonction fait la conversion, comme
/// [`eval_for_mover`] la fait pour l'évaluation.
///
/// Elle existe pour que le bruit reste un **instrument de mesure** même
/// quand la feuille est la **valeur apprise** : sans elle, `EvalNN=1`
/// désactivait silencieusement le bruit (la phase AB7 perdait son
/// dispositif de mesure).
pub fn noise_for_mover(pos: &Position) -> i32 {
    let n = noise_offset(pos);
    if pos.side == Color::White {
        n
    } else {
        -n
    }
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
#[cfg(test)]
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

/// La valeur d'une case pour un genre de pièce : **matériel + PST**, dans
/// la convention « rang = `sq >> 3` » (c'est la convention de la table
/// *noire* ; la table *blanche* est la même, lue à rang renversé).
const fn pst_value(kind: usize, i: usize) -> i16 {
    let p = match kind {
        PAWN => PST_PAWN[i],
        KNIGHT => PST_KNIGHT[i],
        BISHOP => PST_BISHOP[i],
        ROOK => PST_ROOK[i],
        QUEEN => PST_QUEEN[i],
        _ => PST_KING[i],
    };
    p + MATERIAL[kind] as i16
}

/// La table **fusionnée** matériel + PST : une seule lecture par pièce.
///
/// Elle est indexée par `sq` avec la convention « rang = `sq >> 3` » :
/// * une pièce **noire** en `sq` lit `PSTV[kind][sq]` ;
/// * une pièce **blanche** en `sq` lit `PSTV[kind][sq ^ 56]` (le rang
///   renversé, c'est-à-dire la lecture « visuelle » de la table
///   symétrique).
///
/// La fusion du matériel **dans** la table PST est ce qui rend le
/// parcours bitboard possible : le compte du matériel n'a plus besoin
/// d'un `popcount` séparé — il est déjà dans la valeur de la case.
const PSTV: [[i16; 64]; 6] = {
    let mut out = [[0i16; 64]; 6];
    let mut k = 0;
    while k < 6 {
        let mut i = 0;
        while i < 64 {
            out[k][i] = pst_value(k, i);
            i += 1;
        }
        k += 1;
    }
    out
};

/// Evaluation in centipawns, **White-positive**.
///
/// **Parcours bitboard** (phase perf, 2026-10-23) : on ne visite que les
/// cases **occupées** (≈ 32) au lieu des 64 cases de la boîte aux
/// lettres, et on supprime la branche « case vide ? » par case — c'est
/// cette branche qui coûtait le plus (prédiction ratée sur ~50 % des
/// cases).
///
/// La **valeur est identique bit à bit** à l'ancienne version boîte aux
/// lettres : mêmes termes, même somme (l'addition entière est
/// associative), mêmes index de table. C'est vérifié par un test
/// d'identité (`eval_white_reference` ci-dessous) sur un corpus de
/// plusieurs milliers de positions.
#[inline]
pub fn eval_white(pos: &Position) -> i32 {
    let mut acc = 0i32;
    for kind in 0..6 {
        let pieces = pos.bb_piece[kind];
        if pieces == 0 {
            continue;
        }
        // Les blancs : rang renversé (`sq ^ 56`).
        let mut w = pieces & pos.bb_color[WHITE];
        while w != 0 {
            let sq = w.trailing_zeros() as usize;
            acc += PSTV[kind][sq ^ 56] as i32;
            w &= w - 1;
        }
        // Les noirs : rang tel quel.
        let mut b = pieces & pos.bb_color[BLACK];
        while b != 0 {
            let sq = b.trailing_zeros() as usize;
            acc -= PSTV[kind][sq] as i32;
            b &= b - 1;
        }
    }
    acc
}

/// L'ancienne évaluation (parcours de la **boîte aux lettres**), gardée
/// comme **référence** : c'est elle que le test d'identité compare à la
/// version bitboard. Elle n'est compilée qu'en test.
#[cfg(test)]
fn eval_white_reference(pos: &Position) -> i32 {
    let mut acc = 0i32;
    for (sq, m) in pos.mailbox.iter().enumerate() {
        if *m != 0 {
            let code = m - 1;
            let kind = (code % 6) as usize;
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

/// L'évaluation **par lot** (phase NN, tranche N0).
///
/// C'est le **contrat** de l'interface par lot : un lot de positions est
/// évalué, et chaque valeur est **exactement** celle de
/// [`eval_for_mover`] pour la position correspondante. Le contrat est
/// volontairement trivial — le lot est évalué position par position —
/// parce que c'est lui que l'inférence SIMD du réseau consommera : le
/// réseau lit un lot de positions et rend un lot de valeurs, avec la
/// **même convention de signe** que l'évaluation statique (positif = le
/// trait est meilleur).
///
/// Ce contrat est vérifié par un test d'identité (`eval_batch` contre
/// `eval_for_mover`) : sans lui, le réseau et l'évaluation statique
/// pourraient diverger de convention, et plus aucun test du dépôt ne
/// comparerait les deux.
pub fn eval_batch(positions: &[Position]) -> Vec<i32> {
    positions.iter().map(eval_for_mover).collect()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::{MoveList, generate};
    use crate::types::GenKind;
    /// Le corpus du test d'identité : les positions de référence, plus
    /// une marche aléatoire **déterministe** (graine fixe).
    fn corpus(n_positions: usize) -> Vec<Position> {
        const FENS: &[&str] = &[
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 b - - 0 1",
        ];
        let mut out: Vec<Position> = FENS
            .iter()
            .filter_map(|f| Position::from_fen(f).ok())
            .collect();
        let mut rng = 0x1234_5678_9ABC_DEF0u64;
        let mut next = || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        while out.len() < n_positions {
            let fen = FENS[(next() % FENS.len() as u64) as usize];
            let mut pos = Position::from_fen(fen).expect("fen de référence");
            let plies = (next() % 40) as usize;
            for _ in 0..plies {
                let mut list = MoveList::new();
                generate(&pos, &mut list, GenKind::All);
                if list.is_empty() {
                    break;
                }
                let mv = list.get((next() % list.len() as u64) as usize);
                pos.make(mv);
            }
            out.push(pos);
        }
        out
    }

    /// **L'identité de la valeur** : l'évaluation bitboard doit rendre
    /// **exactement** la même valeur que l'ancienne évaluation boîte
    /// aux lettres. C'est le gate de la phase perf : une évaluation qui
    /// change de valeur invalide *toutes* les mesures gelées des phases
    /// 3 à 5 (et le MCTS devient non reproductible).
    #[test]
    fn eval_white_is_bit_identical_to_the_mailbox_version() {
        for pos in corpus(4_000) {
            assert_eq!(
                eval_white(&pos),
                eval_white_reference(&pos),
                "la valeur a changé sur une position du corpus"
            );
        }
    }

    /// **Le contrat par lot** (phase NN, tranche N0) : `eval_batch` rend
    /// exactement les mêmes valeurs que `eval_for_mover`, position par
    /// position. C'est le gate de la tranche N0 : sans lui, le réseau
    /// pourrait être câblé avec une convention différente de celle de
    /// l'évaluation statique, et les deux ne seraient plus comparables.
    #[test]
    fn eval_batch_is_identical_to_eval_for_mover() {
        let positions = corpus(1_000);
        let batch = eval_batch(&positions);
        assert_eq!(batch.len(), positions.len());
        for (i, pos) in positions.iter().enumerate() {
            assert_eq!(
                batch[i],
                eval_for_mover(pos),
                "le lot diverge de l'évaluation unitaire sur la position {i}"
            );
        }
    }

    /// La boîte aux lettres et les bitboards doivent décrire **le même
    /// échiquier** : l'évaluation bitboard ne lit que les bitboards.
    #[test]
    fn mailbox_and_bitboards_agree() {
        for pos in corpus(500) {
            for sq in 0..64usize {
                let m = pos.mailbox[sq];
                let bit = 1u64 << sq;
                if m == 0 {
                    for kind in 0..6 {
                        assert_eq!(pos.bb_piece[kind] & bit, 0, "bitboard fantôme en {sq}");
                    }
                } else {
                    let code = (m - 1) as usize;
                    assert_ne!(pos.bb_piece[code % 6] & bit, 0, "bitboard manquant en {sq}");
                    assert_ne!(pos.bb_color[code / 6] & bit, 0, "couleur manquante en {sq}");
                }
            }
        }
    }

    /// L'évaluation ne dépend pas du trait (elle est exprimée dans le
    /// cadre « Blanc ») — une propriété déjà testée ailleurs, mais qui
    /// doit survivre au changement de représentation.
    #[test]
    fn eval_white_is_side_independent() {
        for pos in corpus(300) {
            let mut flipped = pos.clone();
            flipped.side = if pos.side == Color::White {
                Color::Black
            } else {
                Color::White
            };
            assert_eq!(eval_white(&pos), eval_white(&flipped));
        }
    }
}
