# Logs SPRT — phase 5 (2026-10-22)

Copie **versionnée** des logs `fastchess` produits le 2026-10-22 pour la
phase 5 (le répertoire `prt/` est dans `.gitignore` ; ces copies sont la
**preuve** du gate Elo, comme pour la phase 4).

Commande commune à tous les runs :

```bash
fastchess \
  -engine name=cand cmd="<phase5>/target/release/mychess" args="uci" option.LeafQ=<d> \
  -engine name=base cmd="<phase4-ref>/target/release/mychess" args="uci" \
  -each proto=uci nodes=<N>            # ou tc=8+0.08 / tc=1+0.01 \
  -sprt elo0=0 elo1=5 alpha=0.05 beta=0.05 \
  -openings file=eco_lichess.epd format=epd order=random \
  -rounds 200 -games 2 -concurrency 5 -repeat \
  -resign movecount=3 score=600 -draw movenumber=40 movecount=8 score=10
```

* **candidat** : le binaire phase 5 (option UCI `LeafQ` pilotée par
  `fastchess -engine option.LeafQ=<d>`) ;
* **référence** : la **phase 4 gelée** (worktree `.worktrees/phase4-ref`,
  commit `0abdff2`).

| log | configuration | résultat |
|---|---|---|
| `sprt-phase5-control-q0-vs-p4-20261022T141824Z.log` | **contrôle** : phase 5 `LeafQ=0` contre phase 4, 2 000 nœuds | 400 parties, 120-120-160, **50,00 %**, LLR −0,08 → les deux binaires sont de force identique |
| `sprt-phase5-q1-2k-20261022T141907Z.log` | `LeafQ=1`, 2 000 nœuds | 400 parties, 18-206-176, 26,50 %, **−177,2 ± 26,4 Elo**, LLR −2,83 → **H0** |
| `sprt-phase5-q2-2k-20261022T142016Z.log` | `LeafQ=2`, 2 000 nœuds | 400 parties, 39-170-191, 33,62 %, **−118,1 ± 23,3 Elo**, LLR −2,33 → **H0** |
| `sprt-phase5-q1-10k-20261022T142127Z.log` | `LeafQ=1`, 10 000 nœuds | 370 parties, 1-223-146, 20,00 %, **−240,8 ± 26,7 Elo**, LLR −2,95 → **H0 accepté** |
| `sprt-phase5-q2-10k-20261022T142443Z.log` | `LeafQ=2`, 10 000 nœuds | 400 parties, 27-194-179, 29,12 %, **−154,5 ± 24,9 Elo**, LLR −2,67 → **H0** |
| `sprt-phase5-q1-40k-20261022T142845Z.log` | `LeafQ=1`, 40 000 nœuds | 398 parties, 5-210-183, 24,25 %, **−197,9 ± 24,3 Elo**, LLR −2,95 → **H0 accepté** |
| `sprt-phase5-control-tc1s-20261022T144541Z.log` | **contrôle du budget temps** : TC 1+0.01, phase 5 `LeafQ=0` contre phase 4 | 300 parties, **0** adjugée sur dépassement pour la phase 5 contre **288** pour la phase 4 → le plancher de 50 ms est corrigé (ce match ne mesure pas la force) |
| `sprt-phase5-q1-tc8s-20261022T144748Z.log` | `LeafQ=1`, TC 8+0.08 (contrôle **temps-fixe**) | 360 parties, 1-229-130, 18,33 %, **−259,5 ± 31,0 Elo**, LLR −2,95 → **H0 accepté** |

Le book d'ouvertures est le **remplacement** `eco_lichess.epd` (3 863
positions, `tools/gen_epd_openings.py`) : l'UHO officiel n'était pas
téléchargeable depuis ce poste. Les Elo sont donc **relatifs** à ce
book (cf. `ELO_LOG.md`).
