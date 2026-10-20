//! Sliding-piece attacks: PEXT-indexed lookup on BMI2-capable hardware
//! (Intel, or AMD Zen3/family 0x19+), build-generated proven magic bitboards
//! otherwise (portable: pure arithmetic, also ARM/WASM). Runtime dispatch is
//! per call and never invokes `_pext_u64` without a positive
//! `is_x86_feature_detected!` check (avoids SIGILL on pre-Zen3 AMD / older
//! Intel).
//!
//! DEVIATIONS (documented):
//!  * The PEXT attack tables are built once with `OnceLock` rather than in a
//!    `const`: the ~2 MB rook table trips rustc's hard `long_running_const_eval`
//!    deny. `OnceLock` is thread-safe and leaves the per-node path as a single
//!    relaxed atomic load; there is no mutable `static`.
//!  * The magic fallback tables are generated and PROVEN at build time
//!    (build.rs verifies every occupancy subset against an independent ray
//!    walk); they are deterministic (fixed-seed PRNG), not unverified
//!    published magic constants.

use crate::tables::{BISHOP_DIRS, RAYS, ROOK_DIRS};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

// L2: build-generated, proven magic tables (portable fallback, no BMI2 and
// no x86 required — pure arithmetic).
include!(concat!(env!("OUT_DIR"), "/magic_generated.rs"));

/// Slider dispatch path selector (for benchmarks, tests and debugging).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SliderPath {
    /// Runtime CPUID decision (vendor + family + BMI2 feature).
    Auto,
    /// Force the PEXT-indexed tables (requires BMI2 hardware).
    Pext,
    /// Force the portable fallback: build-generated, proven magic bitboards
    /// (pure arithmetic; also the ARM/WASM path; ray walk stays available as
    /// the in-crate reference via [`classical`]).
    Fallback,
}

/// 0 = unset (init on first use), 1 = fallback magic, 2 = PEXT.
const DISPATCH_UNSET: u8 = 0;
const DISPATCH_MAGIC: u8 = 1;
const DISPATCH_PEXT: u8 = 2;
static DISPATCH: AtomicU8 = AtomicU8::new(DISPATCH_UNSET);

const EDGES: u64 =
    crate::tables::RANK_1 | crate::tables::RANK_8 | crate::tables::FILE_A | crate::tables::FILE_H;

const fn rook_mask(sq: usize) -> u64 {
    // Exclude only the far edge square in each direction (not the whole file),
    // otherwise rooks on the a/h files lose their own vertical blockers.
    let north = RAYS[ROOK_DIRS[0]][sq] & !crate::tables::RANK_8;
    let east = RAYS[ROOK_DIRS[1]][sq] & !crate::tables::FILE_H;
    let south = RAYS[ROOK_DIRS[2]][sq] & !crate::tables::RANK_1;
    let west = RAYS[ROOK_DIRS[3]][sq] & !crate::tables::FILE_A;
    north | east | south | west
}

const fn bishop_mask(sq: usize) -> u64 {
    // Diagonals always terminate on a board edge, so removing all edges is exact.
    let mut m = 0u64;
    let mut i = 0;
    while i < 4 {
        m |= RAYS[BISHOP_DIRS[i]][sq];
        i += 1;
    }
    m & !EDGES
}

const fn build_masks(bishop: bool) -> [u64; 64] {
    let mut t = [0u64; 64];
    let mut s = 0;
    while s < 64 {
        t[s] = if bishop { bishop_mask(s) } else { rook_mask(s) };
        s += 1;
    }
    t
}

pub const ROOK_MASKS: [u64; 64] = build_masks(false);
pub const BISHOP_MASKS: [u64; 64] = build_masks(true);

// const-context: keep the bit loop obvious.
fn pext_const(x: u64, mut mask: u64) -> u64 {
    let mut res = 0u64;
    let mut k = 0u32;
    while mask != 0 {
        let b = mask.isolate_lowest_one();
        if x & b != 0 {
            res |= 1u64 << k;
        }
        mask ^= b;
        k += 1;
    }
    res
}

fn build_table(bishop: bool, masks: &[u64; 64]) -> Vec<[u64; 4096]> {
    let mut t: Vec<[u64; 4096]> = vec![[0u64; 4096]; 64];
    let dirs = if bishop { BISHOP_DIRS } else { ROOK_DIRS };
    for s in 0..64 {
        let mask = masks[s];
        let mut sub = 0u64;
        loop {
            let idx = pext_const(sub, mask) as usize;
            let mut att = 0u64;
            let mut i = 0;
            while i < 4 {
                att |= ray_attack_ref(s as u8, sub, dirs[i]);
                i += 1;
            }
            t[s][idx] = att;
            sub = sub.wrapping_sub(mask) & mask;
            if sub == 0 {
                break;
            }
        }
    }
    t
}

