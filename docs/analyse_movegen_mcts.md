# Move generator et MCTS — analyse des 7 posts Grand Chess Tree

> **Date** : 2026-10-21
> **Objet** : lire la série « Let's build a move generator » comme une base
> possible pour un moteur MCTS, identifier ce qui manque au move generator
> pour maximiser les simulations, sans le spécialiser perft.
> **Sources** : les 7 posts (liens en fin de document), `src/movegen.rs`,
> `src/mcts.rs`, `src/position.rs`, `src/slider.rs`, `benches/core.rs`,
> `ELO_LOG.md`.
> **Mesures** : hôte local Ryzen 5 1600X (Zen1, 6C/12T), build release ;
> les chiffres absolus ne remplacent pas la baseline EPYC de `ELO_LOG.md`.

---

## 1. Verdict

1. **La série est une référence de correction, pas un move generator de
   moteur MCTS.** Elle optimise pour « produire tous les coups légaux,
   portable, avec perft comme oracle ». C'est exactement ce qu'il faut pour
   démarrer ; ce n'est pas le contrat d'un moteur qui doit enchaîner des
   millions de simulations.
2. **Le move generator n'est pas le goulot actuel de `mychess`.** Mesuré :
   `generate(All)` coûte 106-176 ns (startpos) à 186-250 ns (Kiwipete), pour
   ~2,1-2,5 µs par simulation à 1 s (3,1 µs à 6 s), soit **~5-12 % du temps**.
   Le goulot est la marche mémoire dans l'arbre : 528 MiB de RSS à 6 s,
   nps qui chute de 468 k à 327 k, ~70-80 cache-misses par simulation.
3. **Il manque six choses**, dont quatre touchent directement le move
   generator ou son API : une **terminalité rapide** (`has_legal_move`), un
   **buffer réutilisable**, des **métadonnées d'ordre/prior**, et un
   **hash/historique** (absent de la série, déjà présent chez toi). Les deux
   autres — **génération d'évasions/check info** et **NNUE/copy-make** —
   deviennent critiques dès que l'on ajoute des playouts, du qsearch ou un
   réseau.
4. **Le plafond d'une optimisation pure du move generator est ~5-12 % de
   simulations/s.** Le gain principal est ailleurs : arène plate +
   `Edge`/`Node` séparés (+20 à 50 %), puis parallélisme (×2 à 4). Il faut
   faire les deux, mais dans cet ordre : d'abord arrêter de gaspiller de la
   mémoire, ensuite micro-optimiser la génération.
5. **La série apprend les bonnes choses** : génération légale en amont
   (`checkMask`/`pinLines`/`kingDanger`), bitboards + mailbox, move 16 bits,
   magic bitboards, buffer fourni par l'appelant, make/unmake avec undo,
   perft comme invariant. Ce sont les fondations ; il ne faut pas les
   remplacer, il faut les compléter.

---

## 2. Ce que fait chaque post (et ce que ça coûte pour MCTS)

