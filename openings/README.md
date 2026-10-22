# Openings books (SPRT)

Officiels : `uho_lichess_4852_v1.epd` (UHO suite, positions déséquilibrées —
télécharger depuis les releases lichess/openings ou chess2uho ; ~4 800 lignes)
ou `chess.epd`. NE PAS utiliser `builtin_min.epd` pour un SPRT officiel
(≈8 positions : chaque position sera jouée des dizaines de fois → biais
d'apprentissage d'ordre, intervalles gonflés).

`builtin_min.epd` existe pour les smoke-tests de chaîne (fastchess branché,
bestmove consommés) UNIQUEMENT.

## Book de remplacement (2026-10-22)

`uho_lichess_4852_v1.epd` n'était **pas téléchargeable** depuis le poste de
mesure du 2026-10-22 (ni l'API GitHub de code search, ni les miroirs
connus). Pour ne pas rester bloqué, l'échelle Elo de la phase 4 a été
jouée sur un book **de remplacement**, produit par :

```bash
python3 tools/gen_epd_openings.py /tmp/eco_lichess.epd
```

Le script télécharge le jeu de données ECO de lichess
(`lichess-org/chess-openings`, volumes `a.tsv`..`e.tsv`), rejoue chaque
ligne de coups depuis la position initiale et écrit la position en **EPD**
(4 champs, « en passant » rempli seulement s'il est jouable). Résultat :
**3 863 positions** d'ouvertures standard, une par variation ECO.

Ce book est **différent** de l'UHO (positions volontairement
déséquilibrées) : tout Elo mesuré avec lui est **relatif** à ce book et
doit être cité comme tel. Le book officiel doit être re-téléchargé puis
l'échelle rejouée pour une publication.
