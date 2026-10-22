# Plan — phase AB : un alpha-beta dans le dépôt

*Date : 2026-10-22 — suite du tournoi round-robin avec Reckless v0.1.0
(`docs/elo/README.md`) et de l'analyse « le MCTS est plafonné par sa
valeur de feuille » (voir §1).*

## 1. Pourquoi cette phase

Le tournoi du 2026-10-22 a mesuré `mychess` à **≈ 1296 Elo** (échelle
`UCI_Elo` de Stockfish, `movetime 200 ms`) et `Reckless v0.1.0` à
**≈ 2249**. Le diagnostic du plafond n'est pas l'algorithme de
sélection : la phase 4 (PUCT) a gagné **+400 à +600 Elo**, et
l'ablation a montré que le gain venait de la **forme de décroissance du
terme d'exploration**, pas du prior MVV-LVA.

La contrainte qui lie, c'est la **valeur de feuille** :

* la feuille vaut `eval::eval_for_mover` — matériel + PST PeSTO-lite
  (`src/eval.rs`) : aucune lecture tactique ;
* l'erreur d'horizon mesurée par la phase 5 vaut **351 cp en moyenne**, et
  dépasse **une tour dans 19 %** des positions d'échange ;
* la phase 5 (feuille « tactiquement résolue », quiescence-lite) a été
  **négative** (−118 à −241 Elo) et **revertie** : aiguiser l'évaluation
  *artisanale* ne produit pas la force d'un alpha-beta.

Autrement dit : le MCTS de ce dépôt est **structurellement** un
AlphaZero (arbre, prior, valeur de feuille, pas de rollout aléatoire
depuis A1) **sans la valeur apprise**. C'est exactement la configuration
que la littérature décrit comme plafonnée en échecs.

**Ce que cette phase n'est pas** : un plan pour faire de `mychess` un
moteur fort. C'est un plan pour **rendre mesurable** l'affirmation
« l'alpha-beta bat le MCTS à budget égal, à évaluation identique », dans
ce dépôt, avec le protocole de mesure du projet. La décision de pivot
(§6) sortira de la mesure, pas de l'opinion.

## 2. Objectifs

1. **Un opposant local.** Le dépôt n'a aujourd'hui aucun moteur de
   référence : toute la calibration passe par Stockfish, à 950 Elo de
   distance. Sans opposant local, aucune mesure « MCTS vs alpha-beta »
   n'est possible.
2. **Mesurer le déficit tactique** du MCTS à fonction de valeur identique
   (même `eval`, même temps, mêmes positions) : la courbe AB-vs-MCTS en
   fonction de la profondeur est le chiffre qui manque.
3. **Doter le projet d'un搜索引擎 de référence** pour les phases
   suivantes (valeur apprise : NNUE ou petit réseau de policy/value).
4. **Trancher la porte §6** : MCTS conservé comme laboratoire, ou pivot
   alpha-beta.

## 3. Non-objectifs (hors de ce plan)

* **NNUE / réseau appris** : phase NN, conditionnelle (§6). Aucun réseau
  n'est entraîné ici (10⁴–10⁵ GPU-heures : hors de portée).
* **Refonte du core gelé** (`vcore-1.0`) : l'alpha-beta vit dans un
  module **mcts-side** (`src/ab/`), comme `src/mcts/` l'a fait pour le
  MCTS. Le core (position, movegen, zobrist, tables) n'est **pas**
  modifié : c'est la condition pour que les échelles gelées des phases 4
  et 5 restent comparables.
* **Ne pas toucher au chemin MCTS par défaut** : l'option UCI `Search`
  vaut `mcts` par défaut. Toutes les mesures MCTS existantes (phases 3,
  4, 5, tournoi du 2026-10-22) restent reproductibles à l'identique.
* **Pas de parallélisation** de l'alpha-beta dans ce plan (LMR,
  chapitres parallèles) : une référence *sûre* d'abord.

## 4. Protocole de mesure (règles du projet)

