//! Board position: bitboards, mailbox, incremental Zobrist, make/unmake.

use crate::slider::{bishop_attacks, rook_attacks};
use crate::tables::{KING_ATT, KNIGHT_ATT, PAWN_ATTACKERS};
use crate::types::*;
use crate::zobrist::ZOBRIST;
use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum FenError {
    Fields,
    Rank(String),
    Piece(char),
    Side(String),
    Castling(String),
    Ep(String),
    Number(String),
}

impl fmt::Display for FenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for FenError {}

/// State needed to undo a move.
#[derive(Clone, Copy)]
pub struct Undo {
    pub captured: u8, // mailbox code (1..=12), 0 if none
    pub castle: u8,
    pub ep: u8,
    pub halfmove: u8,
    pub zobrist: u64,
}

#[derive(Clone, Copy)]
pub struct Position {
    pub bb_piece: [u64; 6],
    pub bb_color: [u64; 2],
    /// `0` = empty, else piece code + 1 (1..=12).
    pub mailbox: [u8; 64],
    pub side: Color,
    pub castle: u8,
    pub ep: u8, // NO_EP (64) when none
    pub halfmove: u8,
    pub zobrist: u64,
}

impl Position {
    pub fn empty() -> Position {
        Position {
            bb_piece: [0; 6],
            bb_color: [0; 2],
            mailbox: [0; 64],
            side: Color::White,
            castle: 0,
            ep: NO_EP,
            halfmove: 0,
            zobrist: 0,
        }
    }

    pub fn startpos() -> Position {
        Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap()
    }

    #[inline(always)]
    pub fn occ(&self) -> u64 {
        self.bb_color[0] | self.bb_color[1]
    }
    #[inline(always)]
    pub fn color_bb(&self, c: Color) -> u64 {
        self.bb_color[c.idx()]
    }
    #[inline(always)]
    pub fn piece_bb(&self, k: usize) -> u64 {
        self.bb_piece[k]
    }
    /// Square of the king of `c`. *Contract (GATE 2.5 M3): a legal position
    /// is required — both kings present, neither capturable.* A kingless or
    /// king-capturable position is an illegal entry: `debug_assert!` aborts
    /// with the contract message, while in release the result is defined but
    /// out of contract (the generators never emit a king capture, so engine
    /// paths never build a kingless position).
    #[inline(always)]
    pub fn king_sq(&self, c: Color) -> u8 {
        let bb = self.bb_piece[KING] & self.bb_color[c.idx()];
        debug_assert!(
            bb != 0,
            "king_sq: roi absent (position légale exigée — entrée illégale: roi capturé ou prenable)"
        );
        bb.trailing_zeros() as u8
    }
    #[inline(always)]
    pub fn pieces(&self, c: Color, k: usize) -> u64 {
        self.bb_piece[k] & self.bb_color[c.idx()]
    }
    #[inline(always)]
    pub fn piece_on(&self, sq: u8) -> u8 {
        self.mailbox[sq as usize]
    }

    #[inline(always)]
    pub fn ep_square(&self) -> Option<u8> {
        if self.ep == NO_EP { None } else { Some(self.ep) }
    }

    #[inline(always)]
    pub fn is_square_attacked(&self, sq: u8, by: Color) -> bool {
        let bi = by.idx();
        let b = self.bb_color[bi];
        if b == 0 {
            return false;
        }
        if PAWN_ATTACKERS[bi][sq as usize] & self.bb_piece[PAWN] & b != 0 {
            return true;
        }
        if KNIGHT_ATT[sq as usize] & self.bb_piece[KNIGHT] & b != 0 {
            return true;
        }
        if KING_ATT[sq as usize] & self.bb_piece[KING] & b != 0 {
            return true;
        }
        let occ = self.occ();
        if rook_attacks(sq, occ) & (self.bb_piece[ROOK] | self.bb_piece[QUEEN]) & b != 0 {
            return true;
        }
        if bishop_attacks(sq, occ) & (self.bb_piece[BISHOP] | self.bb_piece[QUEEN]) & b != 0 {
            return true;
        }
        false
    }

    #[inline(always)]
    pub fn in_check(&self, c: Color) -> bool {
        self.is_square_attacked(self.king_sq(c), c.opp())
    }

    /// True if `mv` (legal for the side to move) leaves the opponent in check.
    pub fn gives_check(&self, mv: Move) -> bool {
        let mut p = *self;
        p.make(mv);
        p.in_check(self.side.opp())
    }

    #[inline(always)]
    fn clear_square(&mut self, sq: usize) {
        let code = (self.mailbox[sq] - 1) as usize;
        let bit = 1u64 << sq;
        self.bb_piece[code % 6] &= !bit;
        self.bb_color[code / 6] &= !bit;
        self.mailbox[sq] = 0;
    }
    #[inline(always)]
    fn set_square(&mut self, sq: usize, code: u8) {
        let c = code as usize;
        let bit = 1u64 << sq;
        self.bb_piece[c % 6] |= bit;
        self.bb_color[c / 6] |= bit;
        self.mailbox[sq] = code + 1;
    }

