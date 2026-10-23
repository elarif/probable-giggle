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
| `ab-nullmove.log` | `AbNullMove=1` contre `AbNullMove=0` | 17 V – 22 D – 1 N (43,75 %) | **−43,7 ± 100,3** (LOS 18,7 %) |
| `ab-lmr.log` | `AbLmr=1` contre `AbLmr=0` | 21 V – 17 D – 2 N (55,00 %) | **+34,9 ± 95,3** (LOS 77,0 %) |

Les deux duels sont **non concluants** : l'intervalle de confiance
recouvre zéro dans les deux cas. À 40 parties, la précision réelle est
**± 100 Elo** (les parties sont très décisives : 55 % de nulles
seulement), pas les ± 45 Elo qu'un calcul « 50 % de nulles » ferait
croire. Voir `docs/ab/phase8-2026-10-23.md` §4.1.

## Un journal invalide, conservé pour la trace

`ab-nullmove.INVALIDE-st200s.log` est le **premier** duel null-move,
lancé le 2026-10-23 à 03:00 et **tué** à 06:54 (4 parties finies sur
40). Il est **invalide** : `tools/duel_ab.sh` passait alors `--time 200`
directement à `fastchess -each st=`, or fastchess lit **`st=` en
secondes** (« `st=N` — Movetime in seconds ») : le budget était **1000×
trop long** (200 s par coup au lieu de 200 ms) et l'en-tête du journal
annonçait 200 ms. Le script convertit désormais les millisecondes en
secondes et journalise la conversion.
