# Phase 3 — Arène plate et parallélisme du MCTS : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transformer `src/mcts.rs` pour que le moteur encaisse beaucoup plus de simulations par seconde et par octet : (1) remplacer l'arbre `Vec<Child>` par une **arène plate** `Node`/`Edge` à allocation paresseuse, (2) câbler `has_legal_move` dans la **terminaison** du MCTS, (3) ajouter un **parallélisme racine** déterministe, et (4) le rendre pilotable par UCI (`Threads`).

**Architecture:** `src/mcts.rs` garde son algorithmique (UCT v1, value pour le joueur au trait, terminalité en quatre causes + matériel insuffisant, répétition par l'historique de hash). Seule la **structure de données** change : deux arènes POD (`Vec<Node>`, `Vec<Edge>`), les arêtes d'un nœud contiguës, l'enfant alloué **au moment où il est expandé** et pas avant, les statistiques de l'enfant portées par l'**arête** et non par un nœud. La descente, la sélection UCT et la retropropagation sont réécrites sur cette arène, en conservant **exactement** l'ordre des tirages du RNG : le chemin mono-thread est donc **bit-à-bit identique** à l'ancien (filet de sécurité : test différentiel contre l'algorithme v1 recopié dans le test). Le parallélisme est **racine** : les coups racine sont répartis en `T` paquets disjoints, chaque thread possède son propre arbre, les statistiques racine sont fusionnées à la fin. Aucun partage mutable n'existe entre threads — pas de course, pas de lock, déterminisme pour un `(seed, threads)` donné.

**Tech Stack:** Rust edition 2024, MSRV 1.97 (`rust-toolchain.toml`), `rayon` (déjà présent, non utilisé par cette phase), `criterion` 0.5 (dev), `microbench` 0.5 (dev).

## Global Constraints

- Rust edition 2024, toolchain épinglée `1.97.0` par `rust-toolchain.toml`. Ne pas la changer.
- **Comportement mono-thread inchangé bit-à-bit** : même graine ⇒ même coup, même score, même nombre de nœuds, qu'avant la phase 3. C'est la contrainte n°1 du plan, et elle est vérifiée par un test différentiel (Task 1, gate G1).
- `src/movegen.rs`, `src/position.rs`, `src/types.rs`, `src/tables.rs`, `src/slider.rs`, `src/eval.rs` sont **gelés**. Seul `src/mcts.rs` (et son nouveau module `src/mcts/arena.rs`), `src/uci.rs`, les tests et les bancs changent.
- `unsafe` : **interdit** dans cette phase. L'arène est un `Vec` de POD `#[repr(C)]` ; aucun pointeur brut, aucun `transmute`.
- API publique de `mychess::mcts` : `Search::new(pos, hist, limits, seed)` garde sa signature ; le nombre de threads passe par un **constructeur fluency** `Search::with_threads(n)`. `Limits`, `Outcome`, `Rng`, `leaf_value`, `terminal_value`, `eval_normalized` gardent leur signature et leur sémantique.
- Le budget de nœuds reste **honnête** : `go nodes N` consomme N nœuds au total, quelle que soit la valeur de `Threads` (le budget est réparti entre les threads). Le budget de temps est **partagé** : chaque thread court jusqu'à la même échéance.
- `cargo clippy --all-targets` et `cargo fmt -- --check` sont cassés sur `main` (exemples `first_case.rs`, `d9_motif_shard.rs`). Utiliser `cargo clippy --release --tests --bins --lib -- -D warnings` et `rustfmt --edition 2024 --check <fichiers touchés>`.
- Mesure : même session,=h同一个 hôte, au moins deux jambes, rapport de **ratios** (nps, octets/nœud), jamais de seuil absolu. La machine n'est jamais inactive : les valeurs absolues ne valent que pour la session.
- Chaque tâche se termine par une suite verte et un commit unique, en français.

## Hors périmètre

- **NNUE / copy-make** : P2, effort XL (`docs/analyse_movegen_mcts.md` §7). Rien de ce plan n'y touche.
- **Parallélisme d'arbre** (arène partagée, `visits`/`sum` atomiques, virtual loss) : c'est la phase 4. Le plan de la phase 3 livre le parallélisme **racine**, qui est sans course.
- **Réutilisation d'arbre entre `go`** (B2) et **table de transposition** (B3) : non traités. L'arbre est reconstruit à chaque `go`, comme avant.
- **Priors de policy** (`Edge::prior` est prévu dans la structure, il reste à 0) : le hook A2 existe déjà dans l'ancien code, il n'est pas câblé.
- Aucune modification de `src/movegen.rs` : `has_legal_move` est déjà là, il s'agit seulement de le **câbler**.
- Pas de changement de l'algorithme de sélection (UCT, pas de PUCT), pas de qsearch, pas de table de repetition globale (B3).

