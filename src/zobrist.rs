//! Zobrist hashing keys, generated deterministically at compile time.

pub struct Zobrist {
    /// `[piece_code 0..12][square]`
    pub piece: [[u64; 64]; 12],
    pub castle: [u64; 16],
    pub ep_file: [u64; 8],
    pub side: u64,
}

const fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

const fn build() -> Zobrist {
    let mut state = 0x0F42_1234_DEAD_BEEF_u64;
    let mut piece = [[0u64; 64]; 12];
    let mut p = 0;
    while p < 12 {
        let mut s = 0;
        while s < 64 {
            piece[p][s] = splitmix64(&mut state);
            s += 1;
        }
        p += 1;
    }
    let mut castle = [0u64; 16];
    let mut i = 0;
    while i < 16 {
        castle[i] = splitmix64(&mut state);
        i += 1;
    }
    let mut ep_file = [0u64; 8];
    let mut f = 0;
    while f < 8 {
        ep_file[f] = splitmix64(&mut state);
        f += 1;
    }
    let side = splitmix64(&mut state);
    Zobrist {
        piece,
        castle,
        ep_file,
        side,
    }
}

pub const ZOBRIST: Zobrist = build();
