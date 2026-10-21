//! Legal move generation with checkmask / pinmask, in bulk where possible.

use crate::position::Position;
use crate::slider::{between, bishop_attacks, rook_attacks};
use crate::tables::{
    DIR_POS, FILE_A, FILE_H, KING_ATT, KNIGHT_ATT, PAWN_ATTACKERS, RANK_3, RANK_6, RANK_8, RAYS,
};
use crate::types::*;
use std::mem::MaybeUninit;

/// Nombre maximal de coups légaux dans une position : 218.
pub const MAX_MOVES: usize = 218;
/// Capacité du tampon. On garde 256 pour rester au-dessus de `MAX_MOVES`
/// même si l'appelant pousse à la main, comme le font les tests.
const CAPACITY: usize = 256;

/// Fixed-capacity move list on the stack: no allocation in the hot path.
///
/// The buffer is `MaybeUninit`, so `new()` writes only `len` and never touches
/// the 512 bytes behind it. That zeroing cost ~4.5 % of `generate`'s
/// instructions and 0–7 % of its wall time here; it is a net win below roughly
/// twelve emitted moves per call and neutral above.
///
/// # Behaviour at the edges
///
/// * `push` keeps `len` as a `u8` and has no capacity guard, as before. A `u8`
///   index into 256 slots is always in bounds, so the hot path pays neither a
///   bounds check nor a branch; a `u16` counter with an `if` cost 5
///   instructions and 2.6 branches per emitted move, which lost more than the
///   `memset` saved on positions with many moves. Pushing past `CAPACITY` is
///   out of contract either way: release wraps `len`, debug panics on the
///   overflow.
/// * `get` now panics out of bounds, where it used to return whatever the
///   buffer held. Callers already had to stay below `len`.
pub struct MoveList {
    moves: [MaybeUninit<Move>; CAPACITY],
    len: u8,
}

impl MoveList {
    #[inline]
    pub fn new() -> MoveList {
        MoveList {
            // `[const { MaybeUninit::uninit() }; CAPACITY]` builds the array
            // without reading anything, so no `assume_init` (and no `unsafe`)
            // is needed here.
            moves: [const { MaybeUninit::uninit() }; CAPACITY],
            len: 0,
        }
    }
    #[inline(always)]
    pub fn push(&mut self, m: Move) {
        debug_assert!((self.len as usize) < CAPACITY);
        self.moves[self.len as usize].write(m);
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
        assert!(i < self.len as usize, "MoveList::get out of bounds");
        // SAFETY: `i < len`, and the first `len` entries were written by `push`.
        unsafe { self.moves[i].assume_init() }
    }
    pub fn as_slice(&self) -> &[Move] {
        // SAFETY: the first `len` entries are initialised, contiguous, and
        // `Move` is `Copy` with no padding, so the slice layout matches.
        unsafe { std::slice::from_raw_parts(self.moves.as_ptr().cast::<Move>(), self.len as usize) }
    }
    pub fn iter(&self) -> impl Iterator<Item = Move> + '_ {
        self.as_slice().iter().copied()
    }
}

impl Default for MoveList {
    fn default() -> Self {
        Self::new()
    }
}

