//! Part 4 du post : décalages de bitboard, `SquaresBetween`, `IsAttackedBy`.

use crate::position::Position;
use crate::slider::{bishop_attacks, rook_attacks};
use crate::tables::{FILE_A, FILE_H, RANK_1, RANK_8};
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

/// `Legality.SquaresBetween` du post, reconstruite ici par marche de rayon
/// plutôt que lue depuis `tables::BETWEEN` : deux algorithmes indépendants qui
/// doivent coïncider (vérifié par `tests/gct_attacks.rs`).
const fn build_squares_between() -> [[u64; 64]; 64] {
    // Mêmes directions que `tables::DIRS` : N, NE, E, SE, S, SW, W, NW.
    const DIRS: [(i32, i32); 8] = [
        (0, 1),
        (1, 1),
        (1, 0),
        (1, -1),
        (0, -1),
        (-1, -1),
        (-1, 0),
        (-1, 1),
    ];
    let mut t = [[0u64; 64]; 64];
    let mut a = 0;
    while a < 64 {
        let mut d = 0;
        while d < 8 {
            let mut f = (a % 8) as i32;
            let mut r = (a / 8) as i32;
            let mut acc = 0u64;
            loop {
                f += DIRS[d].0;
                r += DIRS[d].1;
                if f < 0 || f >= 8 || r < 0 || r >= 8 {
                    break;
                }
                let s = (r * 8 + f) as usize;
                t[a][s] = acc;
                acc |= 1u64 << s;
            }
            d += 1;
        }
        a += 1;
    }
    t
}

static SQUARES_BETWEEN: [[u64; 64]; 64] = build_squares_between();

/// Cases strictement entre `a` et `b` s'ils sont alignés, sinon 0.
#[inline(always)]
pub fn squares_between(a: u8, b: u8) -> u64 {
    SQUARES_BETWEEN[a as usize][b as usize]
}

/// `Attacks.IsAttackedBy` du post : `sq` est-il attaqué par la couleur `by` ?
///
/// Pièces bon marché d'abord (pion, cavalier, roi), glissantes ensuite — comme
/// le post. Le post écrit `Attacks.Pawn[1 - byColor, sq] & pawns` : par
/// symétrie c'est exactement `PAWN_ATTACKERS[by][sq]`, que `mychess` fournit
/// déjà, et `tests/gct_attacks.rs` vérifie l'identité.
#[inline]
pub fn is_attacked_by(pos: &Position, sq: u8, by: Color) -> bool {
    let them = pos.color_bb(by);
    if them == 0 {
        return false;
    }
    let i = by.idx();
    if crate::tables::PAWN_ATTACKERS[i][sq as usize] & pos.bb_piece[PAWN] & them != 0 {
        return true;
    }
    if crate::tables::KNIGHT_ATT[sq as usize] & pos.bb_piece[KNIGHT] & them != 0 {
        return true;
    }
    if crate::tables::KING_ATT[sq as usize] & pos.bb_piece[KING] & them != 0 {
        return true;
    }
    let occ = pos.occ();
    if rook_attacks(sq, occ) & (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]) & them != 0 {
        return true;
    }
    bishop_attacks(sq, occ) & (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]) & them != 0
}
