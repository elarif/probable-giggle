//! Slow, obvious reference generator + naive perft. Oracle for differential
//! tests against the fast legal movegen. Deliberately naive (per-piece walk).

use crate::movegen::MoveList;
use crate::position::Position;
use crate::types::*;

const KNIGHT_DELTAS: [(i8, i8); 8] =
    [(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)];
const KING_DELTAS: [(i8, i8); 8] =
    [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
const BISHOP_DELTAS: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, -1), (-1, 1)];
const ROOK_DELTAS: [(i8, i8); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

#[inline]
fn step(sq: u8, df: i8, dr: i8) -> Option<u8> {
    let f = (sq % 8) as i8 + df;
    let r = (sq / 8) as i8 + dr;
    if (0..8).contains(&f) && (0..8).contains(&r) {
        Some((r * 8 + f) as u8)
    } else {
        None
    }
}

fn push_promotions(out: &mut MoveList, from: u8, to: u8, is_cap: bool) {
    let base = if is_cap { FLAG_PROMO_CAP_N } else { FLAG_PROMO_N };
    for i in 0..4 {
        out.push(Move::new(from, to, base + i));
    }
}

/// Pseudo-legal moves: may leave the moving side's king in check.
pub fn pseudo_legal(pos: &Position, out: &mut MoveList) {
    let us = pos.side;
    for sq in 0..64u8 {
        let m = pos.piece_on(sq);
        if m == 0 {
            continue;
        }
        let code = m - 1;
        if code / 6 != us.idx() as u8 {
            continue;
        }
        let kind = code % 6;
        match kind {
            k if k == PAWN as u8 => pseudo_pawn(pos, out, sq, us),
            k if k == KNIGHT as u8 => pseudo_leaper(pos, out, sq, us, &KNIGHT_DELTAS),
            k if k == KING as u8 => {
                pseudo_leaper(pos, out, sq, us, &KING_DELTAS);
                pseudo_castle(pos, out, sq, us);
            }
            k if k == BISHOP as u8 => pseudo_slider(pos, out, sq, us, &BISHOP_DELTAS),
            k if k == ROOK as u8 => pseudo_slider(pos, out, sq, us, &ROOK_DELTAS),
            _ => pseudo_slider(pos, out, sq, us, &KING_DELTAS), // queen
        }
    }
}

fn pseudo_leaper(pos: &Position, out: &mut MoveList, sq: u8, us: Color, deltas: &[(i8, i8)]) {
    for (df, dr) in deltas {
        if let Some(to) = step(sq, *df, *dr) {
            let m = pos.piece_on(to);
            if m == 0 {
                out.push(Move::new(sq, to, FLAG_QUIET));
            } else if (m - 1) / 6 != us.idx() as u8 {
                out.push(Move::new(sq, to, FLAG_CAPTURE));
            }
        }
    }
}

fn pseudo_slider(pos: &Position, out: &mut MoveList, sq: u8, us: Color, deltas: &[(i8, i8)]) {
    for (df, dr) in deltas {
        let mut cur = sq;
        while let Some(to) = step(cur, *df, *dr) {
            cur = to;
            let m = pos.piece_on(to);
            if m == 0 {
                out.push(Move::new(sq, to, FLAG_QUIET));
            } else {
                if (m - 1) / 6 != us.idx() as u8 {
                    out.push(Move::new(sq, to, FLAG_CAPTURE));
                }
                break;
            }
        }
    }
}

fn pseudo_pawn(pos: &Position, out: &mut MoveList, sq: u8, us: Color) {
    let white = us == Color::White;
    let dir: i8 = if white { 1 } else { -1 };
    let start_rank = if white { 1 } else { 6 };
    let promo_rank = if white { 7 } else { 0 };

    if let Some(one) = step(sq, 0, dir)
        && pos.piece_on(one) == 0 {
            if (one / 8) as i8 == promo_rank {
                push_promotions(out, sq, one, false);
            } else {
                out.push(Move::new(sq, one, FLAG_QUIET));
                if (sq / 8) as i8 == start_rank
                    && let Some(two) = step(sq, 0, 2 * dir)
                        && pos.piece_on(two) == 0 {
                            out.push(Move::new(sq, two, FLAG_DOUBLE_PUSH));
                        }
            }
        }
    for df in [-1i8, 1] {
        if let Some(to) = step(sq, df, dir) {
            let m = pos.piece_on(to);
            if m != 0 && (m - 1) / 6 != us.idx() as u8 {
                if (to / 8) as i8 == promo_rank {
                    push_promotions(out, sq, to, true);
                } else {
                    out.push(Move::new(sq, to, FLAG_CAPTURE));
                }
            } else if pos.ep_square() == Some(to) {
                out.push(Move::new(sq, to, FLAG_EP_CAPTURE));
            }
        }
    }
}

fn pseudo_castle(pos: &Position, out: &mut MoveList, sq: u8, us: Color) {
    let them = us.opp();
    let back = if us.idx() == WHITE { 0u8 } else { 56u8 };
    let (ks, qs) = if us.idx() == WHITE {
        (CASTLE_WK, CASTLE_WQ)
    } else {
        (CASTLE_BK, CASTLE_BQ)
    };
    if sq != back + 4 || pos.is_square_attacked(sq, them) {
        return;
    }
    if pos.castle & ks != 0
        && pos.piece_on(back + 5) == 0
        && pos.piece_on(back + 6) == 0
        && !pos.is_square_attacked(back + 5, them)
        && !pos.is_square_attacked(back + 6, them)
    {
        out.push(Move::new(sq, back + 6, FLAG_KING_CASTLE));
    }
    if pos.castle & qs != 0
        && pos.piece_on(back + 1) == 0
        && pos.piece_on(back + 2) == 0
        && pos.piece_on(back + 3) == 0
        && !pos.is_square_attacked(back + 3, them)
        && !pos.is_square_attacked(back + 2, them)
    {
        out.push(Move::new(sq, back + 2, FLAG_QUEEN_CASTLE));
    }
}

/// Fully legal moves (pseudo-legal filtered by make / own-king-safety / unmake).
pub fn legal_moves(pos: &Position) -> MoveList {
    let mut pseudo = MoveList::new();
    pseudo_legal(pos, &mut pseudo);
    let mut out = MoveList::new();
    let us = pos.side;
    for mv in pseudo.iter() {
        let mut p = *pos;
        let u = p.make(mv);
        if !p.is_square_attacked(p.king_sq(us), us.opp()) {
            out.push(mv);
        }
        p.unmake(mv, u);
    }
    out
}

/// Naive perft (oracle).
pub fn perft(pos: &mut Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let moves = legal_moves(pos);
    if depth == 1 {
        return moves.len() as u64;
    }
    let mut n = 0;
    for mv in moves.iter() {
        let u = pos.make(mv);
        n += perft(pos, depth - 1);
        pos.unmake(mv, u);
    }
    n
}