/// L1c: attack set of one colour restricted to nothing (kept for the pawn /
/// knight / king part of the fused legality pass — see `build_legality`).
///
/// Single fused legality pass (GATE 2.5 M2/L1c): ONE iteration over the
/// enemy sliders yields the checkers mask, the pinned set (with per-square
/// pin line stored), and the king-danger set — replacing the three
/// previously redundant passes (`attacks_by`'s slider loops,
/// `attackers_to`'s slider scans at the king square and `compute_pins`'s
/// 8-ray walk). Budget: at most 2 attack lookups per slider (a rook or a
/// bishop pays one, a queen two); an enemy slider not relevant to the
/// king's rays still pays its lookup because it may cover a king-adjacent
/// square (danger), but its checker/pin classification is then skipped via
/// the alignment guards.
pub(crate) struct Legality {
    pub ksq: u8,
    pub checkers: u64,
    /// Enemy attacks computed with our king removed from occupancy
    /// (`occ ^ ks_bit`), exactly the set `attacks_by(them, occ ^ ksq)` produced.
    pub danger: u64,
    /// Bit of the enemy king square (0 if absent). Masked out of every target
    /// set (GATE 2.5 M3): a pseudo king-capture is never emitted — a legal
    /// position cannot attack the enemy king, so masking is a no-op on legal
    /// input and merely keeps illegal entries defined, silent and consistent
    /// between `generate` and `count_moves` (inexact counts = contract).
    pub them_king: u64,
    pub pinned: u64,
    /// Pinned squares with their allowed line (`between(ksq, pinner) | pinner`),
    /// in encounter order. A pinned piece's reachable targets are always a
    /// subset of this line: attacks stop at the king (own colour) and at the
    /// pinner (blocking), so neither the full ray nor the beyond-pinner part
    /// is ever generated.
    pub pin_sq: [u8; 8],
    pub pin_lines: [u64; 8],
    pub npins: u8,
}

impl Legality {
    #[inline(always)]
    pub fn line_of(&self, sq: u8) -> u64 {
        for i in 0..self.npins as usize {
            if self.pin_sq[i] == sq {
                return self.pin_lines[i];
            }
        }
        0
    }
    #[inline(always)]
    fn record_pin(&mut self, blocker: u64, line: u64) {
        debug_assert!(blocker.count_ones() == 1 && (blocker & self.pinned) == 0);
        debug_assert!((self.npins as usize) < 8);
        self.pin_sq[self.npins as usize] = blocker.trailing_zeros() as u8;
        self.pin_lines[self.npins as usize] = line;
        self.npins += 1;
        self.pinned |= blocker;
    }
}

/// Isolate the lowest set bit of a non-zero bitboard.
#[inline(always)]
fn lowest_bit(bb: u64) -> u64 {
    debug_assert!(bb != 0, "lowest_bit requires a non-zero bitboard");
    bb.isolate_lowest_one()
}

#[inline(always)]
fn ortho_aligned(sq: u8, ksq: u8) -> bool {
    (sq ^ ksq) & 7 == 0 || (sq >> 3) == (ksq >> 3)
}

#[inline(always)]
fn diag_aligned(sq: u8, ksq: u8) -> bool {
    let df = ((sq & 7) as i16) - ((ksq & 7) as i16);
    let dr = ((sq >> 3) as i16) - ((ksq >> 3) as i16);
    df != 0 && df.abs() == dr.abs()
}

#[inline]
pub(crate) fn build_legality(pos: &Position, us: Color) -> Legality {
    // L1d: colour monomorphised (one branch at the root, none inside).
    if us == Color::White {
        build_legality_impl::<true>(pos)
    } else {
        build_legality_impl::<false>(pos)
    }
}

