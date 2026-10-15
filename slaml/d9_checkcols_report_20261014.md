# Rapport réconciliation d9 — colonnes checks détaillées (2026-10-14)

## État

M4-par terminé `end: 2026-10-14T11:40:58+Z, exit=0` (400/400 shards,
3 kills + resume ckpt). `reconcile.sh` = **7/9 colonnes** :

| colonne | mychess d9 | CPW d9 | verdict |
|---|---|---|---|
| Nodes | 2,439,530,234,167 | idem | OK exact |
| Captures | 125,208,536,153 | idem | OK exact |
| E.p. | 319,496,827 | idem | OK exact |
| Castles | 1,784,356,000 | idem | OK exact |
| Promotions | 17,334,376 | idem | OK exact |
| Checks | 36,095,901,903 | idem | OK exact |
| **Discovery Checks** | **37,103,750** | 37,101,713 | **DIFF +2,037** |
| **Double Checks** | **6,315,946** | 5,547,231 | **DIFF +768,715** |
| Mates | 400,191,963 | idem | OK exact |

Référence CPW re-vérifiée *live* sur
https://www.chessprogramming.org/Perft_Results (2026-10-14) : la
transcription de `slaml/cpw_d9_reference.md` est fidèle. NB la page
elle-même porte une note analogique pour Kiwipete d5 (« double-checks
may be 2645 instead of 2637 ») : la communauté sait ces colonnes
fragiles.

## Éléments d'analyse

1. **Total Checks conservé.** Σ checks = 36,095,901,903 identique des
   deux côtés : l'ensemble « feuilles avec ≥1 échec » est le même. Ce
   qui diffère est la *classification* de ce même ensemble, pas le
   movegen (Nodes/Captures/Ep/Castles/Promo/Mates exacts).
2. **La divergence est d9-spécifique côté hypothèse ép.** d7 (18,026 /
   1,628) et d8 (847,039 / 147,215) : validés T2 (à re-confirmer par
   run) — cf tests/perft/startpos.txt lignes d7/d8. Aucune des
   géométries suspectes (échec double par prise ep : la tour révélée
   sur la rangée + le pion prenant attaquant) n'est atteignable depuis
   la startpos en ≤8 plies d'après la structure des ouvertures — une
   divergence de *convention* sur ce pattern serait invisible à d≤8 et
   explose à d9.
3. **Amplitude cohérente avec ép.** +768,715 = 0.24 % des 319,496,827
   feuilles ep — ordre de grandeur plausible pour « ep-capture double
   échec ». Le +2,037 sur discovery (0.0055 %) est un autre pattern
   rare (à identifier ; candidats : conventions castling-check ou ep).
4. **Définition en jeu.** Convention typiques divergentes :
   (a) un double-échec ep est-il aussi compté en « discovery » ?
   (b) un échec de Roi-roque (le Roi dégage, la Tour arrive) est-il
   « discovered » ou « direct » ? (c) la colonne CPW d9 a-t-elle été
   produite par un générateur ignorant les doubles par ep ?

## Plan de localisation (avant toute levée de freeze)

1. **Micro-FEN ép-double-échec** : position minimale connue (tour
   alignée + prise ep) ; audit manuel + `perft --detailed 1` des deux
   côtés de la convention ; croiser avec python-chess.
2. **Bisect d9** : `divide --depth2`-style sur les 20 racines avec
   compteurs détaillés → isoler les sous-arbres portant les écarts ;
   descendre jusqu'au pattern minimal.
3. **Vieille profondeur (d5–d6) sur FENs construits** atteignant le
   pattern suspect en 6–8 plies : si la convention diffère *aussi* là,
   le bug/convention se manifestera aux profondeurs basses sur FEN
   dédié (sinon il est strictement d9-reachable).
4. Verdict : bug mychess → fix movegen/checks (et re-run shards) ;
   erratum/convention CPW → « Documented deviations » + exemption des
   2 colonnes dans `reconcile.sh` (avec justification en commentaire).

## Conséquence immédiate

BENCH-FREEZE **maintenu** (reconcile exit 1). Pas de mesure Elo avant
verdict (les compteurs détaillés alimentent les priors A2 —
cf. SELFPLAY.md).