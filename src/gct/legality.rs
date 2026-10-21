//! Part 7 du post : les trois bitboards de légalité, en **trois passes
//! séparées** comme dans la série.
//!
//! `crate::movegen` fusionne ces trois passes en une seule (`build_legality`,
//! gain mesuré +19 %) ; ce module ne le fait **pas**, volontairement : la
//! fidélité au post est l'objet de la phase 1, la fusion est une amélioration
//! de phase 2 (voir la section « Hors périmètre » du plan).

use crate::position::Position;
use crate::slider::{bishop_attacks, rook_attacks};
use crate::tables::{
    BETWEEN, BISHOP_DIRS, DIR_POS, KING_ATT, KNIGHT_ATT, PAWN_ATTACKERS, PAWN_ATTACKS, RAYS,
    ROOK_DIRS,
};
use crate::types::*;

/// `Legality.ComputeKingDanger` : cases attaquées par `them` **comme si notre
/// roi n'était pas sur l'échiquier**, pour qu'un slider derrière le roi reste
/// couvert.
///
/// Le post écrit une boucle par pièce ennemie (y compris par pion). C'est
/// conservé ici : le calcul bulk des pions serait une optimisation de phase 2,
/// pas une fidélité au post.
pub fn king_danger(pos: &Position, ksq: u8, them: Color) -> u64 {
    if ksq >= 64 {
        // Roi absent : entrée illégale, mais la fonction reste totale.
        return 0;
    }
    let occ_without_king = pos.occ() ^ (1u64 << ksq);
    let them_bb = pos.color_bb(them);
    let mut danger = 0u64;

    let mut pawns = pos.bb_piece[PAWN] & them_bb;
    while pawns != 0 {
        let sq = pawns.trailing_zeros() as u8;
        pawns &= pawns - 1;
        danger |= PAWN_ATTACKS[them.idx()][sq as usize];
    }

    let mut knights = pos.bb_piece[KNIGHT] & them_bb;
    while knights != 0 {
        let sq = knights.trailing_zeros() as u8;
        knights &= knights - 1;
        danger |= KNIGHT_ATT[sq as usize];
    }

    let mut bq = (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]) & them_bb;
    while bq != 0 {
        let sq = bq.trailing_zeros() as u8;
        bq &= bq - 1;
        danger |= bishop_attacks(sq, occ_without_king);
    }

    let mut rq = (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]) & them_bb;
    while rq != 0 {
        let sq = rq.trailing_zeros() as u8;
        rq &= rq - 1;
        danger |= rook_attacks(sq, occ_without_king);
    }

    let ek = pos.bb_piece[KING] & them_bb;
    if ek != 0 {
        danger |= KING_ATT[ek.trailing_zeros() as usize];
    }
    danger
}

/// `Legality.ComputeCheckers` : pièces ennemies qui attaquent notre roi.
pub fn compute_checkers(pos: &Position, ksq: u8, them: Color) -> u64 {
    if ksq >= 64 {
        return 0;
    }
    let occ = pos.occ();
    let them_bb = pos.color_bb(them);
    let pawns = pos.bb_piece[PAWN] & them_bb;
    let knights = pos.bb_piece[KNIGHT] & them_bb;
    let bq = (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]) & them_bb;
    let rq = (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]) & them_bb;
    (PAWN_ATTACKERS[them.idx()][ksq as usize] & pawns)
        | (KNIGHT_ATT[ksq as usize] & knights)
        | (bishop_attacks(ksq, occ) & bq)
        | (rook_attacks(ksq, occ) & rq)
}

/// `Legality.ComputePins` : pièces clouées et, pour chacune, la ligne sur
/// laquelle elle peut encore se déplacer (`between(ksq, pinner) | pinner`).
///
/// `pin_lines` est indexé **par case** et vaut `u64::MAX` partout, comme dans
/// le post : masquer les cibles d'une pièce par `pin_lines[from]` est alors un
/// no-op pour les pièces non clouées, sans test d'appartenance.
pub fn compute_pins(pos: &Position, ksq: u8, them: Color, us: Color) -> (u64, [u64; 64]) {
    let mut pin_lines = [u64::MAX; 64];
    if ksq >= 64 {
        return (0, pin_lines);
    }
    let occ = pos.occ();
    let us_bb = pos.color_bb(us);
    let them_bb = pos.color_bb(them);
    let enemy_rq = (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]) & them_bb;
    let enemy_bq = (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]) & them_bb;

    let mut pinned = scan_pinners(ksq, us_bb, occ, enemy_rq, ROOK_DIRS, &mut pin_lines);
    pinned |= scan_pinners(ksq, us_bb, occ, enemy_bq, BISHOP_DIRS, &mut pin_lines);
    (pinned, pin_lines)
}

/// `Legality.ScanPinners` : pour chaque direction, le premier bloqueur doit
/// être **à nous**, et le premier bloqueur suivant doit être un slider ennemi
/// de la bonne famille.
fn scan_pinners(
    ksq: u8,
    us_bb: u64,
    occ: u64,
    enemy_sliders: u64,
    dirs: [usize; 4],
    pin_lines: &mut [u64; 64],
) -> u64 {
    let mut pinned = 0u64;
    for &d in dirs.iter() {
        let ray = RAYS[d][ksq as usize];
        let blockers = ray & occ;
        if blockers == 0 {
            continue;
        }
        let first = nearest(blockers, d);
        let first_bit = 1u64 << first;
        if us_bb & first_bit == 0 {
            continue;
        }
        // `RAYS[d][first]` est exactement l'ensemble des cases situées
        // au-delà de `first` sur ce rayon : pas besoin de re-masquer.
        let beyond = RAYS[d][first as usize] & occ;
        if beyond == 0 {
            continue;
        }
        let second = nearest(beyond, d);
        let second_bit = 1u64 << second;
        if enemy_sliders & second_bit != 0 {
            pinned |= first_bit;
            pin_lines[first as usize] = BETWEEN[ksq as usize][second as usize] | second_bit;
        }
    }
    pinned
}

/// Case de `bb` la plus proche de l'origine du rayon, selon le sens `d`.
#[inline(always)]
fn nearest(bb: u64, d: usize) -> u8 {
    debug_assert!(bb != 0);
    if DIR_POS[d] {
        bb.trailing_zeros() as u8
    } else {
        (63 - bb.leading_zeros()) as u8
    }
}
