# mychess — chess engine core

Board representation, legal move generation, make/unmake with incremental
Zobrist, FEN parsing and perft. **No search, no evaluation, no UCI** — this is
the core the AI layer will be built on top of.

> The crate is named `mychess` rather than `core` because a library crate named
> `core` collides with the Rust sysroot crate `core`. Everything else follows the
> agreed contract.

## Layout

| module | role |
|---|---|
| `types` | `Square`/`Piece` newtypes, `Move(u16)` (from 6 / to 6 / flags 4), `GenKind`, `PieceKind` |
| `tables` | `const fn` knight/king/pawn attacks, rays, between |
| `slider` | PEXT-indexed sliding attacks (Intel+BMI2) + classical ray fallback |
| `position` | `Position` (bitboards + mailbox + castle/ep/halfmove + incremental Zobrist), FEN, `make`/`unmake`/`Undo` |
| `movegen` | legal move generation with checkmask / pinmask bulk, `MoveList` on the stack |
| `refgen` | slow obvious oracle (pseudo-legal + make/test) for differential tests |
| `perft` | bulk-counting perft, detailed counters, `divide`, rayon parallel |

## CLI

```
mychess perft  <fen> <depth> [--detailed]   # node count, or full counter table
mychess divide <fen> <depth> [--depth2]     # per-root / per-root-reply counts
mychess bench                               # quick 1-thread and 12-thread nps
```

## Tests

Golden files live in `tests/perft/*.txt` (source:
[chessprogramming.org/Perft_Results](https://www.chessprogramming.org/Perft_Results)).

* **T1 (CI):** node totals up to depth 5 for all six positions; detailed columns
  up to depth 4; fast perft cross-checked against `refgen`.
* **T2 (opt-in, `--ignored`):** depth 6–8 nodes/columns; parallel deep nodes.
* **T3 (opt-in):** startpos depth 9 (parallel).

```sh
cargo test --release                                   # T1 + differential
cargo test --release -- --ignored                      # T2/T3 (long)
cargo bench                                            # Criterion
```

Differential test: 60 self-play games × 80 plies compare fast movegen against the
`refgen` oracle move-by-move and assert `zobrist == recompute` after every make
and unmake.

## Measured performance

AMD Ryzen 5 1600X, `--release` (lto, codegen-units=1):

| workload | result |
|---|---|
| `generate` startpos | ~150 ns / 20 moves |
| `is_square_attacked` | ~10 ns |
| perft(6) 1 thread | 119,060,324 in 1.08 s → **110 Mnps** |
| perft(7) 12 threads | 3,195,901,860 in 3.9 s → **818 Mnps** |
| perft(8) 12 threads | 84,998,978,956 (verified, ~2 min) |
| startpos d9 = 2.44e12 | ≈ 40 min at 1 Gnps; ~50 min at this machine's ~820 Mnps (not run to completion here) |

Criterion benchmarks (`benches/core.rs`) cover `generate_all` and
`generate_captures` on startpos/kiwipete/pos3/pos6, `make`+`unmake`,
`gives_check`, incremental Zobrist, and perft-leaf bulk counting on Kiwipete.

## Documented deviations from the spec

1. **PEXT tables are built once with `OnceLock`, not `const`.** With correct
   masks the ~2 MB rook table trips rustc's hard `long_running_const_eval`
   deny. `OnceLock` is thread-safe, not a mutable `static`, and leaves the
   per-node path as one relaxed atomic load.
2. **Fallback is the classical ray walk, not "magic bitboards"** — no unverified
   published magic constants are used.
3. **PEXT is used only on Intel+BMI2.** On AMD Zen1/2 `pext` is microcoded
   (~18 cycles) and slower than the classical walk; runtime CPUID vendor check
   picks the fast path per machine.
4. **Bulk counting** is used at depth 1 (`MoveList::len`); it is not popcount-based
   counting of a target set, which would be a further optimisation.

## Safety

`unsafe` is confined to the BMI2 `_pext_u64` call in `slider.rs`, guarded by a
runtime `is_x86_feature_detected!("bmi2")` + Intel vendor check. `// SAFETY:`
comments document each use. The rest of the crate is safe Rust; no `transmute`
integer→enum.