    pub fn make(&mut self, mv: Move) -> Undo {
        let from = mv.from() as usize;
        let to = mv.to() as usize;
        let flags = mv.flags();
        let code = self.mailbox[from] - 1;
        let us = (code / 6) as usize;
        let kind = (code % 6) as usize;

        let undo = Undo {
            captured: 0,
            castle: self.castle,
            ep: self.ep,
            halfmove: self.halfmove,
            zobrist: self.zobrist,
        };

        let mut captured = 0u8;
        let mut cap_sq = to;
        if flags == FLAG_EP_CAPTURE {
            cap_sq = if us == WHITE { to - 8 } else { to + 8 };
            captured = self.mailbox[cap_sq];
            self.clear_square(cap_sq);
        } else if flags & 4 != 0 {
            captured = self.mailbox[to];
            self.clear_square(to);
        }

        self.clear_square(from);
        self.set_square(to, code);

        let z = &ZOBRIST;
        let mut zob = self.zobrist;
        zob ^= z.piece[code as usize][from];
        zob ^= z.piece[code as usize][to];
        if captured != 0 {
            zob ^= z.piece[(captured - 1) as usize][cap_sq];
        }

        if flags >= FLAG_PROMO_N {
            let pk = KNIGHT + ((flags - FLAG_PROMO_N) % 4) as usize;
            let newcode = (us * 6 + pk) as u8;
            self.clear_square(to);
            self.set_square(to, newcode);
            zob ^= z.piece[us * 6 + PAWN][to];
            zob ^= z.piece[newcode as usize][to];
        }

        if flags == FLAG_KING_CASTLE || flags == FLAG_QUEEN_CASTLE {
            let (rf, rt) = castle_rook_squares(us, flags == FLAG_KING_CASTLE);
            let rook_code = (us * 6 + ROOK) as u8;
            self.clear_square(rf);
            self.set_square(rt, rook_code);
            zob ^= z.piece[rook_code as usize][rf];
            zob ^= z.piece[rook_code as usize][rt];
        }

        let old_castle = self.castle;
        self.castle &= !(CASTLE_MASK_LUT[from] | CASTLE_MASK_LUT[to]);
        if old_castle != self.castle {
            zob ^= z.castle[old_castle as usize] ^ z.castle[self.castle as usize];
        }

        if self.ep != NO_EP {
            zob ^= z.ep_file[(self.ep & 7) as usize];
        }
        self.ep = if flags == FLAG_DOUBLE_PUSH { ((from + to) / 2) as u8 } else { NO_EP };
        if self.ep != NO_EP {
            zob ^= z.ep_file[(self.ep & 7) as usize];
        }

        self.halfmove = if kind == PAWN || captured != 0 { 0 } else { self.halfmove + 1 };
        self.side = self.side.opp();
        zob ^= z.side;
        self.zobrist = zob;

        Undo { captured, ..undo }
    }

    pub fn unmake(&mut self, mv: Move, undo: Undo) {
        let from = mv.from() as usize;
        let to = mv.to() as usize;
        let flags = mv.flags();
        let us = self.side.opp();

        let moved = self.mailbox[to] - 1;
        self.clear_square(to);
        if flags >= FLAG_PROMO_N {
            let pawn_code = (us.idx() * 6 + PAWN) as u8;
            self.set_square(from, pawn_code);
        } else {
            self.set_square(from, moved);
        }
        if flags == FLAG_EP_CAPTURE {
            let cap_sq = if us.idx() == WHITE { to - 8 } else { to + 8 };
            self.set_square(cap_sq, undo.captured - 1);
        } else if undo.captured != 0 {
            self.set_square(to, undo.captured - 1);
        }

        if flags == FLAG_KING_CASTLE || flags == FLAG_QUEEN_CASTLE {
            let (rf, rt) = castle_rook_squares(us.idx(), flags == FLAG_KING_CASTLE);
            let rook_code = (us.idx() * 6 + ROOK) as u8;
            self.clear_square(rt);
            self.set_square(rf, rook_code);
        }

        self.side = us;
        self.castle = undo.castle;
        self.ep = undo.ep;
        self.halfmove = undo.halfmove;
        self.zobrist = undo.zobrist;
    }

    /// Full Zobrist recomputation (used to validate the incremental hash).
    pub fn recompute_zobrist(&self) -> u64 {
        let z = &ZOBRIST;
        let mut h = 0u64;
        for sq in 0..64usize {
            let m = self.mailbox[sq];
            if m != 0 {
                h ^= z.piece[(m - 1) as usize][sq];
            }
        }
        h ^= z.castle[self.castle as usize];
        if self.ep != NO_EP {
            h ^= z.ep_file[(self.ep & 7) as usize];
        }
        if self.side == Color::Black {
            h ^= z.side;
        }
        h
    }

