# Phase 4 — PUCT et priors de policy : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remplacer la sélection **UCT** du MCTS par une sélection **PUCT** pilotée par des **priors de policy heuristiques** (MVV-LVA + promotions), afin que la recherche explore d'abord les coups qui promettent, et non pas un coup au hasard.

**Architecture:** `src/mcts.rs` garde son algorithmique (valeur pour le joueur au trait, terminaison en quatre causes + matériel insuffisant, arène plate de la phase 3, parallélisme racine par fusion-somme). Ce qui change est **la règle de sélection** et **l'ordre de développement** :

- une fonction `mcts::policy::priors(pos, moves)` renvoie, pour les coups légaux d'un nœud, une **distribution de probabilité** (somme = 1) calculée à partir d'un score **MVV-LVA** (10 × valeur de la victime − valeur de l'attaquant) et des **promotions** ;
- le score est **saturé** puis mélangé à un plancher uniforme (`PRIOR_EPS = 0.25`) : aucun coup n'est affamé, la distribution ne peut pas s'effondrer sur un seul coup ;
- les priors sont calculés **une seule fois**, à l'expansion du nœud, et stockés dans `Edge::prior` (le champ que l'arène de la phase 3 réservait déjà) ;
- la sélection devient `s(a) = Q(a) + c_puct · P(a) · √N / (1 + n(a))` (Silver et al., AlphaZero) ;
- le **chemin UCT d'origine est conservé** (`Policy::Uct`) : c'est l'oracle figé du test différentiel de la phase 3, et la contre-mesure A/B de la phase 4 ;
- le **parallélisme racine** a besoin de **diversité** entre les workers : une sélection déterministe les rendrait identiques (le parallèle se réduit au mono à `N/T` nœuds). Chaque worker **perturbe donc les priors de la racine** par un bruit de Gumbel (tirage par graine), ce qui est l'amorce de C1 (bruit de Dirichlet). Le chemin **mono reste propre et déterministe**.

**Tech Stack:** Rust edition 2024, MSRV 1.97, `rayon` (présent, non utilisé), `criterion` 0.5 (dev).

## Global Constraints

- Rust edition 2024, toolchain épinglée `1.97.0`. Ne pas la changer.
- **Le chemin UCT doit rester bit-à-bit identique à v1** : même graine ⇒ même coup, même score, même nombre de nœuds. C'est la contrainte n°1 : le test différentiel de la phase 3 est **conservé** et épinglé sur `Policy::Uct`. Le chemin PUCT, lui, change **volontairement** le comportement mono (c'est l'objet de la phase) : la référence de qualité change, l'échelle Elo doit donc être rejouée.
- `src/movegen.rs`, `src/position.rs`, `src/types.rs`, `src/tables.rs`, `src/slider.rs`, `src/eval.rs` sont **gelés**. Seul `src/mcts.rs` (+ `src/mcts/policy.rs`, `src/mcts/arena.rs`), `src/uci.rs`, les tests, les bancs et les exemples changent.
- `unsafe` : **interdit**. L'arène reste un `Vec` de POD.
- API publique : `Search::new(pos, hist, limits, seed)` garde sa signature ; `Search::with_threads(n)` et `Search::with_policy(p)` s'ajoutent en constructeurs fluides. `Limits` **gagne** deux champs (`c_puct`, `root_noise`) — tous les littéraux `Limits { .. }` du dépôt sont mis à jour.
- Le budget de nœuds reste **honnête** : `go nodes N` consomme N nœuds au total, quel que soit `Threads`.
- `cargo clippy --release --tests --bins --lib -- -D warnings` et `rustfmt --edition 2024 --check <fichiers touchés>` doivent être propres (le clippy global est cassé sur `main` par des exemples hors périmètre).
- Mesure : **même session**, hôte jamais inactif, rapport de **ratios** (nps, qualité à nœuds constant), jamais de seuil absolu. A/B/A/B obligatoire.
- Chaque tâche se termine par une suite verte et un commit unique, en français.

## Hors périmètre

- **NNUE / tensorisation / mapping d'actions** : D1. L'espace d'actions (4672 = 64 × 73) suppose une table de 73 types de coups que l'encodage `Move(u16)` (from/to/flags) ne fournit pas : le format doit être arbitré **quand il y aura un réseau**, pas avant.
- **Table de transposition (B3)** : le Zobrist incrémental existe déjà, mais les valeurs transposées en zero-sum avec des priors exigent un redéfini du contrat de valeur. Phase suivante.
- **Réutilisation d'arbre entre `go` (B2)** : phase suivante.
- **FPU (A3)** : le PUCT de la phase 4 utilise `Q = 0` pour un enfant non visité (convention AlphaZero). Le FPU est une refinement séparé.
- **Bruit de Dirichlet (C1) complet** : la phase 4 n'implémente que l'amorce nécessaire au parallélisme racine (Gumbel sur les priors de la racine, une fois par recherche). Les paramètres α/ε et la politique d'exploration du self-play restent à faire.
- **`src/movegen.rs` n'est pas modifié** : le tri MVV-LVA ne se fait **pas** dans le générateur (coût sur le chemin chaud, bénéfice nul en MCTS) mais dans le **prior**, calculé une fois par nœud développé.

## Le point qui tranche la conception

En alpha-beta, l'ordre des coups décide de la **vitesse d'élagage**. En MCTS, il n'y a pas d'élagage : ce qui compte, c'est **quel sous-arbre est exploré en premier** et **combien de temps il est gardé**. Le pendant exact du tri est donc :

