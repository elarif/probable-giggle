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
| `mcts` | UCT v1 : un nœud étendu par playout, sélection UCT (`C=1.414`), terminalité complète : mat, pat, 50 coups, **triple répétition** (hash-history : UCI replay + chemin), matériel insuffisant ; RNG xorshift64* seedé (déterminisme par seed) ; **phase 3** : arène plate `Node`/`Edge` (enfant alloué au développement, statistiques de l'enfant portées par l'arête) et terminalité par `has_legal_move` (sans matérialiser de `MoveList`) |
| `uci` | UCI minimal (uci/isready/ucinewgame/setoption/position/go/stop/quit) ; `go nodes N` = contrôle fixe-nœuds des SPRT ; **option `Threads`** (1 par défaut) : parallélisme **racine**, budget de nœuds réparti ou budget de temps partagé ; recherche threadée (stop = AtomicBool sondé toutes les 64 playouts) |

### Structure de l'arbre (phase 3) et hooks NN plus tard

```
Node { visits: u32, sum: f32 (valeur au trait du nœud), term: f32,
       first_edge: u32, n_edges: u8, flags: u8 }                    // 20 o
Edge { mv: Move, child: u32, visits: u32, sum: f32, prior: f32 }   // 20 o
```

* **deux arènes plates** (`Vec<Node>`, `Vec<Edge>`) : les arêtes d'un nœud
  sont contiguës, et le nœud enfant n'est alloué qu'au **développement**
  (deuxième visite de l'arête) — l'arbre v1 allouait un nœud complet par
  coup légal, enfants jamais joués compris ;
* les statistiques de l'enfant vivent sur l'**arête** : `node.visits ==
  edge.visits` et `node.sum == edge.sum` (invariant du miroir), et le scan
  UCT ne lit qu'un tableau contigu ;
* **sélection** : v1 = UCT `-(child.avg) + C·sqrt(ln N / n)` ; **phase 4** : PUCT `Q + c_puct·P·sqrt(N)/(1+n)` sur les enfants visités, la largeur restant le tirage uniforme v1 — `P` est le prior MVV-LVA calculé à l'expansion (`mcts::policy`, stocké dans `Edge::prior`) ;
* **feuille** : depuis A1 = `eval::eval_for_mover` normalisé
  (`eval_normalized`, cp/1000 clamp [-1,1] — convention Q compatible PUCT/NN) ;
  la terminalité exacte {-1,0,1} reste prioritaire à la feuille. (Dans le
  tag `vmcts-v1` la feuille = playout aléatoire — point zéro du ladder.)
* **D1** (NN): policy remplace le hook prior, value remplace le hook eval —
  mêmes surfaces d'appel, zéro refactoring MCTS.

### Phase 3 — arène plate et parallélisme racine

Le chemin mono est **bit-à-bit identique** au moteur v1 (même ordre de
tirage du RNG) : l'échelle SPRT n'est pas invalidée, aucune série Elo n'est
à refaire. Mesures (session unique, alternance v1 / phase 3, hôte Ryzen
5 1600X) : débit **×1,47** (startpos) à **×1,67** (kiwipete), RSS **÷2,5**,
parallélisme racine **×5,3** à 6 threads à budget de nœuds constant et
**×6,4** en mode débit. Plan, décisions et rapport :
`docs/superpowers/plans/2026-10-22-mcts-phase3-arene-parallelisme.md` et
`docs/mcts/phase3-2026-10-22.md`.

### Phase 4 — PUCT et priors de policy

La règle de sélection du MCTS devient **PUCT** : `Q + c_puct·P·sqrt(N)/(1+n)`,
où `P` est le **prior** de policy du coup — un score **MVV-LVA** (captures +
promotions) transformé en distribution de probabilité sur les coups légaux
du nœud, calculé **une fois** à l'expansion et stocké dans `Edge::prior`
(`mcts::policy`). La largeur de l'arbre (tirage uniforme parmi les enfants
non visités) est **inchangée** ; c'est la **profondeur** qui change, et la
forme de décroissance du terme d'exploration (`1/(1+n)` au lieu de
`1/sqrt(n)`) qui fait le gain.

La règle UCT d'origine est **conservée et figée** (`Policy::Uct`) : elle
reste l'oracle du test différentiel de la phase 3 (épinglé dessus) et la
contre-mesure A/B de la phase 4. Options UCI : `Policy` (`puct`/`uct`),
`Cpuct`, `RootNoise`.

