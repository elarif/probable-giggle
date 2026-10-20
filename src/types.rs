//! Core primitive types: colors, pieces, squares, moves, generation kinds.
//!
//! Newtypes over `u8`/`u16` (no `transmute` integer -> enum). `Square` is
//! debug-asserted `< 64`.
//!
//! Board indexing is type-checked: a square is always a `Square`, never a
//! bare `usize` that happens to be in range. `Square::new` is the only
//! constructor and it debug-asserts the bound; `Square::from_index` is the
//! checked fallback used by the parsers.

use std::fmt;

pub const WHITE: usize = 0;
pub const BLACK: usize = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Color {
    White,
    Black,
}

impl Color {
    #[inline(always)]
    pub const fn idx(self) -> usize {
        match self {
            Color::White => WHITE,
            Color::Black => BLACK,
        }
    }
    #[inline(always)]
    pub const fn opp(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
}

// Piece kind indices (also the bitboard slot order).
pub const PAWN: usize = 0;
pub const KNIGHT: usize = 1;
pub const BISHOP: usize = 2;
pub const ROOK: usize = 3;
pub const QUEEN: usize = 4;
pub const KING: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceKind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl PieceKind {
    #[inline(always)]
    pub const fn idx(self) -> usize {
        match self {
            PieceKind::Pawn => PAWN,
            PieceKind::Knight => KNIGHT,
            PieceKind::Bishop => BISHOP,
            PieceKind::Rook => ROOK,
            PieceKind::Queen => QUEEN,
            PieceKind::King => KING,
        }
    }
    pub const fn from_idx(i: usize) -> PieceKind {
        match i {
            PAWN => PieceKind::Pawn,
            KNIGHT => PieceKind::Knight,
            BISHOP => PieceKind::Bishop,
            ROOK => PieceKind::Rook,
            QUEEN => PieceKind::Queen,
            _ => PieceKind::King,
        }
    }
    pub const fn char(self, upper: bool) -> char {
        let c = match self {
            PieceKind::Pawn => 'p',
            PieceKind::Knight => 'n',
            PieceKind::Bishop => 'b',
            PieceKind::Rook => 'r',
            PieceKind::Queen => 'q',
            PieceKind::King => 'k',
        };
        if upper { c.to_ascii_uppercase() } else { c }
    }
}

/// A board square, `0 = a1 .. 63 = h8`.
///
/// The inner value is private: the only way to build a `Square` is through
/// [`Square::new`] (debug-asserted) or [`Square::from_index`] (checked), so
/// an out-of-range square can never reach the bitboards or the mailbox.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Square(u8);

impl Square {
    /// Build a square from a raw index, debug-asserting the `< 64` bound.
    #[inline(always)]
    pub const fn new(i: u8) -> Square {
        debug_assert!(i < 64);
        Square(i)
    }

    /// Checked constructor: returns `None` for an out-of-range index.
    #[inline]
    pub const fn from_index(i: u8) -> Option<Square> {
        if i < 64 { Some(Square(i)) } else { None }
    }

    /// The raw index (`0 = a1 .. 63 = h8`).
    #[inline(always)]
    pub const fn index(self) -> u8 {
        self.0
    }

    /// The index as a `usize`, for array indexing.
    #[inline(always)]
    pub const fn idx(self) -> usize {
        self.0 as usize
    }

    /// File index, `0 = a .. 7 = h`.
    #[inline(always)]
    pub const fn file(self) -> u8 {
        self.0 & 7
    }

    /// Rank index, `0 = rank 1 .. 7 = rank 8`.
    #[inline(always)]
    pub const fn rank(self) -> u8 {
        self.0 >> 3
    }

    /// The single-bit board representation of this square.
    #[inline(always)]
    pub const fn bb(self) -> u64 {
        1u64 << self.0
    }

    /// Build a square from a file (`0 = a`) and a rank (`0 = rank 1`).
    ///
    /// Returns `None` when the coordinates are off the board — the only
    /// way to build a `Square` that is guaranteed in range.
    #[inline]
    pub const fn from_file_rank(file: u8, rank: u8) -> Option<Square> {
        if file < 8 && rank < 8 {
            Some(Square(rank * 8 + file))
        } else {
            None
        }
    }
}

impl fmt::Debug for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            (b'a' + self.file()) as char,
            (b'1' + self.rank()) as char
        )
    }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// Parse a UCI square like `"e4"`.
pub fn square_from_uci(s: &[u8]) -> Option<Square> {
    if s.len() < 2 {
        return None;
    }
    let f = s[0].wrapping_sub(b'a');
    let r = s[1].wrapping_sub(b'1');
    if f < 8 && r < 8 {
        Square::from_index(r * 8 + f)
    } else {
        None
    }
}

