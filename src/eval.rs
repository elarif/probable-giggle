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
#[inline(always)]
pub fn eval_for_mover(pos: &Position) -> i32 {
    if pos.side == Color::White {
        eval_white(pos)
    } else {
        -eval_white(pos)
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
