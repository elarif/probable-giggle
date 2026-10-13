//! Precomputed attack tables built with `const fn` (deterministic, no lazy init).

pub const FILE_A: u64 = 0x0101_0101_0101_0101;
pub const FILE_H: u64 = 0x8080_8080_8080_8080;
pub const RANK_1: u64 = 0x0000_0000_0000_00FF;
pub const RANK_3: u64 = 0x0000_0000_00FF_0000;
pub const RANK_6: u64 = 0x0000_FF00_0000_0000;
pub const RANK_7: u64 = 0x00FF_0000_0000_0000;
pub const RANK_8: u64 = 0xFF00_0000_0000_0000;

pub const DIRS: [(i32, i32); 8] = [
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
];
/// Whether a direction increases the square index (used to pick the nearest blocker).
pub const DIR_POS: [bool; 8] = [true, true, true, false, false, false, false, true];
pub const ROOK_DIRS: [usize; 4] = [0, 2, 4, 6];
pub const BISHOP_DIRS: [usize; 4] = [1, 3, 5, 7];

const fn build_rays() -> [[u64; 64]; 8] {
    let mut t = [[0u64; 64]; 8];
    let mut d = 0;
    while d < 8 {
        let mut s = 0;
        while s < 64 {
            let mut f = (s % 8) as i32;
            let mut r = (s / 8) as i32;
            let mut bb = 0u64;
            loop {
                f += DIRS[d].0;
                r += DIRS[d].1;
                if f < 0 || f >= 8 || r < 0 || r >= 8 {
                    break;
                }
                bb |= 1u64 << ((r * 8 + f) as u32);
            }
            t[d][s] = bb;
            s += 1;
        }
        d += 1;
    }
    t
}

const fn build_leaper(deltas: [(i32, i32); 8]) -> [u64; 64] {
    let mut t = [0u64; 64];
    let mut s = 0;
    while s < 64 {
        let f = (s % 8) as i32;
        let r = (s / 8) as i32;
        let mut i = 0;
        while i < 8 {
            let nf = f + deltas[i].0;
            let nr = r + deltas[i].1;
            if nf >= 0 && nf < 8 && nr >= 0 && nr < 8 {
                t[s] |= 1u64 << ((nr * 8 + nf) as u32);
            }
            i += 1;
        }
        s += 1;
    }
    t
}

/// `[White, Black][sq]` = squares attacked by a pawn of that color standing on `sq`.
const fn build_pawn_attacks() -> [[u64; 64]; 2] {
    let mut t = [[0u64; 64]; 2];
    let mut s = 0;
    while s < 64 {
        let f = (s % 8) as i32;
        let r = (s / 8) as i32;
        let mut w = 0u64;
        if f > 0 && r < 7 {
            w |= 1u64 << (((r + 1) * 8 + (f - 1)) as u32);
        }
        if f < 7 && r < 7 {
            w |= 1u64 << (((r + 1) * 8 + (f + 1)) as u32);
        }
        let mut b = 0u64;
        if f > 0 && r > 0 {
            b |= 1u64 << (((r - 1) * 8 + (f - 1)) as u32);
        }
        if f < 7 && r > 0 {
            b |= 1u64 << (((r - 1) * 8 + (f + 1)) as u32);
        }
        t[0][s] = w;
        t[1][s] = b;
        s += 1;
    }
    t
}

/// `[White, Black][sq]` = squares holding a pawn of that color which attacks `sq`.
const fn build_pawn_attackers() -> [[u64; 64]; 2] {
    let mut t = [[0u64; 64]; 2];
    let mut s = 0;
    while s < 64 {
        let f = (s % 8) as i32;
        let r = (s / 8) as i32;
        let mut w = 0u64;
        if f > 0 && r > 0 {
            w |= 1u64 << (((r - 1) * 8 + (f - 1)) as u32);
        }
        if f < 7 && r > 0 {
            w |= 1u64 << (((r - 1) * 8 + (f + 1)) as u32);
        }
        let mut b = 0u64;
        if f > 0 && r < 7 {
            b |= 1u64 << (((r + 1) * 8 + (f - 1)) as u32);
        }
        if f < 7 && r < 7 {
            b |= 1u64 << (((r + 1) * 8 + (f + 1)) as u32);
        }
        t[0][s] = w;
        t[1][s] = b;
        s += 1;
    }
    t
}

/// `BETWEEN[a][b]` = squares strictly between `a` and `b` when aligned, else 0.
const fn build_between() -> [[u64; 64]; 64] {
    let mut t = [[0u64; 64]; 64];
    let mut a = 0;
    while a < 64 {
        let mut d = 0;
        while d < 8 {
            let mut f = (a % 8) as i32;
            let mut r = (a / 8) as i32;
            let mut acc = 0u64;
            loop {
                f += DIRS[d].0;
                r += DIRS[d].1;
                if f < 0 || f >= 8 || r < 0 || r >= 8 {
                    break;
                }
                let s = (r * 8 + f) as usize;
                t[a][s] = acc;
                acc |= 1u64 << s;
            }
            d += 1;
        }
        a += 1;
    }
    t
}

pub const RAYS: [[u64; 64]; 8] = build_rays();
pub const KNIGHT_ATT: [u64; 64] = build_leaper([
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
]);
pub const KING_ATT: [u64; 64] = build_leaper([
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
]);
pub const PAWN_ATTACKS: [[u64; 64]; 2] = build_pawn_attacks();
pub const PAWN_ATTACKERS: [[u64; 64]; 2] = build_pawn_attackers();
pub static BETWEEN: [[u64; 64]; 64] = build_between();

/// Ray in direction `d` from `sq`, stopping before the first blocker (inclusive of it).
#[inline(always)]
pub const fn ray_attack(sq: u8, occ: u64, d: usize) -> u64 {
    let ray = RAYS[d][sq as usize];
    let blockers = ray & occ;
    if blockers == 0 {
        return ray;
    }
    let first = if DIR_POS[d] {
        blockers.trailing_zeros() as usize
    } else {
        (63 - blockers.leading_zeros()) as usize
    };
    ray ^ RAYS[d][first]
}
