# ELO_LOG.md — journal match/SPRT (futur)

hardware: **AMD EPYC 7763** (Milan, Zen3 family `0x19`, 4C/8SMT, ~2.45 GHz,
BMI2, pas d'AVX-512) — hyperviseur partagé : les mesures nps vivent dans
MIGRATION.md avec des fourchettes larges, le CI n'exige JAMAIS de seuil nps.

Équipes de test prévues (phase MCTS) :
- fastchess SPRT concurrency = **4–6** (4 cœurs physiques : ne pas saturer
  les 8 SMT pour ne pas biaiser les temps de réflexion),
- CPU-only → **Tier D** (pas de GPU dans la chaîne),
- A/B/A/B intercalé même session, jamais cross-jour.

## Entrées

*(aucune partie jouée à ce jour — le cœur n'a ni recherche ni évaluation ;
ce journal accueillera SPRT/fix-ply/Match dès la phase MCTS)*

| date | version | adversaire | conditions | résultat |
|---|---|---|---|---|
| — | — | — | — | — |

## Baseline perf core (GATE 2.5, tag `vcore-1.0`, EPYC 7763 — pré-phase MCTS)

Lignes de performance core consignées pour la phase MCTS (benchs criterion
`base25`=pré-M2 → `final25`=frozen ; détail des A/B par lot : MIGRATION.md) :

| bench (release, criterion) | pré-M2 (`base25`) | frozen (`final25`) |
|---|---|---|
| count_moves/startpos | 78.7 ns | 70.7 ns |
| count_moves/kiwipete | 103.1 ns | 90.7 ns |
| count_moves/pos3 | 47.9 ns | 40.2 ns |
| count_moves/pos6 | 85.3 ns | 74.8 ns |
| perft1t/startpos_d6 | 544.3 ms (219 Mnps) | 463.2 ms (257 Mnps, ×1.17) |
| perft_bulk/kiwipete_d2_2039 | 6.25 µs | 5.55 µs |
| perft_bulk/kiwipete_d3_97862 | 249.0 µs | 247.3 µs |
| make_unmake/{startpos..pos6} | 22.5–24.5 ns | 22.9–24.2 ns (inchangé) |
| gives_check_x8 (÷8/appel) | ~24.6 ns | ~25.0 ns (<80 contrat ✓) |
| mcts_descend30 (÷30/ply, hash inclus) | 21.3 ns | 22.2 ns (<25 contrat ✓) |
| mcts_rep_walkback16 zero/hit4 | 8.28 / 1.32 ns | 8.57 / 1.34 ns |
| mcts_pst_eval/startpos | 32.7 ns | 39.2 ns |
| perft(7) 1T sweep | ~206–210 Mnps | **272 Mnps (×1.32)** |
| perft(7) 8T sweep | 949–984 Mnps | **1089–1113 Mnps (×~1.13)** |

Lots GATE 2.5 M2 : **L1b no-gain** (+0.1 % d6 startpos, conservé
robustesse), **L1c GAIN ×1.19** (fusion checkers/pins/danger, adopté),
**L1d no-gain** (−1.8 % perft1t vs L1c, conservé : count_moves −5…−16 %
p<0.05). M1 : les deux micro-benchs « incohérents » étaient des artefacts de
batch (aucun fix d'impl nécessaire). M3 : contrat position illégale
(non-émission du roi-capture, assert debug, test dédié).