# Calibration Elo — `mychess` contre Stockfish en force limitée

> Date : 2026-10-22 · moteur mesuré : `main` après la phase 5 (`20d5c63` +
> `f44abf5`), c'est-à-dire le comportement de la phase 4 (`LeafQ = 0`) plus
> le correctif du budget temps · ancre : **Stockfish 19** (`sf_19`),
> `UCI_LimitStrength` + `UCI_Elo` · méthode : `tools/elo_estimate.sh`
> (cf. `docs/elo_estimation.md`).

## Résultat

| niveau `UCI_Elo` de Stockfish | parties | score du moteur | écart Elo | ± |
|---|---|---|---|---|
| 1320 | 100 | 62,5 % | +89 | 36 |
| 1350 | 200 | 55,8 % | +40 | 25 |
| 1400 | 200 | 57,5 % | +53 | 25 |
| **1425** | 200 | **46,8 %** | **−23** | 25 |
| 1450 | 100 | 33,0 % | −123 | 37 |
| 1600 | 100 | 20,0 % | −241 | 43 |
| 1750 | 100 | 12,5 % | −338 | 53 |
| 1900 | 100 | 6,5 % | −463 | 70 |
| 2050 | 100 | 2,5 % | −636 | 111 |

**Estimation retenue** (estimateur local : les deux niveaux qui encadrent
la croisée à 50 %, interpolation en espace `logit`, donc en Elo) :

> ## `mychess` ≈ **1417 ± 19 Elo** (échelle `UCI_Elo` de Stockfish)

L'ajustement global pondéré (les 8 points non saturés) donne 1403 : il
sous-estime légèrement, parce que la force de Stockfish **sature** aux
niveaux hauts (le score tombe à 2,5 % et n'est plus linéaire). L'estimateur
local est donc le seul défendable ici, et il n'utilise que les points qui
portent l'information.

## Protocole

* **équité du budget** : `go nodes 40000` pour les **deux** moteurs — le
  résultat ne dépend donc pas de la vitesse de l'hôte ;
* **book** : `eco_lichess.epd` (remplacement ECO lichess, 3 863 positions) —
  le même que l'échelle SPRT du dépôt ;
* **adjudication** : `-resign movecount=3 score=600 -draw movenumber=40
  movecount=8 score=10` (règles du dépôt) ;
* **échantillon** : 100 à 200 parties par niveau, 50 à 100 rondes × 2
  couleurs, `-concurrency 5`, ouvertures en ordre aléatoire ;
* **hôte** : AMD Ryzen 5 1600X (Zen 1), WSL2 — **pas** l'EPYC de référence.

## Reproduction

```bash
tools/elo_estimate.sh target/release/mychess \
    ~/.cache/mychess/sf/stockfish/stockfish-linux-x86-64-universal \
    --levels 1320,1350,1400,1425,1450,1600,1750,1900,2050 --games 200 --nodes 40000
```

Les journaux `fastchess` sont versionnés dans ce répertoire (preuve).

## Réserves

1. **Le protocole domine le chiffre** : 1417 est l'Elo **à 40 000
   nœuds/move**. Le même moteur à 10 s/move, ou à 2 000 nœuds/move, n'a
   pas le même Elo ; c'est la même remarque que pour toute l'échelle du
   dépôt (les Elo de la phase 4 sont des écarts, pas des niveaux).
2. **L'échelle `UCI_Elo` de Stockfish est calibrée sur son propre contrôle
   de temps** : l'utiliser sous un budget de nœuds est une approximation.
   Le ± 19 est **statistique** (échantillonnage des scores) — il ne
   couvre pas ce biais de calibration, qui peut être de l'ordre de la
   centaine de points. C'est la réserve principale sur ce chiffre.
3. **Le book est celui de remplacement** (positions ECO volontairement
   déséquilibrées) : l'Elo absolu est donc relatif à ce book.
4. **La force limitée de Stockfish n'est pas un joueur humain** : un
   Stockfish bridé « se trompe » de façon calibrée, pas humaine. C'est un
   instrument, pas un adversaire.

## Contrôle par tournoi round-robin (2026-10-22, suite à la contestation)

Le chiffre ci-dessus a été **contesté** (trop bas) et vérifié par un
**round-robin** de 9 moteurs, 432 parties, 12 par paire, budget
`nodes=40000`, **sans** la règle d'adjudication de resignation (elle se
déclenche sur le score *rapporté* par Stockfish, que son handicap brouille :
les parties étaient decides « by adjudication », pas aux échecs).

| paire | résultat | attendu |
|---|---|---|
| `mychess` vs `mychess` (mêmes binaire) | **50,0 %** | 50 % ✅ |
| `sf1900` vs `sf1900` (mêmes réglage) | **50,0 %** | 50 % ✅ |

Les **contrôles passent** : l'environnement de mesure est sain. Mais la
matrice des scores est **non transitive** :

| | sf1320 | sf1425 | sf1500 | sf1600 | mychess |
|---|---|---|---|---|---|
| sf1320 | — | 0,33 | 0,42 | 0,17 | 0,58 |
| sf1425 | 0,67 | — | 0,62 | 0,83 | 0,83 |
| sf1600 | 0,83 | 0,17 | 0,54 | — | 1,00 |

`sf1320` bat `mychess` (58 %) mais perd contre `sf1425` (33 %) **et**
contre `sf1600` (17 %) : il n'existe **aucun** classement cohérent. Donc
**l'échelle `UCI_Elo` de Stockfish n'est pas une échelle de force** sous
un budget de nœuds : le handicap ne réduit que le *choix de la racine*
(la profondeur atteinte reste 11 partout), et l'effet sur le résultat est
aléatoire, pas monotone.

**Conclusion : le « 1417 Elo » est annulé.** Il mesurait la position d'un
outil de mesure invalide, pas la force du moteur. Ce qui reste vrai :

* les **contrôles** (mêmes moteurs ⇒ 50 %) sont bons — la méthode de
  mesure du dépôt (SPRT, contrôle d'égalité) est saine ;
* à budget de nœuds égal, `mychess` se situe **au niveau des réglages
  les plus faibles de Stockfish** (il perd 12/12 contre `sf1600`, et
  `sf1320` le bat 58 %) ;
* en partie **chronométrée**, le moteur est **plus faible encore** : la
  formule TC ne lui donne que ~4 % de son temps par coup (400 ms sur un
  budget de 10 s), là où Stockfish utilise 100 % du sien.

D'où la piste de la phase suivante : la **gestion du temps** est un
levier plus rentable que l'éval, et il est mesurable.
