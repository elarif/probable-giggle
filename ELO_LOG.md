# ELO_LOG.md — journal match/SPRT (futur)

hardware de référence : **AMD EPYC 7763** (Milan, Zen3 family `0x19`, 4C/8SMT,
~2.45 GHz, BMI2, pas d'AVX-512) — hyperviseur partagé : les mesures nps vivent
dans MIGRATION.md avec des fourchettes larges, le CI n'exige JAMAIS de seuil
nps.

> ⚠️ **Hôte de mesure ≠ hôte de référence.** Toutes les valeurs marquées
> « EPYC 7763 » ci-dessous ont été mesurées sur `codespaces-f31cc3`
> (8 cœurs). Les mesures du 2026-10-20 ont été prises sur un **hôte
> différent** (`DESKTOP-2RKN1IA`, AMD Ryzen 5 1600X 6C/12T, WSL2). Elles
> **ne sont pas comparables** aux valeurs EPYC et ne les remplacent pas :
> elles valent comme *ordre de grandeur relatif* (A/B/A/B même session) et
> comme *détection de régression*, pas comme mesure de référence. Toute
> valeur de référence future doit être reprise sur l'hôte EPYC.

Équipes de test prévues (phase MCTS) :
- fastchess SPRT concurrency = **4–6** (4 cœurs physiques : ne pas saturer
  les 8 SMT pour ne pas biaiser les temps de réflexion),
- CPU-only → **Tier D** (pas de GPU dans la chaîne),
- A/B/A/B intercalé même session, jamais cross-jour.

## Entrées

*(aucune partie jouée à ce jour — le cœur n'a ni recherche ni évaluation ;
ce journal accueillera SPRT/fix-ply/Match dès la phase MCTS)*

| date | version | adversaire | conditions | résultat |
|---|---|---|---|---|
| — | — | — | — | — |

## Réconciliation M4-d9 — **tranchée le 2026-10-21** (gate 8/8, exit 0)

Le bench-freeze posé par `test.sh` (refus de SPRT tant que M4 n'est pas
réconcilié) est **LEVABLE** : `slaml/reconcile.sh` sort à **8/8, exit 0**.

| colonne | mychess d9 | référence d9 | verdict |
|---|---|---|---|
| Nodes | 2,439,530,234,167 | idem | **OK exact** |
| Captures | 125,208,536,153 | idem | **OK exact** |
| E.p. | 319,496,827 | idem | **OK exact** |
| Castles | 1,784,356,000 | idem | **OK exact** |
| Promotions | 17,334,376 | idem | **OK exact** |
| Checks | 36,095,901,903 | idem | **OK exact** |
| Mates | 400,191,963 | idem | **OK exact** |
| Discovery Checks | 37,103,750 | 37,101,713 | écart de **convention** (non comparable) |
| Double Checks | 6,315,946 | 5,547,221 | écart de **convention** (reconstruction exacte) |

**Le verdict : ce n'est pas un bug de movegen, c'est une différence de
convention.** mychess classe les **mats** selon la même règle que les checks
(nombre d'attaquants du roi, et attaquant == pièce jouée) ; la table de
référence les ventile en colonnes de mats dédiées. Reconstruction exacte au
nœud près :

```text
mychess double_checks
  = ref direct_disc + ref single_disc_mates + ref double_disc
  =  5 547 221   +          768 715   +           10
  =  6 315 946
```

La colonne `Checks` est comparable et exacte : la référence y inclut les mats
comme mychess (35 695 709 940 + 400 191 963 = 36 095 901 903).

Preuve complète : `docs/ecart_d9_convention.md`. Rapport M4 amendé :
`slaml/d9_checkcols_report_20261014.md` (addendum du 2026-10-21).

**Reproduction de l'écart** : deux runs d9 complets, sur deux hôtes
différents, deux chemins slider (pext sur EPYC, magic sur Ryzen), avec et
sans checkpoint → **mêmes chiffres au nœud près**. L'écart n'est donc ni un
artefact machine, ni de codegen, ni de parallélisme.

Conséquence pour la phase MCTS : les compteurs détaillés sont utilisables
pour les priors A2 (`nodes`, `captures`, `ep`, `castles`, `promotions`
exacts), à la réserve que la **ventilation check/mat reste une convention
interne** à mychess, non comparable à l'extérieur.

> Note d'hygiène : `slaml/reconcile.sh` contient un `cd /workspaces/probable-giggle`
> codé en dur (ancien chemin du codespace) — il **échoue en exit 2** sur
> l'hôte courant. À corriger (chemin relatif à la racine du repo) avant le
> prochain usage.

## Baseline perf core (GATE 2.5, tag `vcore-1.0`, EPYC 7763 — pré-phase MCTS)

Lignes de performance core consignées pour la phase MCTS (benchs criterion
`base25`=pré-M2 → `final25`=frozen ; détail des A/B par lot : MIGRATION.md) :

| bench (release, criterion) | pré-M2 (`base25`) | frozen (`final25`) |
|---|---|---|
| count_moves/startpos | 78.7 ns | 70.7 ns |
| count_moves/kiwipete | 103.1 ns | 90.7 ns |
| count_moves/pos3 | 47.9 ns | 40.2 ns |
| count_moves/pos6 | 85.3 ns | 74.8 ns |
| perft1t/startpos_d6 | 544.3 ms (219 Mnps) | 463.2 ms (257 Mnps, ×1.17) |
| perft_bulk/kiwipete_d2_2039 | 6.25 µs | 5.55 µs |
| perft_bulk/kiwipete_d3_97862 | 249.0 µs | 247.3 µs |
| make_unmake/{startpos..pos6} | 22.5–24.5 ns | 22.9–24.2 ns (inchangé) |
| gives_check_x8 (÷8/appel) | ~24.6 ns | ~25.0 ns (<80 contrat ✓) |
| mcts_descend30 (÷30/ply, hash inclus) | 21.3 ns | 22.2 ns (<25 contrat ✓) |
| mcts_rep_walkback16 zero/hit4 | 8.28 / 1.32 ns | 8.57 / 1.34 ns |
| mcts_pst_eval/startpos | 32.7 ns | 39.2 ns |
| perft(7) 1T sweep | ~206–210 Mnps | **272 Mnps (×1.32)** |
| perft(7) 8T sweep | 949–984 Mnps | **1089–1113 Mnps (×~1.13)** |

Lots GATE 2.5 M2 : **L1b no-gain** (+0.1 % d6 startpos, conservé
robustesse), **L1c GAIN ×1.19** (fusion checkers/pins/danger, adopté),
**L1d no-gain** (−1.8 % perft1t vs L1c, conservé : count_moves −5…−16 %
p<0.05). M1 : les deux micro-benchs « incohérents » étaient des artefacts de
batch (aucun fix d'impl nécessaire). M3 : contrat position illégale
(non-émission du roi-capture, assert debug, test dédié).
# Journal Elo — phase MCTS (ladder : un commit = une amélioration = un SPRT)

## Baseline par bench (dénominateur du budget sims/s — consigné en tête)

`mcts_descend30/startpos` (criterion, baseline-A, session final25, pré-v1) :
**638.9→667.3 ns / 30 plies ≈ 21.3–22.2 ns/ply** (make+unmake round-trip,
hash incrémental inclus — sous le contrat core <25 ns/ply). C'est le
DÉNOMINATEUR du budget sims/s : un playout MCTS ajoutera make/unmake par ply
de desc/asc (~22 ns/ply round-trip) + sélection + priors/eval à la feuille.
⚠️ Valeur à RELEVER et reconsigner après réconciliation M4-d9 (bench-freeze :
aucune mesure tant que la ligne d9 n'est pas réconciliée contre la table
CPW) — mention "post-M4" obligatoire sur toute mesure de la phase.

`mcts_rep_walkback16` : 8.6 ns miss / 1.3 ns hit-at-4 → la triple répétition
via hash-history du stack de recherche est quasi-gratuite : intégrée dès la
v1 (ne pas différer).
`mcts_pst_eval/startpos` : 32.7–39.9 ns → le pattern eval A1 est bon marché
vs playout aléatoire (~40-50 µs).

## Re-mesure post-M4 (2026-10-20) — hôte NON-EPYC, valeurs relatives

**Avertissement de comparabilité** : la session de mesures du 2026-10-20 a
tourné sur `DESKTOP-2RKN1IA` (Ryzen 5 1600X, 6C/12T, WSL2), **pas** sur
l'EPYC 7763 de référence. Ces nombres **ne remplacent pas** la baseline
`final25` ci-dessus. Ils répondent à une seule question : *le passage
`4fa6995` → `main` (idiomatic pass + toolchain 1.97) a-t-il dégradé les
chemins chauds MCTS ?* — répondue par A/B/A/B **intercalé, même session**,
deux builds côte à côte.

| bench | A = `4fa6995` (pré-idiomatic) | B = `main` (1169620) | écart B/A |
|---|---|---|---|
| `mcts_descend30/startpos` | 749.4 / 746.5 ns (2 legs) | 709.1 / 703.8 ns | **×0.95 (gain −5.5 %)** |
| `mcts_rep_walkback16/zero_miss` | 7.73 / 7.90 ns | 7.63 / 7.85 ns | ~×0.98 (bruit) |
| `mcts_rep_walkback16/hit_at4` | 1.252 / 1.223 ns | 1.211 / 1.206 ns | ~×0.98 (bruit) |
| `mcts_pst_eval/startpos` | 47.4 / 45.2 ns | 53.8 / 52.7 ns | **×1.15 (écart +15 %, codegen — voir plus bas)** |

### La régression `mcts_pst_eval` n'est pas une régression de l'algorithme

Le corps du bench est **byte-identique** entre les deux commits, et
`src/eval.rs` n'y est même pas appelé (le bench lit `pos.mailbox` et
applique une table de substitution locale). La suspicion initiale — une
régression de la boucle d'éval feuille — ne tient pas. Découpage
effectué :

1. **Réversion du layout de `Position`** (`zobrist`/`halfmove` replacés en
   queue) → **pas de changement** (53.7 ns). **Le layout n'est donc pas en
   cause** (hypothèse initialement envisagée : infirmée).
2. **Réversion de `src/eval.rs`** seule → **pas de changement** (53.8 ns).
3. **Réversion de `src/` + `benches/` complètes** → retour à **45.7 ns**.
   L'écart est donc réel et reproductible, mais il vient de l'ensemble
   `src/` — i.e. de la **compilation**, pas d'une ligne de code.
4. **Sonde à itérations fixes** (`examples/pst_fixed.rs`, 20 M itérations,
   boucle identique, binaire séparé) → POST **9 820 281 887** instructions,
   PRE **9 820 277 249** : **identiques à 5 ppm**. L'algorithme de la
   boucle n'a pas changé d'une instruction. Le coût supplémentaire est
   front-end / layout / branch-prediction, **pas** du travail supplémentaire.
5. **Normalisation sur le binaire criterion lui-même** (instructions
   divisées par le nombre d'itérations, 10 samples) : POST **757,8**
   insn/iter vs PRE **728,0** (+4,09 %) et POST **374,8** cycles/iter vs
   PRE **331,0** (+13,2 %).

Verdict : **pas de régression fonctionnelle** — le compte d'instructions de
la boucle est identique, et l'écart n'est **ni le layout de `Position`, ni
`src/eval.rs`** (toutes deux réversions testées sans effet). Il vient du
**codegen LTO** de l'ensemble `src/` (décalage d'inlining/d'alignement
induit par le idiomatic pass + le toolchain 1.97) : le harnais criterion
mesure un binaire dont l'inlining diffère, la sonde isolée non.

Conséquence pratique : le dénominateur du budget sims/s (A1) repose sur
cette boucle, et elle est **sensible au codegen** — c'est le genre d'écart
qui déplace le rapport sims/s sans changer une seule ligne de logique. À
surveiller à chaque changement de `src/` ; à trancher avec la calibration
S8 (qui a besoin d'une distribution de |ΔQ| mesurée, donc d'un buffer
réel — cf. plan d'instrumentation, tasks 2–7).

> Ces mesures restent **hors hôte de référence** : la valeur `final25`
> (EPYC) reste la référence du dénominateur. Reprendre la re-mesure sur
> l'EPYC avant de figer le budget sims/s.

## Table Elo (source : Rustic/Sirius — une ligne par amélioration testée)

Règles : SPRT elo0=0 elo1=5 alpha=0.05 beta=0.05 ; contrôle par NŒUDS
(2k/10k/40k) puis TC 8+0.08 si passe ; openings UHO_Lichess_4852 (chaque
position deux fois, random). Mises en garde anti-biais self-play (~60 %
stickie Rustic — à documenter par mesure ; contrôle tiers = gauntlet).

| date | commit | description | TC | Elo ± err | jeux | verdict |
|---|---|---|---|---|---|---|
| 2026-10-13 | vmcts-v1 | v1: baseline MCTS UCT/playouts aléatoires; terminalité (mat/pat/50coups/triple rép via hash-history + matériel insuffisant); PRIOR hook prêt (A2), eval hook prêt (A1) | — | **— (baseline, mesure du saut A1 s'y réfère)** | — | baseline |

## Table Elo — suite (mises à jour par entrée, mesures gelées jusqu'à M4)

| date | commit | description | TC | Elo ± err | jeux | verdict |
|---|---|---|---|---|---|---|
| 2026-10-13 (post-M4: PENDING) | A1-uncommitted→ see commit | A1: playouts aléatoires remplacés par eval statique aux feuilles (eval_normalized = eval_for_mover/1000 clamp[-1,1]; terminalité exacte {-1,0,1} conservée AVANT l'eval; hooks PUCT/NN inchangés) | nœuds-fixes 10k (planifié) | PENDING | PENDING | **BLOQUÉ** — réconciliation M4 à 7/9 (voir plus haut) + fastchess absent de la machine |
| 2026-10-20 | 8f52f16 (A1) | A1 commitée sur `main` ; le « see commit » de la ligne ci-dessus est résolu : `8f52f16` | — | — | — | inchangé (mesure A1 toujours bloquée) |
| 2026-10-22 | phase 4 (A2) | A2 : sélection **PUCT** (`Q + c_puct·P·sqrt(N)/(1+n)`) + prior MVV-LVA (`mcts::policy`, stocké dans `Edge::prior`) ; la largeur reste le tirage uniforme v1, la profondeur change ; la règle UCT est **conservée et figée** (`Policy::Uct`) ; options UCI `Policy`/`Cpuct`/`RootNoise` | nœuds-fixes 2k/10k/40k (**H1 aux trois**) + TC 8+0.08 (**H1**) | **+399,7 ± 50,9 (2k)** ; **+596,5 ± 90,9 (10k)** ; **+450,4 ± 55,2 (40k)** ; **+303,3 ± 36,4 (TC 8+0.08)** | 324 / 304 / 316 / 350 | **ACCEPTÉ (H1 aux trois budgets nœuds-fixes et au contrôle temps-fixe)** — voir la section « Échelle Elo — phase 4 » ; écarts au protocole : book ECO lichess (3 863 positions) au lieu de l'UHO, hôte non EPYC |
| 2026-10-22 | phase 5 (A5) | A5 : **feuille tactiquement résolue** — `mcts::qsearch::qvalue` (quiescence-lite captures-seulement, stand-pat, terminalité prioritaire), `Limits.leaf_q`, option UCI `LeafQ` (défaut 0 = phase 4) ; **plus** le correctif du budget temps (plancher de 50 ms supprimé, budget borné par le temps restant − 5 ms) | nœuds-fixes 2k/10k/40k contre la **phase 4 gelée** (`0abdff2`) | **−177,2 ± 26,4 (q1, 2k)** ; **−118,1 ± 23,3 (q2, 2k)** ; **−240,8 ± 26,7 (q1, 10k)** ; **−154,5 ± 24,9 (q2, 10k)** | 400 / 400 / 370 / 400 | **REVERTIE (H0)** — la quiescence-lite est **négative** (−118 à −241 Elo) : le `max` de la quiescence est optimiste et double compte les échanges que l'arbre explorera ; `LeafQ` reste à **0**, le module est conservé comme expérience documentée ; le **correctif du budget temps est conservé** (0 partie adjugée sur dépassement en TC 1+0.01) |
| 2026-10-22 | `f44abf5` | **niveau absolu** : calibration contre Stockfish 19 en force limitée (`UCI_LimitStrength` + `UCI_Elo`), budget **40 000 nœuds/move identique pour les deux moteurs**, book ECO lichess, 100 à 200 parties par niveau, 9 niveaux | nœuds-fixes 40k (les deux moteurs) | **≈ 1417 ± 19** (échelle `UCI_Elo` de Stockfish) | 900 (100 à 200 × 9 niveaux) | **mesuré** — méthode et preuves : `docs/elo/`, `docs/elo_estimation.md` ; l'estimation est **relative au book de remplacement** et au protocole (40k nœuds/move) ; le ± 19 est statistique, il ne couvre pas le biais de calibration de l'échelle `UCI_Elo` |
| 2026-10-22 | `ce3650e` + ce commit | **niveau absolu, refait** : tournoi **round-robin de 11 moteurs** (`mychess` + **Reckless v0.1.0** + l'échelle Stockfish 1320 → 3190), 660 parties, 12 par paire, budget **temps** `movetime 200 ms/coup` — Reckless n'implémente pas `go nodes`, c'est le seul budget commun ; 3 paires de contrôle | `movetime 200 ms` (les 11 moteurs) | **mychess ≈ 1296** ; **Reckless ≈ 2249** (échelle `UCI_Elo` de Stockfish, interpolation locale entre les ancrages encadrants) | 660 (12 par paire) | **mesuré** — sous un budget **de temps**, l'échelle `UCI_Elo` est **monotone** (0 inversion significative sur 15 paires) alors qu'elle ne l'est pas sous un budget de nœuds : le « 1417 » reste **annulé** (autre protocole, ancre invalide) ; **deux défauts trouvés** : `0000` sur une position jouable (**corrigé**) et dépassement du budget temps (7/660, **signalé, non corrigé** — le corriger changerait l'échelle gelée) ; détails : `docs/elo/README.md` |
| 2026-10-22 | ce commit | **phase AB — l'alpha-beta de référence** : negamax + table de transposition + profondeur itérative + PVS/killers/historique dans `src/ab/` (le core reste gelé), option UCI `Search=alpha` (le défaut reste `mcts`) ; **duel contrôlé** AB vs MCTS (même binaire, même évaluation, même temps) | `movetime 200 ms` (les deux) | **l'alpha-beta perd** : 0 victoire, 18 défaites, 60 nulles sur 80 parties ⇒ **−80 Elo** pour l'AB ; et la **valeur** est le goulot : erreur de l'évaluation **≈ 490 cp**, instabilité de la recherche **104 cp / 2 plies** | 80 (40 + 40) | **mesuré** — à évaluation identique, le MCTS l'emporte parce qu'il **amortit** le bruit de l'évaluation (moyenne des feuilles) alors que l'alpha-beta l'**amplifie** (le minimax prend le maximum) ; l'affirmation « il faut une valeur apprise » est **confirmée mais déplacée** : le goulot est la fonction de valeur, pour les deux algorithmes ; détails : `docs/ab/phase1-2026-10-22.md` |

## Échelle Elo — phase 5 (feuille tactiquement résolue), 2026-10-22

* **candidat** : `main` phase 5, option UCI `LeafQ = d` pilotée par
  `fastchess -engine option.LeafQ=<d>` ;
* **référence** : **phase 4 gelée** (worktree `.worktrees/phase4-ref`,
  commit `0abdff2`) — c'est la même échelle que la phase 4, donc les
  Elo sont **comparables** à ceux de la ligne A2 ;
* protocole : identique à la phase 4 (SPRT `elo0=0 elo1=5 α=β=0.05`,
  book `eco_lichess.epd` de remplacement, 200 rondes × 2 parties,
  concurrence 5).

| configuration | parties | V-N-D | score | Elo ± err | LLR | verdict |
|---|---|---|---|---|---|---|
| **contrôle** `LeafQ=0` contre phase 4 (2k) | 400 | 120-120-160 | 50,00 % | 0,0 ± 0,0 | −0,08 | neutre |
| `LeafQ=1` (2 000 nœuds) | 400 | 18-206-176 | 26,50 % | **−177,2 ± 26,4** | −2,83 | **H0** |
| `LeafQ=2` (2 000 nœuds) | 400 | 39-170-191 | 33,62 % | **−118,1 ± 23,3** | −2,33 | **H0** |
| `LeafQ=1` (10 000 nœuds) | 370 | 1-223-146 | 20,00 % | **−240,8 ± 26,7** | **−2,95** | **H0 accepté** |
| `LeafQ=2` (10 000 nœuds) | 400 | 27-194-179 | 29,12 % | **−154,5 ± 24,9** | −2,67 | **H0** |
| `LeafQ=1` (40 000 nœuds) | 398 | 5-210-183 | 24,25 % | **−197,9 ± 24,3** | **−2,95** | **H0 accepté** |
| `LeafQ=1` (TC 8+0.08) | 360 | 1-229-130 | 18,33 % | **−259,5 ± 31,0** | **−2,95** | **H0 accepté** |

**Verdict : la quiescence-lite de la feuille est REVERTIE.** Le contrôle
`LeafQ=0` contre phase 4 est **exactement neutre** (50,00 %, LLR −0,08) :
le harnais n'est pas biaisé, le chemin par défaut est bien bit-à-bit, et
tout l'écart est imputable à la seule résolution tactique de la feuille.

Le mécanisme (mesuré, cf. `docs/mcts/phase5-2026-10-22.md` §4) : le
`max` de la quiescence est **optimiste** (en profondeur 1 la capture est
supposée tenir, la réplique n'est pas vue) et il **double compte** les
échanges que l'arbre explorera lui-même. La valeur de feuille est donc
gonflée de **+354 cp** en moyenne pour le trait à la feuille (`q1`), et
cette inflation positionnelle fausse les Q. La régression est **monotone
en l'inflation** :

| profondeur | inflation moyenne | Elo (2 000 nœuds) | Elo (10 000 nœuds) |
|---|---|---|---|
| `q1` | +354 cp | −177,2 | −240,8 |
| `q2` | +196 cp | −118,1 | −154,5 |

Une feuille de MCTS veut une **espérance**, pas un `max` adversarial :
c'est le cahier des charges de la variante non biaisée (moyenne sur les
captures), qui est la suite naturelle sur ce sujet. Le module
`mcts::qsearch`, ses tests et l'option `LeafQ` (défaut `0`) sont
**conservés** comme expérience documentée et_negative.

Le verdict est **robuste au protocole** : le contrôle **nœuds-fixes** et
le contrôle **temps-fixe** (TC 8+0.08) disent la même chose, avec le
même signe. En temps fixe, la régression est **plus forte** (−259 Elo) :
la quiescence y coûte 27 à 64 % de débit (§2.2 du rapport), donc
autant de nœuds en moins à budget de temps égal.

### Phase 5 — le correctif du budget temps (conservé)

La phase 4 a laissé un **défaut réel** : `parse_go` imposait un
**plancher de 50 ms**, donc le moteur réfléchissait plus longtemps que
son horloge (158/200 parties adjugées sur dépassement en TC 1+0.01). La
phase 5 le corrige : le budget est **borné par le temps restant** moins
une marge de 5 ms, et quand la formule TC ne donne rien (moins de 30 ms
restants, pas d'incrément) le moteur consomme le temps restant au lieu
de 1 ms. Mesure : `go wtime 20` → budget interne 15 ms, réponse à
**16,3 ms** au pire ; le contrôle TC 1+0.01 ne fait **plus aucune**
partie adjugée sur dépassement du côté phase 5. Ce correctif est
**conservé** quelle que soit la décision sur la quiescence.

## Échelle Elo — phase 4 (A2), 2026-10-22
Premier match joué par ce dépôt : `fastchess` a été **compilé depuis les
sources** (`Disservin/fastchess`, commit `ebfd64cb7fac80d4822c50b07dca4fe934779d21`,
installé dans `~/.local/bin`).

* **candidat** : `main` phase 4 — PUCT par défaut ;
* **référence** : phase 3 gelée (`719f983`) — UCT ;
* protocole : SPRT `elo0=0 elo1=5 alpha=0.05 beta=0.05`, `-each proto=uci
  nodes=N`, 200 rondes × 2 parties, concurrence 5, `-resign movecount=3
  score=600`, `-draw movenumber=40 movecount=8 score=10`, ouvertures en
  ordre aléatoire, book identique pour les deux moteurs.

| budget | parties | V-N-D | score | Elo ± err | LLR | verdict |
|---|---|---|---|---|---|---|
| 2 000 nœuds | 324 | 269-4-51 | 90,90 % | **+399,7 ± 50,9** | 2,95 (100,2 %) | **H1 accepté** |
| 10 000 nœuds | 304 | 287-2-15 | 96,88 % | **+596,5 ± 90,9** | 2,96 (100,7 %) | **H1 accepté** |
| 40 000 nœuds | 316 | 274-2-40 | 93,04 % | **+450,4 ± 55,2** | 2,95 (100,2 %) | **H1 accepté** |

**Symétrie** : orientation inverse (candidat = phase 3) → Elo −404,9 ±
49,1, LLR −2,95, H0 accepté. L'écart ne vient pas du sens de
l'appariement.

**Contrôles** (sans eux le tableau ne vaut rien) :

| contrôle | parties | V-N-D | score | LLR |
|---|---|---|---|---|
| phase 3 **contre** phase 3 (même binaire) | 80 | 23-23-34 | 50,00 % | −0,01 |
| phase 4 forcé `Policy=uct` **contre** phase 3 | 400 | 98-98-204 | 50,00 % | −0,08 |

Le second contrôle est le plus important : il prouve que **les deux
binaires sont de force identique à règle de sélection égale** — l'écart
de la phase 4 est imputable à la seule règle PUCT.

### Écarts au protocole gelé (à citer avec les chiffres)

1. **Livre d'ouvertures** : `uho_lichess_4852_v1.epd` n'était pas
   téléchargeable depuis ce poste. Book de remplacement : jeu ECO lichess
   (`lichess-org/chess-openings`), rejoué en EPD par
   `tools/gen_epd_openings.py` → **3 863 positions** d'ouvertures
   standard. Book **différent** de l'UHO (positions volontairement
   déséquilibrées) : les Elo sont **relatifs** à ce book.
2. **Hôte** : machine de mesure ≠ EPYC 7763 de référence. Valeurs
   relatives, pas une référence absolue.
3. **Contrôle temps-fixe** : le protocole prévoit TC 8+0.08 après un
   passage en nœuds-fixes réussi. Un essai preliminary en **TC 1+0.01**
   s'est révélé **invalide** (158/200 parties adjugées sur dépassement de
   temps de 1 à 40 ms) : le budget de réflexion du moteur a un
   **plancher de 50 ms** (`parse_go` dans `src/uci.rs`), donc le moteur
   pense plus longtemps que son temps restant et se fait flaguer. C'est
   un **défaut réel** de gestion du temps (à corriger hors échelle), pas
   un artefact du harnais. En **TC 8+0.08** l'horloge se stabilise
   (~810 ms de point fixe) et plus aucune partie ne se termine sur un
   dépassement.

| contrôle temps-fixe | parties | V-N-D | score | Elo ± err | LLR | verdict |
|---|---|---|---|---|---|---|
| TC 8+0.08, 200 parties | 200 | 132-4-64 | 82,00 % | +263,4 ± 43,7 | 1,61 (54,6 %) | inconclusive |
| TC 8+0.08, 400 rondes | 350 | 256-10-84 | 85,14 % | **+303,3 ± 36,4** | **2,95 (100,2 %)** | **H1 accepté** |

Le run de 200 parties était **inconcluant** : en mode temps le taux de
nulle monte à 32 % (64/200) contre 5 à 16 % en nœuds-fixes — le budget
de ~80 ms/coup rend les deux règles plus proches, le signal est dilué
et 200 parties ne suffisaient pas à franchir la borne. Le run **prolongé
(400 rondes)** franchit la borne : le SPRT s'arrête à **H1 accepté** à
350 parties. Le contrôle temps-fixe **confirme** donc le verdict de
l'échelle nœuds-fixes, avec une ampleur un peu plus faible (+303 contre
+400 à +600), comme attendu : à ~80 k nœuds par coup, les deux règles
sont plus proches qu'à 2 000–40 000 nœuds.

Rapport complet : `docs/mcts/phase4-2026-10-22.md` (§3).

Notes phase 4 (A2) : le gain mesuré est en **convergence en valeur** (erreur
de score ÷ 4 à 7 contre une recherche de référence à 500–2 000 nœuds) et en
**robustesse tactique en parallèle** (17/22 contre 10/22 à 6 threads et
100 nœuds) ; le débit coûte −10 % sur kiwipete mais la mémoire et l'arbre
divisent par ~1,6. **L'ablation montre que le prior MVV-LVA n'apporte rien
de mesurable** : le gain vient de la forme de décroissance du terme
d'exploration. Le prior est livré comme crochet du pipeline
« logits → softmax → prior » que la policy apprise de D1 branchera.
Rapport : `docs/mcts/phase4-2026-10-22.md`.

**Verdict A2 : ACCEPTÉ** sur l'échelle Elo (H1 aux trois budgets
nœuds-fixes **et** au contrôle temps-fixe TC 8+0.08, voir plus haut).
Les proxies des §2.3–2.4 de la phase 4 allaient dans le même sens que
l'Elo : c'est le premier cas où les deux familles de mesures concordent
dans ce dépôt.

Notes A1 : la valeur feuille = éval statique = le saut attendu le plus gros
(Ramanujan 2010 shallow traps ; Monte-Carlo Chess 2012 ; lc0 par réseau).
Budget sims/s : le playout aléatoire (~76 µs) disparaît ; une feuille =
make/unmake de descente (~22 ns/ply) + eval (~40-90 ns/position) → le
dénominateur attendu de sims/s augmente d'un ordre de grandeur ; valeur
exacte mesurée post-M4 (baseline-A criterion revalidée, mention post-M4).

## Tournoi round-robin (11 moteurs) — 2026-10-22, budget en temps

* **champ** : `mychess`, `mychess_ctrl`, **`reckless` (v0.1.0)**,
  `reckless_ctrl`, `sf1320`, `sf1425`, `sf1500`, `sf1600`, `sf1900`,
  `sf1900_ctrl`, `sf3190` — 11 moteurs, 55 paires, **12 parties par
  paire** (660 parties), 46 min 20 s, `-concurrency 5` ;
* **budget** : `movetime 200 ms` par coup, identique pour tous — Reckless
  v0.1.0 n'implémente **pas** `go nodes` (son parseur UCI n'accepte que
  `depth`, `movetime` et `wtime/btime`), c'est le seul budget commun ;
* **livrable** : `tools/tournoi_roundrobin.sh` (lanceur),
  `tools/elo_roundrobin.py` (analyse), journal versionné
  `docs/elo/roundrobin-movetime-200ms-reckless.log` ;
* **contrôles** (même binaire / même réglage ⇒ 50 %) : `mychess` 50,0 %,
  `reckless` 45,8 %, `sf1900` 37,5 % — **l'environnement de mesure est
  sain** ; résidu maximal du modèle de Bradley-Terry : 90 Elo sur 55
  paires (classement **cohérent**) ;
* **résultat** : `sf3190` 100 % · **`reckless` 76,2 %** · `sf1900` 58,0 %
  · `sf1600` 52,1 % · `sf1500` 43,3 % · `sf1425` 26,9 % · `sf1320` 17,4 %
  · **`mychess` 15,8 %** — et, sur l'échelle `UCI_Elo` de Stockfish
  (interpolation locale) : **Reckless ≈ 2249**, **mychess ≈ 1296** ;
* **ce que ça change** : sous un budget **de temps**, l'échelle
  `UCI_Elo` **est** une échelle de force (0 inversion significative sur
  15 paires), alors qu'elle ne l'est pas sous un budget de nœuds. Le
  « 1417 » reste **annulé** : il mesure une autre position, dans un
  protocole dont l'ancre était invalide ;
* **deux défauts trouvés par le tournoi** :
  1. **`0000` sur une position jouable** — le moteur a dépassé son
     budget, `fastchess` lui a envoyé `stop`, et la recherche
     interrompue (racine non développée) a répondu `0000` : un coup
     **illégal**, qui a fait perdre la partie 332. **Corrigé**
     (`uci::fallback_move` + deux tests de non-régression) ;
  2. **dépassement du budget temps** — 7 parties sur 660 impliquent
     `mychess` (6 perdues sur le temps, débordement de 204 à 356 ms, et
     la partie 332 sur le `0000`) : l'échéance n'est testée
     que tous les 64 playouts. **Signalé, non corrigé** : le corriger
     changerait le comportement de la recherche, donc l'échelle gelée
     des phases 4 et 5 — c'est une décision de phase, pas un bugfix.

## Phase AB — l'alpha-beta de référence (2026-10-22)

* **but** : l'affirmation « le MCTS est plafonné par sa valeur de feuille,
  seul un réseau de neurones le débloque » n'était **pas mesurable** dans
  ce dépôt (toutes les mesures passaient par Stockfish, à 950 Elo de
  distance). La phase construit l'**opposant local** qui manquait ;
* **implémentation** : `src/ab/` — negamax fail-soft, quiescence, nulles
  (50 coups + répétition), mat/pat, élagage par distance au mat, table
  de transposition mappée (2^19 entrées, clé complète), profondeur
  itérative (une itération interrompue est **jetée**), PVS, killer moves,
  heuristique d'historique, table PV ; option UCI `Search` (`mcts` par
  défaut : les mesures gelées des phases 3–5 sont intactes) ;
* **protocole du duel** : **contrôlé** — même binaire, même évaluation
  (`eval::eval_for_mover`), même budget (`movetime 200 ms`), book ECO
  lichess, graine 20261022, aucune adjudication ;
* **résultat** : 80 parties, **0 victoire de l'alpha-beta**, 18
  défaites, 60 nulles ⇒ **−80 Elo** pour l'AB (−93,6 ± 50,4 avant
  PVS/killers/historique ; −70,4 ± 46,6 après) ;
* **le diagnostic** (`examples/eval_error`, 200 positions du livre) :
  l'erreur de l'évaluation est de **≈ 490 cp** (≈ 5 pions) et
  l'instabilité de la recherche de **104 cp par deux plies** — l'alpha-
  beta prend le **maximum** sur un arbre, il **amplifie** le bruit ;
  le MCTS **moyenne** ses feuilles, il l'**amortit** ;
* **ce que ça change** : le goulot d'étranglement n'est pas l'algorithme
  mais la **fonction de valeur**, et elle gêne les deux différemment. La
  phase NN (valeur apprise) passe **devant** l'optimisation de la
  recherche : creuser dans un arbre bruité est un gain de profondeur,
  pas de force ;
* **rapport** : `docs/ab/phase1-2026-10-22.md` — plan :
  `docs/ab/plan-alpha-beta-2026-10-22.md`.

## Phase AB8 — les heuristiques de l'alpha-beta, mesurées à la métrique du bruit (2026-10-23)

* **but** : le plan AB4 demandait « plus de débit **et** plus de score » ;
  le plan AB7 le reposait : « une heuristique qui augmente la profondeur
  *et* le bruit n'est pas un gain ». La phase décompose le lot
  PVS + killers + historique et ajoute null-move, razoring, futilité et
  LMR, chacune **activable seule** (options UCI `Ab*`) ;
* **instrument** : `examples/heuristiques_ab` (débit, profondeur, score,
  facteur d'amplification d'AB7) + `tools/duel_ab.sh` (duel AB-vs-AB,
  une seule option qui change) ; 20 positions du livre ECO, `movetime
  200 ms`, graine de bruit `0x4e4f495345000001` ;
* **deux défauts trouvés par la décomposition** :
  1. l'**heuristique d'historique était du code mort** (bonus
     `/1024` pour un incrément de `profondeur²` : le bonus valait
     toujours zéro) — « sans historique » et « base » donnaient les
     mêmes chiffres à la nœud près ;
  2. la **table de transposition stockait des bornes comme des valeurs
     exactes** (fenêtre nulle, quiescence en échec) — ce qui falsifie
     les transpositions et rend toute mesure « à arbre identique »
     invalide. Les deux sont **corrigés** ; le chemin par défaut change,
     donc le duel de la phase AB n'est plus reproductible à l'identique ;
* **mesures** (20 positions) : PVS **−64 % de nœuds**, killers
  **−37 %**, historique **−4 %** (0 % avant correctif), null-move
  **−27 %** de nœuds et **+0,8 plie** à 200 ms, razoring et futilité
  **sans effet** (le garde-fou « capture » les rend inoffensives), LMR
  **+16 % de nœuds** à profondeur fixée ;
* **le résultat qui compte** : le bruit **casse l'élagage**. À 200 ms,
  la profondeur passe de **4,0–4,4** (sans bruit) à **2,5–2,9** (bruit
  490 cp) ; sur une position du livre, une recherche à profondeur 3
  coûte **1 159 093 540 nœuds** avec du bruit contre ≈ 10 000 sans
  (facteur **100 000×**) ;
* **les deux duels AB-vs-AB** (phase AB8b, joués le 2026-10-23 à
  07:00) : null-move=1 contre null-move=0 → 17 V – 22 D – 1 N
  (43,75 %) ⇒ **−43,7 ± 100,3 Elo** (LOS 18,7 %) ; LMR=1 contre
  LMR=0 → 21 V – 17 D – 2 N (55,00 %) ⇒ **+34,9 ± 95,3 Elo** (LOS
  77,0 %). **Les deux sont non concluants** : à 40 parties la
  précision réelle est **± 100 Elo**, pas les ± 45 Elo qu'un calcul
  « 50 % de nulles » ferait croire. Le null-move reste un **gain de
  débit** (−27 % de nœuds), pas un gain de force démontré ; le LMR
  reste **reverti** (aucun gain de force, +16 % de nœuds) ;
* **un défaut d'instrument corrigé en passant** : `tools/duel_ab.sh`
  passait `--time 200` à `fastchess -each st=`, or fastchess lit
  **`st=` en secondes** (« st=N — Movetime in seconds ») : le budget
  était **1000× trop long** (200 s/coup au lieu de 200 ms) et le
  journal annonçait 200 ms. Le premier duel (03:00) a été **tué** à
  06:54 avec 4/40 parties ; son journal est conservé marqué
  **INVALIDE** (`prt/rr/ab-nullmove.INVALIDE-st200s.log`). Le script
  convertit désormais les ms en secondes et journalise la conversion ;
* **décision** : le goulot est **la fonction de valeur**, et cette fois
  c'est *mécanique* : sans valeur fiable, l'alpha-beta n'élague plus et
  ne peut donc pas approfondir. La porte du plan AB tranche vers
  l'issue **C** ; la **phase NN** (valeur apprise) est confirmée comme
  la seule voie, pour les deux algorithmes ;
* **rapport** : `docs/ab/phase8-2026-10-23.md` — plan :
  `docs/ab/plan-heuristiques-2026-10-23.md`.

## Instruments de mesure (état au 2026-10-22)

Le journal est **alimenté** : les maillons manquants ont été fournis.

| instrument | rôle | état |
|---|---|---|
| `test.sh` (SPRT `elo0=0 elo1=5 alpha=0.05 beta=0.05`) | verdict **binaire** « le candidat est-il meilleur ? » | **gelé** (protocole figé, ne pas modifier) — **utilisé le 2026-10-22** pour l'échelle de la phase 4 |
| `fastchess` | moteur de match | **compilé depuis les sources** le 2026-10-22 (`Disservin/fastchess`, `ebfd64c`), installé dans `~/.local/bin` |
| `tools/gen_epd_openings.py` (nouveau) | book EPD de remplacement quand l'UHO n'est pas téléchargeable | **présent** ; produit 3 863 positions à partir du jeu ECO lichess |
| `tools/pr_sprt.py` (PR-SPRT) | **magnitude** Elo ± IC à partir du même log fastchess | **codé, 54 tests OK**, branche `feature/selfplay-instrumentation` (450a6ef) — non poussé ; **validé sur les 5 logs réels de la phase 4** (mêmes chiffres que fastchess) |
| `slaml/sims_abab.sh` | sims/s A/B/A/B (v1 vs A1) | script présent, **jamais exécuté** (pas de log `slaml/sims_abab_*.log`) |

### PR-SPRT — implémenté, mais un défaut ouvert et une validation manquante

* **Défaut ouvert (Important)** : sur un log dont `alpha`/`beta` sont
  inversés (ex. `alpha=0.90 beta=0.95`), `sprt_bounds()` clamp les valeurs
  sans vérifier l'ordre des bornes → `lower > upper` → le verdict sort
  **`accepted` (exit 0)** sur un log malformé. Un gate CI branché sur ce
  code de sortie peut donc valider à tort. Correctif attendu : refuser
  `lower >= upper` → `ValueError` → exit 2. Reproduit :
  ```python
  parse_sprt_log_from_text("SPRT: elo0=0.00 elo1=5.00 alpha=0.90 beta=0.95\n"
                            "LLR: 0.000\nGames: 100\nW/L/D: 55 40 5\n")
  → verdict "accepted"
  ```
* **Validation (partielle) — FAIT le 2026-10-22** : `fastchess` **est**
  installé (compilé depuis les sources) et `prt/` contient des logs réels.
  `tools/pr_sprt.py` (branche `feature/selfplay-instrumentation`, non
  poussée) a été passé sur les **cinq** logs de la phase 4 : il **reproduit
  exactement** les lignes `Elo: X +/- Y` de fastchess
  (+399,7 ± 50,9 ; −404,9 ± 49,1 ; +596,5 ± 90,9 ; +450,4 ± 55,2 ;
  −0,0 ± 34,2) et rend le même verdict de LLR. Le gate de crédibilité
  du parseur est donc **fermé sur le format fastchess** ; reste à le
  fusionner sur `main` (la branche n'est pas poussée) et à le re-tester
  dans le CI.

### Ce qu'il faut avant la première ligne Elo de ce journal

1. ~~Trancher la réconciliation M4-d9~~ — **FAIT le 2026-10-21** : gate 8/8,
   exit 0, écart expliqué par convention (cf. plus haut).
2. ~~Installer `fastchess`, capturer un log réel dans `prt/`~~ — **FAIT le
   2026-10-22** (compilation depuis les sources) ; logs réels dans `prt/`.
   Reste à passer `tools/pr_sprt.py` dessus (Task 8 du plan
   d'instrumentation) pour fermer le gate de crédibilité de l'instrument.
3. ~~Fermer le défaut `alpha`/`beta` de PR-SPRT~~ — **FAIT**
   (`c7f88f6`), avec deux correctifs de suite (`4a9698d`, `5a3fe92`) et un
   troisième sur la lecture des taux (`a64f59b`) ; 108 tests, contrat
   d'exit-code vérifié. Reste à re-reviewer la vague finale.
4. Reprendre la re-mesure des benches sur l'hôte EPYC de référence.
5. Exécuter `slaml/sims_abab.sh` (A1 vs v1) pour chiffrer le saut sims/s.
6. ~~SPRT A1 vs v1 (`test.sh`, nœuds-fixes 10k)~~ — **remplacé** : la
   première ligne Elo jouée est **phase 4 vs phase 3** (nœuds-fixes
   2k/10k/40k), cf. la section « Échelle Elo — phase 4 ». La ligne A1 vs v1
   reste à jouer pour l'historique de l'échelle.

> Rappel SELFPLAY.md §1.5 : l'unité de plafond est **ΔElo par heure-CPU**,
> pas « nombre de parties ». PR-SPRT donne l'Elo ; la CPU-hour reste à
> comptabiliser par l'opérateur. Les deux sont nécessaires pour le gate
> SP-3.

## Signaux design (rapportés, jamais implémentés discrètement)

* A2 (priors) : les données nécessaires (captures via `Move::is_capture`,
  checks via `Position::gives_check`, promotions via flags, PST-delta via
  `eval`) sont TOUTES accessibles par le contrat API du core FROZEN —
  **aucun changement core réclamé** pour les priors heuristiques v1. Le SEE
  pour C2 sera du code mcts-side (`mcts::prior`) — pas de movegen stagée.
* B3 (transpositions) : la table vivra mcts-side (`mcts::tt`) sur les
  Zobrist du core — aucune demande core.
* D1 (NN) : les hooks A1 (eval leaf) / A2 (prior) délimitent déjà la
  surface ; le core n'a jamais besoin de les connaître.