static ROOK_TABLE: OnceLock<Vec<[u64; 4096]>> = OnceLock::new();
static BISHOP_TABLE: OnceLock<Vec<[u64; 4096]>> = OnceLock::new();

#[inline]
fn rook_table() -> &'static Vec<[u64; 4096]> {
    ROOK_TABLE.get_or_init(|| build_table(false, &ROOK_MASKS))
}
#[inline]
fn bishop_table() -> &'static Vec<[u64; 4096]> {
    BISHOP_TABLE.get_or_init(|| build_table(true, &BISHOP_MASKS))
}

/// Arithmetic ray walk used for table construction (no intrinsics).
fn ray_attack_ref(sq: u8, occ: u64, d: usize) -> u64 {
    let dirs = crate::tables::DIRS;
    let mut f = (sq % 8) as i32;
    let mut r = (sq / 8) as i32;
    let mut res = 0u64;
    loop {
        f += dirs[d].0;
        r += dirs[d].1;
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

/// In-crate reference: classical ray walk, used by `_selftest` and tests to
/// validate the PEXT and magic paths.
#[inline(always)]
pub fn classical(sq: u8, occ: u64, dirs: &[usize; 4]) -> u64 {
    crate::tables::ray_attack(sq, occ, dirs[0])
        | crate::tables::ray_attack(sq, occ, dirs[1])
        | crate::tables::ray_attack(sq, occ, dirs[2])
        | crate::tables::ray_attack(sq, occ, dirs[3])
}

// ---- L2 portable fallback: magic bitboards (build-proven tables) ----------

/// Magic-table lookup: pure arithmetic, works on any 64-bit target.
/// Index = `(occ & mask) * magic >> (64 - bits)` into the square's flat
/// region of the build-verified table.
#[inline(always)]
fn magic_lookup(
    sq: u8,
    occ: u64,
    table: &[u64],
    off: &[u32; 65],
    masks: &[u64; 64],
    magics: &[u64; 64],
    bits: &[u8; 64],
) -> u64 {
    let s = sq as usize;
    let idx =
        off[s] as usize + (((occ & masks[s]).wrapping_mul(magics[s])) >> (64 - bits[s])) as usize;
    table[idx]
}

#[inline(always)]
fn magic_rook(sq: u8, occ: u64) -> u64 {
    magic_lookup(
        sq,
        occ,
        &GEN_ROOK_ATTACKS,
        &GEN_ROOK_OFF,
        &GEN_ROOK_MASK,
        &GEN_ROOK_MAGIC,
        &GEN_ROOK_BITS,
    )
}

#[inline(always)]
fn magic_bishop(sq: u8, occ: u64) -> u64 {
    magic_lookup(
        sq,
        occ,
        &GEN_BISHOP_ATTACKS,
        &GEN_BISHOP_OFF,
        &GEN_BISHOP_MASK,
        &GEN_BISHOP_MAGIC,
        &GEN_BISHOP_BITS,
    )
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "bmi2")]
unsafe fn pext(x: u64, mask: u64) -> u64 {
    core::arch::x86_64::_pext_u64(x, mask)
}

#[inline(always)]
pub fn rook_attacks(sq: u8, occ: u64) -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        if use_pext() {
            // SAFETY: guarded by the runtime BMI2 detection above.
            let idx = unsafe { pext(occ, ROOK_MASKS[sq as usize]) } as usize;
            return rook_table()[sq as usize][idx];
        }
    }
    magic_rook(sq, occ)
}

#[inline(always)]
pub fn bishop_attacks(sq: u8, occ: u64) -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        if use_pext() {
            // SAFETY: guarded by the runtime BMI2 detection above.
            let idx = unsafe { pext(occ, BISHOP_MASKS[sq as usize]) } as usize;
            return bishop_table()[sq as usize][idx];
        }
    }
    magic_bishop(sq, occ)
}

#[inline(always)]
pub fn queen_attacks(sq: u8, occ: u64) -> u64 {
    rook_attacks(sq, occ) | bishop_attacks(sq, occ)
}

// ---- dispatch: CPUID vendor + family gating --------------------------------

#[cfg(target_arch = "x86_64")]
fn cpuid_vendor() -> String {
    #[allow(unused_unsafe)] // safe fn on some toolchains, unsafe on others
    // SAFETY: CPUID leaf 0 is unconditionally valid on x86_64.
    let r = unsafe { core::arch::x86_64::__cpuid(0) };
    let mut v = String::with_capacity(12);
    for part in [r.ebx, r.edx, r.ecx] {
        for b in part.to_le_bytes() {
            if b.is_ascii_graphic() {
                v.push(b as char);
            } else {
                return v;
            }
        }
    }
    v
}

