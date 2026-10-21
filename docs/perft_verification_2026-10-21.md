# Vérification Perft vs chessprogramming wiki — 2026-10-21

Source des nombres : https://www.chessprogramming.org/Perft_Results
(Récupération en lecture seule du wiki hébergée par lichess.org — contenu = tables révisées du wiki.)

## Méthode

- Tests golden : `cargo test --release --test perft_golden` (tiers T1 : d≤5 nœuds + colonnes
  détaillées d≤4 + recoupement vs oracle naïf `refgen` d≤3).
- Profondeurs 6–8 : binaire CLI `mychess perft <fen> <d>` (mono) et
  `mychess sweep <fen> <d> <threads>` (`perft_parallel`), comparés aux valeurs de la page.
- 7 positions : startpos, kiwipete, pos3, pos4, pos4 miroir (variante publiée par la page,
  mêmes perft), pos5, pos6.

## Résultats

| Position   | Depth | mychess        | Page          | Statut | Via |
|------------|-------|----------------|---------------|--------|-----|
| startpos   | 6     | 119,060,324    | 119,060,324   | ✅     | T1 + CLI |
| startpos   | 7     | 3,195,901,860  | 3,195,901,860 | ✅     | CLI |
| startpos   | 8     | 84,998,978,956 | 84,998,978,956| ✅     | CLI 4-threads |
| kiwipete   | 6     | 8,031,647,685  | 8,031,647,685 | ✅     | CLI |
| pos3       | 6     | 11,030,083     | 11,030,083    | ✅     | CLI |
| pos3       | 7     | 178,633,661    | 178,633,661   | ✅     | CLI |
| pos3       | 8     | 3,009,794,393  | 3,009,794,393 | ✅     | CLI |
| pos4       | 6     | 706,045,033    | 706,045,033   | ✅     | CLI |
| pos4 miroir| 6     | 706,045,033    | (mêmes perft) | ✅     | CLI |
| pos5       | 1–5   | 44 / 1,486 / 62,379 / 2,103,487 / 89,941,194 | idem | ✅ | T1 |
| pos6       | 6     | 6,923,051,137  | 6,923,051,137 | ✅     | CLI |
| pos6       | 7     | 287,188,994,746| 287,188,994,746 | ✅   | CLI 4-threads |

Colonnes détaillées (captures, e.p., roques, promotions, échecs, discovery, double, mates)
exactes d1–d4 sur toutes les positions (tests T1, goldens = tables de la page, pos4 : la page
ne publie pas discovery/double).

Total validé cette session : ≈ 394 milliards de nœuds de feuilles, zéro écart.
Suite complète `cargo test --release` : tous verts (0 failed).

## Notes

- Kiwipete d5 double-checks : page publie 2637, avec note que le décompte Talkchess discute
  2645 (ambiguïté de définition). Valeur 2637 intégrée au golden (testée uniquement si
  les détails d5 sont activés un jour ; t1_details s'arrête à d4).
- d9 startpos (2.44 T nœuds) : hors de portée d'une session, infrastructure dédiée déjà
  présente dans `slaml/` (shards + checkpoints + `cpw_d9_reference.md`).
- Performance observée (EPYC, 4–8 thr, bulk-counting) : 85–146 M feuilles/s mono,
  320–500 M/s parallèle ; `count_moves` ≈ 3 M/s par thread à ces profondeurs.

## Hors périmètre ici

- `pos6 d8` (11.92 T) et `pos6 d9` (490 T) : la page les publie mais `count_moves` × 287 B
  opérations ≈ dizaines d'heures sur cette machine — exclus du golden (opt-in machine
  rapide uniquement).