// Move flag encoding (low nibble of the 16-bit move).
pub const FLAG_QUIET: u8 = 0;
pub const FLAG_DOUBLE_PUSH: u8 = 1;
pub const FLAG_KING_CASTLE: u8 = 2;
pub const FLAG_QUEEN_CASTLE: u8 = 3;
pub const FLAG_CAPTURE: u8 = 4;
pub const FLAG_EP_CAPTURE: u8 = 5;
pub const FLAG_PROMO_N: u8 = 8;
pub const FLAG_PROMO_B: u8 = 9;
pub const FLAG_PROMO_R: u8 = 10;
pub const FLAG_PROMO_Q: u8 = 11;
pub const FLAG_PROMO_CAP_N: u8 = 12;
pub const FLAG_PROMO_CAP_B: u8 = 13;
pub const FLAG_PROMO_CAP_R: u8 = 14;
pub const FLAG_PROMO_CAP_Q: u8 = 15;

/// A move packed in 16 bits: `from` (6) | `to` (6) | `flags` (4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Move(pub u16);

impl Move {
    pub const NONE: Move = Move(0);

    /// Pack a move from raw square indices.
    ///
    /// The squares are 6-bit fields, so only `0..64` is representable; the
    /// bound is debug-asserted here and the type-safe
    /// [`Move::between`] is preferred in new code.
    #[inline(always)]
    pub const fn new(from: u8, to: u8, flags: u8) -> Move {
        debug_assert!(from < 64 && to < 64);
        Move((from as u16) | ((to as u16) << 6) | ((flags as u16) << 12))
    }

    /// Type-safe constructor: takes two validated [`Square`]s.
    #[inline(always)]
    pub const fn between(from: Square, to: Square, flags: u8) -> Move {
        Move((from.index() as u16) | ((to.index() as u16) << 6) | ((flags as u16) << 12))
    }

    #[inline(always)]
    pub const fn from(self) -> u8 {
        (self.0 & 0x3F) as u8
    }
    #[inline(always)]
    pub const fn to(self) -> u8 {
        ((self.0 >> 6) & 0x3F) as u8
    }
    #[inline(always)]
    pub const fn flags(self) -> u8 {
        (self.0 >> 12) as u8
    }
    #[inline(always)]
    pub const fn is_capture(self) -> bool {
        self.flags() & 4 != 0
    }
    #[inline(always)]
    pub const fn is_promotion(self) -> bool {
        self.flags() >= FLAG_PROMO_N
    }
    #[inline(always)]
    pub const fn is_ep(self) -> bool {
        self.flags() == FLAG_EP_CAPTURE
    }
    #[inline(always)]
    pub const fn is_castle(self) -> bool {
        self.flags() == FLAG_KING_CASTLE || self.flags() == FLAG_QUEEN_CASTLE
    }
    #[inline(always)]
    pub const fn is_double_push(self) -> bool {
        self.flags() == FLAG_DOUBLE_PUSH
    }
    /// Promoted piece kind, if any.
    pub const fn promotion_kind(self) -> Option<PieceKind> {
        let f = self.flags();
        if f < FLAG_PROMO_N {
            return None;
        }
        let base = f - FLAG_PROMO_N; // 0..=7, even = knight,1=bishop,2=rook,3=queen
        Some(PieceKind::from_idx(KNIGHT + (base % 4) as usize))
    }
    #[inline(always)]
    pub const fn from_sq(self) -> Square {
        Square::new(self.from())
    }
    #[inline(always)]
    pub const fn to_sq(self) -> Square {
        Square::new(self.to())
    }

    pub fn uci(self) -> String {
        let mut s = String::with_capacity(5);
        s.push((b'a' + (self.from() & 7)) as char);
        s.push((b'1' + (self.from() >> 3)) as char);
        s.push((b'a' + (self.to() & 7)) as char);
        s.push((b'1' + (self.to() >> 3)) as char);
        if let Some(k) = self.promotion_kind() {
            s.push(k.char(false));
        }
        s
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenKind {
    All,
    CapturesOnly,
}

// Castling-right bit flags.
pub const CASTLE_WK: u8 = 1;
pub const CASTLE_WQ: u8 = 2;
pub const CASTLE_BK: u8 = 4;
pub const CASTLE_BQ: u8 = 8;

/// Sentinel for "no en-passant square" (the ep field is a `u8`, so 64 —
/// one past the last square — is used rather than an `Option`).
pub const NO_EP: u8 = 64;

/// Sentinel for "no square" — one past the last board square.
///
/// 64 is the natural choice: `u64::trailing_zeros()` of an empty bitboard
/// is already 64, so a kingless position needs no extra arithmetic to
/// produce it, and 64 is exactly the bound every 64-entry attack table
/// rejects.
pub const NO_SQUARE: u8 = 64;
