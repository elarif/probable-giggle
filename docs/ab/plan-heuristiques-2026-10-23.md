# Phase AB8 — la profondeur **sans** le bruit : décomposer les heuristiques de l'alpha-beta

*Date : 2026-10-23 — suite de `docs/ab/phase1-2026-10-22.md` (phase AB),
du plan `docs/ab/plan-alpha-beta-2026-10-22.md` (AB4) et du protocole
« bruit » `docs/ab/plan-bruit-2026-10-23.md` (AB7).*

## 1. Ce que les phases précédentes ont établi

* **AB (2026-10-22)** : à évaluation identique et à temps identique,
  l'alpha-beta **ne** bat pas le MCTS (0 victoire en 80 parties,
  ≈ −80 Elo). L'évaluation statique se trompe de **≈ 490 cp**, et la
  recherche est instable de **104 cp par deux plies**.
* **Le diagnostic** : l'alpha-beta prend le **maximum** sur un arbre
  bruité — il **amplifie** ; le MCTS **moyenne** ses feuilles — il
  **amortit**. C'est une **corrélation**, pas une preuve : AB7 est né
  pour la trancher.
* **AB7** : le bruit de valeur est un **instrument** (`eval::set_noise`),
  pas une conviction. Il permet de mesurer la **réponse** de chaque
  algorithme à une valeur bruitée, position par position.

Ce que ces phases ont **dit** du goulot d'étranglement : la **valeur**,
pas l'algorithme. Et ce qu'elles ont **dit** des phases suivantes : une
heuristique de recherche qui gagne de la profondeur **sur une fonction
de valeur qui ne le mérite pas** gagne de la profondeur, **pas** de la
force.

## 2. La question de cette phase

> **Q** : une heuristique d'élagage classique (null-move, razoring,
> futility, LMR) est-elle un **gain** pour cet alpha-beta — ou est-ce un
> gain de **profondeur** sur un arbre **bruité**, c'est-à-dire un gain
> **illusoire** ?

C'est la question que le plan AB4 n'a jamais posée : il demandait « la
heuristique gagne-t-elle du débit **et** du score ? ». Le plan AB7 §6.3
la repose : « une heuristique qui augmente la profondeur *et* le bruit
n'est pas un gain ».