| Part | Contenu | Apport | Dette pour un moteur MCTS |
|---|---|---|---|
| [1](https://grandchesstree.com/move-generator/01-introduction.html) | pipeline, squelette perft, FEN → ASCII | oracle de correction | `Move[]` alloué par nœud ; perft comme seule métrique ; pas de hash |
| [2](https://grandchesstree.com/move-generator/02-board.html) | 12 bitboards + mailbox 8×8, FEN, impression | représentation saine, requêtes O(1) | `Position` est une classe + tableaux ; `RebuildOccupancy` ; pas de Zobrist ; pas de copy-make |
| [3](https://grandchesstree.com/move-generator/03-move-encoding.html) | move 16 bits (from 6 bits, to 6 bits, flag 4 bits), UCI | encodage compact et lisible | pas de score/ordre, pas de victime/attaquant, pas de bit « donne échec » |
| [4](https://grandchesstree.com/move-generator/04-pseudo-legal-moves.html) | pseudo-légal : pions, cavaliers, roi, sliders en ray-scan, castling | énumération complète, tables | ray-scan (remplacé en Part 6) ; pas de tri ; dame traitée deux fois ; pas de captures-only ni d'évasions |
| [5](https://grandchesstree.com/move-generator/05-make-move.html) | make/unmake, undo, `Validate()`, filtre make-then-check | application réversible, invariant vérifiable | `RebuildOccupancy()` à chaque make **et** unmake (12 ORs × 2) ; pas de hash ; pas d'info d'échec |
| [6](https://grandchesstree.com/move-generator/06-magic-bitboards.html) | magics, finder, tables ~840 Kio, drop-in | sliders en un multiply + shift + lookup | pas de PEXT/BMI2 ; tables en tableaux jagged (`ulong[][]`) ; recherche des magics au démarrage |
| [7](https://grandchesstree.com/move-generator/07-legal-moves.html) | `checkers`, `pinned`/`pinLines`, `kingDanger`, `checkMask`, EP par ray, castling | supprime le filtre make-then-check ; génération légale en amont | trois passes de légalité (10-14 lookups slider/nœud) ; `pinLines[64]` initialisé par nœud ; delegate `AttackFn` ; code non compilable tel quel par endroits |

**Le bon choix de la série** : générer **légal** en amont, pas pseudo-légal +
filtre. Pour MCTS, c'est le bon compromis : à chaque nœud étendu on veut la
liste exacte, pas des candidats à rejeter. Les parties réellement
« perft-only » sont :

- le **bulk counting** (`if depth == 1 return n`) ;
- `count_moves` (popcount des cibles, sans émission) ;
- l'absence de hash, d'ordre et de terminalité rapide, qui ne se voient pas
  en perft.

Autrement dit : la série s'arrête au contrat « perft juste ». Le contrat
« MCTS rapide » est plus large.

---

## 3. Le bon modèle de coût pour MCTS

### 3.1 Mesures locales

| Mesure | Valeur | Contexte |
|---|---:|---|
| `generate(All)` startpos | **106-176 ns** | `examples/genbench.rs`, release (3 runs) |
| `generate(All)` Kiwipete | **186-250 ns** | idem |
| `MoveList::new()` (zéro-init 512 o) | **~15 ns** | microbench isolé équivalent |
| `is_square_attacked` | **6,5-8,6 ns** | genbench |
| `rook_attacks` (magic, Zen1) | **2,9-4,2 ns** | genbench |
| make + unmake | **21-22 ns/ply** | `ELO_LOG.md` |
| eval PST | **33-40 ns** | `ELO_LOG.md` ; eval complète ~40-90 ns |
| MCTS, 1 s | **400-468 k sims/s** | UCI `go movetime` |
| MCTS, 3 s | **~395 k sims/s** | idem |
| MCTS, 6 s | **327 k sims/s** | idem |
| RSS 1 s / 3 s / 6 s | **129 / 336 / 528 MiB** | `/usr/bin/time -v` |
| perft 1T / 8T (EPYC) | **272 / ~1 100 Mnps** | `ELO_LOG.md` |

Sur cet hôte partagé (hyperviseur, charge variable), `generate` varie de
~106 (run calme) à ~176 ns pour startpos selon le run. Le tableau donne
la plage observée, pas une constante : les ratios (movegen vs total, perft
vs MCTS) sont plus stables que les valeurs absolues.

Le contraste perft vs MCTS est le point central : **272 Mnps en perft,
0,40-0,47 Msim/s en MCTS**, un facteur ~580-680. Un générateur peut être
excellent en perft et presque sans effet en MCTS, parce que perft ne paie
ni la sélection, ni l'allocation, ni les cache-misses, ni la terminalité.

> Note de reporting : la ligne `info` de l'UCI annonce encore
> `random-playouts` alors que le chemin courant évalue statiquement la
> feuille (A1). Le libellé est en retard sur le code ; les mesures ci-dessus
> portent bien sur le chemin A1.

### 3.2 Budget par simulation

À 1 s : 468 k sims/s → **2,14 µs/sim** (~7 000 cycles à 3,2 GHz) ; à
400 k sims/s, 2,5 µs/sim. À 6 s : 327 k sims/s → **3,06 µs/sim**
(~9 800 cycles).

Décomposition approximative (à instrumenter, pas à prendre au centième) :

| Poste | Coût / simulation | Part à 1 s | Part à 6 s |
|---|---:|---:|---:|
| movegen (~1,2-1,5 appel : 1 terminalité + expansion minoritaire) | 125-260 ns | ~6-12 % | ~4-8 % |
| make (descente, ~10-20 plies, sans unmake) | 110-220 ns | 4-10 % | ~3,5-7 % |
| eval + checks de draw | 100-150 ns | 4-7 % | 3-5 % |
| sélection UCT, allocation, cache | ~1,5-1,9 µs | ~70-86 % | ~78-89 % |

Le movegen pèse donc **~6-12 % à 1 s, ~4-8 % à 6 s**, pas 30 %.
Optimiser `generate` de 30 % donne ~2-4 % de simulations/s. Ce n'est pas
rien, mais ce n'est pas le levier principal.

### 3.3 Ce que disent les compteurs matériels

| Compteur | 1 s | 2 s | Normalisé (ordre de grandeur) |
|---|---:|---:|---:|
| cycles (user) | 3,20 G | 6,51 G | ~6,9-8,2 k/sim |
| instructions | 3,77 G | 6,81 G | ~8 k/sim |
| IPC | 1,18 | 1,05 | — |
| cache-misses | 32,6 M | — | **~70/sim** |
| L1 loads | — | 1,83 G | ~2 300/sim |
| L1 misses | — | 102 M | **~130/sim** (5,6 %) |
| dTLB misses | — | 4,46 M | ~5,6/sim |
| branch-misses | 19,9 M | — | ~42/sim (3,4 %) |

Un IPC de 1,05-1,18 sur un cœur Zen, avec ~70 cache-misses par
simulation, ne dit pas « trop d'instructions » : il dit **« marche de
pointeur dans l'arbre »**. Le nps qui baisse à mesure que le RSS monte
(468 k → 327 k entre 1 s et 6 s) est la même histoire : plus l'arène grossit,
plus chaque sélection touche de la mémoire froide.

**Conséquence pratique** : avant de micro-optimiser `generate`, il faut
réduire le nombre d'octets par nœud et par arête, et éviter les `Vec` par
nœud. C'est ce qui rendra les optimisations de movegen visibles.

Le code de sélection (`src/mcts.rs:296-341`) montre le reste : deux parcours
des enfants par nœud (compter les non-visités, puis les re-parcourir), un
`sqrt` par enfant pour UCT, et des `arena[c.node]` qui sautent dans une arène
de plusieurs centaines de Mo. À 30 enfants et une dizaine de plies, ce seul
poste pèse plus que le movegen.

### 3.4 Quand le movegen redevient central

Le chiffre de 5-12 % vaut pour la configuration actuelle : feuilles évaluées
statiquement, expansion minoritaire, pas de playout. Dès que l'on
ajoute :

- des **playouts** (v1) : `generate` est appelé à chaque ply du playout ;
- du **qsearch** : `CapturesOnly` + évasions à chaque feuille ;
- un **policy** : il faut des métadonnées et un ordre par coup ;
- un **réseau** : l'accumulateur NNUE doit être mis à jour dans make/unmake ;

le movegen peut passer de 5 % à 20-40 %. Les changements ci-dessous ne sont
donc pas perdus : ils préparent ces régimes.

---

## 4. Les six manques

### 4.1 Hash, historique, répétition — absent de la série

La série n'a **aucun Zobrist**. Or MCTS en a besoin pour :

- la **triple répétition** (sinon il faut comparer des positions, trop cher) ;
- la **table de transposition** (B3) et la **réutilisation d'arbre** (B2) ;
- le **cache d'éval** / l'identité d'un nœud pour un réseau ;
- l'adjudication de parties et les tests.

`mychess` a déjà un Zobrist incrémental (`src/position.rs:264`, `make`) et un
historique (`Search::game_hist`, `src/mcts.rs:145`). C'est le bon socle. Deux
points à vérifier :

1. **EP non capturable** : `make` pose la case EP après **toute** double
   poussée, même si aucun pion ennemi ne peut capturer ; un FEN peut faire
   pareil. Deux positions identiques avec/sans cette case auront deux hashs
   différents → répétition ratée (et TT/reuse qui dupliquent). Canonicaliser
   `ep = NO_EP` si `ep_capture_square(us)` est `None`.
2. **Fenêtre de répétition** : `repetition_draw` utilise
   `min(halfmove, len-1)` ; c'est correct tant que `halfmove` est bien remis
   à zéro sur capture/poussée de pion (c'est le cas dans `make`).

La série ne traite ni la répétition ni le 50-move dans le movegen : c'est
normal, ce sont des règles de partie, pas de génération. Mais un moteur MCTS
doit les avoir **au même endroit que la terminalité**, sinon il paie un
`generate` complet pour découvrir un draw.

### 4.2 Terminalité rapide : `has_legal_move`

C'est le manque le plus rentable côté movegen. La série ne fournit que
`Generate` (remplit une liste) et le `Perft.Run` qui compte. Il n'y a pas de
« existe-t-il au moins un coup légal ? ».

Dans `mychess`, `terminal_value` (`src/mcts.rs:412`) appelle `generate` à
**chaque feuille non terminale**, soit une fois par simulation, pour
finalement ne lire que `list.is_empty()`. C'est ~100-180 ns payés pour un booléen.

Un `has_legal_move` peut s'arrêter au premier coup trouvé et travailler par
**non-vacuité de bitboards**, sans énumérer les cibles :

- roi : `KING_ATT[ksq] & !us & !danger & !them_king != 0` ;
- double échec : retour immédiat ;
- pièces non clouées : pseudo-légal = légal, donc un simple `!= 0` sur les
  cibles suffit ;
- pièces clouées : `cibles & line_of(from) != 0` ;
- pions : poussées/captures non vides, promotions incluses ;
- EP : `ep_legal_ray` (déjà présent) ;
- castling : seulement si aucun autre coup n'a été trouvé.

Gain attendu : **+3 à 5 % de simulations/s**, et surtout un chemin de
terminalité qui ne dépend pas de la taille de la liste. Esquisse :

```rust
/// Vrai dès qu'au moins un coup légal existe. Ne remplit aucune liste.
pub fn has_legal_move(pos: &Position) -> bool {
    let us = pos.side;
    let them = us.opp();
    let ctx = build_legality(pos, us);
    if ctx.ksq == NO_SQUARE {
        return false;
    }

    // 1. Roi : `danger` est déjà calculé avec le roi retiré de l'occupation.
    if KING_ATT[ctx.ksq as usize] & !pos.color_bb(us) & !ctx.danger & !ctx.them_king != 0 {
        return true;
    }
    if ctx.checkers.count_ones() >= 2 {
        return false; // double échec : seul le roi peut répondre
    }

    let allowed = if ctx.checkers != 0 {
        let c = ctx.checkers.trailing_zeros() as u8;
        (between(ctx.ksq, c) | ctx.checkers) & !ctx.them_king
    } else {
        !ctx.them_king
    };

    // 2. Pièces non clouées : tester la non-vacuité des cibles, pas les cibles.
    //    (cavalier, fou/dame, tour/dame, pions : même schéma que `generate`,
    //     mais on `return true` au premier bit trouvé.)
    // 3. Pièces clouées : cibles & ctx.line_of(from).
    // 4. EP : `ep_legal_ray`. 5. Castling : en dernier recours.
    // ...
    false
}
```

Puis :

```rust
#[inline]
pub fn terminal_value(pos: &Position, hist: &[u64]) -> Option<f32> {
    if !has_legal_move(pos) {
        return Some(if pos.in_check(pos.side) { -1.0 } else { 0.0 });
    }
    if pos.halfmove >= 100
        || repetition_draw(pos, hist)
        || eval::insufficient_material(pos)
    {
        return Some(0.0);
    }
    None
}
```

### 4.3 Buffer réutilisable, zéro allocation

La série utilise `Span<Move> buf = stackalloc Move[256]` par appel : c'est
mieux que le `Move[]` de la Part 1, mais ce n'est pas réutilisé. En Rust,
`MoveList::new()` (`src/movegen.rs:18`) initialise `[Move::NONE; 256]`, soit
**512 octets de zéro à chaque `generate`**. Mesuré isolément : **~15 ns**,
soit ~9-14 % du coût de `generate(startpos)` selon le run.

Deux options :

1. **Sûre** : un scratch par thread, `clear()` une fois par nœud (len = 0,
   pas de memset). C'est la meilleure option pour MCTS : un seul buffer,
   réutilisé pour chaque expansion.
2. **Sans zéro-init** : `[MaybeUninit<Move>; 256]` + `write` dans `push`,
   `as_slice` sur le préfixe initialisé. Plus rapide, mais `unsafe` : à
   couvrir par Miri et des tests différentiels.

```rust
// Option 1, sans unsafe : le buffer est zéro-init une seule fois.
let mut scratch = MoveList::new();
loop {
    scratch.clear();                 // len = 0
    generate(&pos, &mut scratch, GenKind::All);
    // ... construire les arêtes à partir de scratch.as_slice()
}
```

Le vrai coût mémoire n'est pas la liste (512 o sur la pile), c'est l'arbre.
Mais 15 ns/simulation est gratuit à récupérer.

### 4.4 Ordre, priors, métadonnées

La série émet dans un ordre fixe : pions, cavaliers, fous, tours, dames
(deux fois : diagonales puis orthogonales), roi, castling. Les captures sont
émises avant les coups calmes **à l'intérieur de chaque pièce**, mais il n'y
a :

- aucun tri global (MVV-LVA, SEE) ;
- aucun coup de TT, killer, history ;
- aucune information « donne échec » ;
- aucune victime/attaquant dans le move (seulement `from`, `to`, `flags`).

Pour MCTS, l'ordre ne change pas le coût de `generate`, mais il change la
**forme de l'arbre** : en v1, les enfants non visités sont choisis
uniformément, donc les premiers essais suivent l'ordre de génération. À
10-40 k simulations, un mauvais ordre peut rater une tactique que 200
simulations bien ordonnées auraient trouvée. C'est un levier de **qualité par
simulation** (Elo à nœuds fixes), pas de simulations/s.

Ce qu'il faut ajouter, par ordre de rentabilité :

1. **MVV-LVA** : `victim` (mailbox[to]) et `attacker` (mailbox[from]) sont
   déjà accessibles ; un score `victim*8 - attacker` suffit.
2. **Promotions** : bonus fixe (dame > rook > bishop > knight).
3. **Échecs** : ne pas appeler `gives_check` pour les 30 coups
   (`make` + `is_square_attacked` + `unmake` ≈ 25-30 ns/coup → 750-900 ns/sim).
   Le calculer seulement pour les candidats (dame/tour/fou alignés, cavalier
   à portée, poussée de pion) ou le différer.
4. **Killers/history** : si tu ajoutes du qsearch ou un ordre alpha-beta-like.
5. **Prior de policy** : le movegen expose déjà `(from, to, promo)` ; un
   petit réseau ou une table peut produire `prior` sans seconde
   représentation.

`mychess` a déjà `GenKind::CapturesOnly` (`src/movegen.rs:449`) et
`gives_check` (`src/position.rs:226`), mais ni l'un ni l'autre n'est branché
dans MCTS. C'est un bon point de départ pour un qsearch ou un prior
tactique.

### 4.5 Évasions, check info, captures tactiques

Quand le roi est en échec, la série continue à parcourir **toutes** les
pièces et filtre par `checkMask`. C'est correct, mais ce n'est pas un
générateur d'évasions : on itère des pièces qui ne peuvent pas répondre.

Trois niveaux :

1. **Évasions** : ne parcourir que le roi, les pièces qui peuvent capturer
   l'attaquant ou s'interposer sur la ligne, et les pions qui peuvent
   bloquer. Gain ~2× dans les nœuds en échec, mais <1 % global (les échecs
   sont rares).
2. **`has_legal_move` en échec** : c'est le vrai gain, car la terminalité
   d'un nœud en échec est le cas où l'on veut arrêter tôt.
3. **`GenKind::CapturesOnly`** : déjà là ; pour un qsearch, ajouter les
   promotions (déjà incluses) et, si tu veux, les échecs calmes
   (« quiet checks ») — mais attention au coût de `gives_check`.

### 4.6 NNUE, copy-make, parallélisme

Ce sont les manques « architecturaux » de la série, qui ne se voient pas
tant que l'éval est material + PST.

- **NNUE** : un réseau lc0-style a besoin d'un accumulateur de features mis à
  jour **incrémentalement** dans `make`/`unmake` (ou d'un copy-make + refresh).
  Le movegen ne change pas, mais `Position`/`Undo` doivent être conçus pour
  accueillir cet état dès maintenant. Sinon, la mise à jour de l'accumulateur
  coûtera plus cher que tout le movegen.
- **Copy-make** : `Position` fait 144 octets ; copier peut être compétitif
  avec make/unmake+undo (21-22 ns/ply) pour les descentes courtes, et cela
  simplifie le parallélisme (chaque thread possède sa copie). À benchmarker
  avant de trancher — la série ne pose même pas la question.
- **Parallélisme** : la série est mono-thread et ne discute pas du partage
  des tables. Ton movegen est déjà sans état (tables immuables après
  `OnceLock`, `MoveList` sur la pile), donc thread-safe par construction.
  C'est le plus gros multiplicateur pour « générer le plus de simulations » :
  commencer par le **root parallelism** (split des coups racine, un arbre
  par thread, merge des stats), puis le **tree parallelism** avec virtual
  loss. Attendre ×2 à ×4 sur 6 cœurs.

---

## 5. Dettes de performance dans le code des posts

| Endroit | Coût | Correctif |
|---|---|---|
| Part 5 `RebuildOccupancy()` | recalcul complet (12 bitboards) à chaque make **et** unmake | mise à jour incrémentale de `bb_color`/`bb_piece` (déjà fait dans `mychess`) |
| Part 7 trois passes (`danger`, `checkers`, `pins`) | 10-14 lookups slider/nœud | une passe fusionnée sur les sliders ennemis (déjà fait, L1c ×1,19) |
| Part 7 `pinLines[64] = MaxValue` | 512 o initialisés/nœud | `[u64; 8]` + `npins` (déjà fait) |
| Part 7 `delegate ulong AttackFn` | appel indirect par slider | booléen + monomorphisation, ou deux fonctions |
| Part 4 dame traitée deux fois | 2 boucles sur les mêmes pièces | une boucle, deux appels d'attaque |
| Part 1 `Move[]` par nœud | allocation/nœud | `stackalloc` (fait Part 4) ; en Rust, scratch réutilisé |
| Part 4 `EmitTargets` | branche `capture/quiet` par cible | séparer captures et calmes par bitboard (déjà fait) |
| Part 3 move sans score | pas d'ordre | score par arête / SoA, ou tri des captures |
| Part 6 `ulong[][]` | double indirection | table plate + offsets (déjà fait, `magic_generated.rs`) |
| Part 7 code final | `Span<ulong>` vs `ulong[]`, arité de `GenEnPassant` | **ne compile pas tel quel** ; c'est pédagogique, pas une lib |

Deux erreurs/limites à connaître avant de copier :

- `ComputePins(..., Span<ulong> pinLines)` est ensuite passé à des méthodes
  qui attendent `ulong[]` (`GenPawnsLegal`, `GenSlidersLegal`) : en C#, un
  `Span<T>` ne se convertit pas en `T[]`. De même,
  `GenEnPassant(pos, dest, n, us)` est appelé avec 4 arguments alors que la
  définition en prend 6 (`kingSq`, `checkers`). Le listing final est une
  illustration, pas un projet compilable.
- `FindMagic` utilise `used[idx] == 0` comme sentinelle « libre ». Un
  ensemble d'attaques de slider n'est jamais vide, donc ça passe, mais c'est
  fragile ; préférer un tableau `bool` séparé.

---

## 6. Ce que `mychess` fait déjà mieux que la série

| Sujet | Série | `mychess` |
|---|---|---|
| Hash/Zobrist | absent | incrémental + historique |
| Occupancy | `RebuildOccupancy` par make | incrémentale |
| Légalité | 3 passes | 1 passe fusionnée (L1c) |
| Sliders | magic seulement | magic build-proven + PEXT (dispatch Zen1 → magic) |
| Buffer | `stackalloc` par nœud | `MoveList` sur pile, mais zéro-init |
| Tests | perft | perft + différentiels + twin `count_moves` |
| Perft | non chiffré | 272 Mnps 1T / ~1 100 Mnps 8T (EPYC) |

Il ne faut donc **pas** porter `RebuildOccupancy`, les trois passes ou les
tableaux jagged : c'est déjà réglé. Les manques à porter sont ceux du §4,
pas les dettes du §5.

---

## 7. Plan priorisé pour `mychess`

Les gains ne sont pas additifs : la mémoire et le parallélisme dominent.

| Priorité | Action | Où | Gain attendu | Effort |
|---|---|---|---|---|
| **P0** | Arène plate `Node`/`Edge`, enfants paresseux, réservation par blocs | `src/mcts.rs` | **+20 à 50 % sims/s**, RSS ÷2-3 | M |
| **P0** | `has_legal_move` + terminalité sans `generate` | `src/movegen.rs`, `src/mcts.rs` | **+3 à 5 %** | S |
| **P0** | Parallélisme racine, puis virtual loss | `src/mcts.rs` | **×2 à 4** | L |
| P1 | Scratch `MoveList` réutilisé (`clear`) | `src/movegen.rs` | +0,5 à 1,5 % | S |
| P1 | Micro-légalité : `danger` seulement si le roi a des cibles, pins conditionnels, `line_of` sans boucle | `src/movegen.rs` | +1 à 3 % | M |
| P1 | Ordre/priors : MVV-LVA, promotions, échecs différés | `src/movegen.rs`, `src/mcts.rs` | Qualité (Elo à nœuds fixes) | M |
| P1 | Réutilisation d'arbre (B2) + TT (B3) | `src/mcts.rs` | +10 à 30 % | L |
| P2 | NNUE / copy-make | `src/position.rs` | Dépend du réseau | XL |
| P2 | Hoister le dispatch PEXT, réduire les tables | `src/slider.rs` | <1 % | S |
| P2 | `GenKind::Evasions` | `src/movegen.rs` | <1 % | S |
| P2 | Éval incrémentale (matériel + PST) au lieu de la boucle 64 | `src/eval.rs` | +1 à 2 % | M |

### P0-1 — Arène plate `Node`/`Edge`

Le `Node` actuel (`src/mcts.rs:54`) pèse ~48 o et possède un `Vec<Child>`
(24 o) ; chaque enfant est un `Node` de 48 o **même s'il n'est jamais
visité**. À 6 s, 528 MiB de RSS pour 1,96 M simulations, soit ~280 o/sim :
l'essentiel est de la place réservée pour des enfants jamais touchés.

Cible :

```rust
#[repr(C)]
struct Node {
    visits: u32,
    sum: f32,
    first_edge: u32,
    n_edges: u8,
    flags: u8,      // bit0 = expanded, bit1 = terminal
} // 16 o

#[repr(C)]
struct Edge {
    mv: Move,       // 2 o
    child: u32,     // 4 o, u32::MAX tant que le nœud enfant n'existe pas
    visits: u32,    // 4 o
    sum: f32,       // 4 o
    prior: f32,     // 4 o (A2)
} // 20 o
```

- arène `Vec<Node>`, arène `Vec<Edge>`, pas de `Vec` par nœud ;
- les arêtes d'un nœud sont contiguës (localité) ;
- le nœud enfant n'est alloué qu'à la première visite (lazy expansion) ;
- réserver par blocs (`Vec::with_capacity` ou chunks) pour éviter le
  `realloc` + memcpy d'une arène de 500 MiB.

Gain mémoire : ~20 o/arête + 16 o/nœud, contre 8 o (Child) + 48 o (Node) +
24 o (Vec) aujourd'hui. Pour 30 coups : ~616 o contre ~1,7 Ko, soit **~2,7×
moins**. Le nps devrait remonter et la pente de dégradation s'aplatir.

### P0-2 — Terminalité

Voir §4.2. L'appel de terminalité disparaît ; l'expansion garde un
`generate`, mais elle est minoritaire. C'est le gain le plus simple.

### P0-3 — Parallélisme

Le movegen est prêt : aucune globale mutable, `MoveList` sur la pile,
tables immuables. Le travail est dans `mcts.rs` :

1. **Root parallel** : dupliquer l'arbre racine par thread, split des coups
   racine, merge des visites/valeurs à la fin. Simple, quasi linéaire.
2. **Tree parallel** : arène partagée, `visits`/`sum` atomiques ou par
   thread, virtual loss pour éviter que N threads descendent la même
   branche. Plus complexe, meilleur scaling.

Sur 6 cœurs, viser ×2 à ×4 (le perft 8T de `ELO_LOG` montre ×4, mais MCTS
paie la contention).

### P1 — Ordre et priors

Un ordre statique bon marché suffit pour les premiers k simulations :
captures (MVV-LVA), promotions, puis calmes. Les échecs ne se calculent que
si le budget le permet. Pour un vrai prior, une petite policy (ou une table
de probabilités par coup) branchée sur `Edge::prior` est le hook A2 déjà
prévu.

**Si tu ne fais que trois choses** : (1) `Node`/`Edge` + arène plate, (2)
`has_legal_move`, (3) root parallelism. Ces trois-là changent le budget
sims/s sans toucher à la force ; le reste peut attendre.

---

## 8. Contrat d'un move generator « MCTS-first »

Checklist à garder en tête pour toute évolution :

1. **Générer tous les coups légaux une fois** par nœud étendu, et les
   stocker compactement (arêtes), pas les régénérer à chaque simulation.
2. **`has_legal_move`** pour la terminalité : pas de liste, arrêt au premier
   coup.
3. **`CapturesOnly`** (et évasions si qsearch) sans payer la liste complète.
4. **Zéro allocation** dans le chemin chaud : buffer réutilisé, pas de
   zéro-init de 512 o par appel.
5. **Métadonnées** : capture, victime/attaquant, promotion, échec (paresseux)
   pour l'ordre et les priors.
6. **Ordre déterministe** et, si besoin, tri stable des captures.
7. **Hash + historique** disponibles à chaque nœud (répétition, TT, reuse).
8. **Make/unmake O(1)**, hash incrémental, et plus tard accumulateur NNUE.
9. **Thread-safe par construction** : tables immuables, pas de statique
   mutable, buffers par thread.
10. **Métriques** : ns/`generate`, ns/`has_move`, cache-misses/sim,
    sims/s à force fixée — pas seulement des Mnps de perft.

---

## 9. Pièges « perft-only » à éviter

- Utiliser `count_moves` ou le bulk counting (`if depth == 1`) comme test de
  terminalité : le bulk counting compte tout, il n'arrête pas tôt.
- Générer une liste complète alors qu'on veut seulement « un coup existe ».
- Reconstruire l'occupancy à chaque make (la série le fait).
- Optimiser les Mnps bruts au détriment du hash, de l'ordre et des
  métadonnées.
- Allouer `Move[256]` par nœud et le zéro-initialiser.
- Prendre perft comme seule métrique de performance : il ne voit ni les
  cache-misses, ni l'allocation, ni la terminalité, ni le parallélisme.
- Copier le listing de la Part 7 tel quel : il ne compile pas (Span vs
  array, arité EP).

---

## 10. Protocole de mesure et de validation

1. **Criterion** : `generate_all`, `generate_captures`, `has_legal_move`,
   `make_unmake`, `mcts_descend30` ; baseline sauvegardée, A/B dans la même
   session, deux jambes.
2. **UCI** : `go movetime 500/1000/3000/6000`, même seed, même position ;
   noter nps **et** RSS ; 3 répétitions, médiane.
3. **perf** : `cycles`, `instructions`, `cache-misses`,
   `L1-dcache-load-misses`, `dTLB-load-misses` ; normaliser **par
   simulation**, pas par seconde. Objectif : faire baisser les misses, pas
   seulement les ns.
4. **Elo à nœuds fixes** (10k/40k) pour vérifier qu'on n'a pas échangé de la
   qualité contre de la vitesse.
5. **Invariants** : perft inchangé sur toutes les positions dorées ;
   `generate.len() == count_moves` ; `has_legal_move == (len > 0)` ;
   `zobrist == recompute_zobrist()` après make et unmake ; compteur
   d'allocations à zéro dans le chemin chaud.
6. **Hôte** : les chiffres ci-dessus viennent d'un Ryzen 5 1600X (Zen1,
   PEXT lent → magics). La baseline de référence reste l'EPYC 7763 de
   `ELO_LOG.md` ; re-mesurer avant de figer un budget sims/s.

---

## 11. Annexes

### A. Commandes de mesure

```bash
# microbench movegen
cargo run --release --example genbench

# nps + RSS à budget variable
for ms in 500 1000 3000 6000; do
  /usr/bin/time -v sh -c "(printf 'uci\nisready\nposition startpos\ngo movetime $ms\n'; sleep $((ms/1000+2)); printf 'quit\n') | ./target/release/mychess uci" 2>&1 | grep -E '^info|Maximum resident'
done

# compteurs matériels
perf stat -e cycles,instructions,cache-misses,L1-dcache-loads,L1-dcache-load-misses,dTLB-load-misses \
  sh -c "(printf 'uci\nisready\nposition startpos\ngo movetime 2000\n'; sleep 3; printf 'quit\n') | ./target/release/mychess uci >/dev/null"
```

### B. Chiffres bruts (hôte local, 2026-10-21)

```none
generate startpos: 106-176 ns/call   (3 runs, hôte partagé)
generate kiwipete: 186-250 ns/call
is_square_attacked: 6.5-8.6 ns
rook_attacks: 2.9-4.2 ns
MoveList::new (microbench équivalent): ~15 ns

MCTS startpos (RSS = maximum resident set size, KiB) :
  500 ms :   230 976 nodes, 461 952 nps,  82 788 KiB (~80,8 MiB)
  1 000 ms:  467 584 nodes, 467 584 nps, 132 156 KiB (~129,1 MiB)
  3 000 ms: 1 184 512 nodes, 394 837 nps, 344 372 KiB (~336,3 MiB)
  6 000 ms: 1 963 520 nodes, 327 253 nps, 540 504 KiB (~527,8 MiB)

perf 1 s : 3.204 G cycles, 3.771 G instructions (1.18 IPC),
           32.56 M cache-misses, 19.88 M branch-misses (3.43 %)
perf 2 s : 6.513 G cycles, 6.808 G instructions (1.05 IPC),
           1.826 G L1 loads, 102.4 M L1 misses (5.61 %),
           4.46 M dTLB misses
```

### C. Sources

- [Part 1 — The Pipeline and Perft](https://grandchesstree.com/move-generator/01-introduction.html)
- [Part 2 — Representing the Board](https://grandchesstree.com/move-generator/02-board.html)
- [Part 3 — Encoding a Move](https://grandchesstree.com/move-generator/03-move-encoding.html)
- [Part 4 — Generating Pseudo-Legal Moves](https://grandchesstree.com/move-generator/04-pseudo-legal-moves.html)
- [Part 5 — Making a Move](https://grandchesstree.com/move-generator/05-make-move.html)
- [Part 6 — Magic Bitboards](https://grandchesstree.com/move-generator/06-magic-bitboards.html)
- [Part 7 — Legal Move Generation](https://grandchesstree.com/move-generator/07-legal-moves.html)
- Dépôt : [`src/movegen.rs`](../src/movegen.rs), [`src/mcts.rs`](../src/mcts.rs),
  [`src/position.rs`](../src/position.rs), [`src/slider.rs`](../src/slider.rs),
  [`benches/core.rs`](../benches/core.rs), [`ELO_LOG.md`](../ELO_LOG.md).