#[inline]
fn build_legality_impl<const W: bool>(pos: &Position) -> Legality {
    let us = if W { Color::White } else { Color::Black };
    let them = us.opp();
    let occ = pos.occ();
    let ksq = pos.king_sq(us);
    let us_bb = pos.color_bb(us);
    let them_all = pos.bb_color[them.idx()];

    // GATE 2.5 M3 — entry contract: a LEGAL position is required. Debug:
    // loud, with the contract message, at the single build site. Release:
    // defined and silent — the king capture is simply never emitted (see
    // `Legality::them_king`); counts are inaccurate for illegal input by
    // contract, never a crash.
    let them_king = pos.bb_piece[KING] & them_all;
    debug_assert!(
        them_king != 0,
        "position légale exigée : roi adverse ABSENT (entrée illégale)"
    );
    debug_assert!(
        them_king == 0 || !pos.is_square_attacked(them_king.trailing_zeros() as u8, us),
        "enemy king capturable ⇒ position illégale entrée (comptes inexactes = contrat)"
    );

    // No king for the side to move (illegal entry, out of contract): the
    // position has no legal move at all. The shift below is masked so that
    // `ksq == NO_SQUARE` (64) yields a zero bit instead of an out-of-range
    // shift, and the caller sees an empty `danger` set — the kingless
    // entry is defined, silent and crash-free, with no extra branch on the
    // hot path.
    let ks_bit = 1u64 << (ksq & 63);
    let occ2 = occ ^ ks_bit;
    // Table index for the king square: masked so a kingless position
    // (ksq == NO_SQUARE == 64) reads a valid (but meaningless) entry
    // instead of indexing out of bounds. The `danger = u64::MAX` write at
    // the end makes the result "no legal move" regardless.
    let ksq_i = (ksq & 63) as usize;

    let mut ctx = Legality {
        ksq,
        checkers: 0,
        danger: 0,
        them_king,
        pinned: 0,
        pin_sq: [0; 8],
        pin_lines: [0; 8],
        npins: 0,
    };

    // Leaper checkers + leaper part of the danger set (pawns, knights, king).
    let pw = pos.bb_piece[PAWN] & them_all;
    if pw != 0 {
        ctx.danger |= if W {
            // W: 'them' are Black, attacking downwards
            ((pw & !FILE_H) >> 7) | ((pw & !FILE_A) >> 9)
        } else {
            // !W: 'them' are White, attacking upwards
            ((pw & !FILE_H) << 9) | ((pw & !FILE_A) << 7)
        };
        ctx.checkers |= PAWN_ATTACKERS[them.idx()][ksq_i] & pw;
    }
    let nk = pos.bb_piece[KNIGHT] & them_all;
    ctx.checkers |= KNIGHT_ATT[ksq_i] & nk;
    let mut n = nk;
    while n != 0 {
        ctx.danger |= KNIGHT_ATT[n.trailing_zeros() as usize];
        n &= n - 1;
    }
    let tk = pos.bb_piece[KING] & them_all;
    if tk != 0 {
        let att = KING_ATT[tk.trailing_zeros() as usize];
        ctx.danger |= att;
        ctx.checkers |= att & ks_bit;
    }

    // Sliders: rook-line pass then bishop-line pass (a queen pays one lookup
    // in each; a dedicated rook/bishop only in its pass). `occ2` removes the
    // king so each attack set also yields the checker by testing `ks_bit`
    // (ray unblocked between slider and king) — equivalent to scanning
    // rook/bishop attacks FROM the king square.
    let mut s = (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]) & them_all;
    while s != 0 {
        let sq = s.trailing_zeros() as u8;
        let bit = lowest_bit(s);
        s ^= bit;
        let att = rook_attacks(sq, occ2);
        if att & ks_bit != 0 {
            ctx.checkers |= bit;
        } else {
            let bl = between(ksq_i as u8, sq) & occ;
            if bl != 0 && (bl & (bl - 1)) == 0 && bl & us_bb != 0 && ortho_aligned(sq, ksq) {
                ctx.record_pin(bl, between(ksq_i as u8, sq) | bit);
            }
        }
        ctx.danger |= att;
    }
    let mut s = (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]) & them_all;
    while s != 0 {
        let sq = s.trailing_zeros() as u8;
        let bit = lowest_bit(s);
        s ^= bit;
        let att = bishop_attacks(sq, occ2);
        if att & ks_bit != 0 {
            ctx.checkers |= bit;
        } else {
            let bl = between(ksq_i as u8, sq) & occ;
            if bl != 0 && (bl & (bl - 1)) == 0 && bl & us_bb != 0 && diag_aligned(sq, ksq) {
                ctx.record_pin(bl, between(ksq_i as u8, sq) | bit);
            }
        }
        ctx.danger |= att;
    }

    // Kingless position (illegal entry, out of contract): there is no
    // legal move at all. `danger` is set to "everything is forbidden", so
    // the king-move mask comes out empty and no other piece may move
    // either (the caller's `allowed` mask derives from `them_king` and
    // `checkers`, both zero here — so only the king moves were at risk).
    // The whole thing is one mask write, no branch.
    if ksq == NO_SQUARE {
        ctx.danger = u64::MAX;
    }
    ctx
}

