# Duels AB-vs-AB — phase AB8b (2026-10-23)

Copie **versionnée** des journaux `fastchess` des duels de la phase
AB8b (le répertoire `prt/` est dans `.gitignore` ; ces copies sont la
**preuve** des duels).

Commande commune aux deux duels :

```bash
tools/duel_ab.sh --rounds 20 \
    --a "<option>=1" --b "<option>=0" \
    --out prt/rr/<nom>.log --pgn prt/rr/<nom>.pgn
```

Protocole : même binaire (`target/release/mychess`), une seule option UCI
qui change, `movetime 200 ms` par coup (`fastchess st=0.200`,
`timemargin=200 ms`), livre ECO lichess en ordre aléatoire (graine
`20261023`), aucune adjudication, 40 parties (20 rondes × 2 couleurs),
concurrence 5, hôte 12 cœurs.

| log | duel | résultat | Elo ± err |
|---|---|---|---|
| `ab-nullmove.log` | `AbNullMove=1` contre `AbNullMove=0` | 19 V – 19 D – 2 N (50,00 %) | **−0,0 ± 122,8** (LOS 50,0 %) |
| `ab-lmr.log` | `AbLmr=1` contre `AbLmr=0` | 20 V – 19 D – 1 N (51,25 %) | **+8,7 ± 71,1** (LOS 59,6 %) |

Les deux duels sont **non concluants** : le point estimate est plat et
l'intervalle de confiance recouvre zéro. À 40 parties, la précision
réelle est **± 70 à ± 120 Elo** (la variance est forte et la
proportion de nulles varie d'un duel à l'autre : 40 % ici, 75 % pour
le LMR), pas les ± 45 Elo qu'un calcul « 50 % de nulles » ferait
croire. Voir `docs/ab/phase8-2026-10-23.md` §4.1.

## L'avertissement que les chiffres donnent eux-mêmes

Le **même** protocole, rejoué sur une machine **chargée** (5 cœurs
occupés par le vieux run `st=200`, que l'arrêt avait laissé en vie),
avait donné **−43,7 ± 100,3** (null-move) et **+34,9 ± 95,3** (LMR) ;
sur machine calme (92 % idle), **−0,0** et **+8,7**. L'écart entre les
deux exécutions (≈ 44 Elo) est du même ordre que l'écart que le duel
prétend mesurer : **à ce budget, le duel ne mesure rien**. Seul un
volume de parties bien supérieur (la phase 5 du MCTS en jouait 400
par point) ou un SPRT aurait une résolution utile.

## Un journal invalide, conservé pour la trace

`ab-nullmove.INVALIDE-st200s.log` est le **premier** duel null-move,
lancé le 2026-10-23 à 03:00 et **tué** à 06:54 (4 parties finies sur
40). Il est **invalide** : `tools/duel_ab.sh` passait alors `--time 200`
directement à `fastchess -each st=`, or fastchess lit **`st=` en
secondes** (« `st=N` — Movetime in seconds ») : le budget était **1000×
trop long** (200 s par coup au lieu de 200 ms) et l'en-tête du journal
annonçait 200 ms. Le script convertit désormais les millisecondes en
secondes et journalise la conversion.
