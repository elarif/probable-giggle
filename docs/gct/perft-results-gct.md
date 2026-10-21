# Table de perft détaillée Grand Chess Tree — extraction et conventions

Source : https://grandchesstree.com/perft-results (page « Perft Results »),
table JSON `assets/data/homepage_perft.json`, extraite le 2026-10-21.

Trois positions sont publiées : `startpos`, `kiwipete`, `sje`
(SJE's Symmetric Alternative).

## Colonnes publiées

`nodes`, `unique`, `captures`, `enpassants`, `castles`, `promotions`,
`checks`, `mates`.

## Conversions obligatoires avant comparaison avec `mychess`

`mychess` produit `PerftCounts { nodes, captures, ep, castles, promotions,
checks, discovery_checks, double_checks, mates }` via
`perft::perft_detailed`. Une seule colonne se convertit (`checks`), trois ne
sont pas comparables (`unique`, `discovery`, `double`) ; les six autres sont
des identités.

| Colonne GCT | Colonne `mychess` | Conversion |
|---|---|---|
| `nodes` | `nodes` | identité |
| `captures` | `captures` | identité |
| `enpassants` | `ep` | identité |
| `castles` | `castles` | identité |
| `promotions` | `promotions` | identité |
| `checks` | `checks` | **`mychess.checks = gct.checks + gct.mates`** |
| `mates` | `mates` | identité |
| `unique` | — | **aucune** : compte des positions *distinctes*, pas des nœuds |
| `discovery` (décomposition GCT) | `discovery_checks` | **non comparable** |
| `double` (décomposition GCT) | `double_checks` | **non comparable** |

La convention `checks` est vérifiée : sur la position initiale à d4, la table
GCT publie `checks = 461` et `mates = 8` ; `mychess` mesure `checks = 469`,
soit 461 + 8. Sur Kiwipete d3 : GCT `checks = 992`, `mates = 1`, `mychess`
mesure 993.

La décomposition `discovery` / `double` de la table GCT n'est pas celle de
`mychess` : voir `docs/ecart_d9_convention.md` (écart tranché comme une
différence de convention, pas un bug). Ces deux colonnes valent donc `-`
dans les goldens de `tests/perft/gct/`.

## FEN des goldens

La table GCT publie pour Kiwipete une FEN sans champs de demi-coup ni de
compteur. `Position::from_fen` exige le champ demi-coup (`parts[4]`) mais pas
le compteur de coups (`parts[5]`, optionnel et seulement validé s'il est
présent) : les goldens complètent ces champs (`0 1`), sans aucun effet sur le
perft.

## Portée retenue

Les goldens s'arrêtent à **d7** : au-delà, la table GCT devient inexploitable
en temps raisonnable sur l'hôte de développement (d8 = 84,9 Md de nœuds pour
startpos). Les profondeurs d0–d7 suffisent à valider le générateur, y compris
les colonnes détaillées.
