# Phase « générateur de coups » — la comparaison et le correctif (2026-10-23)

*La question : « 125 ns comparé à 2 ns, c'est énorme » — et « compare
notre code à celui des moteurs ». Ce document est la réponse mesurée.*

---

## 1. La comparaison, d'abord (même machine, mêmes positions)

Référence : **`toanth/motors`** (`gears/src/games/chess/movegen.rs` +
`motors/src/search/chess/caps.rs`) — le moteur **CAPS-LiTE**, en Rust,
« > 3000 Elo » d'après son dépôt, avec l'évaluation **LiTE** (tunable).
Banc comparatif écrit dans leur dépôt (`gears/benches/movegen_cmp.rs`),
mêmes FEN que notre `benches/movegen.rs`.

| position | coups | **CAPS-LiTE** pseudo-légal | **CAPS-LiTE** + légalité par coup | **mychess** (légal par construction) |
|---|---|---|---|---|
| startpos | 20 | 85,0 ns (4,25 ns/coup) | 250,8 ns (12,5 ns/coup) | **99,5 ns (5,0 ns/coup)** |
| kiwipete | 48 | 135,5 ns (2,8 ns/coup) | 528,5 ns (11,0 ns/coup) | **187,8 ns (3,9 ns/coup)** |
| pos3 | 14 | 62,3 ns | 163,1 ns | **79,9 ns** |
| sje | 46 | 114,6 ns | 506,3 ns | **173,3 ns** |

Et le perft (génération + make/unmake), qui est la mesure de référence
de la communauté :

| | **mychess** | **CAPS-LiTE** |
|---|---|---|
| perft(4) startpos | **148-152 Mnps** | 14,7 Mnps |
| perft(4) kiwipete | **254,5 Mnps** | 14,8 Mnps |

**Deux conclusions, et une correction de la question :**

1. le « 2 ns » de la question est un coût **par coup**, comparé au
   coût **total** (125 ns pour 32 coups). Le comparable est
   **3,9-5,0 ns/coup pour nous**, **2,8-4,3 ns/coup** pour le cœur
   pseudo-légal de CAPS-LiTE ;
2. notre générateur est **10 à 17 fois plus rapide que CAPS-LiTE** en
   perft, et **2,5 à 2,9 fois plus rapide** que sa génération légale
   complète (leur architecture — pseudo-légal + test de légalité
   **par coup** — coûte 8 ns/coup de plus que la nôtre, qui est
   *légale par construction*).

Le générateur **n'était pas** le goulot. Le goulot, c'est la
recherche : nous générions **tous** les coups (35 tranquilles compris)
à **chaque** nœud.

---

## 2. Ce que la comparaison a fait perdre, et ce qu'elle a trouvé

En voulant ajouter un mode « coups tranquilles seuls » (le complément
du mode « prises seules »), on a dû **définir** ce que meant le mode
« prises seules ». La définition du dépôt était **bruyée** : il
émettait aussi les coups tranquilles du **roi** (aucun test dans la
boucle du roi) — et **tout le dépôt le compensait** (la quiescence du
MCTS filtre les coups « qui ne sont pas un échange »).

Corriger cette définition a **révélé deux défauts de la quiescence**
de l'alpha-beta, que le bruit masquait :

1. la liste des captures n'était **jamais vide** (le roi y mettait ses
   coups tranquilles) : la quiescence ne pouvait pas distinguer « pas de
   prise » de « **pat** », et rendait `0` — une **nulle** — pour toute
   position calme ;
2. plus grave, la valeur de l'enfant n'était **pas inversée**
   (`search.rs` : `self.quiescence(-beta, -alpha, ply + 1)` sans le
   `-`). La quiescence rendait donc la valeur **de l'adversaire** :
   une position perdante était notée **gagnante**.

Mesuré (position `k7/8/8/q7/8/4P3/8/4K3 w`, les blancs perdants d'une
dame) :

| | avant | après |
|---|---|---|
| score | **+820 cp** (faux) | **−820 cp** (juste) |
| nœuds (profondeur 4) | 5 141 | 2 865 |

Le seuil « 500 cp » de deux tests encodait ces valeurs fausses : les
tests ont été réécrits pour exprimer **l'intention** (la capture doit
être jouée), et deux tests de régression ont été ajoutés.

---

## 3. Le plan d'amélioration, tranche par tranche

| tranche | quoi | gate | verdict |
|---|---|---|---|
| **G1** | le mode « prises seules » n'émet plus les coups tranquilles du roi ; **spécialisation à la compilation** (`const TACTICAL_ONLY` / `const QUIET_ONLY`, comme `toanth/motors` et comme le `count_moves_impl` que le dépôt avait déjà) | perft + tests | **FAIT** |
| **G2** | les **deux défauts de la quiescence** (signe, pat/nulle) | tests de régression | **FAIT** |
| **G3** | **génération échelonnée** dans la recherche : le tactique d'abord, le tranquille à la demande (le découpage des moteurs à `MovePicker`) | débit + profondeur à 200 ms | **FAIT** |
| **G4** | le **match de 400 parties** : le gate (après contre avant), l'ancre externe (CAPS-LiTE), le MCTS contre CAPS-LiTE | Elo ± IC | **en cours** |

---

## 4. Les mesures

| | point de départ | + G1/G2 (quiescence) | + G3 (échelonnée) |
|---|---|---|---|
| alpha-beta à 200 ms | 3,31 Mnps, **profondeur 4** | 4,19 Mnps, profondeur 5 | **4,6 Mnps, PROFONDEUR 6** |
| ns par nœud | 298 | 239 | **203-226** |
| génération « tout » | 125 ns | 138-152 ns | 132-139 ns |
| captures seules | 86 ns (7,09 coups) | 83 ns (4,94 coups) | 84-88 ns (4,94 coups) |

À budget temps égal, le moteur gagne **deux plies** : une vient d'un
**défaut** (G2), une d'une **optimisation** (G3).

---

## 5. Ce qu'on retient

1. **une comparaison mal posée coûte cher** : « 125 ns contre 2 ns »
   comparait un total à un par-coup. La vraie question — « le
   générateur est-il un goulot ? » — a une réponse mesurée : non, il
   est **plus rapide** qu'un moteur de 3000 Elo ;
2. **un défaut peut se cacher derrière un autre** : le mode bruité du
   générateur masquait deux bugs de la quiescence, dont un qui rendait
   la valeur des positions calmes **inversée**. Le moteur mesuré à
   « 1296 Elo » jouait avec des feuilles inversées ;
3. **la génération échelonnée** (tactique d'abord) est le levier de
   débit du dépôt : le nœud de l'alpha-beta passe de 298 à ~210 ns.

## 6. La suite

* **l'évaluation** (le goulot restant : erreur ≈ 490 cp) ;
* le **null-move avec recherche de vérification** (le nôtre est le
  null-move naïf, celui qui perd des Elo) ;
* le **SMP** (l'alpha-beta est mono-thread).
