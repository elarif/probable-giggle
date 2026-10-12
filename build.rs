//! L2: build-time generation of proven magic bitboard tables.
//!
//! Magic candidates are found with a deterministic PRNG and each one is
//! *proven at build time*: for every occupancy subset of the square's mask,
//! the multiply-shift index must map to the attack set given by an
//! independent, naive ray-walk reimplementation. A successful build ships
//! tables correct by construction; the ray-walk fallback stays in
//! `src/tables.rs` as the human-auditable reference.
//!
//! Output: `$OUT_DIR/magic_generated.rs` with flat attack tables + offsets,
//! consumed by `src/slider.rs` as the portable (no-BMI2) fallback. Magic
//! lookup is pure arithmetic — no intrinsics — so it also serves ARM/WASM.

use std::fs;
use std::path::PathBuf;

/// splitmix64 — deterministic, reproducible tables.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Sparse random bitboards (classic magic-search triple AND).
    fn sparse(&mut self) -> u64 {
        self.next() & self.next() & self.next()
    }
}

#[derive(Clone, Copy)]
struct Dir(i32, i32);

const ORTHO: [Dir; 4] = [Dir(0, 1), Dir(1, 0), Dir(0, -1), Dir(-1, 0)];
const DIAG: [Dir; 4] = [Dir(1, 1), Dir(1, -1), Dir(-1, -1), Dir(-1, 1)];

fn ray_attack_ref(sq: u8, occ: u64, d: Dir) -> u64 {
    let mut f = (sq % 8) as i32;
    let mut r = (sq / 8) as i32;
    let mut res = 0u64;
    loop {
        f += d.0;
        r += d.1;
        if !(0..8).contains(&f) || !(0..8).contains(&r) {
            break;
        }
        let s = (r * 8 + f) as u32;
        res |= 1u64 << s;
        if occ & (1u64 << s) != 0 {
            break;
        }
    }
    res
}

fn attacks_ref(sq: u8, occ: u64, dirs: [Dir; 4]) -> u64 {
    dirs.iter().map(|&d| ray_attack_ref(sq, occ, d)).fold(0, |a, b| a | b)
}

fn rook_mask(sq: u8) -> u64 {
    let mut m = 0u64;
    for &d in &ORTHO {
        let ray = ray_attack_ref(sq, 0, d);
        let file_bits = if d.0 == 1 {
            0x8080_8080_8080_8080u64
        } else if d.0 == -1 {
            0x0101_0101_0101_0101u64
        } else {
            0
        };
        let rank_bits = if d.1 == 1 {
            0xFF00_0000_0000_0000u64
        } else if d.1 == -1 {
            0x0000_0000_0000_00FFu64
        } else {
            0
        };
        m |= ray & !(file_bits | rank_bits);
    }
    m
}

fn bishop_mask(sq: u8) -> u64 {
    const EDGES: u64 = 0xFF00_0000_0000_0000
        | 0x0000_0000_0000_00FF
        | 0x8080_8080_8080_8080
        | 0x0101_0101_0101_0101;
    let mut m = 0u64;
    for &d in &DIAG {
        m |= ray_attack_ref(sq, 0, d);
    }
    m & !EDGES
}

/// Enumerate all subsets of `mask` (carry-rmul order), apply `f` to each.
fn for_each_subset(mask: u64, mut f: impl FnMut(u64)) {
    let mut sub = 0u64;
    loop {
        f(sub);
        sub = sub.wrapping_sub(mask) & mask;
        if sub == 0 {
            break;
        }
    }
}

struct MagicTable {
    masks: [u64; 64],
    magics: [u64; 64],
    off: [u32; 65],
    bits: [u8; 64],
    attacks: Vec<u64>,
}

