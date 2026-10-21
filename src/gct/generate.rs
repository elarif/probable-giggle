//! Part 7 du post : génération des coups **légaux**.
//!
//! Ordre du post, conservé : coups de roi d'abord, retour immédiat sur double
//! échec, calcul du `check_mask`, puis pions / cavaliers / glissantes, et le
//! roque en dernier (tâche 5).
//!
//! Corrections et simplifications assumées :
//!   - pas de `delegate AttackFn` : les appels `bishop_attacks` / `rook_attacks`
//!     sont directs et monomorphisés ;
//!   - la dame est traitée en un passage (`bishop ∪ rook`) au lieu de deux,
//!     ce qui supprime le doublon de la Part 4 sans changer l'ensemble émis ;
//!   - `pin_lines[from]` vaut `u64::MAX` pour les pièces non clouées, donc le
//!     masquage est inconditionnel — c'est déjà le contrat de
//!     [`crate::gct::legality::compute_pins`] ;
//!   - les cibles excluent toujours la case du roi ennemi : le post ne le fait
//!     pas, mais un coup de capture de roi n'est jamais légal et le contrat
//!     d'entrée du projet (GATE 2.5 M3) impose qu'aucun tel coup ne soit émis,
//!     même sur entrée illégale. Sur une position légale c'est un no-op.

use crate::gct::attacks::squares_between;
use crate::gct::attacks::{shift_n, shift_ne, shift_nw, shift_s, shift_se, shift_sw};
use crate::gct::legality::{compute_checkers, compute_pins, king_danger};
use crate::movegen::MoveList;
use crate::position::Position;
use crate::slider::{bishop_attacks, rook_attacks};
use crate::tables::{KING_ATT, KNIGHT_ATT, RANK_1, RANK_3, RANK_6, RANK_8};
use crate::types::*;

/// Génère tous les coups légaux de la position et renvoie le nouvel indice du
/// buffer (contrat de la série : « append into a caller-owned buffer, return
/// the new index »).
pub fn generate(pos: &Position, out: &mut MoveList) -> usize {
    let us = pos.side;
    let them = us.opp();

    // Contrat d'entrée (GATE 2.5 M3) : position légale exigée. Roi absent =
    // entrée illégale : aucun coup légal, silencieux, sans panique.
    // `king_sq` porte un `debug_assert!` sur un roi absent (profil debug
    // seulement) : on teste `has_king` AVANT de l'appeler.
    if !pos.has_king(us) {
        return out.len();
    }
    let ksq = pos.king_sq(us);

    let occ = pos.occ();
    let us_bb = pos.color_bb(us);
    let them_king = pos.bb_piece[KING] & pos.color_bb(them);
    // Les pièces adverses capturables : jamais le roi.
    let enemy = pos.color_bb(them) & !them_king;
    // Cases qu'aucune pièce ne peut rejoindre : les nôtres et le roi ennemi.
    let blocked = us_bb | them_king;

    let danger = king_danger(pos, ksq, them);
    let checkers = compute_checkers(pos, ksq, them);
    let (pinned, pin_lines) = compute_pins(pos, ksq, them, us);

    // 6. Coups de roi : toujours en premier, toujours sans masque d'échec.
    gen_king_moves(out, ksq, blocked, danger, enemy);

    // 7. Double échec : seuls les coups de roi sont légaux.
    let ncheck = checkers.count_ones();
    if ncheck >= 2 {
        return out.len();
    }

    // 8. check_mask = capturer le vérificateur ou bloquer la ligne.
    let check_mask = if ncheck == 1 {
        let c = checkers.trailing_zeros() as u8;
        checkers | squares_between(ksq, c)
    } else {
        u64::MAX
    };

    // 9. Pièces.
    gen_pawns::<true>(pos, out, check_mask, pinned, &pin_lines, occ, enemy, us);
    gen_pawns::<false>(pos, out, check_mask, pinned, &pin_lines, occ, enemy, us);
    gen_knights(pos, out, blocked, check_mask, &pin_lines, enemy);
    gen_sliders(pos, out, blocked, check_mask, &pin_lines, occ, enemy);

    // 10. Roque — ajouté à la tâche 5.
    out.len()
}

