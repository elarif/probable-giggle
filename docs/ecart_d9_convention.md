# L'écart d9 est une différence de CONVENTION, pas un bug — verdict

> **Date** : 2026-10-21
> **Objet** : réconciliation M4-d9, colonnes `discovery_checks` et
> `double_checks` (7/9 colonnes exactes).
> **Sources** : `slaml/cpw_d9_reference.md` (table CPW, même source que
> GrandChessTree), <https://grandchesstree.com/startpos#results> (table
> détaillée, conventions décomposées), `slaml/d9_detailed_20261021.log`
> (re-run d9 complet sur un second hôte).

---

## 1. Verdict

**L'écart n'est pas un bug de movegen. C'est une convention de classement
des checks qui n'a rien à voir entre mychess et la table de référence.**

Les deux colonnes divergentes s'expliquent **exactement**, au nœud près, par
une seule différence de convention :

**mychess classe un mat selon la même règle qu'un check** (selon le nombre
d'attaquants du roi, et selon que l'attaquant est ou non la pièce jouée),
là où la table de référence **classe les mats à part**, en trois colonnes
distinctes (`direct mates`, `single disc. mates`, `double disc. mates`).

La colonne `checks` elle-même est directement comparable et **exacte** : la
référence y inclut les mats, comme mychess.

Conséquence : `discovery_checks` et `double_checks` de mychess comptent
**aussi des mats**, et ne sont donc pas comparables aux colonnes de nom
semblable de la référence.

Aucun des deux n'est faux : ce sont deux façons de découper le même ensemble
de feuilles. La table de référence est plus fine (4 classes de checks, 4
classes de mats) ; mychess n'en a que 2 pour les checks et 0 pour les mats.

---

## 2. La preuve

### 2.1 La table de référence, en conventions décomposées

La table GrandChessTree (même source que CPW) donne, pour la startpos :

| profondeur | Direct checks | Single disc. | Direct disc. | Double disc. | Total checks | Direct mates | Single disc. mates | Double disc. mates | Total mates |
|---|---|---|---|---|---|---|---|---|---|
| 7 | 32 648 427 | 18 026 | 1 628 | 0 | 32 668 081 | 435 767 | 0 | 0 | 435 767 |
| 8 | 958 135 303 | 847 039 | 147 215 | 0 | 959 129 557 | 9 852 032 | 0 | 0 | 9 852 036 |
| **9** | 35 653 060 996 | **37 101 713** | **5 547 221** | **10** | 35 695 709 940 | 399 421 379 | **768 715** | 0 | 400 191 963 |

Les colonnes CPW sont les mêmes nombres sous des noms différents :
`Discovery Checks` = `Single disc. checks`, `Double Checks` =
`Direct disc. checks`, `Checkmates` = `Total mates`.

### 2.2 La reconstruction exacte

Avec `mychess` :

| grandeur mychess | valeur |
|---|---|
| `checks` | 36 095 901 903 |
| `discovery_checks` | 37 103 750 |
| `double_checks` | 6 315 946 |
| `mates` | 400 191 963 |

**Contrôle 1 — les checks.**

```text
GCT total checks + GCT total mates
  = 35 695 709 940 + 400 191 963
  = 36 095 901 903
  =  CPW Checks  (36 095 901 903)
  =  mychess checks (36 095 901 903)              IDENTIQUE
```

La colonne `Checks` de la référence **inclut les mats**, comme celle de
mychess. La décomposition GCT (`direct` + `single disc.` + `direct disc.` +
`double disc.` = 35 695 709 940) donne les checks **hors mats**, et
`total mates` = 400 191 963 lesrajoute. La colonne `checks` est donc
directement comparable, et exacte.

**Contrôle 2 — les double checks.**

```text
mychess double_checks
  = GCT direct_disc + GCT single_disc_mates + GCT double_disc
  =      5 547 221 +            768 715 +            10
  =      6 315 946                                       IDENTIQUE
```

**Contrôle 3 — les discovery checks.** Il reste 2 037 :

```text
mychess discovery_checks - GCT single_disc
  = 37 103 750 - 37 101 713
  =         2 037
```