## Plafond attendu, à dire dès maintenant

Trois budgets distincts, à ne pas confondre :

1. **Mémoire (RSS)** : l'analyse §7 annonce ~280 octets par simulation aujourd'hui, dont l'essentiel en `Vec<Child>` (24 o d'en-tête + 8 o par entrée) et en nœuds enfants jamais développés (~48 o chacun). L'arène plate descend le coût théorique à ~20 o par arête + 20 o par nœud développé. C'est le gain le plus fiable : il ne dépend d'aucune mesure de temps.
2. **Simulations/s** : l'analyse §7 annonce **+20 à 50 %** de nps pour l'arène plate, **+3 à 5 %** pour `has_legal_move`. Ces chiffres viennent de la même analyse que le reste du plan ; ils sont des **hypothèses à mesurer**, pas des résultats.
3. **Parallélisme** : ×2 à ×4 sur 6 cœurs est l'estimation de l'analyse §7 pour le parallélisme MCTS. Le parallélisme **racine** ne partage pas la profondeur : à budget de nœuds constant, chaque paquet racine reçoit `N/T` nœuds au lieu de `N`, donc la profondeur effective **baisse**. Le gain de temps est réel (×T sur le débit), le gain de force à nœuds constant est **négatif ou nul** — c'est le prix connu du parallélisme racine, et c'est pourquoi `Threads` reste à 1 par défaut pour l'échelle SPRT à nœuds fixes (`test.sh`).

## Structure des fichiers

| Fichier | Rôle | Tâche |
|---|---|---|
| `src/mcts/arena.rs` | **nouveau** : arène plate `Node`/`Edge` | 1 |
| `src/mcts.rs` | descente sur arène plate, terminaison `has_legal_move`, parallélisme racine | 1, 2, 3 |
| `src/uci.rs` | option `Threads`, câblage, ligne `info` | 4 |
| `tests/mcts_phase3.rs` | **nouveau** : différentiel v1, invariants d'arène, terminalité, parallélisme | 1, 2, 3, 4 |
| `tests/mcts_uci.rs` | option `Threads` de bout en bout | 4 |
| `benches/mcts.rs` | **nouveau** : nps mono/parallèle, RSS | 5 |
| `examples/mcts_probe.rs` | **nouveau** : sonde nps/RSS reproductible | 5 |
| `examples/mcts_quality.rs` | **nouveau** : batterie de mats, accord des coups mono/parallèle | 5 |
| `docs/mcts/phase3-2026-10-22.md` | **nouveau** : rapport de mesure | 5 |

---

### Task 1: Arène plate `Node`/`Edge`

**Files:**
- Create: `src/mcts/arena.rs`
- Modify: `src/mcts.rs`

**Interfaces:**
- Consumes: `mychess::types::Move`, `mychess::position::Position`.
- Produces: `mychess::mcts::arena::{Arena, Node, Edge}` (module `pub(crate)`), utilisé par `src/mcts.rs`.

**Note de conception, à lire avant d'agir.** L'ancien arbre alloue, à l'expansion d'un nœud, **un `Node` complet par coup légal**, alors que la plupart de ces enfants ne seront jamais développés. La politique de sélection v1 visitant d'abord les enfants non visités, chaque enfant est bien visité une fois — mais **développé** (arête + descendants) beaucoup plus tard, souvent jamais. La nouvelle structure sépare les deux :

- **`Edge`** = un coup racine du nœud + les statistiques de la position **enfant** (`visits`, `sum`) + l'index du nœud enfant, `NO_CHILD` tant que l'enfant n'a pas été développé. La sélection UCT lit **uniquement** le tableau d'arêtes, qui est contigu : le scan de sélection ne touche plus le tableau de nœuds.
- **`Node`** = les statistiques **du** nœud + sa plage d'arêtes contiguës (`first_edge`, `n_edges`) + un drapeau `terminal` et sa valeur.

Invariant central, vérifié par test : **les statistiques d'un nœud enfant sont la copie de celles de l'arête qui y mène** (`node.visits == edge.visits`, `node.sum == edge.sum`). L'arène n'a donc pas deux sources de vérité : l'arête est la source, le nœud en est le miroir (utile pour la fusion racine et les tests).

Autre invariant : **les arêtes d'un nœud sont contiguës** (`first_edge..first_edge+n_edges`), et un nœud n'a pas d'arête avant son expansion (`n_edges == 0`).

Taille visée : `Node` 20 o (4 `u32` + 2 `f32` + 2 `u8` + bourrage), `Edge` 20 o (`Move` 2 o + bourrage 2 o + 4 `u32`/`f32`). Contrainte de l'analyse §7 : `Node` 16 o, `Edge` 20 o. On s'écarte de 4 o sur `Node` parce que la valeur terminale y est stockée ; l'écart est documenté et le contrat mesuré est « ≤ 24 o par nœud, 20 o par arête ».