* **Core gelé** : `src/position.rs`, `src/movegen.rs`, `src/types.rs`,
  `src/zobrist.rs`, `src/tables.rs`, `src/slider.rs` ne changent pas.
* **Évaluation identique** : l'alpha-beta utilise **le même**
  `eval::eval_for_mover` que la feuille MCTS. C'est la condition pour que
  la comparaison AB-vs-MCTS soit une comparaison d'**algorithmes**, pas
  d'évaluations.
* **Budgets comparables** : à temps égal (`movetime 200 ms`) et à nœuds
  sémantiques égaux (une partie AB « 20k nœuds » contre un MCTS « 20k
  playouts » ne sont **pas** le même budget : le playout MCTS est une
  descente, pas un nœud).
* **Hôte** : l'hôte de la session est noté dans chaque rapport ; les
  valeurs relatives (ns/nœud) valent, les valeurs absolues (Elo) sont
  relatives à l'hôte.
* **Preuve** : chaque phase produit (a) un rapport dans `docs/ab/`, (b)
  une entrée `ELO_LOG.md`, (c) un commit atomique, (d) ses tests.

## 5. Les phases

### AB1 — le squelette negamax

*Hypothèse* : un negamax correct, sans table de transposition, à
profondeur fixe, suffit à établir l'écart de force au MCTS.

* `src/ab/` : `negamax` fail-soft, fenêtre pleine, score côté **au
  trait** ;
* **quiescence** : stand-pat + captures (et promotions) ; en échec, on
  joue les esquives ; l'éval n'est jamais lue sur une position « agitée » ;
* **règles de nulle** : 50 coups (`halfmove >= 100`) et **répétition**
  (clé Zobrist : occurrence dans l'historique de la partie ou dans la
  ligne de recherche) → score 0 ;
* **mat / pat** : aucune pièce légale → mat (`-MATE + ply`) ou pat (0) ;
  les scores de mat sont **adjustés en TT** (distance au mat) ;