fn build_magic(seed: u64, what: &str, dirs: [Dir; 4], mask_of: fn(u8) -> u64) -> MagicTable {
    let mut rng = Rng(seed);
    let mut masks = [0u64; 64];
    let mut bits = [0u8; 64];
    for s in 0..64u8 {
        masks[s as usize] = mask_of(s);
        bits[s as usize] = masks[s as usize].count_ones() as u8;
    }
    let tab_len: u32 = bits.iter().map(|&b| 1u32 << b).sum();
    let mut off = [0u32; 65];
    for s in 0..64usize {
        off[s + 1] = off[s] + (1u32 << bits[s]);
    }
    let mut attacks = vec![0u64; tab_len as usize];
    let mut magics = [0u64; 64];

    for s in 0..64u8 {
        let sidx = s as usize;
        let mask = masks[sidx];
        let n = 1usize << bits[sidx];
        let shift = 64 - bits[sidx];

        // Precompute the exact attack set for every occupancy subset, in
        // carry-rmul order (index i == pext(sub, mask)).
        let mut subs = Vec::with_capacity(n);
        let mut atts = Vec::with_capacity(n);
        for_each_subset(mask, |sub| {
            subs.push(sub);
            atts.push(attacks_ref(s, sub, dirs));
        });
        assert_eq!(subs.len(), n, "{what} sq {s}: subset count");

        // Magic search with epoch-stamped slot map (no per-try reset).
        let mut seen = vec![(0u64, 0u32); n];
        let mut epoch = 0u32;
        let mut found = 0u64;
        'tries: for _ in 0..8_000_000u32 {
            let magic = rng.sparse();
            // cheap pre-filter: require entropy in the top bits
            if (mask.wrapping_mul(magic) >> 56).count_ones() < 6 {
                continue;
            }
            epoch += 1;
            for i in 0..n {
                let slot = (subs[i].wrapping_mul(magic) >> shift) as usize;
                let (a, g) = seen[slot];
                if g != epoch {
                    seen[slot] = (atts[i], epoch);
                } else if a != atts[i] {
                    continue 'tries;
                }
            }
            // Candidate valid: fill the flat table region.
            for i in 0..n {
                let slot = (subs[i].wrapping_mul(magic) >> shift) as usize;
                attacks[off[sidx] as usize + slot] = atts[i];
            }
            magics[sidx] = magic;
            found = magic;
            break;
        }
        assert!(found != 0, "{what}: no magic found for square {s}");
    }

    // Build-time PROOF: re-walk every subset and require the flat table
    // entry reached through the runtime lookup path to equal the reference.
    for s in 0..64u8 {
        let sidx = s as usize;
        let mask = masks[sidx];
        let shift = 64 - bits[sidx];
        for_each_subset(mask, |sub| {
            let slot = (sub.wrapping_mul(magics[sidx]) >> shift) as usize;
            let idx = off[sidx] as usize + slot;
            let want = attacks_ref(s, sub, dirs);
            assert_eq!(attacks[idx], want, "{what} sq {s} sub {sub:#x}: disproved");
        });
    }

    // Cross-check the mask construction against the naive reference: a rook
    // mask square must never include the square itself, etc.
    for s in 0..64u8 {
        assert!(masks[s as usize] & (1u64 << s) == 0, "{what} sq {s} mask self-bit");
    }

    MagicTable { masks, magics, off, bits, attacks }
}

fn emit_u64s(w: &mut String, name: &str, data: &[u64]) {
    w.push_str(&format!("pub static {name}: [u64; {}] = [\n", data.len()));
    for chunk in data.chunks(6) {
        w.push_str("    ");
        for v in chunk {
            w.push_str(&format!("{v:#018x}u64, "));
        }
        w.push('\n');
    }
    w.push_str("];\n");
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));

    let rook = build_magic(0x0C0FFEE0_C0FFEE01, "rook", ORTHO, rook_mask);
    let bish = build_magic(0xB16B_00B5_CAFE_00D0, "bishop", DIAG, bishop_mask);

    let mut src = String::with_capacity(2 << 20);
    src.push_str("// @generated by build.rs — proven magic bitboard tables (deterministic).\n");
    src.push_str("// DO NOT EDIT. Every entry was verified against an independent ray walk\n");
    src.push_str("// at build time (build.rs `build_magic` proof loop).\n");
    emit_u64s(&mut src, "GEN_ROOK_MASK", &rook.masks);
    emit_u64s(&mut src, "GEN_ROOK_MAGIC", &rook.magics);
    emit_u64s(&mut src, "GEN_BISHOP_MASK", &bish.masks);
    emit_u64s(&mut src, "GEN_BISHOP_MAGIC", &bish.magics);
    src.push_str(&format!("pub static GEN_ROOK_OFF: [u32; 65] = {:?};\n", rook.off));
    src.push_str(&format!("pub static GEN_BISHOP_OFF: [u32; 65] = {:?};\n", bish.off));
    src.push_str(&format!("pub static GEN_ROOK_BITS: [u8; 64] = {:?};\n", rook.bits));
    src.push_str(&format!("pub static GEN_BISHOP_BITS: [u8; 64] = {:?};\n", bish.bits));
    emit_u64s(&mut src, "GEN_ROOK_ATTACKS", &rook.attacks);
    emit_u64s(&mut src, "GEN_BISHOP_ATTACKS", &bish.attacks);

    fs::write(out_dir.join("magic_generated.rs"), src).expect("write magic_generated.rs");
    println!("cargo:rustc-env=MYCHESS_ROOK_TAB={}", rook.attacks.len());
    println!("cargo:rustc-env=MYCHESS_BISHOP_TAB={}", bish.attacks.len());
}