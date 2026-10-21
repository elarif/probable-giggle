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
///
/// Field order matters: the small fields are grouped in the tail so the
/// struct stays 16 bytes (the same size as the original 5-field version).
#[derive(Clone, Copy)]
pub struct Undo {
    pub zobrist: u64,
    /// Halfmove clock (plies since the last capture or pawn move).
    pub halfmove: u8,
    pub captured: u8, // mailbox code (1..=12), 0 if none
    pub castle: u8,
    pub ep: u8,
    /// Square the captured piece was removed from (en-passant: behind the
    /// landing square; ordinary capture: the landing square). `u8::MAX`
    /// when nothing was captured.
    pub cap_sq: u8,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub bb_piece: [u64; 6],
    pub bb_color: [u64; 2],
    /// `0` = empty, else piece code + 1 (1..=12).
    pub mailbox: [u8; 64],
    pub zobrist: u64,
    /// Halfmove clock (plies since the last capture or pawn move).
    ///
    /// Saturating at 255: the 50-move rule only cares about `>= 100`, and
    /// the repetition window only needs a value well above it, so the
    /// counter never needs more than 8 bits. Saturating (rather than
    /// wrapping) keeps it monotonic and overflow-free.
    pub halfmove: u8,
    pub side: Color,
    pub castle: u8,
    pub ep: u8, // NO_EP (64) when none
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
    ///
    /// The release path is **total**: a kingless position yields
    /// [`NO_SQUARE`] (64) — which is exactly what `trailing_zeros()` of an
    /// empty bitboard returns, so no extra arithmetic or branch is needed.
    /// Callers that index a table with the result must therefore check for
    /// [`NO_SQUARE`]; the movegen does, and the "no king" case is handled
    /// as "no legal moves".
    #[inline(always)]
    pub fn king_sq(&self, c: Color) -> u8 {
        let bb = self.bb_piece[KING] & self.bb_color[c.idx()];
        debug_assert!(
            bb != 0,
            "king_sq: roi absent (position légale exigée — entrée illégale: roi capturé ou prunable)"
        );
        // `trailing_zeros()` of 0 is 64 == NO_SQUARE: a kingless position
        // falls out of the normal path with no branch.
        bb.trailing_zeros() as u8
    }

    /// `true` when the position has a king for `c`.
    #[inline(always)]
    pub fn has_king(&self, c: Color) -> bool {
        self.bb_piece[KING] & self.bb_color[c.idx()] != 0
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
        if self.ep == NO_EP {
            None
        } else {
            Some(self.ep)
        }
    }

    /// The square holding the *captured* pawn for an en-passant capture by
    /// `us`, if the en-passant square is set **and** a pawn of the
    /// opposite colour actually stands there.
    ///
    /// A FEN may name an en-passant square without the matching pawn (some
    /// tools emit the ep square unconditionally after any double push).
    /// Such an entry is syntactically valid but semantically inconsistent;
    /// treating it as a capture would remove whatever stands on the square
    /// (possibly nothing at all), so the ep capture is simply not
    /// available. This keeps `make`/`unmake` sound on any input.
    ///
    /// The check is written to be cheap on the perft hot path, where the
    /// en-passant square is absent in the overwhelming majority of nodes:
    /// the `NO_EP` test comes first, so the common case costs one
    /// compare and a branch.
    #[inline(always)]
    pub fn ep_capture_square(&self, us: Color) -> Option<u8> {
        let ep = self.ep;
        if ep == NO_EP {
            return None;
        }
        // The captured pawn sits one rank "behind" the ep square, from the
        // mover's point of view. On the first/last rank the shift leaves
        // the board — rejected below.
        let cap_sq = if us == Color::White {
            ep.wrapping_sub(8)
        } else {
            ep.wrapping_add(8)
        };
        if cap_sq >= 64 {
            return None;
        }
        // The captured piece must be an enemy pawn: mailbox code 1 = white
        // pawn, 7 = black pawn.
        let enemy_pawn = if us == Color::White { 7u8 } else { 1u8 };
        if self.mailbox[cap_sq as usize] != enemy_pawn {
            return None;
        }
        Some(cap_sq)
    }

