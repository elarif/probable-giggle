//! Part 4 du post : décalages de bitboard, `SquaresBetween`, `IsAttackedBy`.

use crate::position::Position;
use crate::slider::{bishop_attacks, rook_attacks};
use crate::tables::{FILE_A, FILE_H, KING_ATT, KNIGHT_ATT, PAWN_ATTACKERS, RANK_1, RANK_8};
use crate::types::*;

/// Décalages du post (`Bitboards.ShiftN`, `ShiftS`, …). Le masque de bord est
/// appliqué **avant** le décalage, comme dans le listing : un bit sur la
/// rangée 8 ne « réapparaît » pas sur la rangée 1.
#[inline(always)]
pub const fn shift_n(b: u64) -> u64 {
    (b & !RANK_8) << 8
}
#[inline(always)]
pub const fn shift_s(b: u64) -> u64 {
    (b & !RANK_1) >> 8
}
#[inline(always)]
pub const fn shift_e(b: u64) -> u64 {
    (b & !FILE_H) << 1
}
#[inline(always)]
pub const fn shift_w(b: u64) -> u64 {
    (b & !FILE_A) >> 1
}
#[inline(always)]
pub const fn shift_ne(b: u64) -> u64 {
    (b & !RANK_8 & !FILE_H) << 9
}
#[inline(always)]
pub const fn shift_nw(b: u64) -> u64 {
    (b & !RANK_8 & !FILE_A) << 7
}
#[inline(always)]
pub const fn shift_se(b: u64) -> u64 {
    (b & !RANK_1 & !FILE_H) >> 7
}
#[inline(always)]
pub const fn shift_sw(b: u64) -> u64 {
    (b & !RANK_1 & !FILE_A) >> 9
}

/// Cases strictement entre `a` et `b` s'ils sont alignés, sinon 0.
///
/// Le post déclare cette table (`Legality.SquaresBetween`) ; elle existe déjà
/// dans ce crate sous la forme de [`crate::tables::BETWEEN`], construite par
/// la même marche de rayon. La dupliquer ici ne prouvait rien : une copie
/// égale à son original passe n'importe quel test d'égalité. La preuve
/// d'exactitude est donc portée par `tests/gct_attacks.rs`, qui la compare à
/// une identité de bitboards indépendante.
#[inline(always)]
pub fn squares_between(a: u8, b: u8) -> u64 {
    crate::slider::between(a, b)
}

/// `Attacks.IsAttackedBy` du post : `sq` est-il attaqué par la couleur `by` ?
///
/// Pièces bon marché d'abord (pion, cavalier, roi), glissantes ensuite — comme
/// le post. Le post écrit `Attacks.Pawn[1 - byColor, sq] & pawns` : par
/// symétrie c'est exactement `PAWN_ATTACKERS[by][sq]`, que `mychess` fournit
/// déjà. `tests/gct_attacks.rs` fait deux vérifications distinctes : un
/// contrôle de dérive face à `Position::is_square_attacked` (même algorithme,
/// donc pas un oracle indépendant) et un oracle géométrique indépendant.
#[inline]
pub fn is_attacked_by(pos: &Position, sq: u8, by: Color) -> bool {
    let them = pos.color_bb(by);
    if them == 0 {
        return false;
    }
    let i = by.idx();
    if PAWN_ATTACKERS[i][sq as usize] & pos.bb_piece[PAWN] & them != 0 {
        return true;
    }
    if KNIGHT_ATT[sq as usize] & pos.bb_piece[KNIGHT] & them != 0 {
        return true;
    }
    if KING_ATT[sq as usize] & pos.bb_piece[KING] & them != 0 {
        return true;
    }
    let occ = pos.occ();
    if rook_attacks(sq, occ) & (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]) & them != 0 {
        return true;
    }
    bishop_attacks(sq, occ) & (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]) & them != 0
}
