//! Legal move generation with checkmask / pinmask, in bulk where possible.

use crate::position::Position;
use crate::slider::{between, bishop_attacks, rook_attacks};
use crate::tables::{
    DIR_POS, FILE_A, FILE_H, KING_ATT, KNIGHT_ATT, PAWN_ATTACKERS, PAWN_ATTACKS, RANK_3, RANK_6,
    RANK_8, RAYS,
};
use crate::types::*;

/// Fixed-capacity move list on the stack: no allocation in the hot path.
pub struct MoveList {
    moves: [Move; 256],
    len: u8,
}

impl MoveList {
    #[inline]
    pub fn new() -> MoveList {
        MoveList { moves: [Move::NONE; 256], len: 0 }
    }
    #[inline(always)]
    pub fn push(&mut self, m: Move) {
        debug_assert!((self.len as usize) < 256);
        self.moves[self.len as usize] = m;
        self.len += 1;
    }
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len as usize
    }
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    #[inline(always)]
    pub fn get(&self, i: usize) -> Move {
        self.moves[i]
    }
    pub fn as_slice(&self) -> &[Move] {
        &self.moves[..self.len as usize]
    }
    pub fn iter(&self) -> impl Iterator<Item = Move> + '_ {
        self.moves[..self.len as usize].iter().copied()
    }
}

impl Default for MoveList {
    fn default() -> Self {
        Self::new()
    }
}

#[inline(always)]
fn attacks_by(pos: &Position, by: Color, occ: u64) -> u64 {
    let b = pos.bb_color[by.idx()];
    let pawns = pos.bb_piece[PAWN] & b;
    let mut att = if by == Color::White {
        ((pawns & !FILE_H) << 9) | ((pawns & !FILE_A) << 7)
    } else {
        ((pawns & !FILE_H) >> 7) | ((pawns & !FILE_A) >> 9)
    };
    let mut n = pos.bb_piece[KNIGHT] & b;
    while n != 0 {
        att |= KNIGHT_ATT[n.trailing_zeros() as usize];
        n &= n - 1;
    }
    let k = pos.bb_piece[KING] & b;
    if k != 0 {
        att |= KING_ATT[k.trailing_zeros() as usize];
    }
    let mut rq = (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]) & b;
    while rq != 0 {
        att |= rook_attacks(rq.trailing_zeros() as u8, occ);
        rq &= rq - 1;
    }
    let mut bq = (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]) & b;
    while bq != 0 {
        att |= bishop_attacks(bq.trailing_zeros() as u8, occ);
        bq &= bq - 1;
    }
    att
}

#[inline(always)]
pub(crate) fn attackers_to(pos: &Position, sq: u8, by: Color) -> u64 {
    let bi = by.idx();
    let b = pos.bb_color[bi];
    let occ = pos.occ();
    let mut m = PAWN_ATTACKERS[bi][sq as usize] & pos.bb_piece[PAWN] & b;
    m |= KNIGHT_ATT[sq as usize] & pos.bb_piece[KNIGHT] & b;
    m |= KING_ATT[sq as usize] & pos.bb_piece[KING] & b;
    m |= rook_attacks(sq, occ) & (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]) & b;
    m |= bishop_attacks(sq, occ) & (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]) & b;
    m
}

/// Pinned pieces of `us`, split by pin orientation.
pub fn compute_pins(pos: &Position, us: Color, ksq: u8) -> (u64, u64) {
    let occ = pos.occ();
    let mut diag = 0u64;
    let mut ortho = 0u64;
    for d in 0..8 {
        let ray = RAYS[d][ksq as usize];
        let blockers = ray & occ;
        if blockers == 0 {
            continue;
        }
        let nearest = if DIR_POS[d] {
            blockers.trailing_zeros()
        } else {
            63 - blockers.leading_zeros()
        };
        let fb = 1u64 << nearest;
        if pos.bb_color[us.idx()] & fb == 0 {
            continue;
        }
        let beyond = RAYS[d][nearest as usize] & occ;
        if beyond == 0 {
            continue;
        }
        let second = if DIR_POS[d] {
            beyond.trailing_zeros()
        } else {
            63 - beyond.leading_zeros()
        };
        let sb = 1u64 << second;
        let slider = if d % 2 == 0 {
            pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]
        } else {
            pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]
        };
        if pos.bb_color[us.opp().idx()] & sb != 0 && slider & sb != 0 {
            if d % 2 == 0 {
                ortho |= fb;
            } else {
                diag |= fb;
            }
        }
    }
    (diag, ortho)
}