La phase AB4 (dans le plan d'origine) est donc **complétée** par cette
phase, et non remplacée : les deux mesures sont faites, mais la
**décision** passe par la métrique du bruit.

## 3. L'hypothèse

> **H8** : à budget temps égal, une heuristique d'élagage augmente la
> profondeur atteinte **et** augmente le **facteur d'amplification** du
> bruit de la valeur (elle coupe sur des bornes calculées sur une
> valeur bruitée, et elle explore un arbre plus grand, donc elle trouve
> plus de crêtes de bruit). Le gain de profondeur est alors **annulé**
> par le gain de bruit : **le score ne s'améliore pas**.

La phase ne suppose pas que H8 soit vraie. Les trois issues :

| issue | lecture | décision |
|---|---|---|
| **H8 confirmée** | les heuristiques **n'augmentent pas** le score à temps égal, et/ou augmentent le facteur d'amplification | les heuristiques sont **reverties** (option `0` par défaut) ; le goulot est **la valeur** ⇒ la phase NN (AB6) est la seule voie |
| **H8 infirmée** | au moins une heuristique **augmente le score** à temps égal, sans augmentation proportionnelle de l'amplification | cette heuristique est **gardée** (option `1` par défaut), et le moteur AB progresse |
| **mixte** | certaines heuristics gagnent, d'autres non | on ne garde que celles qui gagnent, **une par une**, et on rejoue le duel AB-vs-MCTS avec le lot retenu |

## 4. Le dispositif

### 4.1 Les options (le moteur)

`ab::Options` rend **séparément activables** les six briques de la
recherche. Les trois premières (PVS, killer moves, heuristique
d'historique) sont **déjà** dans le moteur ; les trois autres
(null-move, razoring/futility, LMR) sont **nouvelles** et sont
implémentées par cette phase.

| option | défaut | effet |
|---|---|---|
| `AbPvs` | **1** | fenêtre nulle sur les coups suivants, re-cherche si le coup dépasse `alpha` |
| `AbKillers` | **1** | les deux coups qui ont coupé, essayés en tête de l'ordre |
| `AbHistory` | **1** | bonus incrémental par couple (départ, arrivée) |
| `AbNullMove` | **0** | coup gratuit (`R = 2`) : si le trait ne perd pas après un coup gratuit, la position est gagnante |
| `AbRazoring` | **0** | à profondeur ≤ 2, un score statique très bas est vérifié à profondeur réduite |
| `AbFutility` | **0** | à profondeur ≤ 2, un score statique très haut est rendu sans chercher les coups |
| `AbLmr` | **0** | réduction de profondeur sur les coups tranquilles, re-cherche si le coup promet |

> Les **défauts** sont choisis pour que le chemin par défaut soit
> **bit-à-bit** celui de la phase AB gelée : PVS + killers + historique
> (le lot de la phase AB1), et **rien** de nouveau. Les mesures des
> phases 3 à 5 et le duel de la phase AB restent donc reproductibles.

Les options sont **UCI** (`option name AbNullMove type spin default 0
min 0 max 1`, etc.) : le duel (`tools/tournoi_ab_vs_mcts.sh`) et le
nouveau duel AB-vs-AB (`tools/duel_ab.sh`) peuvent donc les piloter sans
modifier le moteur.

### 4.2 L'instrument (la mesure)

`examples/heuristiques_ab.rs` mesure **quatre** choses par configuration,
sur le même corpus de positions (le livre ECO du projet) :

| métrique | ce qu'elle dit | comment |
|---|---|---|
| **débit** | la heuristique donne-t-elle des nœuds ? | nombre de nœuds moyen à profondeur 5 |
| **profondeur** | à temps égal, gagne-t-elle de la profondeur ? | profondeur itérative **complète** atteinte à `movetime 200 ms` |
| **score** | la profondeur gagnée se traduit-elle en score ? | score moyen à profondeur 5, bruit nul |
| **amplification** | la profondeur gagnée se paye-t-elle en bruit ? | facteur d'amplification (AB7) à **temps égal** : `(valeur bruitée − valeur nette) / A` |

Le facteur d'amplification est mesuré à **budget temps égal** (c'est
là que la comparaison est honnête : à profondeur fixée, une heuristique
qui élague change la taille de l'arbre, donc l'amplification, sans que
ce soit un défaut).

## 5. Le protocole

* **Corpus** : le livre ECO lichess du projet
  (`/tmp/openings_src/eco_lichess.epd`, 3 863 positions), **20
  positions** par configuration (les 20 premières, dans l'ordre du
  livre — donc **identiques** d'une configuration à l'autre).
* **Graine** : graine de bruit `0x4e4f495345000001` (celle d'AB7),
  graine de recherche fixe.
* **Bruit** : amplitude **250 cp** (l'amplitude de la mesure d'AB7) et
  **490 cp** (l'erreur d'évaluation réellement mesurée) ; le point à
  bruit **nul** est la référence.
* **Répétabilité** : la mesure est **déterministe** à profondeur fixée ;
  à budget temps, elle dépend de l'hôte — le rapport note l'hôte.

## 6. Les critères de décision (par heuristique)

Une heuristique est **gardée** si, et seulement si :

1. elle **augmente** la profondeur atteinte à temps égal (ou réduit le
   nombre de nœuds à profondeur fixée) — sans quoi elle ne sert à rien ;
2. elle **ne dégrade pas** le score à profondeur fixée de plus de
   **20 cp** en moyenne (le seuil est celui de la stabilité de la
   recherche : 104 cp par deux plies, donc 20 cp est sous le bruit
   mesuré) ;
3. elle **n'augmente pas** le facteur d'amplification de plus de **20 %**
   *relativement* à son gain de profondeur.

Sinon, elle est **revertie** (option `0` par défaut), et le rapport
documente la mesure comme **expérience négative** — le même traitement
que la phase 5 du MCTS (`LeafQ`).

**Le lot n'est jamais gardé en bloc** : chaque heuristique est
décidée seule. C'est la correction du défaut de la phase AB4
(documented dans `docs/ab/phase1-2026-10-22.md` §6 : « décomposer le
lot PVS + killers + historique en mesures séparées »).

## 7. Les tests (les invariants)

Les heuristiques ne sont acceptées que si les **invariants** tiennent —
ils sont testés (`tests/alpha_beta_heuristiques.rs`), pas affirmés :

* le **mat en un** et le **mat en deux** sont toujours trouvés, même
  toutes options activées (le null-move ne peut pas élaguer un mat) ;
* le **pat** vaut toujours 0 (pas −MATE) ;
* le coup rendu est toujours **légal** (jamais `0000`) ;
* la PV est **légale** de bout en bout ;
* la recherche reste **déterministe** ;
* le **défaut** (PVS + killers + historique, rien de nouveau) donne
  **exactement** le même score et le même nombre de nœuds que la phase
  AB gelée — c'est l'invariant qui garantit que les mesures gelées
  restent valides ;
* le **razoring** ne coupe pas un coup gagnant (une position où le
  trait gagne du matériel garde un score positif) ;
* le **LMR** ne change pas le coup dans une position tactique (mat en
  deux) ;
* le **null-move** ne s'applique pas à une position **zugzwang** (fin
  de pions) : le score y reste correct.

## 8. Ce que la phase ne fait pas

* **Pas de parallélisation** de l'alpha-beta (hors périmètre du plan
  AB, §3).
* **Pas de refonte du core gelé** (`vcore-1.0`) : le null-move est
  implémenté **côté AB**, en basculant le trait et la clé Zobrist — le
  core n'est pas touché.
* **Pas de changement du chemin MCTS** : `Search=mcts` reste le
  défaut, et les mesures gelées des phases 3 à 5 restent
  reproductibles à l'identique.
* **Pas de phase NN** : c'est la phase **suivante** (AB6), et elle
  reste conditionnelle (issue C de la porte du plan AB).

## 9. Les suites

1. **AB8** (ce document) : implémentation, mesures, décision par
   heuristique ;
2. **AB8b** : le duel **AB-vs-AB** (option contre option) en Elo, pour
   confirmer la décision de l'instrument par une mesure de force ;
3. **AB5 (complété)** : 200 parties par paire, la suite tactique,
   l'Elo contre Stockfish en force limitée ;
4. **AB6 (conditionnel)** : la valeur apprise.

## 10. Reproduction

```bash
# les invariants (heuristiques + décisions)
cargo test --release --test alpha_beta_heuristiques

# l'instrument : débit, profondeur, score, amplification
cargo run --release --example heuristiques_ab -- \
    /tmp/openings_src/eco_lichess.epd 20 250,490

# un duel AB-vs-AB (une heuristique contre la référence)
tools/duel_ab.sh --rounds 20 --a "AbNullMove=1" --b "AbNullMove=0"
```