- [ ] **Step 1: Écrire l'oracle v1 avant de toucher au moteur**

Copier l'algorithme v1 **tel quel** dans `tests/mcts_phase3.rs` (module interne `mod v1_ref`), avec sa propre terminaison par `generate` (pas `has_legal_move`) et son propre RNG. Il sert d'oracle : le nouveau moteur doit rendre exactement le même coup, le même score et le même nombre de nœuds sur une batterie de `(position, graine, nœuds)`.

- [ ] **Step 2: Implémenter `src/mcts/arena.rs`**

```rust
pub(crate) const NO_CHILD: u32 = u32::MAX;
pub(crate) const F_EXPANDED: u8 = 0b01;
pub(crate) const F_TERMINAL: u8 = 0b10;

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct Node {
    pub visits: u32,
    pub sum: f32,
    pub term: f32,
    pub first_edge: u32,
    pub n_edges: u8,
    pub flags: u8,
    _pad: [u8; 2],
}
```

L'arène expose : `new`, `node`, `node_mut`, `edge`, `edge_mut`, `edges_of(node) -> &[Edge]`, `alloc_node`, `add_edges(node, &[Move])`, `len_nodes`, `len_edges`.

- [ ] **Step 3: Réécrire la descente sur l'arène**

Règles de la descente, dans l'ordre (l'ordre des tirages RNG est normatif — voir la note ci-dessous) :

1. nœud terminal → backprop de la valeur terminale, fin ;
2. nœud non développé et (`visits == 0` ou plafond de profondeur) → **feuille** : `leaf_value`, backprop, fin ;
3. nœud non développé et `visits > 0` → **expansion** : `generate`, terminaison (liste vide → mat/pat ; sinon 50-coups / répétition / matériel insuffisant → nul), sinon création des arêtes ;
4. sélection : enfant non visité si il y en a (uniforme), sinon argmax UCT ;
5. descente : `make` du coup, push du hash, push de l'arête dans le chemin.

*Note normative sur le RNG.* Dans l'ancien code, l'expansion tire `rng.next_u64() % n_enfants` **dans la branche d'expansion**, et la sélection non visitée tire `rng.next_u64() % n_non_visités`. Pour que la trajectoire mono-thread soit **identique**, le nouveau code doit tirer **exactement un nombre** par expansion et **exactement un** par sélection, dans le même ordre, avec le même modulo. En particulier, après l'expansion, la sélection doit être prise dans le bloc « sélection » (et non dans le bloc « expansion »), et le modulo doit porter sur le **nombre d'arêtes du nœud** — identique à l'ancien `children.len()`.

*Note sur l'allocation paresseuse.* L'arête porte `child == NO_CHILD` tant que l'enfant n'a pas été développé. À la descente dans une arête dont `visits == 0`, l'enfant est une **feuille** : aucun nœud n'est alloué. Le nœud enfant est créé à la **deuxième** visite de l'arête, et il est initialisé avec les statistiques de l'arête (invariant du miroir). Le nombre de nœuds alloués est donc égal au nombre de nœuds **développés**, pas au nombre de coups légaux.

- [ ] **Step 4: Rétropropagation sur les arêtes**

Le chemin de la descente est une liste d'**arêtes** (la racine n'a pas d'arête entrante). Pour chaque arête, en remontant : `edge.sum += v ; edge.visits += 1`, puis, si le nœud enfant existe, `node.sum += v ; node.visits += 1` (miroir), puis `v = -v`. À la fin, la racine reçoit `v` (valeur pour le joueur au trait à la racine).

- [ ] **Step 5: Vérifier le gate G1 (différentiel v1)**

```bash
cargo test --release --test mcts_phase3 v1_ref 2>&1 | tail -20
```
Attendu : l'oracle v1 et le nouveau moteur rendent le **même** coup, le **même** score et le **même** nombre de nœuds sur toute la batterie. Un écart = régression, pas « détail d'implémentation ».

