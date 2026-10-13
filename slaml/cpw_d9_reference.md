# CPW Perft_Results — référence de réconciliation d9 (récupéré 2026-10-13, lecture seule)

Source : https://www.chessprogramming.org/Perft_Results (startpos)
Les compteurs CPW considèrent « moves to the leaf positions only » — la même
sémantique que `perft_detailed` (compteurs comptés à la feuille).

## Initial Position — Depth 9

| colonne | valeur |
|---|---|
| Nodes | 2,439,530,234,167 |
| Captures | 125,208,536,153 |
| E.p. | 319,496,827 |
| Castles | 1,784,356,000 |
| Promotions | 17,334,376 |
| Checks | 36,095,901,903 |
| Discovery Checks | 37,101,713 |
| Double Checks | 5,547,231 |
| Checkmates | 400,191,963 |

Autres colonnes utiles (contrôles croisés plus courts si besoin) :
- d7 : Nodes 3,195,901,860 / Captures 108,329,926 / E.p. 319,617 / Castles
  883,453 / Promotions 0 / Checks 33,103,848 / Disc 18,026 / Dbl 1628 /
  Mates 435,767.
- d8 : Nodes 84,998,978,956 / Captures 3,523,740,106 / E.p. 7,187,977 /
  Castles 23,605,205 / Checks 968,981,593 / Disc 847,039 / Dbl 147,215 /
  Mates 9,852,036.

NB : toute colonne divergente = bug détaillé-compteurs à corriger AVANT
mesure d'élo (les compteurs détaillés alimentent les priors en A2).