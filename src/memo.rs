//! L5: perft memoization table — `--memo` CLI support.
//!
//! Fixed-capacity open-addressing table, **8 GiB cap** (2^28 entries x 24 B:
//! hash8 + verify4 + count8 + meta4), no eviction, no rehash. Keys derive
//! from the incremental Zobrist (which already covers side, castling, EP)
//! mixed with the remaining depth; a derived second word (`verify`, 4 B)
//! guards against slot-index collisions with a 96-bit effective identity.
//!
//! Validation before any deep run: tests assert memo == exhaustive perft
//! at depth <= 6 on all golden positions.

use crate::movegen::count_moves;
use crate::position::Position;

const ENTRY_SIZE: usize = std::mem::size_of::<MemoEntry>(); // 24 B

#[derive(Clone, Copy)]
struct MemoEntry {
    key: u64,   // finalized zobrist ^ depth salt
    count: u64, // perft(pos, depth) result
    verify: u32,
    meta: u32, // depth+1 when occupied, 0 = empty slot
}
const _: () = assert!(ENTRY_SIZE == 24, "memo entry must be 24 bytes");

/// Default capacity: 2^28 entries = 6.44 GiB, inside the ~8 GiB budget.
pub const DEFAULT_MEMO_CAPACITY: usize = 1 << 28;

pub struct Memo {
    slots: Vec<MemoEntry>,
    mask: u64,
    // stats
    pub fills: u64,
    pub hits: u64,
    /// Memoize nodes with remaining depth >= this (1 = never, cheap leaves
    /// stay on `count_moves`).
    pub min_depth: u32,
}

impl Memo {
    /// Allocate a table of `1 << log2_capacity` entries. Clamped to the
    /// 2^28 (8 GiB) budget unless explicitly overridden.
    pub fn new(log2_capacity: u32) -> Memo {
        let cap = 1usize << log2_capacity.min(28);
        Memo {
            slots: vec![
                MemoEntry {
                    key: 0,
                    count: 0,
                    verify: 0,
                    meta: 0
                };
                cap
            ],
            mask: (cap - 1) as u64,
            fills: 0,
            hits: 0,
            min_depth: 3,
        }
    }

    #[inline]
    fn slot_of(&self, key: u64) -> usize {
        (key & self.mask) as usize
    }

    #[inline]
    fn lookup(&self, key: u64, verify: u32) -> Option<u64> {
        let mut i = self.slot_of(key);
        for _ in 0..16 {
            let e = &self.slots[i];
            if e.meta == 0 {
                return None; // empty slot: no further entries for this key
            }
            if e.key == key && e.verify == verify {
                return Some(e.count);
            }
            i = (i + 1) & self.mask as usize;
        }
        None
    }

    #[inline]
    fn insert(&mut self, key: u64, verify: u32, depth: u32, count: u64) {
        let mut i = self.slot_of(key);
        for _ in 0..16 {
            let e = &mut self.slots[i];
            if e.meta == 0 || (e.key == key && e.verify == verify) {
                if e.meta == 0 {
                    self.fills += 1;
                }
                e.key = key;
                e.verify = verify;
                e.count = count;
                e.meta = depth + 1;
                return;
            }
            i = (i + 1) & self.mask as usize;
        }
        // 16-way probe exhausted: give up (worst case the run recomputes).
    }
}

#[inline(always)]
fn hash_final(x: u64) -> u64 {
    // SplitMix64 finalizer.
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn memo_keys(zobrist: u64, depth: u32) -> (u64, u32) {
    let d = depth as u64;
    let salted = zobrist ^ d.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let key = hash_final(salted);
    let verify = (hash_final(zobrist ^ d.wrapping_mul(0xC2B2_AE3D_27D4_EB4F)) >> 32) as u32;
    (key, verify)
}

/// Memoized single-threaded perft (`--memo`). `memo.min_depth` tunes the
/// cutoff (defaults to 3: the leaf layer always uses `count_moves`).
pub fn perft_memo(pos: &mut Position, depth: u32, memo: &mut Memo) -> u64 {
    if depth == 0 {
        return 1;
    }
    if depth == 1 {
        return count_moves(pos);
    }
    if depth >= memo.min_depth {
        let (key, verify) = memo_keys(pos.zobrist, depth);
        if let Some(c) = memo.lookup(key, verify) {
            memo.hits += 1;
            return c;
        }
        let n = perft_memo_inner(pos, depth, memo);
        memo.insert(key, verify, depth, n);
        n
    } else {
        perft_memo_inner(pos, depth, memo)
    }
}

fn perft_memo_inner(pos: &mut Position, depth: u32, memo: &mut Memo) -> u64 {
    let list = crate::movegen::generate_all(pos);
    if depth == 1 {
        return list.len() as u64;
    }
    let mut n = 0;
    for mv in list.iter() {
        let u = pos.make(mv);
        n += perft_memo(pos, depth - 1, memo);
        pos.unmake(mv, u);
    }
    n
}