- [ ] **Step 6: Vérifier le gate G2 (invariants d'arène)**

```bash
cargo test --release --test mcts_phase3 arena 2>&1 | tail -20
```
Attendu : après une recherche, (a) les arêtes de chaque nœud sont contiguës, (b) `node.visits == edge.visits` et `node.sum == edge.sum` pour tout nœud développé, (c) `n_nodes` est très inférieur à `n_edges` (allocation paresseuse effective), (d) aucun nœud n'est alloué pour une arête jamais revisitée.

---

### Task 2: Terminaison par `has_legal_move`

**Files:**
- Modify: `src/mcts.rs` (`terminal_value`, `leaf_value`)

**Interfaces:**
- Consumes: `mychess::movegen::has_legal_move`.
- Produces: aucun changement de signature.

*Note de conception.* `terminal_value` est appelé sur **chaque feuille** de chaque simulation. Il matérialisait une `MoveList` complète (512 octets) pour décider d'un mat ou d'un pat. `has_legal_move` répond à la même question sans matérialiser la liste (rapport de phase 2 : 0,20 à 0,40× le coût d'un `generate` complet). L'ordre des vérifications est **normatif** : le mat passe avant les tireurs (une position mate par un automate 50 coups reste un mat).

- [ ] **Step 1: Remplacer le test « liste vide » par `has_legal_move`**

```rust
pub fn terminal_value(pos: &Position, hist: &[u64]) -> Option<f32> {
    if !has_legal_move(pos) {
        return Some(if pos.in_check(pos.side) { -1.0 } else { 0.0 });
    }
    if pos.halfmove >= 100 { return Some(0.0); }
    if repetition_draw(pos, hist) { return Some(0.0); }
    if eval::insufficient_material(pos) { return Some(0.0); }
    None
}
```

La branche d'expansion, qui a **déjà** la liste des coups légaux sous la main, garde son test `list.is_empty()` : pas de coût supplémentaire, et la sémantique est identique.

- [ ] **Step 2: Vérifier le gate G3 (équivalence de terminaison)**

```bash
cargo test --release --test mcts_phase3 terminal 2>&1 | tail -20
```
Attendu : pour une batterie de positions (mat, pat, 50 coups, répétition, matériel insuffisant, positions vivantes), la nouvelle terminaison et l'ancienne (par `generate`) rendent **la même** valeur, y compris `None`.

- [ ] **Step 3: Re-vérifier G1**

La terminaison changeant l'ordre d'évaluation, la trajectoire mono-thread **peut** changer. Le gate G1 doit **toujours** passer : c'est la preuve que le câblage n'a rien changé au résultat. Si G1 casse, c'est que l'ordre des vérifications a bougé : le corriger, ne pas assouplir le test.

---

### Task 3: Parallélisme racine

> **Décision d'exécution (2026-10-22).** La première implémentation a suivi
> le plan ci-dessous : les coups racine étaient **répartis en paquets
> disjoints**, un par worker, et les statistiques étaient fusionnées paquet
> par paquet. La mesure l'a **renversé** : les nombres de visites de deux
> paquets disjoints ne sont pas comparables (le paquet dont le meilleur coup
> a moins de concurrents concentre ses visites sur lui), et la règle v1 « le
> plus visité » éliminait alors les mat. La fusion livrée est donc une
> **somme** : chaque worker explore toute la racine, les statistiques sont
> additionnées coup par coup, et les visites redeviennent comparables. Les
> Steps 1 à 3 ci-dessous décrivent le mécanisme, pas la forme de la fusion.
> La mesure et la démonstration sont dans
> `docs/mcts/phase3-2026-10-22.md` (§ « La mesure qui a changé la
> conception »).

**Files:**
- Modify: `src/mcts.rs`

**Interfaces:**
- Produces: `Search::with_threads(self, n: usize) -> Search` ; `Search::run` dispatche mono (`threads <= 1`) ou parallèle.

*Note de conception — pourquoi racine et pas arbre.* L'arène partagée (statistiques atomiques, virtual loss) est la vraie réponse au parallélisme MCTS, mais elle exige une arène à croissance sûre sous concurrence : soit des chunks à pointeurs stables, soit des verrous sur le chemin critique. C'est un chantier à part (phase 4), avec un vrai risque de course. Le parallélisme **racine** n'a **aucun** état partagé : chaque thread possède son arbre, et les paquets de coups racine sont disjoints. Le prix est connu et assumé : à budget de nœuds constant, la profondeur effective baisse (chaque paquet reçoit `N/T` nœuds).

*Note de conception — budget.* `go nodes N` : le budget est **réparti** entre les threads (`N/T` chacun, le reste au premier thread), de sorte que le total reste N et que l'échelle SPRT à nœuds fixes reste comparable. `go movetime T` : le budget est une **échéance partagée**, chaque thread court jusqu'à elle, donc le total de nœuds est `≈T×` celui du mono-thread — c'est le mode « débit maximal ».

- [ ] **Step 1: Répartir les coups racine**

La liste des coups légaux racine est calculée une fois, puis répartie en tourniquet (`coup i` → thread `i mod T`) : l'ordre de génération est conservé dans chaque paquet, donc le parcours des arêtes reste déterministe.

- [ ] **Step 2: Exécuter les paquets en threads**

`std::thread::scope` : `T-1` threads démarrés en plus du thread courant, qui traite le paquet 0. Chaque thread a son propre `Arena`, son propre `Rng` (graine dérivée de `seed` par SplitMix), son propre `Scratch`. Aucun verrou, aucune donnée partagée.