/// `MoveGenerator.EmitTargets` (Part 4) : captures d'abord, puis coups calmes.
#[inline]
fn emit_targets(out: &mut MoveList, from: u8, targets: u64, enemy: u64) {
    let mut caps = targets & enemy;
    while caps != 0 {
        let to = caps.trailing_zeros() as u8;
        caps &= caps - 1;
        out.push(Move::new(from, to, FLAG_CAPTURE));
    }
    let mut quiets = targets & !enemy;
    while quiets != 0 {
        let to = quiets.trailing_zeros() as u8;
        quiets &= quiets - 1;
        out.push(Move::new(from, to, FLAG_QUIET));
    }
}

/// `GenKingMoves` : `!blocked` (nos pièces et le roi ennemi) et `!danger`.
#[inline]
fn gen_king_moves(out: &mut MoveList, ksq: u8, blocked: u64, danger: u64, enemy: u64) {
    let targets = KING_ATT[ksq as usize] & !blocked & !danger;
    emit_targets(out, ksq, targets, enemy);
}

/// `GenKnightsLegal` : un cavalier cloué ne bouge jamais (sa ligne de clouage
/// n'intersecte aucune de ses cibles), donc `pin_lines[sq]` le neutralise.
#[inline]
fn gen_knights(
    pos: &Position,
    out: &mut MoveList,
    blocked: u64,
    check_mask: u64,
    pin_lines: &[u64; 64],
    enemy: u64,
) {
    let mut kn = pos.bb_piece[KNIGHT] & pos.color_bb(pos.side);
    while kn != 0 {
        let s = kn.trailing_zeros() as u8;
        kn &= kn - 1;
        // `blocked` couvre nos pièces et le roi ennemi : aucune cible ne peut
        // donc être une capture de roi, même sur entrée illégale.
        let targets = KNIGHT_ATT[s as usize] & !blocked & check_mask & pin_lines[s as usize];
        emit_targets(out, s, targets, enemy);
    }
}

/// `GenSlidersLegal` : fous, tours, puis dames en un seul passage.
#[inline]
fn gen_sliders(
    pos: &Position,
    out: &mut MoveList,
    blocked: u64,
    check_mask: u64,
    pin_lines: &[u64; 64],
    occ: u64,
    enemy: u64,
) {
    let us_bb = pos.color_bb(pos.side);
    let mut b = pos.bb_piece[BISHOP] & us_bb;
    while b != 0 {
        let s = b.trailing_zeros() as u8;
        b &= b - 1;
        let targets = bishop_attacks(s, occ) & !blocked & check_mask & pin_lines[s as usize];
        emit_targets(out, s, targets, enemy);
    }
    let mut r = pos.bb_piece[ROOK] & us_bb;
    while r != 0 {
        let s = r.trailing_zeros() as u8;
        r &= r - 1;
        let targets = rook_attacks(s, occ) & !blocked & check_mask & pin_lines[s as usize];
        emit_targets(out, s, targets, enemy);
    }
    let mut q = pos.bb_piece[QUEEN] & us_bb;
    while q != 0 {
        let s = q.trailing_zeros() as u8;
        q &= q - 1;
        let targets = (bishop_attacks(s, occ) | rook_attacks(s, occ))
            & !blocked
            & check_mask
            & pin_lines[s as usize];
        emit_targets(out, s, targets, enemy);
    }
}

