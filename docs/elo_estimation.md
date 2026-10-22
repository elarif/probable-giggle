# Déterminer un Elo approché du moteur

> Date : 2026-10-22 · outil : `tools/elo_estimate.sh` · ancre : Stockfish
> en force limitée (`UCI_LimitStrength` + `UCI_Elo`).

## 0. La question mal posée, et sa version correcte

« Quel est l'Elo de mon moteur ? » n'a pas de réponse : **un Elo n'est pas
une propriété du moteur, c'est une propriété d'un protocole** (le contrôle
de temps ou le budget de nœuds, le livre d'ouvertures, le format
d'adjudication). Le même binaire vaut 1200 à 4 000 nœuds/move et 1900 à
10 s/move.

La question utile est donc double :

1. **à quel niveau se place le moteur**, sur une échelle reconnue
   (celle de Stockfish, l'ancrage le plus calibré qui existe) ?
2. **dans quel protocole** ce placement a-t-il été mesuré ?

Ce document décrit la méthode retenue pour (1) et (2), l'outil qui la met
en œuvre, et les autres voies possibles avec leur coût.

## 1. La méthode retenue : calibration par force limitée

### Le principe

Stockfish sait **limiter volontairement** sa force et s'auto-déclare :

```text
setoption name UCI_LimitStrength value true
setoption name UCI_Elo value 1320        # 1320 .. 3190
```

L'échelle `UCI_Elo` est calibrée par les développeurs de Stockfish : un
Stockfish réglé à 1600 joue « comme un joueur de 1600 » (dans les
conditions de calibration). On mesure donc le score du moteur contre
Stockfish à plusieurs niveaux, et on lit le **niveau où le score passe
à 50 %** : c'est l'Elo du moteur sur l'échelle Stockfish.

C'est la méthode la plus employée pour estimer la force d'un moteur
faible : elle ne demande qu'**une seule** ancre, dont la force est
connue et reproductible, au lieu d'un ladder de moteurs dont les cotes
publicées sont hétérogènes (CCRL, CEPL, lichess) et souvent périmées.

### Le protocole

* **ancre** : Stockfish 19 (`sf_19`), une seule binaire,.limit strength ;
* **équité du budget** : les deux moteurs reçoivent **le même** budget —
  soit le même nombre de nœuds par coup, soit le même TC. C'est ce qui
  rend la mesure reproductible d'un hôte à l'autre : à budget de nœuds
  égal, la vitesse de l'hôte n'entre pas dans le résultat ;
* **livre** : le book local (ECO lichess, 3 863 positions) — le même que
  l'échelle SPRT du dépôt, pour que les deux mesures soient
  comparables ;
* **adjudication** : les règles du dépôt (`-resign movecount=3 score=600
  -draw movenumber=40 movecount=8 score=10`) ;
* **échantillon** : 100 parties par niveau (50 rondes × 2 couleurs).

### L'outil

```bash
# la calibration complète (5 niveaux x 100 parties, 40k noeuds/move)
tools/elo_estimate.sh target/release/mychess \
    ~/.cache/mychess/sf/stockfish/stockfish-linux-x86-64-universal \
    --levels 1320,1450,1600,1750,1900,2050 --games 100 --nodes 40000

# en temps fixe (le regime qui interesse un joueur)
tools/elo_estimate.sh target/release/mychess <stockfish> \
    --levels 1320,1450,1600,1750,1900 --games 100 --tc 10+0.1
```

Le script écrit un log `fastchess` par niveau, puis interpole la
croisée à 50 % (l'Elo est linéaire en `logit` du score) et propage
l'écart-type binomial de chaque score dans l'échelle Elo : la sortie
donne le niveau de croisement **et son intervalle**.

### Les pièges rencontrés (ils sont dans le script, pour mémoire)

* `args="uci"` **casse** Stockfish : il interprète un argument de ligne
  de commande comme un fichier à charger, sort avant le handshake, et
  fastchess le déclare « not responsive » ;
* `-each proto=uci nodes=40000` doit être **deux arguments** distincts ;
  regroupés, fastchess échoue (« Unsupported protocol ») et le script
  attend à l'infini ;
* le journal de parties de fastchess est sur **stdout** (l'option `-log`
  est le log de debogage) ;
* fastchess doit tourner avec `stdin` sur `/dev/null`.

## 2. Le résultat mesuré (2026-10-22)

Moteur : `main` après la phase 5 (donc le comportement de la phase 4,
`LeafQ = 0`, plus le correctif du budget temps). Protocole : **40 000
nœuds/move pour les deux moteurs**, book ECO lichess, 100 à 200 parties
par niveau, 9 niveaux.

| `UCI_Elo` SF | 1320 | 1350 | 1400 | 1425 | 1450 | 1600 | 1750 | 1900 | 2050 |
|---|---|---|---|---|---|---|---|---|---|
| score | 62,5 % | 55,8 % | 57,5 % | **46,8 %** | 33,0 % | 20,0 % | 12,5 % | 6,5 % | 2,5 % |

> **`mychess` ≈ 1417 ± 19 Elo** (échelle `UCI_Elo` de Stockfish)

Les deux estimateurs du script : l'ajustement global pondéré donne 1403
mais son intervalle est gonflé (la force sature aux niveaux hauts) ;
l'**estimateur local**, qui n'utilise que les deux niveaux encadrant la
croisée à 50 % (1400 et 1425), donne **1417 ± 19**. C'est lui qui est
retenu. Détail et journaux : `docs/elo/`.

## 3. Les autres voies, et leur coût

| voie | principe | coût | limite principale |
|---|---|---|---|
| **calibration Stockfish** (retenue) | un niveau SF à force limitée | 1 binaire + ~15 min de match | l'échelle `UCI_Elo` est calibrée sur le TC de SF : sous un budget de nœuds, c'est une approximation (±50 Elo ?) |
| **ladder / gauntlet** | contre des moteurs à Elo **publié** (CCRL, CEPL, lichess) | 5 à 10 binaires, un book UHO, plusieurs heures | les cotes publiées sont hétérogènes et périmées ; le book UHO n'est pas téléchargeable depuis ce poste |
| **PR-SPRT sur les matchs du dépôt** | magnitude avec IC de l'écart de Elo | `tools/pr_sprt.py` (branche `feature/selfplay-instrumentation`) | ne donne qu'un **écart** : il faut une ancre pour l'absolu |
| **bot lichess** | rating Glicko sur le pool lichess | le bot existe déjà (`lichessbot/`) | ±100 Elo à 50 parties, Glicko ≠ Elo UCI, et le rating dépend du TC et des adversaires humains |
| **self-play** (P5) | classement interne | le pipeline self-play | mesure la **cohérence** de la boucle, pas une force absolue |
| **Tablebases / puzzle** | forcing tactics | rapide | ne dit rien de la force globale |

**La combinaison qui donne le mieux** : la calibration ci-dessus pour
l'absolu, et le PR-SPRT du dépôt pour la **précision des écarts** entre
phases. Les deux se complètent : l'un place, l'autre mesure les
amplitudes.

## 4. Les réserves (à lire avec le chiffre)

1. **Le book est déséquilibré** : le book de remplacement (ECO lichess)
   contient des positions volontairement déséquilibrées. L'Elo absolu
   dépend donc du book : c'est une mesure *relative* à ce book, comme
   tous les Elo du dépôt depuis la phase 4. Rejouer sur l'UHO officiel
   (lorsqu'un poste peut le télécharger) pour publier.
2. **L'hôte n'est pas l'EPYC de référence** : à budget de nœuds égal
   cela n'affecte que le *temps* de mesure, pas le résultat. En TC
   fixe, c'est une source de biais.
3. **La force limitée de SF n'est pas SF** : un SF bridé joue
   différemment d'un joueur de 1600 (il « se trompe » de façon
   calibrée, pas humaine). L'estimation est donc un **ordre de
   grandeur**.
4. **Le protocole domine le chiffre** : l'Elo à 4 000 nœuds/move et
   l'Elo à 10 s/move diffèrent de plusieurs centaines de points. Toute
   citation d'Elo hors de son protocole est trompeuse.

## 5. Reproduction

```bash
# l'ancre
mkdir -p ~/.cache/mychess/sf && cd ~/.cache/mychess/sf
curl -sL -o sf.tar.gz \
  https://github.com/official-stockfish/Stockfish/releases/download/sf_19/stockfish-linux-x86-64-universal.tar.gz
tar xzf sf.tar.gz && chmod +x stockfish/stockfish-linux-x86-64-universal

# la calibration
tools/elo_estimate.sh target/release/mychess \
    ~/.cache/mychess/sf/stockfish/stockfish-linux-x86-64-universal \
    --levels 1320,1450,1600,1750,1900,2050 --games 100 --nodes 40000
```

Les journaux `fastchess` sont écrits dans `prt/elo/` (hors dépôt) ; le
résumé est dans `ELO_LOG.md`.