#[inline(always)]
pub fn attackers_to(pos: &Position, sq: u8, by: Color) -> u64 {
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

/// Pinned pieces of `us`, split by pin orientation. Reference implementation
/// (readable): the per-node hot path uses the fused [`build_legality`] pass.
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
fn push_promotions(out: &mut MoveList, from: u8, to: u8, is_cap: bool) {
    let base = if is_cap {
        FLAG_PROMO_CAP_N
    } else {
        FLAG_PROMO_N
    };
    out.push(Move::new(from, to, base));
    out.push(Move::new(from, to, base + 1));
    out.push(Move::new(from, to, base + 2));
    out.push(Move::new(from, to, base + 3));
}

/// L1b: en-passant legality WITHOUT make/unmake. After the EP capture (our
/// pawn leaves `from`, lands on `ep`, the enemy pawn disappears from
/// `cap_sq`), only sliders can *newly* hit our king, exclusively through the
/// two vacated squares — leaper attacks are invariant under the move, removed
/// pieces can only remove attacks. The slider test is run on the post-move
/// occupancy (our pawn reinstated on `ep`), which also covers: pinnned-pawn
/// EPs (diagonal/orthogonal exposure) and, when in check, a slider checker
/// blocked by the landing pawn. Degenerate entries (no pawn at `cap_sq`)
/// stay defined: this test needs no mailbox and no make (the make/unmake
/// version would unconditionally clear `cap_sq`).
///
/// Contract note: on illegal entries (enemy king capturable, adjacent kings)
/// the leaper part of the old make-based test is no longer re-run, so counts
/// may differ from the oracle — illegal input is out of contract (GATE 2.5
/// M3), not a crash.
#[inline(always)]
fn ep_legal_ray(pos: &Position, them: Color, from: u8, ep: u8, ksq: u8, occ: u64) -> bool {
    if them == Color::Black {
        ep_legal_ray_impl::<true>(pos, from, ep, ksq, occ)
    } else {
        ep_legal_ray_impl::<false>(pos, from, ep, ksq, occ)
    }
}

/// L1b: en-passant legality WITHOUT make/unmake. `W` = white to move; after
/// the EP capture (our pawn leaves `from`, lands on `ep`, the enemy pawn
/// disappears from `cap_sq`), only sliders can *newly* hit our king,
/// exclusively through the two vacated squares — leaper attacks are invariant
/// under the move, removed pieces can only remove attacks. The slider test
/// runs on the post-move occupancy (our pawn reinstated on `ep`), which also
/// covers pinned-pawn EPs (diagonal/orthogonal exposure) and, when in check,
/// a slider checker blocked by the landing pawn. Degenerate entries (no pawn
/// at `cap_sq`) stay defined: this test needs no mailbox and no make (the
/// make/unmake version would unconditionally clear `cap_sq`).
///
/// Contract note: on illegal entries (enemy king capturable, adjacent kings)
/// the leaper part of the old make-based test is no longer re-run, so counts
/// may differ from the oracle — illegal input is out of contract (GATE 2.5
/// M3), not a crash.
#[inline(always)]
fn ep_legal_ray_impl<const W: bool>(pos: &Position, from: u8, ep: u8, ksq: u8, occ: u64) -> bool {
    let cap_sq = if W { ep - 8 } else { ep + 8 };
    let occ2 = occ ^ (1u64 << from) ^ (1u64 << ep) ^ (1u64 << cap_sq);
    // The vacated pair lies on the ep-rank: a horizontal rook/queen ray
    // through the now-open rank between king and captured pawn is the classic
    // discovered check; the diagonal/second vacated square needs the full
    // slider scans anyway (the landing pawn can re-block through `ep`).
    let b = pos.bb_color[if W { BLACK } else { WHITE }];
    rook_attacks(ksq, occ2) & (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]) & b == 0
        && bishop_attacks(ksq, occ2) & (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]) & b == 0
}

