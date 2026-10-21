# Movegen : portage GCT vs `movegen` vs `motors` — mesures du 2026-10-21

Rapport de la tâche 8 du plan « portage GCT ». Aucun code moteur n'est modifié
ici : le portage `src/gct/` (tâches 1–7) est mesuré contre `src/movegen.rs`
et contre `motors`.

## Protocole

- Hôte : AMD Ryzen 5 1600X (Zen 1, 6 cœurs / 12 threads), hyperviseur partagé
  (WSL2). La machine n'était **pas** au repos : `loadavg` relevé avant chaque
  série retenue, entre 2.0 et 3.6 ; il est monté à 5.0–7.6 pendant les runs
  non retenus (`--quick`, cache froid, test profond), d'autres travaux
  tournant sur l'hôte. Dans ces conditions seuls les **ratios intra-session**
  sont interprétables ; aucune valeur absolue de ce rapport ne doit être
  comparée à une autre session.
- Criterion 0.5.1, deux jambes indépendantes de la même session
  (`--warm-up-time 1 --measurement-time 3`), médiane des deux jambes retenue.
- Harnais de perft : une exécution par cellule (débit), A/B/A/B de 4 jambes
  pour le point de comparaison principal (kiwipete d5, médiane des 4).
- `perf stat -e instructions,cycles,cache-misses,L1-dcache-load-misses`
  (compteurs `:u`, `perf_event_paranoid=2`).
- `motors` épinglé à `fdb697373e03d21e26a254dea0f8613a596ca630`, cloné et
  construit par `tools/compare_movegen.sh` dans
  `${XDG_CACHE_HOME:-$HOME/.cache}/mychess/motors`.
