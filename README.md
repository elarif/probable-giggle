# mychess — chess engine core

Board representation, legal move generation, make/unmake with incremental
Zobrist, FEN parsing and perft. **No search, no evaluation, no UCI** — this is
the core the AI layer will be built on top of.

> The crate is named `mychess` rather than `core` because a library crate named
> `core` collides with the Rust sysroot crate `core`. Everything else follows the
> agreed contract.

**hardware: EPYC 7763** (Milan, Zen3 family 0x19, 4C/8SMT, BMI2, no AVX-512;
hyperviseur: les nps sont des fourchettes, jamais des seuils).
Futur : pas de GPU → **Tier D CPU-only** ; phase MCTS : fastchess SPRT
concurrency = 4–6 (4 cœurs physiques).

## Layout

| module | role |
|---|---|
| `types` | `Square`/`Piece` newtypes, `Move(u16)` (from 6 / to 6 / flags 4), `GenKind`, `PieceKind` |
| `tables` | `const fn` knight/king/pawn attacks, rays, between |
| `slider` | PEXT-indexed sliding attacks (Intel or AMD Zen3+, BMI2) + build-proven magic fallback (portable, ARM/WASM) |
| `position` | `Position` (bitboards + mailbox + castle/ep/halfmove + incremental Zobrist), FEN, `make`/`unmake`/`Undo` |
| `movegen` | legal move generation with checkmask / pinmask bulk, `MoveList` on the stack |
| `refgen` | slow obvious oracle (pseudo-legal + make/test) for differential tests |
| `perft` | bulk-counting perft, detailed counters, `divide`, rayon parallel |

## CLI

```
mychess perft  <fen> <depth> [--detailed]     # node count, or full counter table
mychess perft  <fen> <depth> --memo           # memoized (8 GiB cap, d>=3)
mychess divide <fen> <depth> [--depth2]       # per-root / per-root-reply counts
mychess sweep  <fen> <depth> <threads_csv> [--reps N]   # thread-sweep A/B/A/B helper
mychess bench                                 # quick 1-thread and N-thread nps
```

Every invocation logs the active slider path on stderr, e.g.
`[slider path: pext (AuthenticAMD family 0x19 + bmi2+popcnt+movbe)]`.
`MYCHESS_SLIDER=pext|fallback` forces one path for A/B runs.

## Tests

Golden files live in `tests/perft/*.txt` (source:
[chessprogramming.org/Perft_Results](https://www.chessprogramming.org/Perft_Results)).

* **T1 (CI):** node totals up to depth 5 for all six positions; detailed columns
  up to depth 4; fast perft cross-checked against `refgen`; **both slider
  paths (pext + magic fallback) run the whole T1 net forced**
  (`tests/slider_paths.rs`).
* **T2 (opt-in, `--ignored`):** depth 6–8 nodes/columns; parallel deep nodes.
* **T3 (opt-in):** startpos depth 9 (parallel).

```sh
cargo test --release                                   # T1 + differential, both paths
cargo test --release -- --ignored                      # T2/T3 (long)
cargo bench                                            # Criterion (+ --bench slider for path A/B)
```

Differential test: 60 self-play games × 80 plies compare fast movegen against the
`refgen` oracle move-by-move and assert `zobrist == recompute` after every make
and unmake.

## Measured performance

**hardware: EPYC 7763** (Zen3, 4C/8SMT, shared hypervisor — wide intervals,
no nps thresholds in CI):

| workload | result |
|---|---|
| `generate` startpos | ~98 ns / 20 moves |
| make+unmake pair | ~24 ns |
| perft(6) 1 thread | 119,060,324 in 0.55 s → **~210 Mnps** |
| perft(6) 8 threads | → **~890 Mnps** |
| perft(7) 8 threads | 3,195,901,860 in 3.2–3.4 s → **~950 Mnps** |
| perft(8) 8 threads | 84,998,978,956 (verified, 90 s) |
| startpos d9 = 2.44e12 | **2,439,530,234,167 verified in 40:34 at 1002 Mnps 8T** |

Historical (AMD Ryzen 5 1600X, pre-migration):

| workload | result |
|---|---|
| `generate` startpos | ~150 ns / 20 moves |
| `is_square_attacked` | ~10 ns |
| perft(6) 1 thread | 110 Mnps |
| perft(7) 12 threads | 818 Mnps |

Criterion benchmarks (`benches/core.rs`) cover `generate_all` and
`generate_captures` on startpos/kiwipete/pos3/pos6, `make`+`unmake`,
`gives_check`, incremental Zobrist, and perft-leaf bulk counting on Kiwipete.
`benches/slider.rs` A/Bs the dispatch paths (`MYCHESS_SLIDER=pext|fallback`).

## Documented deviations from the spec

1. **PEXT tables are built once with `OnceLock`, not `const`.** With correct
   masks the ~2 MB rook table trips rustc's hard `long_running_const_eval`
   deny. `OnceLock` is thread-safe, not a mutable `static`, and leaves the
   per-node path as one relaxed atomic load.
2. **The portable fallback is build-generated, proven magic bitboards** —
   `build.rs` finds magic constants with a deterministic PRNG and *proves*
   every table entry at build time against an independent ray-walk
   reimplementation (no unverified published magic constants ever ship).
3. **PEXT is used when BMI2 AND (Intel OR AMD family ≥ 0x19).** AMD Zen1/2
   microcode `pext` (~18 cycles) loses to the fallback; Zen3+ has hardware
   pext. Runtime CPUID vendor+family check picks the fast path per machine;
   the log line at startup records the decision.
4. **Leaf counting at `depth == 1` is popcount-based** (`movegen::count_moves`
   mirrors `generate` but popcounts target bitboards — no MoveList write, no
   move encoding). Differential-checked against the generator and the oracle
   on golden positions and thousands of self-play positions.

## Safety

`unsafe` is confined to the BMI2 `_pext_u64` call in `slider.rs`, guarded by a
runtime `is_x86_feature_detected!("bmi2")` + CPUID vendor/family check
(`bmi2 && (Intel || AMD family >= 0x19)`). `// SAFETY:` comments document each
use. The magic fallback path is pure arithmetic (no intrinsics, no unsafe). The
rest of the crate is safe Rust; no `transmute` integer→enum.
