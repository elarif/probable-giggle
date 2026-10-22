# Logs SPRT — phase 4 (2026-10-22)

Copie **versionnée** des logs `fastchess` produits le 2026-10-22 (le
répertoire `prt/` est dans `.gitignore` ; ces copies sont la **preuve**
du gate Elo, comme le log d9 pour le gate M4).

Commande commune à tous les runs :

```bash
fastchess \
  -engine name=cand cmd="<phase4>/target/release/mychess" args="uci" \
  -engine name=base cmd="<phase3>/target/release/mychess" args="uci" \
  -each proto=uci nodes=<N> \
  -sprt elo0=0 elo1=5 alpha=0.05 beta=0.05 \
  -openings file=eco_lichess.epd format=epd order=random \
  -rounds 200 -games 2 -concurrency 5 -repeat \
  -resign movecount=3 score=600 -draw movenumber=40 movecount=8 score=10
```

| log | configuration | résultat |
|---|---|---|
| `sprt-phase4-2k-20261022T104552Z.log` | candidat = phase 4 (PUCT), base = phase 3 (UCT), 2 000 nœuds | 324 parties, 269-4-51, **+399,7 ± 50,9 Elo**, LLR 2,95 → **H1 accepté** |
| `sprt-phase4-2k-reverse-20261022T104656Z.log` | candidat = phase 3, base = phase 4, 2 000 nœuds (symétrie) | 316 parties, 2-262-52, −404,9 ± 49,1 Elo, LLR −2,95 → H0 accepté |
| `sprt-phase4-10k-20261022T104922Z.log` | 10 000 nœuds | 304 parties, 287-2-15, **+596,5 ± 90,9 Elo**, LLR 2,96 → **H1 accepté** |
| `sprt-phase4-40k-20261022T105145Z.log` | 40 000 nœuds | 316 parties, 274-2-40, **+450,4 ± 55,2 Elo**, LLR 2,95 → **H1 accepté** |
| `sprt-control-uct-vs-uct-2k-20261022T104807Z.log` | **contrôle** : phase 4 forcé `Policy=uct` contre phase 3, 2 000 nœuds | 400 parties, 98-98-204, 50,00 %, LLR −0,08 → les deux binaires sont de force identique à règle égale |
| `sprt-phase4-tc1s-20261022T110121Z.log` | **contrôle invalide** : TC 1+0.01 | 158/200 parties adjugées sur dépassement de temps → révèle le plancher de 50 ms du budget temps |
| `sprt-phase4-tc8s-20261022T111056Z.log` | contrôle temps-fixe, TC 8+0.08, 200 parties | 200 parties, 132-4-64, +263,4 ± 43,7 Elo, LLR 1,61 → inconclusive |
| `sprt-phase4-tc8s-extended-20261022T113438Z.log` | contrôle temps-fixe, TC 8+0.08, **400 rondes** | 350 parties, 256-10-84, **+303,3 ± 36,4 Elo**, LLR 2,95 → **H1 accepté** |

Le book d'ouvertures est le **remplacement** `eco_lichess.epd` (3 863
positions, `tools/gen_epd_openings.py`) : l'UHO officiel n'était pas
téléchargeable depuis ce poste. Les Elo sont donc **relatifs** à ce
book (cf. `ELO_LOG.md`).

Le contrôle « phase 3 contre phase 3 » (même binaire deux fois, 80
parties, 50,00 %, LLR −0,01) a été joué hors dépôt de log.