Mesures (session unique, alternance A/B/A/B) : débit **−10 %** sur kiwipete
(+6 % sur startpos) mais **RSS ÷1,55** et arbre **÷1,6** ; batterie
tactique à 100 nœuds : **22/22** en mono et **17/22** à 6 threads contre
**20/22** et **10/22** pour l'UCT ; convergence en valeur (erreur de score
contre une recherche de référence) divisée par **4 à 7** à 500–2 000 nœuds.
**Ablation : le prior MVV-LVA lui-même n'apporte rien de mesurable** — le
gain vient de la forme de décroissance ; le prior est livré comme **crochet**
du pipeline « logits → softmax → prior » que la policy apprise de D1
branchera. Plan et rapport : `docs/superpowers/plans/2026-10-22-mcts-phase4-puct-priors.md`
et `docs/mcts/phase4-2026-10-22.md`.

**L'Elo de la phase 4 est mesuré et la phase est acceptée** : `fastchess`
a été compilé depuis les sources le 2026-10-22, et l'échelle SPRT
(`elo0=0 elo1=5`, α=β=0.05) **phase 4 (PUCT) contre phase 3 (UCT)** donne
**+399,7 ± 50,9 Elo** à 2 000 nœuds, **+596,5 ± 90,9** à 10 000 et
**+450,4 ± 55,2** à 40 000 : **H1 accepté aux trois budgets**. Le contrôle
temps-fixe (TC 8+0.08) confirme : **+303,3 ± 36,4**, H1 accepté à 350
parties. Deux
contrôles : phase 3 contre phase 3 → 50,00 % (LLR −0,01) ; phase 4 forcé
en `Policy=uct` contre phase 3 → 50,00 % (LLR −0,08) — l'écart est donc
imputable à la seule règle PUCT. Écarts au protocole : book d'ouvertures
de remplacement (ECO lichess, 3 863 positions, `tools/gen_epd_openings.py`)
au lieu de l'UHO, et hôte non EPYC — d'où des Elo **relatifs**, à rejouer
sur le book officiel et l'hôte de référence.
### Phase 5 — feuille tactiquement résolue (quiescence-lite) : **mesurée, revertie**