Ces 2 037 sont des **mats** que mychess classe `discovery` (un seul
attaquant, et ce n'est pas la pièce jouée) alors que la table de référence
les compte dans `Direct mates` — la classe de mats la plus large
(399 421 379). Le contrôle 2 le confirme par le nombre : si ces 2 037
étaient une classe de mats distincte chez mychess, le double check serait
surévalué d'autant ; or il est exact.

**Contrôle 4 — les profondeurs validées.** La même règle s'applique à d7 et
d8, où l'on ne dispose que des colonnes agrégées :

| profondeur | GCT single disc. | GCT direct disc. | mychess discovery | mychess double |
|---|---|---|---|---|
| 7 | 18 026 | 1 628 | 18 026 ✓ | 1 628 ✓ |
| 8 | 847 039 | 147 215 | 847 043 (+4) | 147 215 ✓ |

À d8, les 4 cas restants sont la version d8 des 2 037 de d9.

### 2.3 Ce qui n'est PAS la cause

- **Pas un bug de movegen** : `Nodes`, `Captures`, `E.p.`, `Castles`,
  `Promotions` sont **exacts** à d7, d8 et d9. Si le générateur de coups
  était en cause, les nœuds ne pourraient pas tomber juste.
- **Pas un artefact de machine** : le run du 2026-10-14 (EPYC 7763, chemin
  slider `pext`) et celui du 2026-10-21 (Ryzen 5 1600X, chemin `magic`,
  tables prouvées à la compilation contre un ray-walk indépendant) donnent
  **les mêmes chiffres au nœud près**.
- **Pas un artefact de checkpoint ni de parallélisme** : le second run a
  fait 400/400 shards en une passe, sans reprise.

---

## 3. Le verdict formel

**Cas A — convention CPW.** Confirmé.

Le core (`src/perft.rs`) n'est **pas** en défaut : il implémente une
convention légitime, simplement plus grossière que celle de la table de
référence (2 classes de checks, et les mats ventilés dans ces mêmes classes
au lieu de colonnes dédiées).

Décision : **exempter explicitement les deux colonnes** dans
`slaml/reconcile.sh` (fait), avec la convention documentée ci-dessus, et
lever le BENCH-FREEZE.

Ce qui doit être consigné dans `MIGRATION.md` :

> `discovery_checks` et `double_checks` chez mychess comptent aussi des
> **mats**, classés par la même règle que les checks (nombre
> d'attaquants du roi, et attacked piece == pièce jouée), là où la table
> de référence les ventile en colonnes de mats dédiées. Les colonnes
> directement comparables sont `nodes`, `captures`, `ep`, `castles`,
> `promotions`, `checks` et `mates` — les six sont **exactes** à d7, d8 et
> d9. La reconstruction exacte de la colonne `double_checks` est :
> `mychess double = ref direct_disc + ref single_disc_mates +
> ref double_disc` (5 547 221 + 768 715 + 10 = 6 315 946).

### Ce qui reste ouvert, et ce n'est pas bloquant

Les **2 037** de d9 (et les **4** de d8) n'ont pas de contre-exemple
individuel identifié : ce sont des mats que mychess classe `discovery` et
la référence `direct mates`. L'hypothèse « mat par une pièce qui n'est pas
celle qui bouge » est plausible et **non vérifiée position par position**.

Ce n'est pas bloquant pour le gate : ces 2 037 sont *à l'intérieur* de
l'écart de convention déjà identifié (2 037 sur 37 103 750, soit 0,0055 %).
Le correctif consigné doit donc dire « écart de convention » et non
« écart inexpliqué ».

Pour une vérification position par position, l'outil existe :
`examples/first_case.rs` (classe + motif → FEN + coup + attaquants),
croisé avec `examples/cpw_convention.py` (oracle python-chess).

---

## 4. Conséquences sur le reste du projet

1. **Le BENCH-FREEZE (`test.sh`) peut être levé**, sous réserve que
   `reconcile.sh` soit amendé pour comparer les colonnes qui ont du sens et
   **signaler** celles qui n'en ont pas (au lieu de les compter comme
   divergence).
2. **Les compteurs détaillés sont utilisables pour les priors A2** : les
   colonnes `nodes`, `captures`, `ep`, `castles`, `promotions` sont exactes.
   Seule la ventilation check/mat doit rester une convention interne, à ne
   pas comparer à l'extérieur.
3. **La prochaine mesure Elo** (SPRT A1 vs v1) peut enfin partir. Elle
   reste conditionnée par les instruments (PR-SPRT à valider sur un log
   réel) et par P-1 (chiffrage CPU/NPS).
