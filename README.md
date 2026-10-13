# mychess — chess engine core

Board representation, legal move generation, make/unmake with incremental
Zobrist, FEN parsing and perft. **No search, no evaluation, no UCI** — this is
the core the AI layer will be built on top of.

> The crate is named `mychess` rather than `core` because a library crate named
> `core` collides with the Rust sysroot crate `core`. Everything else follows the
> agreed contract.

**hardware: AMD EPYC 7763** (Milan, Zen3 family 0x19, 4C/8SMT, BMI2,
chemin pext actif, no AVX-512; hyperviseur: les nps sont des fourchettes,
jamais des seuils). **d9 = 2,439,530,234,167 @ 1002 Mnps 8T** (vérifié
pré-GATE-2.5; GATE-2.5 M2 a ajouté +17–19 % leaf/1T et ×1.12 8T, voir
MIGRATION.md).
Futur : pas de GPU → **Tier D CPU-only** ; phase MCTS : fastchess SPRT
concurrency = 4–6 (4 cœurs physiques).

> GATE 2.5 : le core est FROZEN après clôture (`vcore-1.0`). La phase MCTS
> le consomme via l'API existante (`Position`, `movegen::generate`,
> `count_moves`, `perft`, make/unmake + Zobrist incrémental) ; pas de
> refactoring core hors bug prouvé (perft revalidé). Contrat d'entrée des
> générateurs : **position légale exigée** (voir « Safety » et
> `generate`/`refgen` doc) — en release un pseudo coup roi-capture n'est
> jamais émis et ne panique jamais ; en debug l'entrée illégale est
> assertée avec message.

## Layout

| module | role |
|---|---|
| `types` | `Square`/`Piece` newtypes, `Move(u16)` (from 6 / to 6 / flags 4), `GenKind`, `PieceKind` |
| `tables` | `const fn` knight/king/pawn attacks, rays, between |
| `slider` | PEXT-indexed sliding attacks (Intel or AMD Zen3+, BMI2) + build-proven magic fallback (portable, ARM/WASM) |
| `position` | `Position` (bitboards + mailbox + castle/ep/halfmove + incremental Zobrist), FEN, `make`/`unmake`/`Undo` |
| `movegen` | legal move generation: fused single-pass checkers/pins/danger (L1c), ray-trick EP legality (L1b), colour-monomorphised leaf `count_moves` (L1d), `MoveList` on the stack |
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
  (`tests/slider_paths.rs`); king-capture contract frozen on illegal entries
  (`tests/illegal_contract.rs`, debug asserts + release silent).
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

**hardware: AMD EPYC 7763** (Zen3, 4C/8SMT, shared hypervisor — wide intervals,
no nps thresholds in CI). Frozen at GATE 2.5 (tag `vcore-1.0`), same-session
interleaved A/B per lot (MIGRATION.md holds the per-lot A/B tables):

| workload | result |
|---|---|
| `count_moves` (leaf) startpos / kiwipete / pos3 / pos6 | ~70 / ~91 / ~40 / ~75 ns per call (criterion, final25) |
| perft(6) 1T criterion (`perft1t/startpos_d6`) | 463 ms → **257 Mnps** (pre-M2: 544 ms / 219 — ×1.17 cumulé, ×1.19 pour L1c seul) |
| perft(7) 1T sweep | 3.20e9 in 11.7 s → **~272 Mnps** (pre-M2 ~206, ×1.32) |
| perft(7) 8T sweep | **~1090–1110 Mnps** (pre-M2 949–984, ×~1.13) |
| make+unmake pair | ~23 ns |
| `gives_check` (per call) | ~24 ns (< 80 ns contract) |
| MCTS descent (make+unmake 30 plies incl. hash) | ~22 ns/ply (< 25 contract) |
| MCTS walkback16 (repetition) | 8.6 ns miss / 1.3 ns hit-at-4 |
| MCTS PST eval pattern | ~39 ns |

Historical (pre-GATE-2.5, EPYC 7763, sessions L1–L6):

| workload | result |
|---|---|
| perft(6) 1 thread | 119,060,324 in 0.55 s → ~210 Mnps |
| perft(6) 8 threads | → ~890 Mnps |
| perft(7) 8 threads | 3,195,901,860 in 3.2–3.4 s → ~950 Mnps |
| perft(8) 8 threads | 84,998,978,956 (verified, 90 s) |
| startpos d9 = 2.44e12 | **2,439,530,234,167 verified in 40:34 at 1002 Mnps 8T** |

