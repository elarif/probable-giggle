# Duels AB-vs-AB — phase AB8b (2026-10-23)

Copie **versionnée** des journaux `fastchess` des duels de la phase
AB8b (le répertoire `prt/` est dans `.gitignore` ; ces copies sont la
**preuve** des duels).

## Les duels définitifs — 400 parties, SPRT gelé

Commande commune aux trois :

```bash
tools/duels_ab8b_sprt.sh
```

Protocole : même binaire (`target/release/mychess`), une seule option UCI
qui change, `movetime 200 ms` par coup (`fastchess st=0.200`,
`timemargin=200 ms`), livre ECO lichess en ordre aléatoire (graine
`20261023`), aucune adjudication, **400 parties** (200 rondes × 2
couleurs), concurrence 5, hôte 12 cœurs, machine calme (les trois
duels sont joués **en séquence**), SPRT **gelé** du dépôt
(`elo0=0 elo1=5 alpha=0.05 beta=0.05`).

| log | duel | résultat | Elo ± err | LLR |
|---|---|---|---|---|
| `ab-controle-sprt.log` | **contrôle** : `AbNullMove=1` contre `AbNullMove=1` | 182 V – 201 D – 17 N (47,62 %) | −16,5 ± 30,7 | −0,35 (−11,7 %) |
| `ab-nullmove-sprt.log` | `AbNullMove=1` contre `AbNullMove=0` | 147 V – 228 D – 25 N (39,88 %) | **−71,3 ± 32,1** | −1,27 (−43,2 %) |
| `ab-lmr-sprt.log` | `AbLmr=1` contre `AbLmr=0` | 178 V – 189 D – 33 N (48,62 %) | −9,6 ± 31,5 | −0,21 (−7,2 %) |

**Le contrôle d'abord** : les deux moteurs du contrôle sont le même
binaire avec la même option, le résultat doit être nul. Il l'est
(−16,5 ± 30,7, aucun faux positif) — et il révèle le **biais du
protocole**, de l'ordre de **−16 Elo** (0,5 σ). Tout écart se lit
*contre* ce biais.

**Le null-move est négatif** : l'intervalle [−103, −39] exclut zéro ;
rapporté au contrôle, l'effet est de **−54,8 ± 44,4 Elo** (1,2 σ).
Verdict : **ni acceptée** — le null-move n'est pas un gain de force, il
est **reverti** (défaut 0 ; le code était déjà à 0).

**Le LMR est neutre** : −9,6 ± 31,5, LLR −0,21. Il reste **reverti**.

Précision : 400 parties ⇒ **± 30 Elo** (contre ± 100 à 40 parties).

## Les duels de 40 parties (première passe)

Mêmes options, mais **40 parties** et **sans SPRT** (estimation d'Elo
seule) : `ab-nullmove.log` (19 V – 19 D – 2 N, −0,0 ± 122,8 Elo) et
`ab-lmr.log` (20 V – 19 D – 1 N, +8,7 ± 71,1 Elo). Ces deux duels
étaient **trop courts pour mesurer quoi que ce soit** : le même
protocole, rejoué sur une machine **chargée** (5 cœurs occupés par le
vieux run `st=200`, que l'arrêt avait laissé en vie), donnait −43,7 et
+34,9 Elo. Ils sont conservés pour la trace, pas comme preuve.

## Un journal invalide, conservé pour la trace

`ab-nullmove.INVALIDE-st200s.log` est le **premier** duel null-move,
lancé le 2026-10-23 à 03:00 et **tué** à 06:54 (4 parties finies sur
40). Il est **invalide** : `tools/duel_ab.sh` passait alors `--time 200`
directement à `fastchess -each st=`, or fastchess lit **`st=` en
secondes** (« `st=N` — Movetime in seconds ») : le budget était **1000×
trop long** (200 s par coup au lieu de 200 ms) et l'en-tête du journal
annonçait 200 ms. Le script convertit désormais les millisecondes en
secondes et journalise la conversion.

Voir `docs/ab/phase8-2026-10-23.md` §4.1.

## Le duel de la revue — la borne de quiescence (2026-10-23)

| log | duel | résultat | Elo ± err | LLR |
|---|---|---|---|---|
| `quiescence-borne.log` | **borne de quiescence** : 8 prises contre 64 prises | 69 V – 58 D – 73 N (49,00 %) | −6,95 ± 33,80 | −0,10 (−3,5 %) |

Protocole : **deux binaires du même moteur** (`/tmp/mychess-q8` contre
`/tmp/mychess-q64`), une seule différence : la constante
`MAX_QUIESCENCE_CAPTURES` (8 contre 64) ; `Search=alpha`,
`st=0.200` (200 ms par coup), `timemargin=200`, livre ECO lichess en
ordre aléatoire (graine 20261023), aucune adjudication, **200 parties**,
concurrence 10, SPRT `elo0=0 elo1=5 alpha=0.05 beta=0.05`.

C'est un duel **interne** : il ne dit rien de la force **absolue** de
l'alpha-beta (l'adversaire externe, CAPS-LiTE, est hors service), mais
il mesure **exactement** l'effet de la borne — le seul changement de la
revue de correctness qui modifie ce que le moteur joue. Le verdict est
**neutre en force**, pour **−35 % de nœuds**.

Voir `docs/revue-2026-10-23.md` §5.1.