/// Effective CPU family (e.g. 0x19 = 25 = Zen3 EPYC 7003, 6 = Intel).
#[cfg(target_arch = "x86_64")]
fn cpu_family() -> u16 {
    #[allow(unused_unsafe)] // safe fn on some toolchains, unsafe on others
    // SAFETY: CPUID leaf 1 is unconditionally valid on x86_64.
    let r = unsafe { core::arch::x86_64::__cpuid(1) };
    let base = ((r.eax >> 8) & 0xF) as u16;
    let ext = ((r.eax >> 20) & 0xFF) as u16;
    if base == 0xF { base + ext } else { base }
}

/// `pext` is fast on all Intel, and on AMD from Zen3 (family 0x19, Milan
/// included) where it is a single-cycle hardware instruction. On AMD Zen1/2
/// `pext` is microcoded (~18 cycles) and loses to the ray walk.
#[cfg(target_arch = "x86_64")]
fn auto_select_pext(bmi2: bool) -> bool {
    if !bmi2 {
        return false;
    }
    match cpuid_vendor().as_str() {
        "GenuineIntel" => true,
        "AuthenticAMD" => cpu_family() >= 0x19,
        _ => false,
    }
}

/// Lazily resolve the dispatch path (once per process): env override
/// `MYCHESS_SLIDER=pext|fallback` (alias `rays`/`magic`) for A/B
/// benchmarking, else CPUID auto-detection.
#[cfg(target_arch = "x86_64")]
fn init_dispatch() -> u8 {
    let bmi2 = std::is_x86_feature_detected!("bmi2");
    let env = std::env::var("MYCHESS_SLIDER")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let v = match env.as_str() {
        "pext" if bmi2 => DISPATCH_PEXT,
        "pext" => {
            eprintln!("mychess: MYCHESS_SLIDER=pext ignored, BMI2 not detected -> fallback");
            DISPATCH_MAGIC
        }
        "fallback" | "rays" | "magic" => DISPATCH_MAGIC,
        _ => {
            if auto_select_pext(bmi2) {
                DISPATCH_PEXT
            } else {
                DISPATCH_MAGIC
            }
        }
    };
    DISPATCH.store(v, Ordering::Relaxed);
    v
}

#[inline(always)]
fn use_pext() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        let v = DISPATCH.load(Ordering::Relaxed);
        if v != DISPATCH_UNSET {
            return v == DISPATCH_PEXT;
        }
        init_dispatch() == DISPATCH_PEXT
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// Override the dispatch path (tests / A/B benchmarks). `Auto` restores
/// runtime detection. Forcing `Pext` on hardware without BMI2 keeps the
/// fallback (no SIGILL).
pub fn force_path(p: SliderPath) {
    #[cfg(target_arch = "x86_64")]
    match p {
        SliderPath::Auto => DISPATCH.store(DISPATCH_UNSET, Ordering::Relaxed),
        SliderPath::Fallback => DISPATCH.store(DISPATCH_MAGIC, Ordering::Relaxed),
        SliderPath::Pext => {
            if std::is_x86_feature_detected!("bmi2") {
                DISPATCH.store(DISPATCH_PEXT, Ordering::Relaxed);
            } else {
                eprintln!("mychess: force_path(Pext) ignored, BMI2 not detected");
                DISPATCH.store(DISPATCH_MAGIC, Ordering::Relaxed);
            }
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    let _ = p;
}

/// Name of the path used after dispatch init: `"pext"` or `"magic"`.
pub fn path_name() -> &'static str {
    if use_pext() { "pext" } else { "magic" }
}

/// Human-readable description for startup logging, e.g.
/// `pext (AuthenticAMD family 0x19 + bmi2 + movbe)`.
pub fn describe() -> String {
    let mut s = format!("slider path: {}", path_name());
    #[cfg(target_arch = "x86_64")]
    {
        let vendor = cpuid_vendor();
        let fam = cpu_family();
        let mut feats = Vec::new();
        for (present, name) in [
            (std::is_x86_feature_detected!("bmi2"), "bmi2"),
            (std::is_x86_feature_detected!("popcnt"), "popcnt"),
            (std::is_x86_feature_detected!("movbe"), "movbe"),
        ] {
            if present {
                feats.push(name);
            }
        }
        let feats = feats.join("+");
        s += &format!(" ({vendor} family {fam:#04x} + {feats})");
    }
    s
}

/// Squares between `a` and `b` (0 when not aligned).
#[inline(always)]
pub fn between(a: u8, b: u8) -> u64 {
    crate::tables::BETWEEN[a as usize][b as usize]
}

/// Debug helper: verify the active dispatch path agrees with the classical
/// ray walk over every square and a spread of occupancies.
pub fn _selftest() -> bool {
    for sq in 0..64u8 {
        for occ in [0u64, 0x00FF_0000_0000_FF00, 0x8142_2418_1824_4281, u64::MAX] {
            if classical(sq, occ, &ROOK_DIRS) != rook_attacks(sq, occ) {
                return false;
            }
            if classical(sq, occ, &BISHOP_DIRS) != bishop_attacks(sq, occ) {
                return false;
            }
        }
    }
    true
}