1. le **prior** `P(a)` (le MVV-LVA), qui décide de l'ordre de développement ;
2. le **bonus d'exploration** `c_puct · P(a) · √N / (1 + n(a))`, qui décide de la persistance.

C'est ce que la phase 4 implémente, et c'est pourquoi le tri MVV-LVA **n'est pas** ajouté au générateur.

## Structure des fichiers

| Fichier | Rôle | Tâche |
|---|---|---|
| `src/mcts/policy.rs` | **nouveau** : priors MVV-LVA, distribution, perturbation de la racine | 1 |
| `src/mcts/arena.rs` | `Edge::prior` devient utilisé ; `add_edges` porte les priors | 1 |
| `src/mcts.rs` | `Policy`, PUCT, priors à l'expansion, worker/limites | 1, 2 |
| `src/uci.rs` | options `Policy`, `Cpuct`, `RootNoise` ; ligne `info` | 3 |
| `tests/mcts_phase4.rs` | **nouveau** : propriétés des priors, déterminisme, mats, A/B qualité | 1, 2, 4 |
| `tests/mcts_phase3.rs` | le différentiel v1 est **épinglé** sur `Policy::Uct` | 2 |
| `benches/mcts.rs`, `examples/mcts_probe.rs`, `examples/mcts_quality.rs` | groupes et sondes PUCT | 4 |
| `docs/mcts/phase4-2026-10-22.md` | **nouveau** : rapport de mesure | 4 |

---

### Task 1: Les priors de policy (`src/mcts/policy.rs`)

**Interfaces:**
- Consumes: `mychess::position::Position`, `mychess::types::Move`, `mychess::eval::material`.
- Produces: `mychess::mcts::policy::{priors, perturb, raw_units}` (module public).

- [ ] **Step 1: le score MVV-LVA**

  `raw_units(pos, mv)` : 0 pour un coup calme ; `PRIOR_CAPTURE_WEIGHT · (10·victim − attacker) / 8900` pour une capture ; `PRIOR_PROMOTION_WEIGHT · valeur_promotion / 900` pour une promotion. La capture en passant prend un pion ; la promotion et la capture s'additionnent.

  La formule est le MVV-LVA classique : la victime compte dix fois, l'attaquant une fois — **un pion qui prend une dame (8900) prime sur une dame qui prend un pion (100)**.

- [ ] **Step 2: la distribution**

  `priors(pos, moves)` : `w_i = 1 + raw_units_i`, puis `P_i = (1 − ε)·w_i/Σw + ε/n` avec `ε = PRIOR_EPS = 0.25`. Somme = 1 exactement, tous les `P_i > 0`.

- [ ] **Step 3: la perturbation de la racine**

  `perturb(priors, rng, scale)` : bruit de Gumbel sur le logarithme des priors (`P' ∝ P·exp(g)`), renormalisé. Appelé **une fois par recherche**, sur la racine, dans le **mode parallèle seulement**.

### Task 2: PUCT dans le moteur

- [ ] **Step 1: `Policy` + `Limits`**

  `pub enum Policy { Uct, Puct }` (défaut `Puct`), `Search::with_policy`. `Limits` gagne `c_puct` (défaut 1.5) et `root_noise` (défaut 1.0).

- [ ] **Step 2: les priors à l'expansion**

  À l'expansion d'un nœud, `policy::priors` est calculé et rangé dans les arêtes (`Edge::prior`). Le chemin UCT ne calcule rien (le flux RNG et le comportement restent intacts).

- [ ] **Step 3: la sélection PUCT**

  `s(a) = Q(a) + c_puct·P(a)·√N/(1+n(a))`, `Q(a) = −sum(a)/n(a)` (convention de signe v1 : la somme de l'arête est côté **joueur au trait de l'enfant**), `Q = 0` si `n = 0`. Le tirage UCT d'origine est déplacé tel quel dans `select_uct` (frozen).

### Task 3: UCI

- [ ] **Step 1: les options** `Policy` (`puct`/`uct`), `Cpuct`, `RootNoise`.
- [ ] **Step 2: la ligne `info`** annonce la politique.

### Task 4: tests, mesures, rapport

- [ ] **Step 1: `tests/mcts_phase4.rs`** — distribution, ordre MVV-LVA (pion>dame), déterminisme, mat en un, légalité, A/B qualité.
- [ ] **Step 2: `tests/mcts_phase3.rs`** — le différentiel v1 est épinglé sur `Policy::Uct` (le comportement mono par défaut change, c'est voulu).
- [ ] **Step 3: mesures** — nps (mono, parallèle), RSS, qualité à nœuds constant, A/B/A/B dans la même session.
- [ ] **Step 4: rapport** `docs/mcts/phase4-2026-10-22.md` + README + ELO_LOG.

## Gr gates de sortie

| Gate | Critère |
|---|---|
| G1 | Le chemin `Policy::Uct` reste **bit-à-bit** identique à v1 (différentiel de la phase 3, épinglé). |
| G2 | Les priors somment à 1, sont strictement positifs, et l'ordre MVV-LVA est respecté (pion prend dame > dame prend pion > coup calme). |
| G3 | Le chemin PUCT est **déterministe** (mono) et **légal** (le coup rendu est dans la liste légale). |
| G4 | Le mat en un est trouvé en mono **et** en parallèle. |
| G5 | Le parallèle garde son avantage : à budget de nœuds constant, le résultat parallèle n'est **pas** le résultat mono à `N/T` nœuds (diversité des workers). |
