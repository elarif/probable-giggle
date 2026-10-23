# Phase AB7 — le protocole « bruit » : tester le **mécanisme**, pas seulement le résultat

*Date : 2026-10-23 — suite de `docs/ab/phase1-2026-10-22.md` (phase AB1) et
du plan `docs/ab/plan-alpha-beta-2026-10-22.md`.*

## 1. Ce que la phase AB1 a établi, et ce qu'elle n'a pas établi

Le duel **contrôlé** AB vs MCTS (même binaire, même évaluation, même
temps, 80 parties) donne **0 victoire de l'alpha-beta** (−80 Elo). La
mesure de la valeur (`examples/eval_error`, 200 positions) donne une
erreur d'évaluation de **≈ 490 cp** et une instabilité de la recherche de
**104 cp par deux plies**.

L'explication proposée : l'alpha-beta prend le **maximum** sur un arbre —
il **amplifie** le bruit de la valeur ; le MCTS **moyenne** ses feuilles —
il l'**amortit**.

C'est une **corrélation**, pas une preuve : le duel et la mesure de la
valeur sont compatibles avec une autre explication (profondeur atteinte,
gestion du temps, ordre des coups). La phase AB7 existe pour **trancher
l'explication elle-même**, pas pour re-mesurer le duel.

## 2. L'hypothèse à tester

> **H** : à évaluation identique, *plus* la valeur est bruitée, *plus*
> l'alpha-beta s'effondre et *moins* le MCTS s'effondre — parce que le
> premier prend le maximum sur un arbre bruité et le second moyenne des
> feuilles bruitées.

Si **H** est fausse, le diagnostic de la phase AB1 est faux, et la porte
de décision du plan (`docs/ab/plan-alpha-beta-2026-10-22.md` §6, issues
C et D) doit être rediscutée. Si **H** est vraie, la phase NN (valeur
apprise) est confirmée comme la seule voie, **et** le MCTS est le
meilleur hôte pour une valeur encore imparfaite.

## 3. Le dispositif : un bruit de valeur contrôlé

`src/eval.rs` expose un **bruit de valeur déterministe** :

* `eval::set_noise(cp, seed)` : l'amplitude `cp` (centipions) et la graine ;
* `eval::noise_offset(pos)` : l'offset d'une position, dans `[-cp, +cp]` ;
* il s'applique dans **`eval_for_mover`** — le point d'entrée **unique**
  de la valeur, partagé par la feuille du MCTS **et** celle de l'alpha-beta.

C'est la condition pour que la comparaison porte sur l'**algorithme** et
pas sur l'évaluation.

### 3.1 Les trois propriétés (et pourquoi chacune est nécessaire)

| propriété | définition | pourquoi |
|---|---|---|
| **déterminisme** | la même position + la même graine + la même amplitude ⇒ le même offset | la table de transposition de l'AB doit rester **cohérente** : une entrée mémorisée reste la valeur de cette position. Un bruit tiré par appel rendrait la TT fausse, et l'AB ne mesurerait plus l'amplification mais du bruit de mesure |
| **indépendance du trait** | l'erreur appartient au **plateau** : le même plateau, joué par les blancs ou les noirs, est bruité **du même côté** (cadre « Blanc ») | c'est la seule lecture cohérente d'une « erreur d'évaluation » : l'erreur est une propriété de la position de l'échiquier, pas de la position vue par un camp. L'offset est donc appliqué **avant** le changement de signe du trait |
| **forme fixée** | le champ est un vecteur unique dans `[-1, +1]`, **mis à l'échelle** par l'amplitude | sans cela, chaque point de la courbe mesurerait un *bruit différent* et la pente n'aurait aucun sens. Deux amplitudes donnent le **même** champ, de même forme |

Ces trois propriétés sont **testées** (`tests/eval_noise.rs`, 7 tests),
pas seulement affirmées — y compris l'invariant que le champ est
*mis à l'échelle* (l'offset à 400 cp vaut bien 4 × l'offset à 100 cp, à
l'arrondi près).

Le hash est le finaliseur de **SplitMix64** : sans avalanche, les
positions voisines (dont les clés Zobrist ne diffèrent que de quelques
bits) recevraient des bruits voisins, et le champ aurait une *structure*
au lieu d'être un bruit.

### 3.2 Les options UCI

* `EvalNoise` (spin, centipions, défaut **0**) — l'amplitude ;
* `EvalNoiseSeed` (string, défaut `0x4e4f495345000001`) — la graine du
  champ.

La graine du bruit est **indépendante** de la graine de recherche
(`Seed`) : le champ de bruit doit être le même d'un point de la courbe à
l'autre, quelle que soit la graine du moteur.

À amplitude 0, l'évaluation est **bit-à-bit** celle d'avant : les
mesures gelées des phases 3 à 5 ne sont pas touchées.

## 4. Le protocole de mesure

