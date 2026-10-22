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

## Tournoi refait avec Reckless v0.1.0 (2026-10-22, budget en temps)

> Le tournoi de la section précédente est **refait** avec un moteur de
> plus : **Reckless v0.1.0** (<https://github.com/codedeliveryservice/Reckless/releases/tag/v0.1.0>).
> Le protocole a dû **changer** : voir « Pourquoi le protocole a changé ».
> Journal : `docs/elo/roundrobin-movetime-200ms-reckless.log` ·
> Lancer : `tools/tournoi_roundrobin.sh` · Analyse : `tools/elo_roundrobin.py`.

### Pourquoi le protocole a changé

Le tournoi précédent jouait à **budget de nœuds** (`go nodes 40000` pour
tous les moteurs). **Reckless v0.1.0 n'implémente pas `go nodes`** : son
analyseur UCI (`engine/src/parser.rs`) n'accepte que `depth`, `movetime`
et `wtime/btime/winc/binc`. Un `go nodes N` y tombe en
`TimeControl::Infinite` — c'est-à-dire en **recherche infinie** : la
partie se bloque, pas un résultat.

Le seul budget que **tous** les moteurs honorent est donc le **temps** :
`movetime 200 ms` par coup, identique pour chacun. C'est l'analogue
direct du budget de nœuds (« le même budget **par coup** ») et cela
supprime au passage le **biais de gestion d'horloge** que le contrôle de
temps « horloge » introduisait (`mychess` ne recevait que ~4 % de son
temps par coup, cf. la section précédente).

Le protocole précédent est donc **incomparable** à celui-ci : les deux
matrices ne se superposent pas. C'est le prix de l'ajout de Reckless, et
il est assumé.

> La release `v0.1.0` ne publie qu'un binaire **Windows** (`reckless.exe`).
> Le moteur est donc compilé depuis la **source du tag `v0.1.0`**
> (commit `ef98d47`), binaire installé dans
> `~/.cache/mychess/engines/reckless-0.1.0`.

### Protocole

| | |
|---|---|
| moteurs | 11 : `mychess`, `mychess_ctrl`, `reckless`, `reckless_ctrl`, `sf1320`, `sf1425`, `sf1500`, `sf1600`, `sf1900`, `sf1900_ctrl`, `sf3190` |
| parties | 660 (55 paires × 12 parties), 46 min 20 s, `-concurrency 5` |
| budget | `movetime 200 ms` par coup pour **tous** les moteurs |
| tolérance | `timemargin 200 ms` (une itération trop longue ne fait pas adjuger la partie) |
| book | `eco_lichess.epd` (3 863 positions), ordre aléatoire, graine `20261022` |
| adjudication | **aucune** (ni `-resign`, ni `-draw`) : même règle que le tournoi précédent — ces règles se déclenchent sur le score *rapporté* par le moteur, que le handicap de Stockfish brouille |
| résultat | 613 mats, 38 nulles, **9 parties interrompues** par un dépassement de temps (dont une adjugée « coup illégal », voir « Les défauts trouvés par le tournoi ») |

Les trois paires **de contrôle** (même binaire, ou même réglage) sont
`mychess`/`mychess_ctrl`, `reckless`/`reckless_ctrl` et
`sf1900`/`sf1900_ctrl` : elles doivent être à 50 %. C'est le seul test
valide de l'environnement de mesure.

### Résultats

Classement (score sur les parties **jouées** ; les 9 parties
interrompues par un dépassement de temps sont **écartées** de la
matrice — voir « Les défauts trouvés par le tournoi ») :

| rang | moteur | V-N-D | parties | score | Elo Bradley-Terry (champ) |
|---|---|---|---|---|---|
| 1 | `sf3190` | 120-0-0 | 120 | 100,0 % | +1901 |
| 2 | `reckless_ctrl` | 90-26-4 | 120 | 76,7 % | +616 |
| 3 | **`reckless`** | **90-27-3** | **120** | **76,2 %** | **+603** |
| 4 | `sf1900_ctrl` | 71-45-3 | 119 | 60,9 % | +189 |
| 5 | `sf1900` | 67-48-4 | 119 | 58,0 % | +122 |
| 6 | `sf1600` | 58-53-9 | 120 | 52,1 % | −26 |
| 7 | `sf1500` | 51-67-1 | 119 | 43,3 % | −222 |
| 8 | `sf1425` | 27-81-9 | 117 | 26,9 % | −605 |
| 9 | `mychess_ctrl` | 12-82-19 | 113 | 19,0 % | −797 |
| 10 | `sf1320` | 18-95-5 | 118 | 17,4 % | −861 |
| 11 | **`mychess`** | **9-89-19** | **117** | **15,8 %** | **−919** |

**Contrôles** (paires de force identique, doivent être à 50 %) :

| paire | parties | score | écart |
|---|---|---|---|
| `mychess` vs `mychess_ctrl` | 12 | 50,0 % | 0,0 σ |
| `reckless` vs `reckless_ctrl` | 12 | 45,8 % | 0,3 σ |
| `sf1900` vs `sf1900_ctrl` | 12 | 37,5 % | 0,9 σ |

Les trois contrôles sont à 50 % : **l'environnement de mesure est sain**.
Le résidu maximal du modèle de Bradley-Terry est de **90 Elo** (sur 55
paires) : le classement est **cohérent** (transitif), contrairement à la
matrice à budget de nœuds.

### L'échelle `UCI_Elo` est-elle une échelle de force *ici* ?

C'est le test qui avait annulé le « 1417 ». Testé sur les 15 paires de
l'échelle `UCI_Elo` : **aucune inversion significative** (le nominally
le plus fort bat toujours le nominally le plus faible). Sous un budget
**de temps**, l'échelle `UCI_Elo` de Stockfish se comporte donc comme
une échelle de force — ce qu'elle n'était pas sous un budget de nœuds,
où le handicap ne réduisait que le *choif* de la racine.