* **ordre des coups** : MVV-LVA (10 × victime − agresseur) ;
* **interface UCI** : option `Search` (`mcts` par défaut, `alpha` pour
  l'alpha-beta) ; `go depth N`, `go movetime T`, `go nodes N`, `go infinite`
  ; ligne `info depth … score cp … nodes … nps … pv …` ;
* **tests** : mat en 1, mat en 2, pat, nulle par répétition, nulle par
  50 coups, score d'une finale de tours, coup légal en toute position
  (aucun `0000`), PV légale et reproductible.

*Critère d'acceptation* : les tests passent, `cargo test --release` vert,
et le moteur AB **bat** un MCTS de même force sur une position de mat
(forcée) — preuve que la recherche alphabétique est correcte.

### AB2 — table de transposition

*Hypothèse* : la TT réduit le débit de nœuds d'un facteur ≥ 2 à
profondeur égale (mesure sur kiwipete).

* TT mappée, adresse = clé Zobrist, **clé complète** stockée (détection
  de collision), remplacement *depth-preferred* ;
* sondaage : score exact / borne inférieure / borne supérieure, avec
  **ajustement des scores de mat** à la profondeur de stockage ;
* le coup de la TT passe **en premier** dans l'ordre des coups.

*Critère d'acceptation* : le **score** à profondeur fixée est **identique**
avec et sans TT (invariance), et le nombre de nœuds diminue. Un écart de
score est un bug, pas une amélioration.

### AB3 — profondeur itérative et gestion du temps

* profondeur itérative 1..N, la meilleure itération **complète** est
  conservée (une itération interrompue par le deadline est **jeteée** :
  c'est le bug que la phase 5 du MCTS a appris) ;
* budget temps : `movetime` (marge 5 ms) et TC (`wtime/btime` + inc) ;
  l'échéance est testée **tous les 1024 nœuds** (pas 64 : voir
  `BUDGET_POLL_EVERY` et la dette documentée dans `ELO_LOG.md`) ;
* `stop` : interruption propre, la meilleure itération completed est
  rendue ;
* PV extraite de la TT (chaîne des coups de la TT), `info` complet.

*Critère d'acceptation* : le temps d'appel est **toujours** respecté
(marge 5 ms), `go movetime 200` renvoie en 200 ms ± 5 ms, et le score
retourné est celui de la dernière itération **complète**.

### AB4 — heuristiques d'ordre et élagages (une par une, mesurées)

Chacune est **activée séparément** et validée contre le protocole gelé
(AB3 sans l'heuristique) :

1. **coup de la TT en premier** (déjà en AB2) ;
2. **killer moves** (2 par ply) ;
3. **heuristique d'historique** (table 64×64, bonus incrémental) ;
4. **null-move pruning** (R ≠ 1) ;
5. **razoring / futility** aux premiers plies ;
6. **LMR** (late move reductions) — la plus risquée, la dernière.

*Critère d'acceptation* : gain de débit **et** de score à temps égal,
sur le protocole gelé ; une heuristique qui n'améliore **ni** l'un **ni**
l'autre est **revertie** (précedents : phase 5, `LeafQ`).

### AB4b — la contrainte « même évaluation », et la qualité de la valeur

*Hypothèse* : ce qui sépare les deux algorithmes n'est pas l'algorithme
mais la **fonction de valeur** — et, plus précisément, le **bruit** de
cette fonction.

* l'évaluation statique (`eval::eval_for_mover`) est la vérité terrain de
  la recherche : si elle se trompe, la recherche optimise le mauvais
  critère, quelle que soit la profondeur ;
* un alpha-beta **approfondit dans le bruit** : son score oscille d'une
  profondeur à l'autre, et il suit l'oscillation ;
* le MCTS **moyenne** ses feuilles : il amortit le bruit, au prix de la
  profondeur.

*Mesure* : `examples/eval_error` — pour N positions du livre,
`|eval(pos) − score(recherche, profondeur d)|` (l'erreur de la valeur) et
`|score(d) − score(d+1)|` (l'instabilité, donc le bruit amplifié).

*Critère d'acceptation* : la mesure est **reproductible** et figure dans
le rapport de phase ; elle doit être able d'expliquer (ou d'infirmer) le
résultat du duel AB-vs-MCTS.

### AB5 — la mesure qui tranche

* **AB vs MCTS**, à **évaluation identique** et **temps identique** :
  200 parties par couple, book ECO lichess, graine fixe, aucune
  adjudication ;
* **suite tactique** : positions tactiques (mat en n, gain de pièce,
 ievreb) → taux de résolution à profondeur 1..8, AB vs MCTS. C'est le
  chiffre qui quantifie le déficit tactique ;
* **Elo vs Stockfish** (force limitée, `UCI_LimitStrength`), protocole
  SPRT du projet (`tools/`, `docs/elo/`) : l'AB est-il au-dessus de 1296 ?
  au-dessus de Reckless (2249) ?

### AB6 — conditionnel : la valeur apprise (NN)

Si la porte §6 est franchie : brancher un **réseau** comme fonction de
valeur (NNUE public pour l'alpha-beta, ou petit net de policy/value pour
le MCTS). Les crochets existent déjà pour le MCTS (feuille A1, prior A2
« logits → softmax → prior » du D1). **Hors périmètre de ce plan** :
c'est une phase à part, avec son propre protocole (licences des nets,
inférence, mesure).

## 6. La porte de décision

À la fin d'AB5, une seule de ces trois issues :

| issue | critère | décision |
|---|---|---|
| **A** | l'AB (même eval) bat le MCTS de **≥ 150 Elo** à temps égal, **et** l'AB + eval atteint **≥ 1900** sur l'échelle Stockfish | **pivot** : l'alpha-beta devient le moteur principal ; le MCTS reste le laboratoire d'allocation (les phases 3–5 gardent leur valeur) ; la phase NN (AB6) s'ouvre |
| **B** | l'AB bat le MCTS de ≥ 150 Elo mais reste **< 1600** | **hybride** : l'AB sert d'opposant et de vérificateur tactique (le MCTS propose, l'AB dispose) ; la phase NN est prioritaire sur les deux algorithmes |
| **C** | l'AB **ne** bat pas le MCTS de 150 Elo (même évaluation) **et** la mesure AB4b montre que l'erreur de la valeur est **grande** devant la différence de force | **la porte ne porte pas sur l'algorithme** : le MCTS n'est pas « plus fort », il est **moins amplificateur de bruit**. On ne compare plus deux algorithmes mais deux amplificateurs d'un même signal bruité. Le MCTS reste le moteur (le MCTS « bat » l'AB en amortissant le bruit), et la **seule** voie de progression est la **valeur apprise** (phase NN) — pour l'un **ou** l'autre algorithme |
| **D** | l'AB ne bat pas le MCTS de 150 Elo **et** l'erreur de la valeur est **faible** (l'évaluation est fine) | alors le diagnostic « la valeur est le plafond » est faux : c'est la gestion du temps, l'ordre des coups ou l'élagage qui limitent. Ce plan s'arrête ici (documenté comme expérience négative) |

> La porte est **biconditionnelle** (issue C vs D) : c'est la mesure
> AB4b (la qualité de la valeur) qui départage les deux. Sans elle, une
> différence de force entre les deux algorithmes ne dit rien sur le
> goulot d'étranglement — elle peut n'être que le reflet de la manière
> dont chacun amplifie le bruit de la **même** évaluation.

Dans **A** et **B**, l'MCTS n'est pas supprimé : il est **reclassé** —
« laboratoire d'allocation et de politique », avec ses mesures gelées
(phases 3–5) conservées comme référence.

## 7. Risques

| risque | parade |
|---|---|
| une TT ou une heuristique d'ordre **casse** la correction (score faux) | tests d'**invariance** : à profondeur fixée, le score est identique avec et sans TT / avec et sans élagage ; le test échoue si le score bouge |
| l'AB devient **plus fort** mais plus **lent** que le MCTS (comparaison faussée) | le protocole compare à **temps** égal, pas à nœuds égaux ; le débit est reporté dans le rapport |
| la phase 5 du MCTS a montré qu'**une** feuille « plus fine » peut *nuire* | chaque improvement est mesurée séparément et revertie si elle ne gagne pas sur **les deux** axes (débit, score) |
| dérive du protocole (hote, book, graine) | l'en-tête de chaque journal porte le protocole complet, comme pour le tournoi du 2026-10-22 |

## 8. Suivi

| phase | état | livrable |
|---|---|---|
| AB1 | **fait** | `src/ab/search.rs` : negamax, quiescence, nulles, mat/pat, ordre MVV-LVA — `tests/alpha_beta.rs` (10 tests) |
| AB2 | **fait** | `src/ab/tt.rs` : TT mappée (2^19 entrées), bornes exact/inférieure/supérieure, ajustement des scores de mat — test d'invariance (le score ne bouge pas) |
| AB3 | **fait** | profondeur itérative (une itération interrompue est **jetée**), budget temps (marge 5 ms), PV (table PV), `stop` propre |
| AB4 | **partiel** | PVS + killer moves + heuristique d'historique (un lot : à décomposer en mesures séparées) ; reste : null-move, razoring/futility, LMR |
| AB4b | **en cours** | `examples/eval_error.rs` : erreur de la valeur et instabilité de la recherche |
| AB5 | **partiel** | `tools/tournoi_ab_vs_mcts.sh` (duel **contrôlé**, même binaire, même eval, même temps) + `tools/diag_ab_vs_mcts.py` (diagnostic des parties) — reste : 200 parties par paire, suite tactique, Elo vs Stockfish |
| AB6 | conditionnel | la valeur apprise |

Chaque phase close produit : un rapport `docs/ab/phase<N>-<date>.md`, une
entrée `ELO_LOG.md`, un commit, et ses tests.
