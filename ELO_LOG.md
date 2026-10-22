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
| 2026-10-22 | phase 4 (A2) | A2 : sélection **PUCT** (`Q + c_puct·P·sqrt(N)/(1+n)`) + prior MVV-LVA (`mcts::policy`, stocké dans `Edge::prior`) ; la largeur reste le tirage uniforme v1, la profondeur change ; la règle UCT est **conservée et figée** (`Policy::Uct`) ; options UCI `Policy`/`Cpuct`/`RootNoise` | nœuds-fixes 2k/10k/40k (planifié) | PENDING | PENDING | **BLOQUÉ** — fastchess absent de la machine ; la **ligne de référence de la phase 3 est à rejouer** (le comportement mono par défaut a changé) |

Notes phase 4 (A2) : le gain mesuré est en **convergence en valeur** (erreur
de score ÷ 4 à 7 contre une recherche de référence à 500–2 000 nœuds) et en
**robustesse tactique en parallèle** (17/22 contre 10/22 à 6 threads et
100 nœuds) ; le débit coûte −10 % sur kiwipete mais la mémoire et l'arbre
divisent par ~1,6. **L'ablation montre que le prior MVV-LVA n'apporte rien
de mesurable** : le gain vient de la forme de décroissance du terme
d'exploration. Le prior est livré comme crochet du pipeline
« logits → softmax → prior » que la policy apprise de D1 branchera.
Rapport : `docs/mcts/phase4-2026-10-22.md`.

Notes A1 : la valeur feuille = éval statique = le saut attendu le plus gros
(Ramanujan 2010 shallow traps ; Monte-Carlo Chess 2012 ; lc0 par réseau).
Budget sims/s : le playout aléatoire (~76 µs) disparaît ; une feuille =
make/unmake de descente (~22 ns/ply) + eval (~40-90 ns/position) → le
dénominateur attendu de sims/s augmente d'un ordre de grandeur ; valeur
exacte mesurée post-M4 (baseline-A criterion revalidée, mention post-M4).

## Instruments de mesure (état au 2026-10-20)

Le journal ne peut pas encore être alimenté : deux maillons manquent.

| instrument | rôle | état |
|---|---|---|
| `test.sh` (SPRT `elo0=0 elo1=5 alpha=0.05 beta=0.05`) | verdict **binaire** « le candidat est-il meilleur ? » | **gelé** (protocole figé, ne pas modifier) |
| `tools/pr_sprt.py` (PR-SPRT) | **magnitude** Elo ± IC à partir du même log fastchess | **codé, 54 tests OK**, branche `feature/selfplay-instrumentation` (450a6ef) — non poussé |
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
* **Validation manquante** : `fastchess` **n'est pas installé** sur la
  machine et `prt/` est vide → le parseur **n'a jamais vu un log réel**.
  Tous ses comportements dérivent de la lecture du code source upstream
  (fastchess `v1.8.2-alpha`), pas d'une capture. C'est le gate de crédibilité
  de l'instrument, et il est **ouvert**.

### Ce qu'il faut avant la première ligne Elo de ce journal

1. ~~Trancher la réconciliation M4-d9~~ — **FAIT le 2026-10-21** : gate 8/8,
   exit 0, écart expliqué par convention (cf. plus haut).
2. Installer `fastchess`, capturer un log réel dans `prt/`, valider
   `tools/pr_sprt.py` dessus (Task 8 du plan d'instrumentation).
3. ~~Fermer le défaut `alpha`/`beta` de PR-SPRT~~ — **FAIT**
   (`c7f88f6`), avec deux correctifs de suite (`4a9698d`, `5a3fe92`) et un
   troisième sur la lecture des taux (`a64f59b`) ; 108 tests, contrat
   d'exit-code vérifié. Reste à re-reviewer la vague finale.
4. Reprendre la re-mesure des benches sur l'hôte EPYC de référence.
5. Exécuter `slaml/sims_abab.sh` (A1 vs v1) pour chiffrer le saut sims/s.
6. SPRT A1 vs v1 (`test.sh`, nœuds-fixes 10k), puis PR-SPRT sur le log
   pour la magnitude.

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