    pub fn from_fen(fen: &str) -> Result<Position, FenError> {
        let parts: Vec<&str> = fen.split_whitespace().collect();
        if parts.len() < 4 {
            return Err(FenError::Fields);
        }
        let mut pos = Position::empty();
        let mut r = 7i32;
        for row in parts[0].split('/') {
            if !(0..8).contains(&r) {
                return Err(FenError::Rank(parts[0].to_string()));
            }
            let mut f = 0i32;
            for c in row.chars() {
                if let Some(n) = c.to_digit(10) {
                    f += n as i32;
                } else {
                    if !(0..8).contains(&f) {
                        return Err(FenError::Rank(parts[0].to_string()));
                    }
                    let color = if c.is_ascii_uppercase() { Color::White } else { Color::Black };
                    let kind = match c.to_ascii_lowercase() {
                        'p' => PieceKind::Pawn,
                        'n' => PieceKind::Knight,
                        'b' => PieceKind::Bishop,
                        'r' => PieceKind::Rook,
                        'q' => PieceKind::Queen,
                        'k' => PieceKind::King,
                        _ => return Err(FenError::Piece(c)),
                    };
                    let code = (color.idx() * 6 + kind.idx()) as u8;
                    pos.set_square((r * 8 + f) as usize, code);
                    f += 1;
                }
            }
            r -= 1;
        }
        pos.side = match parts[1] {
            "w" => Color::White,
            "b" => Color::Black,
            other => return Err(FenError::Side(other.to_string())),
        };
        pos.castle = 0;
        if parts[2] != "-" {
            for c in parts[2].chars() {
                pos.castle |= match c {
                    'K' => CASTLE_WK,
                    'Q' => CASTLE_WQ,
                    'k' => CASTLE_BK,
                    'q' => CASTLE_BQ,
                    _ => return Err(FenError::Castling(parts[2].to_string())),
                };
            }
        }
        pos.ep = if parts[3] == "-" {
            NO_EP
        } else {
            let b = parts[3].as_bytes();
            if b.len() != 2 {
                return Err(FenError::Ep(parts[3].to_string()));
            }
            let f = b[0].wrapping_sub(b'a');
            let rr = b[1].wrapping_sub(b'1');
            if f >= 8 || rr >= 8 {
                return Err(FenError::Ep(parts[3].to_string()));
            }
            rr * 8 + f
        };
        pos.halfmove =
            parts.get(4).and_then(|s| s.parse().ok()).ok_or(FenError::Number("halfmove".into()))?;
        // Validate fullmove field if present (not stored).
        if let Some(s) = parts.get(5)
            && s.parse::<u32>().is_err() {
                return Err(FenError::Number("fullmove".into()));
            }
        pos.zobrist = pos.recompute_zobrist();
        Ok(pos)
    }
}

#[inline(always)]
pub fn castle_rook_squares(color_idx: usize, king_side: bool) -> (usize, usize) {
    let back = if color_idx == WHITE { 0 } else { 56 };
    if king_side { (back + 7, back + 5) } else { (back, back + 3) }
}

#[inline(always)]
pub fn castle_mask(sq: u8) -> u8 {
    match sq {
        4 => CASTLE_WK | CASTLE_WQ,
        0 => CASTLE_WQ,
        7 => CASTLE_WK,
        60 => CASTLE_BK | CASTLE_BQ,
        56 => CASTLE_BQ,
        63 => CASTLE_BK,
        _ => 0,
    }
}

/// L4 micro-tuning: branch-free castle-rights mask per square. `make` applies
/// `castle &= !(LUT[from] | LUT[to])` instead of two 7-arm matches.
const fn build_castle_mask_lut() -> [u8; 64] {
    let mut t = [0u8; 64];
    t[4] = CASTLE_WK | CASTLE_WQ;
    t[0] = CASTLE_WQ;
    t[7] = CASTLE_WK;
    t[60] = CASTLE_BK | CASTLE_BQ;
    t[56] = CASTLE_BQ;
    t[63] = CASTLE_BK;
    t
}
pub(crate) const CASTLE_MASK_LUT: [u8; 64] = build_castle_mask_lut();

impl fmt::Debug for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f)?;
        for r in (0..8).rev() {
            write!(f, "{} ", r + 1)?;
            for c in 0..8 {
                let m = self.mailbox[r * 8 + c];
                if m == 0 {
                    write!(f, ". ")?;
                } else {
                    let code = m - 1;
                    let kind = PieceKind::from_idx((code % 6) as usize);
                    write!(f, "{} ", kind.char(code / 6 == 0))?;
                }
            }
            writeln!(f)?;
        }
        writeln!(f, "  a b c d e f g h")?;
        writeln!(
            f,
            "side={:?} castle={:#06b} ep={} halfmove={} zobrist={:#018x}",
            self.side, self.castle, self.ep, self.halfmove, self.zobrist
        )
    }
}