### 4.1 Le mécanisme (l'instrument décisif) — `examples/bruit_amplification`

Mesurer l'amplification **directement**, sans passer par des parties :

1. pour chaque position d'un corpus, la valeur racine est calculée avec
   le bruit **nul** (la référence) ;
2. puis avec le bruit **activé** à l'amplitude `A` ;
3. l'écart est normalisé par `A` : le **facteur d'amplification**, sans
   dimension.

Ce que l'hypothèse **prédit** :

* **Alpha-beta** : la valeur racine est un **maximum** sur un arbre de
  feuilles bruitées. Le maximum *sélectionne* les feuilles au bruit
  favorable ⇒ l'espérance du score est **biaisée vers le haut**, et
  l'écart croît **avec la profondeur** (plus de feuilles ⇒ plus de chances
  de trouver une crête). Le facteur doit **croître avec la profondeur**.
* **MCTS** : la valeur racine est une **moyenne** sur les feuilles
  visitées ; les offsets de positions distinctes sont indépendants ⇒ la
  moyenne amortit le bruit, d'autant mieux que le budget de nœuds est
  grand. Le facteur doit **décroître avec le budget** et rester **proche
  de zéro**.

C'est le test décisif, et il est **enormement** plus sensible que le
duel : le duel mesure une différence de force à ±46 Elo, l'instrument
mesure l'amplification position par position.

Configurations comparées (mêmes positions, même graine de recherche, à
amplitude nulle) : `ab d3`, `ab d4`, `mcts 2k`, `mcts 5k`.

### 4.2 Le résultat (le duel) — `tools/tournoi_ab_vs_mcts.sh --noise`

Le duel **contrôlé** de la phase AB1, rejoué à plusieurs amplitudes :

* même binaire, même livre ECO lichess, même graine, `movetime 200 ms`,
  aucune adjudication ;
* **40 parties** par point (la précision d'un point est alors ± 45 Elo :
  suffisant pour lire une *pente*, pas pour classer deux points voisins) ;
* amplitudes : **100**, **250**, **490** cp (l'amplitude de l'erreur
  réellement mesurée) ;
* la métrique est l'Elo du duel à chaque amplitude.

> **Correction par rapport à l'ébauche** : le point « bruit 0 » de
> l'ébauche était présenté comme « la valeur parfaite ». C'est **faux** :
> l'évaluation statique a *déjà* une erreur d'environ 490 cp par rapport
> à ce que la recherche en pense. Le point 0 est donc la **valeur
> actuelle**, et la courbe mesure l'effet d'un bruit **ajouté**, pas
> l'effet d'une valeur parfaite. C'est la formulation honnête, et c'est
> celle qui rend la pente interprétable.

## 5. Ce qu'on attend (et ce que ça prouverait)

| résultat | lecture |
|---|---|
| le facteur de l'AB **croît avec la profondeur**, celui du MCTS **décroît avec le budget** | **H est confirmée** : le diagnostic tient, la phase NN est justifiée pour les deux algorithmes, et le MCTS est le bon hôte pour une valeur imparfaite |
| les deux facteurs sont **plats** | le bruit n'explique pas le duel : la phase AB1 a mal attribué la force — il faut chercher du côté de la **profondeur** (le MCTS joue « plus large », l'alpha-beta « plus profond ») |
| le facteur de l'AB est **négatif** (la recherche *repousse* le bruit) | l'explication « maximum ⇒ amplification » est fausse : l'AB est alors *protégé* par sa structure d'élagage, et c'est le MCTS qu'il faut réaccuser |

Dans les deux derniers cas, le résultat est **négatif mais utile** : il
disqualifie une explication et évite d'engager la phase NN sur une
mauvaise hypothèse.

## 6. Les suites (dans l'ordre)

1. **AB7 (ce document)** : le facteur d'amplification (mécanisme), puis
   la courbe de duel (résultat) ;
2. **AB5 (complété)** : 200 parties par paire, la suite tactique, l'Elo
   contre Stockfish en force limitée ;
3. **AB4 (complété)** : null-move, razoring/futility, LMR — **mesurés
   avec la métrique de bruit**, pas seulement avec le score : une
   heuristique qui augmente la profondeur *et* le bruit n'est pas un
   gain ;
4. **AB6 (la valeur apprise)** : la phase NN, avec l'interface déjà en
   place (`eval::eval_for_mover` est le point d'entrée unique ; le prior
   du MCTS est le crochet de la policy apprise).

## 7. Reproduction

```bash
# les tests du dispositif (7 invariants)
cargo test --release --test eval_noise

# le facteur d'amplification (le test décisif)
cargo run --release --example bruit_amplification -- \
    /tmp/openings_src/eco_lichess.epd 40 100,250,490

# un point de la courbe de duel
tools/tournoi_ab_vs_mcts.sh --rounds 20 --noise 250 --out prt/rr/bruit-250.log
```