/// `GenPawnsLegal` : poussées simples et doubles, captures, promotions.
///
/// Les cibles sont calculées en **bulk** (un décalage par famille de coups)
/// puis chaque cible retrouve sa case d'origine par `delta_from`, comme le
/// post (`EmitPawnLegal(..., deltaFrom, ...)`). C'est ce qui permet d'appliquer
/// le masque de clouage sans boucler sur les pions.
#[allow(clippy::too_many_arguments)]
fn gen_pawns<const W: bool>(
    pos: &Position,
    out: &mut MoveList,
    check_mask: u64,
    pinned: u64,
    pin_lines: &[u64; 64],
    occ: u64,
    enemy: u64,
    us: Color,
) {
    if us != (if W { Color::White } else { Color::Black }) {
        return;
    }
    let pawns = pos.bb_piece[PAWN] & pos.color_bb(us);
    if pawns == 0 {
        return;
    }
    let empty = !occ;

    if W {
        let one = shift_n(pawns) & empty;
        let two = shift_n(one & RANK_3) & empty;
        let promo = one & RANK_8;
        let quiet = one & !RANK_8;
        let cap_ne = shift_ne(pawns) & enemy;
        let cap_nw = shift_nw(pawns) & enemy;

        emit_pawn_pushes(out, promo & check_mask, 8, true, pinned, pin_lines);
        emit_pawn_pushes(out, quiet & check_mask, 8, false, pinned, pin_lines);
        emit_pawn_pushes(out, two & check_mask, 16, false, pinned, pin_lines);
        emit_pawn_capture(
            out,
            cap_ne & RANK_8 & check_mask,
            9,
            true,
            pinned,
            pin_lines,
        );
        emit_pawn_capture(
            out,
            cap_nw & RANK_8 & check_mask,
            7,
            true,
            pinned,
            pin_lines,
        );
        emit_pawn_capture(
            out,
            cap_ne & !RANK_8 & check_mask,
            9,
            false,
            pinned,
            pin_lines,
        );
        emit_pawn_capture(
            out,
            cap_nw & !RANK_8 & check_mask,
            7,
            false,
            pinned,
            pin_lines,
        );
    } else {
        let one = shift_s(pawns) & empty;
        let two = shift_s(one & RANK_6) & empty;
        let promo = one & RANK_1;
        let quiet = one & !RANK_1;
        let cap_se = shift_se(pawns) & enemy;
        let cap_sw = shift_sw(pawns) & enemy;

        emit_pawn_pushes(out, promo & check_mask, -8, true, pinned, pin_lines);
        emit_pawn_pushes(out, quiet & check_mask, -8, false, pinned, pin_lines);
        emit_pawn_pushes(out, two & check_mask, -16, false, pinned, pin_lines);
        emit_pawn_capture(
            out,
            cap_se & RANK_1 & check_mask,
            -7,
            true,
            pinned,
            pin_lines,
        );
        emit_pawn_capture(
            out,
            cap_sw & RANK_1 & check_mask,
            -9,
            true,
            pinned,
            pin_lines,
        );
        emit_pawn_capture(
            out,
            cap_se & !RANK_1 & check_mask,
            -7,
            false,
            pinned,
            pin_lines,
        );
        emit_pawn_capture(
            out,
            cap_sw & !RANK_1 & check_mask,
            -9,
            false,
            pinned,
            pin_lines,
        );
    }
}

/// Une pièce clouée ne peut atteindre que les cases de sa ligne de clouage.
#[inline(always)]
fn pin_allows(from: u8, to: u8, pinned: u64, pin_lines: &[u64; 64]) -> bool {
    pinned & (1u64 << from) == 0 || pin_lines[from as usize] & (1u64 << to) != 0
}

/// `EmitPawnLegal` pour les poussées (simples, doubles, promotions de poussée).
#[inline]
fn emit_pawn_pushes(
    out: &mut MoveList,
    targets: u64,
    delta_from: i16,
    promotion: bool,
    pinned: u64,
    pin_lines: &[u64; 64],
) {
    let mut t = targets;
    while t != 0 {
        let to = t.trailing_zeros() as u8;
        t &= t - 1;
        let from = (to as i16 - delta_from) as u8;
        if !pin_allows(from, to, pinned, pin_lines) {
            continue;
        }
        if promotion {
            for i in 0..4u8 {
                out.push(Move::new(from, to, FLAG_PROMO_N + i));
            }
        } else {
            let flag = if delta_from.unsigned_abs() as u8 == 16 {
                FLAG_DOUBLE_PUSH
            } else {
                FLAG_QUIET
            };
            out.push(Move::new(from, to, flag));
        }
    }
}

/// `EmitPromos` (version capture) et `EmitPawnLegal` (version capture).
#[inline]
fn emit_pawn_capture(
    out: &mut MoveList,
    targets: u64,
    delta_from: i16,
    promotion: bool,
    pinned: u64,
    pin_lines: &[u64; 64],
) {
    let mut t = targets;
    while t != 0 {
        let to = t.trailing_zeros() as u8;
        t &= t - 1;
        let from = (to as i16 - delta_from) as u8;
        if !pin_allows(from, to, pinned, pin_lines) {
            continue;
        }
        if promotion {
            for i in 0..4u8 {
                out.push(Move::new(from, to, FLAG_PROMO_CAP_N + i));
            }
        } else {
            out.push(Move::new(from, to, FLAG_CAPTURE));
        }
    }
}