Historical (AMD Ryzen 5 1600X, pre-migration):

| workload | result |
|---|---|
| `generate` startpos | ~150 ns / 20 moves |
| `is_square_attacked` | ~10 ns |
| perft(6) 1 thread | 110 Mnps |
| perft(7) 12 threads | 818 Mnps |

Criterion benchmarks (`benches/core.rs`) cover `generate_all`,
`generate_captures` and `count_moves` on startpos/kiwipete/pos3/pos6,
`make`+`unmake`, `gives_check_x8` (per-call semantics, divisor in the name),
the MCTS baselines (`mcts_descend30`, `mcts_rep_walkback16` zero/hit,
`mcts_pst_eval`), perft-leaf bulk counting on Kiwipete and the pre-committed
M2 gate metric `perft1t/startpos_d6`. `benches/slider.rs` A/Bs the dispatch
paths (`MYCHESS_SLIDER=pext|fallback`).

## Phase MCTS (v1 — tag `vmcts-v1`)

Le core (`vcore-1.0`) est **FROZEN** : la recherche le consomme via son
contrat API (`Position`, `generate`, `make`/`unmake`, Zobrist incrémental,
`is_square_attacked`) — aucun refactoring core hors bug prouvé.

### Modules

| module | rôle |
|---|---|
| `eval` | matériel (100/300/300/500/900) + PST symétriques (Michniewski) ; `eval_white` (blanc+) / `eval_for_mover` (trait+) — **hook A1** (v1 ne l'utilise PAS encore : playouts aléatoires) |
| `mcts` | UCT v1 : un nœud étendu par playout, sélection UCT (`C=1.414`), playout aléatoire → résultat {-1,0,1} ; terminalité complète : mat, pat, 50 coups, **triple répétition** (hash-history : UCI replay + chemin + playout), matériel insuffisant ; RNG xorshift64* seedé (déterminisme par seed) |
| `uci` | UCI minimal (uci/isready/ucinewgame/setoption/position/go/stop/quit) ; `go nodes N` = contrôle fixe-nœuds des SPRT ; recherche threadée (stop = AtomicBool sondé toutes les 64 playouts) |

### Structure de nœud (et hooks NN plus tard)

```
Node { visits: u64, sum: f32 (valeur au trait du nœud), term: Option<f32>,
       children: Vec<Child>, expanded: bool }    Child { mv: Move, node: u32 }
```

* **sélection** : v1 = UCT `-(child.avg) + C·sqrt(ln N / n)` ; **A2** le
  remplace par PUCT (le `Child::prior` + hook `prior(pos, mv)` apparaissent là) ;
* **feuille** : depuis A1 = `eval::eval_for_mover` normalisé
  (`eval_normalized`, cp/1000 clamp [-1,1] — convention Q compatible PUCT/NN) ;
  la terminalité exacte {-1,0,1} reste prioritaire à la feuille. (Dans le
  tag `vmcts-v1` la feuille = playout aléatoire — point zéro du ladder.)
* **D1** (NN): policy remplace le hook prior, value remplace le hook eval —
  mêmes surfaces d'appel, zéro refactoring MCTS.

### Mesure Elo (chaque amélioration = un SPRT)

`./test.sh <cand_bin> <base_bin> [nodes] [rounds]` lance fastchess
(SPTRT elo0=0/elo1=5 α=β=0.05 ; tc=nodes=N ; openings EPD déséquilibrées
chaque position deux fois ; concurrency 5). Une ligne par test dans
ELO_LOG.md (modèle Rustic/Sirius). Garde-fou : le script refuse de tourner
tant que M4-d9 (`slaml/d9_detailed.log`) n'est pas réconcilié vers
`slaml/cpw_d9_reference.md` (BENCH-FREEZE), sauf `MYCHESS_ELO_ALLOW=1`.
Openings officielles : placer `openings/uho_lichess_4852_v1.epd`
(voir openings/README.md ; `builtin_min.epd` = smoke seulement).

### Ladder (une étape à la fois, SPRT seul verdict)

A1 eval-leaf → A2 PUCT+priors → A3 FPU → A4 cpuct → B1 virtual loss+parallel →
B2 tree reuse → B3 transpositions → C1 dirichlet → C2 priors SEE/history →
C3 tapered eval. Interdits : RAVE, ordre Sirius copié, multi-améliorations.

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