#[inline(always)]
fn pin_line(ksq: u8, from: u8) -> u64 {
    for ray in &RAYS {
        let r = ray[ksq as usize];
        if r & (1u64 << from) != 0 {
            return r | between(ksq, from);
        }
    }
    0
}

fn push_promotions(out: &mut MoveList, from: u8, to: u8, is_cap: bool) {
    let base = if is_cap { FLAG_PROMO_CAP_N } else { FLAG_PROMO_N };
    out.push(Move::new(from, to, base));
    out.push(Move::new(from, to, base + 1));
    out.push(Move::new(from, to, base + 2));
    out.push(Move::new(from, to, base + 3));
}

/// Generate all legal moves (or noisy moves for `CapturesOnly`).
pub fn generate(pos: &Position, out: &mut MoveList, kind: GenKind) {
    let us = pos.side;
    let them = us.opp();
    let occ = pos.occ();
    let us_bb = pos.color_bb(us);
    let them_bb = pos.color_bb(them);
    let empty = !occ;
    let ksq = pos.king_sq(us);
    let checkers = attackers_to(pos, ksq, them);
    let ncheck = checkers.count_ones();

    // King moves. The king is removed from occupancy so sliders x-ray through it.
    let danger = attacks_by(pos, them, occ ^ (1u64 << ksq));
    let mut kt = KING_ATT[ksq as usize] & !us_bb & !danger;
    while kt != 0 {
        let to = kt.trailing_zeros() as u8;
        kt &= kt - 1;
        let cap = (them_bb >> to) & 1 != 0;
        out.push(Move::new(ksq, to, if cap { FLAG_CAPTURE } else { FLAG_QUIET }));
    }

    if ncheck >= 2 {
        return; // double check: only king moves
    }

    let allowed = if ncheck == 1 {
        let c = checkers.trailing_zeros() as u8;
        between(ksq, c) | checkers
    } else {
        !0u64
    };
    let (pin_diag, pin_ortho) = compute_pins(pos, us, ksq);
    let pinned = pin_diag | pin_ortho;

    let captures_only = kind == GenKind::CapturesOnly;

    // ---- Pawns ----
    let pawns = pos.bb_piece[PAWN] & us_bb;
    let free = pawns & !pinned;
    gen_pawns_bulk(pos, out, us, them_bb, free, allowed, empty, captures_only);
    let mut pp = pawns & pinned;
    while pp != 0 {
        let from = pp.trailing_zeros() as u8;
        pp &= pp - 1;
        gen_pawn_pinned(pos, out, us, them_bb, from, allowed & pin_line(ksq, from), empty, captures_only);
    }

    // En passant. Validated with a local make/unmake: capturing en passant can
    // remove two pawns from a rank and expose the king to a rook/queen.
    if let Some(ep) = pos.ep_square() {
        let mut m = PAWN_ATTACKERS[us.idx()][ep as usize] & pawns;
        while m != 0 {
            let from = m.trailing_zeros() as u8;
            m &= m - 1;
            let mv = Move::new(from, ep, FLAG_EP_CAPTURE);
            let mut b = *pos;
            b.make(mv);
            if !b.is_square_attacked(b.king_sq(us), them) {
                out.push(mv);
            }
        }
    }

    // ---- Knights (a pinned knight never moves) ----
    let mut kn = pos.bb_piece[KNIGHT] & us_bb & !pinned;
    while kn != 0 {
        let s = kn.trailing_zeros() as u8;
        kn &= kn - 1;
        let t = KNIGHT_ATT[s as usize] & !us_bb & allowed;
        emit_slider(out, s, t, them_bb, captures_only);
    }

    // ---- Bishops + queens (diagonal part) ----
    let mut bq = (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]) & us_bb;
    while bq != 0 {
        let s = bq.trailing_zeros() as u8;
        bq &= bq - 1;
        let mut t = bishop_attacks(s, occ) & !us_bb & allowed;
        if pinned & (1u64 << s) != 0 {
            t &= pin_line(ksq, s);
        }
        emit_slider(out, s, t, them_bb, captures_only);
    }

    // ---- Rooks + queens (orthogonal part) ----
    let mut rq = (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]) & us_bb;
    while rq != 0 {
        let s = rq.trailing_zeros() as u8;
        rq &= rq - 1;
        let mut t = rook_attacks(s, occ) & !us_bb & allowed;
        if pinned & (1u64 << s) != 0 {
            t &= pin_line(ksq, s);
        }
        emit_slider(out, s, t, them_bb, captures_only);
    }

    // ---- Castling ----
    if kind == GenKind::All && ncheck == 0 {
        gen_castle(pos, out, us, occ);
    }
}