La phase 5 attaque l'**effet horizon** de la feuille : depuis A1, la
valeur d'une feuille est l'éval statique d'une position *quelconque*, qui
peut être au milieu d'un échange. Le nouveau module `mcts::qsearch`
calcule `qvalue(pos, d)` : la valeur **pour le trait** après avoir résolu
les échanges sur `d` plies, avec la convention classique du **stand-pat**
(le trait peut refuser l'échange). `Limits.leaf_q` et l'option UCI
`LeafQ` le pilotent ; `leaf_q = 0` est **bit-à-bit** le chemin de la
phase 4.

**Le résultat est négatif, et il est mesuré.** Contre la phase 4 gelée,
en nœuds-fixes :

| configuration | parties | Elo ± err | LLR |
|---|---|---|---|
| `LeafQ=1`, 2 000 nœuds | 400 | **−177,2 ± 26,4** | −2,83 |
| `LeafQ=2`, 2 000 nœuds | 400 | **−118,1 ± 23,3** | −2,33 |
| `LeafQ=1`, 10 000 nœuds | 370 | **−240,8 ± 26,7** | −2,95 (H0) |
| `LeafQ=2`, 10 000 nœuds | 400 | **−154,5 ± 24,9** | −2,67 |
| `LeafQ=1`, 40 000 nœuds | 398 | **−197,9 ± 24,3** | −2,95 (H0) |
| `LeafQ=1`, TC 8+0.08 (temps-fixe) | 360 | **−259,5 ± 31,0** | −2,95 (H0) |
| contrôle `LeafQ=0` contre phase 4 | 400 | 0,0 ± 0,0 | −0,08 (neutre) |

Le mécanisme est mesuré lui aussi : le `max` de la quiescence est
**optimiste** (en profondeur 1, la capture est supposée tenir : la
réplique n'est pas vue) et il **double compte** les échanges que l'arbre
explorera de lui-même une fois le nœud développé. Résultat : la valeur
de feuille est gonflée de **+354 cp en moyenne** pour le trait à la
feuille, et cette inflation — positionnelle, donc systématique — fausse
les Q et rend le moteur excessivement défensif. La régression est
**monotone en l'inflation** (`q1` : +354 cp → −177 Elo ; `q2` : +196 cp
→ −118 Elo), ce qui confirme le diagnostic. Le débit, lui, perd 27 à
64 % à `LeafQ=1` (88,7 % des positions ont un échange, et la quiescence
refait une génération légale complète par feuille).

**Décision : `LeafQ` reste à `0`** (le comportement de la phase 4), le
module et ses tests sont conservés comme **expérience documentée**, et le
défaut de conception est écrit : une feuille de MCTS veut une
**espérance**, pas un `max` adversarial. La suite naturelle est la
variante **non biaisée** (moyenne sur les captures au lieu du max),
dont la correction serait dix fois plus petite.

Le verdict est **le même dans les deux régimes de mesure** : en
nœuds-fixes (−177 à −241 Elo) et en **temps-fixe** (TC 8+0.08,
−259 Elo), où la régression est **plus forte** encore — la quiescence y
coûte 27 à 64 % de débit, donc autant de nœuds en moins à budget de
temps égal.

La phase 5 a en revanche **corrigé un vrai défaut** laissé par la phase 4 :
le budget de réflexion avait un **plancher de 50 ms**, si bien que le
moteur réfléchissait plus longtemps que son horloge (158/200 parties
adjugées sur dépassement en TC 1+0.01). Le budget est désormais **borné
par le temps restant** moins une marge de 5 ms, et `go wtime 20` répond
en 16,3 ms au pire. Plan et rapport :
`docs/superpowers/plans/2026-10-22-mcts-phase5-feuille-quiescence.md` et
`docs/mcts/phase5-2026-10-22.md`.

### Mesure Elo (chaque amélioration = un SPRT)
`./test.sh <cand_bin> <base_bin> [nodes] [rounds]` lance fastchess
(SPTRT elo0=0/elo1=5 α=β=0.05 ; tc=nodes=N ; openings EPD déséquilibrées
chaque position deux fois ; concurrency 5). Une ligne par test dans
ELO_LOG.md (modèle Rustic/Sirius). Garde-fou : le script refuse de tourner
tant que M4-d9 (`slaml/d9_detailed.log`) n'est pas réconcilié vers
`slaml/cpw_d9_reference.md` (BENCH-FREEZE), sauf `MYCHESS_ELO_ALLOW=1`.
Openings officielles : placer `openings/uho_lichess_4852_v1.epd`
(voir openings/README.md ; `builtin_min.epd` = smoke seulement).

`fastchess` n'était pas installé sur le poste de mesure ; il a été
**compilé depuis les sources** le 2026-10-22 :

```bash
git clone --depth 1 https://github.com/Disservin/fastchess.git && cd fastchess && make
install fastchess ~/.local/bin/
```

Si l'UHO n'est pas téléchargeable, `python3 tools/gen_epd_openings.py
/tmp/eco_lichess.epd` produit un book de remplacement (3 863 positions ECO
lichess) — **les Elo mesurés avec ce book sont relatifs**, à rejouer sur
l'UHO pour publier.

### Ladder (une étape à la fois, SPRT seul verdict)

A1 eval-leaf → **A2 PUCT+priors (fait, phase 4, Elo +400 à +600)** → A3 FPU →
A4 cpuct → B1 virtual loss+parallel → B2 tree reuse → B3 transpositions →
C1 dirichlet → C2 priors SEE/history → C3 tapered eval. Interdits : RAVE,
ordre Sirius copié, multi-améliorations.

> **Phase 5 (quiescence-lite de la feuille) : REVERTIE** — mesurée, −118 à
> −241 Elo contre la phase 4. Elle n'entre donc pas dans le ladder ; le
> correctif du budget temps qu'elle a produit est **conservé**. La
> variante **non biaisée** de la feuille (une espérance, pas un `max`) est
> la candidate suivante sur ce sujet.

La suite du ladder côté entraînement est planifiée dans SELFPLAY.md
(**v2** — plan corrigé, canonique) : chiffrage CPU/NPS en P-1, décision
écrite self-play vs distillation (SP-4), labels par provenance+confiance,
exploration racine S17, gates en pente S16 v2. Diff v1→v2 tracé dans le
plan.

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