- Chemin froid du harnais exercé pour de vrai :
  `MYCHESS_CACHE=/tmp/task8-cold-cache tools/compare_movegen.sh` clone depuis
  GitHub, construit `motors` (son `.cargo/config.toml` en `target-cpu=native`
  s'applique, le build partant de son propre répertoire) et rend les mêmes
  nœuds sur les trois positions. Ce run a eu lieu sous `loadavg` ~5 : ses
  débits ne sont pas retenus (mychess y descend à 84.5 Mnodes/s), seul le
  chemin clone → build → mesure est validé.

### Étiquettes de build — la comparaison n'est pas iso-technique

| | mychess (`movegen`, `gct`) | motors |
|---|---|---|
| binaire | `mychess` / `gctperft`, scalaire | `Motors-x86_64-avx2` (SIMD AVX2) |
| `-Ctarget-cpu` | défaut (x86-64 générique) | `native` (`.cargo/config.toml` de motors) |
| profil | `lto=true`, `codegen-units=1` | `lto=true`, `debug=1`, `panic=abort` |
| slider | magics (Zen 1 : pas de `PEXT` rapide) | bitboards + Kogge-Stone, pas de table d'attaques |
| parallélisme du perft | mono-thread | **parallèle (rayon) par défaut** ; `go perft N threads 1` pour le forcer mono-thread |

Conséquences à garder en tête pour lire les chiffres :

1. `motors` est compilé pour `native` et contient du SIMD ; nos deux
   générateurs sont scalaires et compilés pour du x86-64 générique. Le ratio
   `movegen`/`motors` mélange donc l'écart d'algorithme **et** l'écart de
   vectorisation/`target-cpu`.
2. Sur ce Zen 1, la détection de `mychess` choisit le chemin **magic**, pas
   `PEXT` (`[slider path: magic (AuthenticAMD family 0x17 + bmi2+popcnt+movbe)]`).
   Les deux générateurs `mychess` utilisent les mêmes tables, donc le ratio
   `gct`/`movegen` reste juste.
3. Le perft de `motors` est **multi-thread par défaut** : à kiwipete d6,
   `perf stat` mesure 3.4 s de temps mur pour 38.4 s de temps CPU, soit ~11
   cœurs actifs. Toutes les comparaisons de débit ci-dessous utilisent
   `threads 1` ; le débit parallèle est donné séparément, pour mémoire.
4. `MoveList::new()` (~15 ns) est inclus dans les deux bras du bench
   `generate` : c'est le coût réel d'un appelant, et il est identique des deux
   côtés.

## Correction des sorties

Trois implémentations, mêmes nœuds partout. `tools/compare_movegen.sh` échoue
si les trois colonnes diffèrent, et si le compte s'écarte de la table Grand
Chess Tree publiée (valeurs attendues codées dans le script).

| position | depth | nœuds (GCT) | mychess | gct | motors |
|---|---|---|---|---|---|
| startpos | 6 | 119 060 324 | 119 060 324 | 119 060 324 | 119 060 324 |
| kiwipete | 5 | 193 690 690 | 193 690 690 | 193 690 690 | 193 690 690 |
| sje | 5 | 164 075 551 | 164 075 551 | 164 075 551 | 164 075 551 |

`loadavg` au lancement de cette série : 2.42 2.84 3.64. Débits (Mnodes/s,
mono-thread, une exécution par cellule) :

| position | depth | nœuds | mychess | gct | motors |
|---|---|---|---|---|---|
| startpos | 6 | 119 060 324 | 199.6 | 101.2 | 253.3 |
| kiwipete | 5 | 193 690 690 | 297.2 | 170.0 | 450.0 |
| sje | 5 | 164 075 551 | 309.4 | 170.5 | 382.6 |

Ratios de ces cellules (gct/movegen, gct/motors, movegen/motors) :
startpos 0.507 / 0.400 / 0.788 ; kiwipete 0.572 / 0.378 / 0.660 ;
sje 0.551 / 0.446 / 0.809.

Passe de confirmation du harnais (`loadavg` 1.70, mêmes binaires) :
startpos 181.2 / 104.6 / 249.0, kiwipete 305.2 / 173.4 / 444.6, sje
314.7 / 158.1 / 372.2 Mnodes/s, soit gct/movegen = 0.577 / 0.568 / 0.502.
Les trois ratios restent dans la même bande 0.50–0.58 : la conclusion ne
dépend pas d'une exécution.

Point de comparaison principal, A/B/A/B sur kiwipete d5 (4 jambes alternées,
mêmes binaires, `loadavg` 2.82 avant la série) :

| jambe | mychess (Mnodes/s) | gct (Mnodes/s) |
|---|---|---|
| 1 | 295.9 | 170.3 |
| 2 | 304.7 | 150.5 |
| 3 | 299.8 | 170.8 |
| 4 | 308.7 | 174.7 |
| **médiane** | **302.25** | **170.55** |

Ratio `gct`/`movegen` = 170.55 / 302.25 = **0.564** (soit `movegen` 1.77×
plus rapide). La dispersion par jambe est de ±3 % pour `movegen`, ±8 % pour
`gct` (150.5–174.7) : l'ordre de grandeur de la variance annoncée (~±20 %)
n'est pas atteint sur ce point, mais les extrêmes de `gct` suffisent à
interdire de lire ces débits comme des constantes.

`motors` parallèle (par défaut, non comparable, pour mémoire) :
startpos d6 1273.2 Mnodes/s, kiwipete d5 2282.5 Mnodes/s, sje d5 2109.4
Mnodes/s — soit 5.0×, 5.1× et 5.5× son propre débit mono-thread, et 7.7×
`movegen` là où les deux sont mesurés (kiwipete d5).

## Coût par nœud (criterion, médianes)

Médiane des deux jambes (jambes brutes ci-dessous).

| bench | gct | movegen | ratio gct/movegen |
|---|---|---|---|
| generate/startpos | 189.85 ns | 110.30 ns | 1.721 |
| generate/kiwipete | 217.01 ns | 197.54 ns | 1.099 |
| generate/pos3 | 117.11 ns | 85.61 ns | 1.368 |
| generate/sje | 209.42 ns | 164.75 ns | 1.271 |
| perft/startpos_d4 | 2.1564 ms | 1.1237 ms | 1.919 |
| perft/kiwipete_d3 | 571.95 µs | 316.24 µs | 1.808 |
| count/startpos (vs `count_moves`) | 187.04 ns | 87.03 ns | 2.149 |
| count/kiwipete (vs `count_moves`) | 210.55 ns | 123.14 ns | 1.710 |
| count/sje (vs `count_moves`) | 209.62 ns | 95.16 ns | 2.203 |
| count/pos3 (vs `count_moves`) | 113.90 ns | 46.44 ns | 2.453 |

Les valeurs `gct` de la colonne `count` viennent des bras `count/gct` de
`benches/gct.rs`, qui étaient **byte-identiques** aux bras `generate/gct` : le
portage n'a pas d'équivalent de `count_moves` (son `generate` construit une
`MoveList` et renvoie l'indice d'append). Ces bras ont donc été supprimés ; les
valeurs restent celles de `generate/gct` mesuré dans le groupe `count`, et
l'asymétrie qu'ils révèlent est celle qui explique le ratio de perft de la
section « Lecture ».

Jambes brutes (point estimate criterion, ns sauf mention) :

| bench | jambe 1 | jambe 2 |
|---|---|---|
| generate/gct/startpos | 192.38 | 187.32 |
| generate/movegen/startpos | 114.09 | 106.51 |
| generate/gct/kiwipete | 221.91 | 212.11 |
| generate/movegen/kiwipete | 200.35 | 194.72 |
| generate/gct/pos3 | 120.28 | 113.94 |
| generate/movegen/pos3 | 89.813 | 81.399 |
| generate/gct/sje | 209.37 | 209.47 |
| generate/movegen/sje | 167.28 | 162.22 |
| count/gct/startpos | 193.74 | 180.34 |
| count/movegen_count_moves/startpos | 91.587 | 82.475 |
| count/gct/kiwipete | 210.67 | 210.42 |
| count/movegen_count_moves/kiwipete | 125.64 | 120.64 |
| count/gct/pos3 | 114.01 | 113.78 |
| count/movegen_count_moves/pos3 | 47.253 | 45.630 |
| count/gct/sje | 211.90 | 207.34 |
| count/movegen_count_moves/sje | 94.008 | 96.306 |
| perft/gct/startpos_d4 | 2.0912 ms | 2.2215 ms |
| perft/movegen/startpos_d4 | 1.1360 ms | 1.1114 ms |
| perft/gct/kiwipete_d3 | 566.84 µs | 577.05 µs |
| perft/movegen/kiwipete_d3 | 319.88 µs | 312.59 µs |

`loadavg` avant les deux jambes : 2.01 et 2.66.

## Compteurs matériels

`perf stat`, kiwipete d6 = 8 031 647 685 nœuds, binaires release, `motors`
avec `threads 1` (`loadavg` 2.63 avant la série). Les divisions par nœud
utilisent ce total pour les trois implémentations (le perft bulk fait
~193.7 M appels à `generate`, mais la convention « par nœud » reste la seule
commune aux trois).

| implémentation | instructions / nœud | cycles / nœud | L1-dcache-load-misses | IPC |
|---|---|---|---|---|
| mychess | 33.08 | 13.01 | 12 670 996 (0.00158 / nœud) | 2.54 |
| gct | 47.30 | 23.25 | 64 719 070 (0.00806 / nœud) | 2.03 |
| motors | 19.95 | 10.80 | 37 038 131 (0.00461 / nœud) | 1.85 |

Bruts : mychess 265 709 600 697 instructions / 104 507 327 409 cycles ;
gct 379 893 660 141 / 186 701 069 320 ; motors 160 209 170 312 / 86 764 892 297.
`motors` parallèle (sans `threads 1`) compte le **même** total d'instructions
(160 208 700 794, à 0.0003 % du run mono-thread) en 3.4 s de temps mur pour
38.4 s de temps CPU : le travail par nœud est identique, le surplus de temps
CPU est de l'attente/spin des threads rayon, et non des instructions utiles.
Seul le run mono-thread est donc utilisable pour les cycles/nœud.

Lecture : `gct` exécute ~43 % d'instructions de plus par nœud que `movegen`
(47.30 vs 33.08) et paie 1.8× les cycles — cohérent avec les trois passes de
légalité séparées. `motors` fait ~40 % d'instructions de moins que `movegen`
(19.95 vs 33.08) : c'est le bulk `danger` (Kogge-Stone) et le bulk counting
des pions, pas la vectorisation seule.

## Lecture

### Ce que le chiffre principal mesure

Le perft n'appelle pas le générateur de la même façon dans les deux
implémentations : `src/perft.rs` compte les coups au bord (`count_moves`, aucune
liste), alors que `src/gct/perft.rs` construit une `MoveList` à **chaque** nœud,
y compris au bord. Le rapport `0.564` compare donc deux **stratégies de
feuille** autant que deux générateurs ; ce n'est pas un rapport de coût de
génération.

Arithmétique de l'écart, avec les seules valeurs publiées ci-dessus. Kiwipete d5
compte 193 690 690 nœuds, dont 4 085 603 nœuds de profondeur 1 — la couche où
les deux perfts génèrent les coups de feuille — et 99 950 nœuds internes :

- couche de feuille côté `movegen` (`count_moves`) : 4 085 603 × 123.14 ns
  ≈ 0.503 s ;
- couche de feuille côté `gct` (`generate` + `MoveList`) :
  4 085 603 × 217.01 ns ≈ 0.887 s ;
- delta de la seule couche de feuille : **≈ 0.384 s**, soit ≈ 78 % de l'écart
  total de temps mesuré entre les deux perfts sur ce point
  (193 690 690 / 170.55 − 193 690 690 / 302.25 = 1.136 s − 0.641 s ≈ 0.495 s).

Le delta des nœuds internes est négligeable (99 950 × (217.01 − 197.54) ≈ 2 ms).
Le reste (≈ 0.11 s) est le surcoût du perft réel par rapport aux microbenchmarks
criterion — `make`/`unmake`, récursion, cache — qui frappe les deux
implémentations dans les mêmes proportions et n'est imputable à aucun des deux
générateurs.

Le ratio **générateur contre générateur** est celui des lignes `generate`, qui
comparent bien le même travail des deux côtés (`MoveList::new()` inclus dans les
deux bras) : **1.10× sur kiwipete** (217.01 / 197.54), **1.72× sur startpos**
(189.85 / 110.30), 1.37× sur pos3 et 1.27× sur sje.

Autrement dit : les trois passes de légalité séparées, la boucle par pièce
ennemie de `king_danger` et le `MoveList` du portage pèsent sur le ratio
générateur-générateur (1.10–1.72× selon la position), **pas** sur le facteur
1.77. Un `gct` plus rapide que `movegen` aurait signalé une erreur de mesure ;
ce n'est pas le cas, mais le `1.77×` ne mesure pas la conception en trois
passes : il mesure un débit de perft, stratégie de feuille incluse.

### L'écart avec `motors`

L'écart avec `motors` est le vrai objectif : `motors` calcule `danger` en bulk
(Kogge-Stone), sans table d'attaques, et c'est la cible de la phase 2.
Mono-thread, `movegen` atteint 66–81 % du débit de `motors` (1.66× plus
d'instructions par nœud, 1.24–1.52× moins de Mnodes/s selon la position). Le
perft parallèle de `motors` (~5× son propre mono-thread) n'est pas une
comparaison de générateurs et n'entre pas dans le verdict.

Contexte qui borne la portée du chiffre : `motors` est en AVX2 +
`target-cpu=native`, nos binaires en x86-64 générique ; une partie de l'écart
de débit `movegen`/`motors` disparaîtrait avec un build `-Ctarget-cpu=native` (le
slider resterait sur le chemin magic, Zen 1 n'ayant pas de `PEXT` rapide).
Aucune mesure de ce build n'a été faite ici : le chiffre « natif vs natif »
reste à établir.

## Verdict

- Exactitude : les trois implémentations donnent les mêmes comptes sur les
  trois positions (119 060 324 / 193 690 690 / 164 075 551), validés contre
  la table Grand Chess Tree (nœuds **et** colonnes détaillées, conversion
  `checks = checks + mates`) par les tâches 1–7.
- Performance, **débit de perft** (kiwipete d5, A/B/A/B médian) : `gct` =
  56.4 % de `movegen`. Ce ratio inclut deux stratégies de feuille différentes
  (`MoveList` au bord contre `count_moves`) : il ne mesure pas le coût du
  générateur.
- Performance, **générateur contre générateur** (`generate`, même travail des
  deux côtés) : `gct` est 1.10× plus lent sur kiwipete et 1.72× sur startpos.
- Performance contre `motors` (harnais, mono-thread, même position) :
  `movegen` = 66.0 %, `gct` = 37.8 %. En criterion par nœud, `gct` va de 1.72×
  (startpos) à 1.92× (perft d4) plus lent que `movegen` ; la ligne `perft`
  inclut là encore la stratégie de feuille, la ligne `generate` non.
- Décision : `src/movegen.rs` reste le générateur de production. `src/gct/`
  devient la base de travail de la phase 2 (bulk `danger`, buffer réutilisable,
  `has_legal_move`).

## Commandes exécutées

```sh
# jambes criterion (2 jambes, médiane des point estimates)
cargo bench --bench gct -- --warm-up-time 1 --measurement-time 3

# harnais (clone/build motors + 3 positions, nodes vérifiés)
cat /proc/loadavg
tools/compare_movegen.sh

# A/B/A/B 4 jambes, kiwipete d5
KIWI='r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1'
for leg in 1 2 3 4; do
  ./target/release/mychess perft "$KIWI" 5
  ./target/release/examples/gctperft "$KIWI" 5
done

# compteurs (kiwipete d6)
perf stat -e instructions,cycles,cache-misses,L1-dcache-load-misses \
  ./target/release/mychess perft "$KIWI" 6
perf stat -e instructions,cycles,cache-misses,L1-dcache-load-misses \
  ./target/release/examples/gctperft "$KIWI" 6
perf stat -e instructions,cycles,cache-misses,L1-dcache-load-misses bash -c \
  'printf "ugi\nposition fen %s\ngo perft 6 threads 1\nquit\n" "$KIWI" |
   ~/.cache/mychess/motors/target/release/motors'

# moteurs parallèle (pour mémoire)
printf 'ugi\nposition fen %s\ngo perft 5\nquit\n' "$KIWI" |
  ~/.cache/mychess/motors/target/release/motors

# chemin froid du cache (clone + build réels, cache alternatif)
MYCHESS_CACHE=/tmp/task8-cold-cache tools/compare_movegen.sh
```

## Vérifications

- `cargo bench --bench gct -- --quick` : les 16 benchmarks s'exécutent
  (20 avant la suppression des huit bras `count/gct` dupliqués).
- `cargo clippy --release --tests --bins --lib -- -D warnings` : propre.
- `cargo clippy --release --benches -- -D warnings` : propre, **aucune**
  alerte dans le `benches/slider.rs` préexistant.
- `cargo test --release` : toutes les suites passent (0 échec).
- `cargo test --release --test gct_perft -- --ignored` : `t2_gct_perft_mid`
  (toutes les lignes GCT d5–d6, compteurs détaillés) passe en 756 s.
- `rustfmt --edition 2024 --check benches/gct.rs` : propre.
- `bash -n tools/compare_movegen.sh` : propre ; `shellcheck` absent de l'hôte.

## Ce qui n'a pas été mesuré, et pourquoi

- **Build `-Ctarget-cpu=native` de `mychess`** : ce n'était pas dans le
  périmètre de la tâche (aucun fichier de build ne devait changer) ; le
  rapport signale l'asymétrie au lieu de la corriger.
- **Débit mono-thread de `motors` sans AVX2** : impossible sans reconstruire
  `motors` avec un autre `target-cpu`, donc avec un binaire différent de
  l'épinglage.
- **`shellcheck`** : absent de l'hôte (`command -v shellcheck` ne renvoie
  rien). Seul `bash -n` a été exécuté.
- **Machine au repos** : `loadavg` 2.0–3.6 sur les séries retenues, jusqu'à
  7.6 sur les runs non retenus (hôte partagé). Les ratios sont
  intra-session ; les valeurs absolues ne sont pas des constantes.
- **Rien d'autre** : tout le reste du périmètre (bench, harnais, compteurs,
  vérifications) a été exécuté ; voir la section Vérifications.