#[inline]
fn emit_slider(out: &mut MoveList, from: u8, targets: u64, them: u64, captures_only: bool) {
    let caps = targets & them;
    let mut c = caps;
    while c != 0 {
        let to = c.trailing_zeros() as u8;
        c &= c - 1;
        out.push(Move::new(from, to, FLAG_CAPTURE));
    }
    if captures_only {
        return;
    }
    let mut q = targets & !them;
    while q != 0 {
        let to = q.trailing_zeros() as u8;
        q &= q - 1;
        out.push(Move::new(from, to, FLAG_QUIET));
    }
}

#[allow(clippy::too_many_arguments)]
fn gen_pawns_bulk(
    _pos: &Position,
    out: &mut MoveList,
    us: Color,
    them: u64,
    pawns: u64,
    allowed: u64,
    empty: u64,
    captures_only: bool,
) {
    if pawns == 0 {
        return;
    }
    if us == Color::White {
        let push = (pawns << 8) & empty;
        let promo = push & RANK_8 & allowed;
        let quiet = push & !RANK_8 & allowed;
        let double = ((push & RANK_3) << 8) & empty & allowed;
        let cap_r = ((pawns & !FILE_H) << 9) & them & allowed;
        let cap_l = ((pawns & !FILE_A) << 7) & them & allowed;

        emit_pawn_pushes(out, promo, 8, true, captures_only, false);
        emit_pawn_pushes(out, quiet, 8, false, captures_only, false);
        emit_pawn_pushes(out, double, 16, false, captures_only, false);
        emit_pawn_caps(out, cap_r, 9, true, captures_only);
        emit_pawn_caps(out, cap_l, 7, true, captures_only);
    } else {
        let push = (pawns >> 8) & empty;
        let promo = push & crate::tables::RANK_1 & allowed;
        let quiet = push & !crate::tables::RANK_1 & allowed;
        let double = ((push & RANK_6) >> 8) & empty & allowed;
        let cap_r = ((pawns & !FILE_H) >> 7) & them & allowed;
        let cap_l = ((pawns & !FILE_A) >> 9) & them & allowed;

        emit_pawn_pushes(out, promo, 8, true, captures_only, true);
        emit_pawn_pushes(out, quiet, 8, false, captures_only, true);
        emit_pawn_pushes(out, double, 16, false, captures_only, true);
        emit_pawn_caps_down(out, cap_r, 7, true, captures_only);
        emit_pawn_caps_down(out, cap_l, 9, true, captures_only);
    }
}

#[inline]
fn emit_pawn_pushes(
    out: &mut MoveList,
    targets: u64,
    back: u8,
    promo: bool,
    captures_only: bool,
    down: bool,
) {
    let mut t = targets;
    while t != 0 {
        let to = t.trailing_zeros() as u8;
        t &= t - 1;
        let from = if down { to + back } else { to - back };
        if promo {
            push_promotions(out, from, to, false);
        } else if !captures_only {
            let flag = if back == 16 { FLAG_DOUBLE_PUSH } else { FLAG_QUIET };
            out.push(Move::new(from, to, flag));
        }
    }
}

#[inline]
fn emit_pawn_caps(out: &mut MoveList, targets: u64, back: u8, _cap: bool, _captures_only: bool) {
    let promo_mask = RANK_8;
    let mut t = targets;
    while t != 0 {
        let to = t.trailing_zeros() as u8;
        t &= t - 1;
        if promo_mask & (1u64 << to) != 0 {
            push_promotions(out, to - back, to, true);
        } else {
            out.push(Move::new(to - back, to, FLAG_CAPTURE));
        }
    }
}