    /// Un pion adverse peut-il capturer en passant sur `ep` ?
    ///
    /// Sert à canonicaliser la case ep : une case annoncée sans captureur
    /// n'a aucun effet légal, mais polluerait le hash Zobrist.
    #[inline(always)]
    fn has_ep_capturer(&self, ep: u8, us: usize) -> bool {
        let them = 1 - us;
        if ep >= 64 {
            return false;
        }
        PAWN_ATTACKERS[them][ep as usize] & self.bb_piece[PAWN] & self.bb_color[them] != 0
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

    /// Is the king of `c` in check?
    ///
    /// A kingless position is *not* in check (there is no king to
    /// attack) — the entry is out of contract, but the answer stays
    /// defined and cannot index out of bounds.
    #[inline(always)]
    pub fn in_check(&self, c: Color) -> bool {
        let ksq = self.king_sq(c);
        // A kingless position yields NO_SQUARE (64) — out of range for the
        // 64-entry attack tables, so it must be caught here.
        if ksq == NO_SQUARE {
            return false;
        }
        self.is_square_attacked(ksq, c.opp())
    }

    /// True if `mv` (legal for the side to move) leaves the opponent in check.
    pub fn gives_check(&self, mv: Move) -> bool {
        let mut p = *self;
        p.make(mv);
        p.in_check(self.side.opp())
    }

    /// Clear the piece on `sq`.
    ///
    /// The square is expected to hold a piece (the bitboards and the
    /// mailbox must stay in sync); the empty case is debug-asserted.
    ///
    /// The implementation is **branchless**: an empty square has
    /// `mailbox == 0`, so `m - 1` wraps to 255 — and `255 / 6 == 42` would
    /// index past `bb_color`. Masking the colour index with `& 1` keeps the
    /// access in range while leaving the result a no-op: an empty square
    /// has its bit clear in every bitboard, so clearing it changes
    /// nothing. This keeps the hot `make`/`unmake` path free of branches.
    #[inline(always)]
    fn clear_square(&mut self, sq: usize) {
        let m = self.mailbox[sq];
        debug_assert!(
            m != 0,
            "clear_square on an empty square (mailbox/bitboard desync)"
        );
        let code = m.wrapping_sub(1) as usize;
        let keep = !(1u64 << sq);
        self.bb_piece[code % 6] &= keep;
        self.bb_color[(code / 6) & 1] &= keep;
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
            cap_sq: NO_SQUARE,
            halfmove: self.halfmove,
            zobrist: self.zobrist,
        };

        let mut captured = 0u8;
        let mut cap_sq = to as u8;
        if flags == FLAG_EP_CAPTURE {
            let mover = if us == WHITE {
                Color::White
            } else {
                Color::Black
            };
            if let Some(sq) = self.ep_capture_square(mover) {
                cap_sq = sq;
                captured = self.mailbox[cap_sq as usize];
                self.clear_square(cap_sq as usize);
            }
        } else if flags & 4 != 0 {
            captured = self.mailbox[to];
            if captured != 0 {
                self.clear_square(to);
            }
        }
        let cap_sq_recorded = if captured != 0 { cap_sq } else { NO_SQUARE };

        self.clear_square(from);
        self.set_square(to, code);

        let z = &ZOBRIST;
        let mut zob = self.zobrist;
        zob ^= z.piece[code as usize][from];
        zob ^= z.piece[code as usize][to];
        if captured != 0 {
            zob ^= z.piece[(captured - 1) as usize][cap_sq as usize];
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
        // Canonicalisation : la case ep n'est conservee que si un pion adverse
        // peut effectivement y capturer. Sans cela, un double pas sans
        // captureur laisse une case « fantome » dans le hash Zobrist, et deux
        // positions identiques atteintes autrement hashent differemment
        // (repetition et transposition ratees). Le pion mobile est deja pose
        // sur `to` a ce stade, donc l'ensemble des captureurs est exact.
        // `to` est aussi la case du pion capturable (un double pas n'est
        // genere que pour un pion) : la condition d'attaquant ci-dessus
        // implique donc la presence du pion derriere la case ep, et
        // `ep_capture_square` n'a pas besoin d'etre rejoue ici.
        self.ep = if flags == FLAG_DOUBLE_PUSH {
            let candidate = ((from + to) / 2) as u8;
            if self.has_ep_capturer(candidate, us) {
                candidate
            } else {
                NO_EP
            }
        } else {
            NO_EP
        };
        if self.ep != NO_EP {
            zob ^= z.ep_file[(self.ep & 7) as usize];
        }

        self.halfmove = if kind == PAWN || captured != 0 {
            0
        } else {
            self.halfmove.saturating_add(1)
        };
        self.side = self.side.opp();
        zob ^= z.side;
        self.zobrist = zob;

        Undo {
            captured,
            cap_sq: cap_sq_recorded,
            ..undo
        }
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
        if undo.captured != 0 {
            // `cap_sq` is where the piece was actually removed from
            // (behind the landing square for an en-passant capture).
            self.set_square(undo.cap_sq as usize, undo.captured - 1);
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

    /// Parse a FEN string into a [`Position`].
    ///
    /// The parser is strict: a malformed board (wrong number of ranks, a
    /// rank that does not sum to 8, a piece off the board) is rejected
    /// rather than silently accepted. A FEN that is *syntactically* valid
    /// but *semantically* illegal (no king, two kings, an en-passant
    /// square with no pawn to capture) is still accepted — the engine
    /// contract for illegal positions is defined in
    /// [`crate::movegen::generate`], and the movegen layer must stay
    /// crash-free on such entries.
    pub fn from_fen(fen: &str) -> Result<Position, FenError> {
        let parts: Vec<&str> = fen.split_whitespace().collect();
        if parts.len() < 4 {
            return Err(FenError::Fields);
        }
        let mut pos = Position::empty();
        let ranks: Vec<&str> = parts[0].split('/').collect();
        if ranks.len() != 8 {
            return Err(FenError::Rank(parts[0].to_string()));
        }
        for (row_idx, row) in ranks.iter().enumerate() {
            // FEN rank 8 comes first; the board is indexed a1 = 0.
            let r = 7 - row_idx as i32;
            let mut f = 0i32;
            for c in row.chars() {
                if let Some(n) = c.to_digit(10) {
                    f += n as i32;
                } else {
                    if !(0..8).contains(&f) {
                        return Err(FenError::Rank(parts[0].to_string()));
                    }
                    let color = if c.is_ascii_uppercase() {
                        Color::White
                    } else {
                        Color::Black
                    };
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
            // A rank must describe exactly 8 files.
            if f != 8 {
                return Err(FenError::Rank(parts[0].to_string()));
            }
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
        // The clock is clamped to 255 (the 50-move threshold is 100; the
        // exact value beyond that is irrelevant and the counter saturates).
        pos.halfmove = parts
            .get(4)
            .and_then(|s| s.parse::<u16>().ok())
            .map(|v| v.min(255) as u8)
            .ok_or(FenError::Number("halfmove".into()))?;
        // Validate fullmove field if present (not stored).
        if let Some(s) = parts.get(5)
            && s.parse::<u32>().is_err()
        {
            return Err(FenError::Number("fullmove".into()));
        }
        // Normalize the castling rights: a FEN may claim a right the board
        // does not support (no king on e1, no rook on h1/a1). Such an
        // entry is out of contract, but it must not produce a castling
        // move that moves a piece which is not there — so the right is
        // dropped here, once, instead of being re-checked at every node.
        pos.castle &= pos.supported_castle_rights();
        // Meme canonicalisation qu'a la sortie de `make`, mais ici les deux
        // conditions sont necessaires : une FEN peut annoncer une case ep avec
        // un pion adverse qui l'attaque sans qu'aucun pion ne soit capturable
        // dessous (position impossible par le jeu). `ep_capture_square` porte
        // la condition « pion adverse derriere la case » ; `has_ep_capturer`
        // porte la condition « un pion du camp au trait attaque la case ».
        let capturable = pos.ep_capture_square(pos.side).is_some()
            && pos.has_ep_capturer(pos.ep, pos.side.opp().idx());
        if pos.ep != NO_EP && !capturable {
            pos.ep = NO_EP;
        }
        pos.zobrist = pos.recompute_zobrist();
        Ok(pos)
    }

    /// The castling rights the board actually supports: the king must be on
    /// its home square and the castling rook on its own.
    fn supported_castle_rights(&self) -> u8 {
        let mut rights = 0;
        for (color, back) in [(Color::White, 0usize), (Color::Black, 56usize)] {
            let kings = self.bb_piece[KING] & self.color_bb(color);
            let rooks = self.bb_piece[ROOK] & self.color_bb(color);
            if kings & (1u64 << (back + 4)) == 0 {
                continue;
            }
            if rooks & (1u64 << (back + 7)) != 0 {
                rights |= if color == Color::White {
                    CASTLE_WK
                } else {
                    CASTLE_BK
                };
            }
            if rooks & (1u64 << back) != 0 {
                rights |= if color == Color::White {
                    CASTLE_WQ
                } else {
                    CASTLE_BQ
                };
            }
        }
        rights
    }
}

#[inline(always)]
pub fn castle_rook_squares(color_idx: usize, king_side: bool) -> (usize, usize) {
    let back = if color_idx == WHITE { 0 } else { 56 };
    if king_side {
        (back + 7, back + 5)
    } else {
        (back, back + 3)
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