/// Generate all legal moves (or noisy moves for `CapturesOnly`).
///
/// # Contract (GATE 2.5 M3)
///
/// A **legal** position is required (both kings present, side to move not
/// generating while the enemy king is capturable — i.e. the enemy king is not
/// attackable by us). This is exactly the invariant `perft`, self-play and
/// the MCTS layer maintain. On illegal input the behaviour is **defined and
/// silent**: no king-capture pseudo move is ever emitted (the enemy king
/// square is masked out of every target set) and no panic occurs — counts are
/// simply inaccurate, by contract. In debug builds the illegal entry is
/// loudly asserted (`debug_assert!` with the contract message, see
/// `build_legality_impl` / `Position::king_sq`).
pub fn generate(pos: &Position, out: &mut MoveList, kind: GenKind) {
    let us = pos.side;
    let them = us.opp();
    let occ = pos.occ();
    let us_bb = pos.color_bb(us);
    let them_bb = pos.color_bb(them);
    let empty = !occ;
    let ctx = build_legality(pos, us);
    let ksq = ctx.ksq;

    // No king for the side to move (illegal entry, out of contract): there
    // is no legal move at all. Defined and silent — see the contract above.
    if ksq == NO_SQUARE {
        return;
    }

    let ncheck = ctx.checkers.count_ones();

    // King moves. The king is removed from occupancy so sliders x-ray through it.
    // M3: enemy king square never a target (no-op on legal input).
    let mut kt = KING_ATT[(ksq & 63) as usize] & !us_bb & !ctx.danger & !ctx.them_king;
    while kt != 0 {
        let to = kt.trailing_zeros() as u8;
        kt &= kt - 1;
        let cap = (them_bb >> to) & 1 != 0;
        out.push(Move::new(
            ksq,
            to,
            if cap { FLAG_CAPTURE } else { FLAG_QUIET },
        ));
    }

    if ncheck >= 2 {
        return; // double check: only king moves
    }

    let allowed = if ncheck == 1 {
        let c = ctx.checkers.trailing_zeros() as u8;
        (between(ksq, c) | ctx.checkers) & !ctx.them_king
    } else {
        !ctx.them_king
    };
    let pinned = ctx.pinned;

    let captures_only = kind == GenKind::CapturesOnly;

    // ---- Pawns ----
    let pawns = pos.bb_piece[PAWN] & us_bb;
    let free = pawns & !pinned;
    gen_pawns_bulk(pos, out, us, them_bb, free, allowed, empty, captures_only);
    if ctx.npins != 0 {
        for i in 0..ctx.npins as usize {
            let from = ctx.pin_sq[i];
            // Only pawn pins matter here; sliders use their own loop below.
            if pawns & (1u64 << from) != 0 {
                gen_pawn_pinned(
                    pos,
                    out,
                    us,
                    them_bb,
                    from,
                    allowed & ctx.pin_lines[i],
                    empty,
                    captures_only,
                );
            }
        }
    }

    // En passant (L1b): legality by ray trick — no make/unmake per candidate.
    // In check: an EP capture is legal only if it removes the checking pawn
    // (the double-pusher standing on `cap_sq`) or blocks a slider ray by
    // landing on `ep`; otherwise post-move king safety is decided by the
    // slider scan on the modified occupancy (see `ep_legal_ray`).
    //
    // `ep_capture_square` returns `None` when the FEN names an en-passant
    // square with no enemy pawn behind it: there is nothing to capture, so
    // no ep move is offered (a malformed entry stays consistent — the
    // twin `count_moves` path applies the same rule).
    //
    // The two conditions are nested rather than combined in a tuple: a
    // tuple pattern evaluates both elements eagerly, which would call
    // `ep_capture_square` on every node even though an en-passant square
    // is rare.
    if let Some(ep) = pos.ep_square()
        && let Some(cap_sq) = pos.ep_capture_square(us)
    {
        let evades_check = if ncheck == 1 {
            ((ctx.checkers & (1u64 << cap_sq)) | (allowed & (1u64 << ep))) != 0
        } else {
            true
        };
        if evades_check {
            let mut m = PAWN_ATTACKERS[us.idx()][ep as usize] & pawns;
            while m != 0 {
                let from = m.trailing_zeros() as u8;
                m &= m - 1;
                if ep_legal_ray(pos, them, from, ep, ksq, occ) {
                    out.push(Move::new(from, ep, FLAG_EP_CAPTURE));
                }
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
            t &= ctx.line_of(s);
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
            t &= ctx.line_of(s);
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
            let flag = if back == 16 {
                FLAG_DOUBLE_PUSH
            } else {
                FLAG_QUIET
            };
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
fn emit_pawn_caps_down(
    out: &mut MoveList,
    targets: u64,
    back: u8,
    _cap: bool,
    _captures_only: bool,
) {
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
        if (8..16).contains(&from) && _pos.piece_on(from + 8) == 0 && _pos.piece_on(from + 16) == 0
        {
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
        if (48..56).contains(&from) && _pos.piece_on(from - 8) == 0 && _pos.piece_on(from - 16) == 0
        {
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

/// Generate the legal castling moves for `us`.
///
/// Besides the usual square-emptiness and attack checks, the king and the
/// castling rook must **actually stand on their home squares**. A FEN may
/// claim castling rights the board does not support (no rook on h1/a1, king
/// not on e1); emitting the move then would move a piece that is not there
/// and corrupt the mailbox. Such an entry is out of contract, but it must
/// stay defined and crash-free.
fn gen_castle(pos: &Position, out: &mut MoveList, us: Color, occ: u64) {
    let them = us.opp();
    let back: u8 = if us.idx() == WHITE { 0 } else { 56 };
    let (ks_bit, qs_bit) = if us.idx() == WHITE {
        (CASTLE_WK, CASTLE_WQ)
    } else {
        (CASTLE_BK, CASTLE_BQ)
    };
    // Fast exit: no castling right for this side at all. Most positions
    // have none, and this keeps every check below off the hot path.
    if pos.castle & (ks_bit | qs_bit) == 0 {
        return;
    }
    let king_sq = back + 4;
    if pos.castle & ks_bit != 0 {
        let f = back + 5;
        let g = back + 6;
        if occ & (1 << f) == 0
            && occ & (1 << g) == 0
            && !pos.is_square_attacked(f, them)
            && !pos.is_square_attacked(g, them)
        {
            out.push(Move::new(king_sq, back + 6, FLAG_KING_CASTLE));
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
            out.push(Move::new(king_sq, back + 2, FLAG_QUEEN_CASTLE));
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
/// L1d: colour monomorphised — the dispatcher below is the ONLY colour
/// branch; inside `count_moves_impl` every `if W` folds at compile time
/// (pawn shifts, EP capture direction, pawn tables, castle bits), so a leaf
/// contains no colour branch at all.
///
/// Used by perft at `depth == 1`: a leaf position needs only `MoveList::len`,
/// never the moves themselves. Promotions count 4 moves per target square and
/// en passant / castling keep their exact legality gates.
///
/// Differential-checked against `generate(.., All).len()` on golden positions
/// and thousands of self-play positions (tests).
///
/// # Contract (GATE 2.5 M3)
///
/// Same entry contract as [`generate`]: legal positions only; illegal input
/// is defined, silent and never emits (or counts) a king capture.
#[inline]
pub fn count_moves(pos: &Position) -> u64 {
    if pos.side == Color::White {
        count_moves_impl::<true>(pos)
    } else {
        count_moves_impl::<false>(pos)
    }
}

#[inline]
fn count_moves_impl<const W: bool>(pos: &Position) -> u64 {
    let us = if W { Color::White } else { Color::Black };
    let them = us.opp();
    let occ = pos.occ();
    let us_bb = pos.color_bb(us);
    let them_bb = pos.color_bb(them);
    let empty = !occ;
    let ctx = build_legality_impl::<W>(pos);
    let ksq = ctx.ksq;

    // No king for the side to move (illegal entry, out of contract): no
    // legal move at all. Mirrors `generate` exactly (twin property).
    if ksq == NO_SQUARE {
        return 0;
    }

    let ncheck = ctx.checkers.count_ones();

    // King moves (always legal here: `!us_bb & !danger`; M3: enemy king
    // square masked out — no-op on legal input).
    let mut n =
        (KING_ATT[(ksq & 63) as usize] & !us_bb & !ctx.danger & !ctx.them_king).count_ones() as u64;
    if ncheck >= 2 {
        return n; // double check: only king moves
    }

    let allowed = if ncheck == 1 {
        let c = ctx.checkers.trailing_zeros() as u8;
        (between(ksq, c) | ctx.checkers) & !ctx.them_king
    } else {
        !ctx.them_king
    };
    let pinned = ctx.pinned;

    // ---- Pawns (bulk target bitboards) ----
    let pawns = pos.bb_piece[PAWN] & us_bb;
    let free = pawns & !pinned;
    n += count_pawns_bulk::<W>(them_bb, free, allowed, empty);
    if ctx.npins != 0 {
        for i in 0..ctx.npins as usize {
            let from = ctx.pin_sq[i];
            if pawns & (1u64 << from) != 0 {
                n += count_pawn_pinned::<W>(pos, them_bb, from, allowed & ctx.pin_lines[i], empty);
            }
        }
    }

    // ---- En passant (L1b): same ray-trick validation as in generate ----
    // Same rule as `generate`: an ep square with no enemy pawn behind it
    // offers no ep capture (malformed FEN stays consistent between the
    // two twin paths). The conditions are nested so that
    // `ep_capture_square` is only called when an ep square exists.
    if let Some(ep) = pos.ep_square()
        && let Some(cap_sq) = pos.ep_capture_square(if W { Color::White } else { Color::Black })
    {
        let evades_check = if ncheck == 1 {
            ((ctx.checkers & (1u64 << cap_sq)) | (allowed & (1u64 << ep))) != 0
        } else {
            true
        };
        if evades_check {
            let attackers = if W {
                PAWN_ATTACKERS[0]
            } else {
                PAWN_ATTACKERS[1]
            };
            let mut m = attackers[ep as usize] & pawns;
            while m != 0 {
                let from = m.trailing_zeros() as u8;
                m &= m - 1;
                if ep_legal_ray_impl::<W>(pos, from, ep, ksq, occ) {
                    n += 1;
                }
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
            t &= ctx.line_of(s);
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
            t &= ctx.line_of(s);
        }
        n += t.count_ones() as u64;
    }

    // ---- Castling (All-kind semantics, only when not in check) ----
    if ncheck == 0 {
        n += count_castles::<W>(pos, occ);
    }
    n
}

/// Popcount twin of `gen_pawns_bulk` for `GenKind::All` (L1d: colour folded).
#[inline]
fn count_pawns_bulk<const W: bool>(them: u64, pawns: u64, allowed: u64, empty: u64) -> u64 {
    if pawns == 0 {
        return 0;
    }
    if W {
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

/// Popcount twin of `gen_pawn_pinned` for `GenKind::All` (L1d: colour folded).
#[inline]
fn count_pawn_pinned<const W: bool>(
    pos: &Position,
    them: u64,
    from: u8,
    mask: u64,
    _empty: u64,
) -> u64 {
    let mut n = 0;
    if W {
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

/// Popcount twin of `gen_castle` (counts 1 per legal side; L1d: colour folded).
#[inline]
fn count_castles<const W: bool>(pos: &Position, occ: u64) -> u64 {
    let back: u8 = if W { 0 } else { 56 };
    let (ks_bit, qs_bit) = if W {
        (CASTLE_WK, CASTLE_WQ)
    } else {
        (CASTLE_BK, CASTLE_BQ)
    };
    let them = if W { Color::Black } else { Color::White };
    // Fast exit: no castling right for this side (mirrors `gen_castle`).
    if pos.castle & (ks_bit | qs_bit) == 0 {
        return 0;
    }
    let king_sq = back + 4;
    let king_code = (if W { 0 } else { 6 }) + KING;
    let rook_code = (if W { 0 } else { 6 }) + ROOK;
    let mut n = 0;
    // The king must be on its home square, and the castling rook on its own
    // home square (same rule as `gen_castle` — the twin property must hold
    // on any entry). The king check is folded into the per-side branches so
    // that a side without a castling right pays nothing for it.
    if pos.castle & ks_bit != 0
        && pos.mailbox[king_sq as usize] as usize == king_code + 1
        && pos.mailbox[back as usize + 7] as usize == rook_code + 1
    {
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
    if pos.castle & qs_bit != 0
        && pos.mailbox[king_sq as usize] as usize == king_code + 1
        && pos.mailbox[back as usize] as usize == rook_code + 1
    {
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

#[cfg(test)]
mod tests_movelist {
    use super::*;

    #[test]
    fn une_liste_neuve_est_vide() {
        let l = MoveList::new();
        assert_eq!(l.len(), 0);
        assert!(l.is_empty());
        assert_eq!(l.as_slice().len(), 0);
        assert_eq!(l.iter().count(), 0);
    }

    #[test]
    fn default_est_vide() {
        let l = MoveList::default();
        assert!(l.is_empty());
    }

    #[test]
    fn push_puis_get_rend_les_coups_dans_l_ordre() {
        let mut l = MoveList::new();
        let a = Move::new(8, 16, FLAG_QUIET);
        let b = Move::new(1, 18, FLAG_CAPTURE);
        let c = Move::new(48, 56, FLAG_PROMO_Q);
        l.push(a);
        l.push(b);
        l.push(c);
        assert_eq!(l.len(), 3);
        assert_eq!(l.get(0), a);
        assert_eq!(l.get(1), b);
        assert_eq!(l.get(2), c);
        assert_eq!(l.as_slice(), &[a, b, c]);
        assert_eq!(l.iter().collect::<Vec<_>>(), vec![a, b, c]);
    }

    #[test]
    fn remplir_jusqu_au_maximum_legal_puis_relire() {
        const { assert!(MAX_MOVES < CAPACITY) };
        let mut l = MoveList::new();
        for i in 0..MAX_MOVES as u16 {
            l.push(Move(i));
        }
        assert_eq!(l.len(), MAX_MOVES);
        assert_eq!(l.get(0), Move(0));
        assert_eq!(l.get(MAX_MOVES - 1), Move(MAX_MOVES as u16 - 1));
        assert_eq!(l.as_slice().len(), MAX_MOVES);
    }

    #[test]
    fn une_liste_reutilisee_repart_de_zero() {
        let mut l = MoveList::new();
        l.push(Move::new(0, 8, FLAG_QUIET));
        let mut l2 = MoveList::new();
        assert!(l2.is_empty());
        l2.push(Move::new(1, 9, FLAG_QUIET));
        assert_eq!(l2.len(), 1);
    }
}