La relation « Elo du champ » → « Elo nominal » n'est **pas linéaire**
(les ancrages s'écartent de la droite ajustée de 4 964 Elo) : le
handicap ne retire pas la force d'une façon linéaire en `UCI_Elo`. La
droite globale n'est donc pas exploitable ; l'estimateur retenu est
l'**interpolation locale** entre les deux ancrages qui encadrent le
moteur :

| moteur | position | estimation (échelle `UCI_Elo`) |
|---|---|---|
| `reckless` | entre `sf1900` (1900) et `sf3190` (3190) | **≈ 2249** |
| `mychess` | sous `sf1320` (1320) | **≈ 1296** (extrapolation) |

Le verdict est **robuste** : en comptant les 9 parties interrompues par
un dépassement de temps au lieu de les écarter, on obtient 2258 et 1293.

### Les défauts trouvés par le tournoi

Le tournoi a trouvé **deux défauts réels** de `mychess`, qu'aucun test
unitaire ne voyait :

1. **`0000` sur une position jouable** (corrigé, `src/uci.rs`). La
   partie 332 a été perdue par « coup illégal » : `mychess` avait
   **dépassé son temps**, fastchess lui a envoyé `stop` (pour qu'il
   cesse de réfléchir) et attendait son `bestmove` ; la recherche
   interrompue n'avait pas encore développé la racine, et le moteur a
   répondu `0000` — un **coup illégal** dans une position qui avait
   10 coups légaux. Le moteur répond maintenant un **coup légal** dès
   que la recherche n'a rien produit (`uci::fallback_move`), et `0000`
   reste la réponse correcte pour une position terminale. Deux tests
   de non-régression (`go nodes 1` ⇒ un coup légal ; position de mat ⇒
   `0000`).
2. **Dépassement du budget temps** (signalé, **non corrigé**). 7 parties
   sur 660 (1,1 %) impliquent `mychess` : 6 ont été perdues sur le temps
   (débordement de 204 à 356 ms) et la septième est la partie 332, perdue
   sur le `0000`. La cause est identifiée : l'échéance
   n'est testée que **tous les 64 playouts** (`BUDGET_POLL_EVERY`), donc
   le dernier lot peut dépasser d'autant. (2 autres parties ont
   dépassé le temps avec Stockfish — `sf1320` et `sf1900` : le phénomène
   n'est pas propre à `mychess`, mais il y est **3 fois** plus fréquent
   — 6 temps pour `mychess`/`mychess_ctrl` contre 2 pour Stockfish.)
   **Corriger ce point
   changerait le comportement de la recherche** (le nombre de playouts
   avant le test d'échéance), donc l'échelle gelée des phases 4 et 5 :
   c'est une décision de phase, pas un correctif de bug.

### Limites

* **Un Elo n'est pas une propriété d'un moteur, mais d'un protocole.**
  2249 et 1296 ne valent que pour « `movetime` 200 ms, book
  `eco_lichess`, sans adjudication ». Le « 1417 » annulé reste annulé :
  il mesure une autre position, dans un protocole dont l'ancre était
  invalide.
* L'estimation de `mychess` est une **extrapolation** sous le bas de
  l'échelle : il ne fait que 32 % contre `sf1320` (3,5 points de score
  en moyenne sur 11 parties), et le modèle le place **juste en dessous**
  de `sf1320`.
* La force de Stockfish en `UCI_LimitStrength` est calibrée par ses
  auteurs pour son propre contrôle de temps : la lire à 200 ms/coup
  reste une approximation.
* Un seul hôte, une seule session, 12 parties par paire : l'intervalle
  de confiance sur un score de paire est de ±14 points (1 σ), ce qui
  explique les 0,3 σ et 0,9 σ des contrôles.
* Le budget de 200 ms est **court** : les moteurs y sont en limite de
  profondeur, et le classement pourrait bouger à 1 s/coup.