#[inline]
fn emit_pawn_caps_down(out: &mut MoveList, targets: u64, back: u8, _cap: bool, _captures_only: bool) {
    let promo_mask = crate::tables::RANK_1;
    let mut t = targets;
    while t != 0 {
        let to = t.trailing_zeros() as u8;
        t &= t - 1;
        if promo_mask & (1u64 << to) != 0 {
            push_promotions(out, to + back, to, true);
        } else {
            out.push(Move::new(to + back, to, FLAG_CAPTURE));
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn gen_pawn_pinned(
    _pos: &Position,
    out: &mut MoveList,
    us: Color,
    them: u64,
    from: u8,
    mask: u64,
    _empty: u64,
    captures_only: bool,
) {
    if us == Color::White {
        let one = from + 8;
        if _pos.piece_on(one) == 0 && mask & (1u64 << one) != 0 {
            if one >= 56 {
                push_promotions(out, from, one, false);
            } else if !captures_only {
                out.push(Move::new(from, one, FLAG_QUIET));
            }
        }
        if (8..16).contains(&from) && _pos.piece_on(from + 8) == 0 && _pos.piece_on(from + 16) == 0 {
            let two = from + 16;
            if mask & (1u64 << two) != 0 && !captures_only {
                out.push(Move::new(from, two, FLAG_DOUBLE_PUSH));
            }
        }
        for df in [-1i8, 1] {
            let tf = (from % 8) as i8 + df;
            if !(0..8).contains(&tf) {
                continue;
            }
            let to = (from as i8 + 8 + df) as u8;
            if them & (1u64 << to) != 0 && mask & (1u64 << to) != 0 {
                if to >= 56 {
                    push_promotions(out, from, to, true);
                } else {
                    out.push(Move::new(from, to, FLAG_CAPTURE));
                }
            }
        }
    } else {
        let one = from - 8;
        if _pos.piece_on(one) == 0 && mask & (1u64 << one) != 0 {
            if one < 8 {
                push_promotions(out, from, one, false);
            } else if !captures_only {
                out.push(Move::new(from, one, FLAG_QUIET));
            }
        }
        if (48..56).contains(&from) && _pos.piece_on(from - 8) == 0 && _pos.piece_on(from - 16) == 0 {
            let two = from - 16;
            if mask & (1u64 << two) != 0 && !captures_only {
                out.push(Move::new(from, two, FLAG_DOUBLE_PUSH));
            }
        }
        for df in [-1i8, 1] {
            let tf = (from % 8) as i8 + df;
            if !(0..8).contains(&tf) {
                continue;
            }
            let to = (from as i8 - 8 + df) as u8;
            if them & (1u64 << to) != 0 && mask & (1u64 << to) != 0 {
                if to < 8 {
                    push_promotions(out, from, to, true);
                } else {
                    out.push(Move::new(from, to, FLAG_CAPTURE));
                }
            }
        }
    }
}

fn gen_castle(pos: &Position, out: &mut MoveList, us: Color, occ: u64) {
    let them = us.opp();
    let back = if us.idx() == WHITE { 0 } else { 56 };
    let (ks_bit, qs_bit) = if us.idx() == WHITE {
        (CASTLE_WK, CASTLE_WQ)
    } else {
        (CASTLE_BK, CASTLE_BQ)
    };
    if pos.castle & ks_bit != 0 {
        let f = back + 5;
        let g = back + 6;
        if occ & (1 << f) == 0
            && occ & (1 << g) == 0
            && !pos.is_square_attacked(f, them)
            && !pos.is_square_attacked(g, them)
        {
            out.push(Move::new(back + 4, back + 6, FLAG_KING_CASTLE));
        }
    }
    if pos.castle & qs_bit != 0 {
        let b = back + 1;
        let c = back + 2;
        let d = back + 3;
        if occ & (1 << b) == 0
            && occ & (1 << c) == 0
            && occ & (1 << d) == 0
            && !pos.is_square_attacked(d, them)
            && !pos.is_square_attacked(c, them)
        {
            out.push(Move::new(back + 4, back + 2, FLAG_QUEEN_CASTLE));
        }
    }
}

/// Convenience: generate and collect into a fresh list.
pub fn generate_all(pos: &Position) -> MoveList {
    let mut l = MoveList::new();
    generate(pos, &mut l, GenKind::All);
    l
}

pub fn generate_captures(pos: &Position) -> MoveList {
    let mut l = MoveList::new();
    generate(pos, &mut l, GenKind::CapturesOnly);
    l
}

/// Leaf-only fast path (L1): count legal moves WITHOUT emitting them into a
/// `MoveList`. Mirrors `generate` exactly — same checkmask, pins, en-passant
/// validation and castling conditions — but replaces every `push` by a
/// popcount over the corresponding target bitboard.
///
/// Used by perft at `depth == 1`: a leaf position needs only `MoveList::len`,
/// never the moves themselves. Promotions count 4 moves per target square and
/// en passant / castling keep their exact legality gates.
///
/// Differential-checked against `generate(.., All).len()` on golden positions
/// and thousands of self-play positions (tests).
pub fn count_moves(pos: &Position) -> u64 {
    let us = pos.side;
    let them = us.opp();
    let occ = pos.occ();
    let us_bb = pos.color_bb(us);
    let them_bb = pos.color_bb(them);
    let empty = !occ;
    let ksq = pos.king_sq(us);
    let checkers = attackers_to(pos, ksq, them);
    let ncheck = checkers.count_ones();

    // King moves (always legal here: `!us_bb & !danger`).
    let danger = attacks_by(pos, them, occ ^ (1u64 << ksq));
    let mut n = (KING_ATT[ksq as usize] & !us_bb & !danger).count_ones() as u64;
    if ncheck >= 2 {
        return n; // double check: only king moves
    }

    let allowed = if ncheck == 1 {
        let c = checkers.trailing_zeros() as u8;
        between(ksq, c) | checkers
    } else {
        !0u64
    };
    let (pin_diag, pin_ortho) = compute_pins(pos, us, ksq);
    let pinned = pin_diag | pin_ortho;

    // ---- Pawns (bulk target bitboards) ----
    let pawns = pos.bb_piece[PAWN] & us_bb;
    let free = pawns & !pinned;
    n += count_pawns_bulk(us, them_bb, free, allowed, empty);
    let mut pp = pawns & pinned;
    while pp != 0 {
        let from = pp.trailing_zeros() as u8;
        pp &= pp - 1;
        n += count_pawn_pinned(pos, us, them_bb, from, allowed & pin_line(ksq, from), empty);
    }

    // ---- En passant: same make/unmake validation as in generate ----
    if let Some(ep) = pos.ep_square() {
        let mut m = PAWN_ATTACKERS[us.idx()][ep as usize] & pawns;
        while m != 0 {
            let from = m.trailing_zeros() as u8;
            m &= m - 1;
            let mv = Move::new(from, ep, FLAG_EP_CAPTURE);
            let mut b = *pos;
            b.make(mv);
            if !b.is_square_attacked(b.king_sq(us), them) {
                n += 1;
            }
        }
    }

    // ---- Knights (a pinned knight never moves) ----
    let mut kn = pos.bb_piece[KNIGHT] & us_bb & !pinned;
    while kn != 0 {
        let s = kn.trailing_zeros() as u8;
        kn &= kn - 1;
        n += (KNIGHT_ATT[s as usize] & !us_bb & allowed).count_ones() as u64;
    }

    // ---- Bishops + queens (diagonal part) ----
    let mut bq = (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]) & us_bb;
    while bq != 0 {
        let s = bq.trailing_zeros() as u8;
        bq &= bq - 1;
        let mut t = bishop_attacks(s, occ) & !us_bb & allowed;
        if pinned & (1u64 << s) != 0 {
            t &= pin_line(ksq, s);
        }
        n += t.count_ones() as u64;
    }

    // ---- Rooks + queens (orthogonal part) ----
    let mut rq = (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]) & us_bb;
    while rq != 0 {
        let s = rq.trailing_zeros() as u8;
        rq &= rq - 1;
        let mut t = rook_attacks(s, occ) & !us_bb & allowed;
        if pinned & (1u64 << s) != 0 {
            t &= pin_line(ksq, s);
        }
        n += t.count_ones() as u64;
    }

    // ---- Castling (All-kind semantics, only when not in check) ----
    if ncheck == 0 {
        n += count_castles(pos, us, occ);
    }
    n
}

/// Popcount twin of `gen_pawns_bulk` for `GenKind::All`.
fn count_pawns_bulk(us: Color, them: u64, pawns: u64, allowed: u64, empty: u64) -> u64 {
    if pawns == 0 {
        return 0;
    }
    if us == Color::White {
        let push = (pawns << 8) & empty;
        let promo = push & RANK_8 & allowed;
        let quiet = push & !RANK_8 & allowed;
        let double = ((push & RANK_3) << 8) & empty & allowed;
        let cap_r = ((pawns & !FILE_H) << 9) & them & allowed;
        let cap_l = ((pawns & !FILE_A) << 7) & them & allowed;
        // Each promotion square yields 4 moves (N/B/R/Q).
        4 * promo.count_ones() as u64
            + quiet.count_ones() as u64
            + double.count_ones() as u64
            + 4 * (cap_r & RANK_8).count_ones() as u64
            + (cap_r & !RANK_8).count_ones() as u64
            + 4 * (cap_l & RANK_8).count_ones() as u64
            + (cap_l & !RANK_8).count_ones() as u64
    } else {
        let push = (pawns >> 8) & empty;
        let promo = push & crate::tables::RANK_1 & allowed;
        let quiet = push & !crate::tables::RANK_1 & allowed;
        let double = ((push & RANK_6) >> 8) & empty & allowed;
        let cap_r = ((pawns & !FILE_H) >> 7) & them & allowed;
        let cap_l = ((pawns & !FILE_A) >> 9) & them & allowed;
        4 * promo.count_ones() as u64
            + quiet.count_ones() as u64
            + double.count_ones() as u64
            + 4 * (cap_r & crate::tables::RANK_1).count_ones() as u64
            + (cap_r & !crate::tables::RANK_1).count_ones() as u64
            + 4 * (cap_l & crate::tables::RANK_1).count_ones() as u64
            + (cap_l & !crate::tables::RANK_1).count_ones() as u64
    }
}

/// Popcount twin of `gen_pawn_pinned` (White and Black) for `GenKind::All`.
fn count_pawn_pinned(pos: &Position, us: Color, them: u64, from: u8, mask: u64, _empty: u64) -> u64 {
    let mut n = 0;
    if us == Color::White {
        let one = from + 8;
        if pos.piece_on(one) == 0 && mask & (1u64 << one) != 0 {
            n += if one >= 56 { 4 } else { 1 };
        }
        if (8..16).contains(&from) && pos.piece_on(from + 8) == 0 && pos.piece_on(from + 16) == 0 {
            let two = from + 16;
            if mask & (1u64 << two) != 0 {
                n += 1;
            }
        }
        for df in [-1i8, 1] {
            let tf = (from % 8) as i8 + df;
            if !(0..8).contains(&tf) {
                continue;
            }
            let to = (from as i8 + 8 + df) as u8;
            if them & (1u64 << to) != 0 && mask & (1u64 << to) != 0 {
                n += if to >= 56 { 4 } else { 1 };
            }
        }
    } else {
        let one = from - 8;
        if pos.piece_on(one) == 0 && mask & (1u64 << one) != 0 {
            n += if one < 8 { 4 } else { 1 };
        }
        if (48..56).contains(&from) && pos.piece_on(from - 8) == 0 && pos.piece_on(from - 16) == 0 {
            let two = from - 16;
            if mask & (1u64 << two) != 0 {
                n += 1;
            }
        }
        for df in [-1i8, 1] {
            let tf = (from % 8) as i8 + df;
            if !(0..8).contains(&tf) {
                continue;
            }
            let to = (from as i8 - 8 + df) as u8;
            if them & (1u64 << to) != 0 && mask & (1u64 << to) != 0 {
                n += if to < 8 { 4 } else { 1 };
            }
        }
    }
    n
}

/// Popcount twin of `gen_castle` (counts 1 per legal side).
fn count_castles(pos: &Position, us: Color, occ: u64) -> u64 {
    let them = us.opp();
    let back = if us.idx() == WHITE { 0 } else { 56 };
    let (ks_bit, qs_bit) = if us.idx() == WHITE {
        (CASTLE_WK, CASTLE_WQ)
    } else {
        (CASTLE_BK, CASTLE_BQ)
    };
    let mut n = 0;
    if pos.castle & ks_bit != 0 {
        let f = back + 5;
        let g = back + 6;
        if occ & (1 << f) == 0
            && occ & (1 << g) == 0
            && !pos.is_square_attacked(f, them)
            && !pos.is_square_attacked(g, them)
        {
            n += 1;
        }
    }
    if pos.castle & qs_bit != 0 {
        let b = back + 1;
        let c = back + 2;
        let d = back + 3;
        if occ & (1 << b) == 0
            && occ & (1 << c) == 0
            && occ & (1 << d) == 0
            && !pos.is_square_attacked(d, them)
            && !pos.is_square_attacked(c, them)
        {
            n += 1;
        }
    }
    n
}

// Keep PAWN_ATTACKS referenced (used by refgen / tests for documentation).
#[allow(dead_code)]
pub(crate) const _PAWN_ATTACKS: [[u64; 64]; 2] = PAWN_ATTACKS;
