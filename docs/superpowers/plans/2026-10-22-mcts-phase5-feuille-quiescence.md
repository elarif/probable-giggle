# Phase 5 — feuille tactiquement résolue (quiescence-lite) : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rendre la **valeur de feuille** du MCTS **tactiquement correcte** : au lieu d'évaluer statiquement une position quelconque, **résoudre les échanges** qui s'y jouent avant d'évaluer (quiescence-lite sur le générateur captures-seulement déjà présent), et corriger au passage le **défaut de budget temps** découvert par le contrôle Elo de la phase 4.

**Contexte (ce que la phase 4 a laissé).** La phase 4 (PUCT + priors) est **acceptée** : +399,7 / +596,5 / +450,4 Elo aux trois budgets nœuds-fixes, et +303,3 Elo au contrôle temps-fixe. Le moteur a gagné en *décision* ; son point faible s'est déplacé vers son *évaluation*. La feuille est aujourd'hui `eval_for_mover(pos)` — matériel + PST — sur une position **quelconque** : si cette position est au milieu d'un échange, l'éval est **aveugle** à la capture qui va se produire.

**Mesure de départ** (`examples/leaf_probe`, corpus de playouts aléatoires) : l'écart entre l'éval de feuille actuelle et la valeur après résolution des captures est de **351 cp en moyenne** sur les positions prises dans un échange, et l'écart dépasse **une tour dans 19 %** de ces positions. Le générateur captures-seulement existe déjà (`GenKind::CapturesOnly`) mais **n'est utilisé par rien**.

**Architecture:** `src/mcts.rs` garde son algorithmique (arène plate, terminaison, PUCT/UCT, parallélisme racine). Ce qui change est **la fonction qui produit la valeur d'une feuille**, plus un **réglage de temps** :

- un nouveau module `src/mcts/qsearch.rs` expose `qvalue(pos, depth) -> i32` : une recherche **captures-seulement** (quiescence) qui résout la chaîne d'échanges sur `depth` plies et rend la valeur **pour le trait**, avec **stand-pat** (l'éval statique reste une option) ;
- `mcts::leaf_value(pos, hist, leaf_q)` applique la terminalité **d'abord** (les valeurs exactes {-1, 0, 1} ne sont jamais remplacées), puis la valeur résolue quand `leaf_q > 0`, sinon l'éval statique (comportement de la phase 4, **bit-à-bit** conservé) ;
- `Limits` gagne un champ `leaf_q: u32` (`0` = off) et l'option UCI `LeafQ` le rend pilotable — c'est ce qui permet l'A/B **sans changer une ligne de l'algorithme** ;
- `src/uci.rs` corrige le **budget temps** : le plancher de 50 ms et l'absence de marge font que le moteur réfléchit plus longtemps que son temps restant (158/200 parties adjugées sur dépassement en TC 1+0.01).

**Tech Stack:** Rust edition 2024, MSRV 1.97, `criterion` 0.5 (dev), `fastchess` (compilé depuis les sources, hors dépôt).

## Global Constraints

- Rust edition 2024, toolchain épinglée `1.97.0`. Ne pas la changer.
- **Le chemin par défaut de la phase 4 doit rester bit-à-bit** : `leaf_q = 0` ⇒ `leaf_value` est **exactement** l'ancienne fonction. Le test différentiel de la phase 3 (épinglé sur `Policy::Uct`) et les tests de la phase 4 doivent continuer de passer **sans modification**.
- `src/movegen.rs`, `src/position.rs`, `src/types.rs`, `src/tables.rs`, `src/slider.rs` sont **gelés**. Seul `src/mcts.rs` (+ `src/mcts/qsearch.rs`), `src/uci.rs`, les tests, les bancs et les exemples changent. `src/eval.rs` est lu, pas modifié.
- `unsafe` : **interdit**.
- API publique : `Search::new(...)` garde sa signature ; `Limits` **gagne** un champ (`leaf_q`) — tous les littéraux `Limits { .. }` du dépôt sont mis à jour. `Search::with_leaf_q(n)` s'ajoute en constructeur fluent (miroir de `with_policy`).
- Le budget de nœuds reste **honnête** : `go nodes N` consomme N nœuds au total, quel que soit `Threads`.
- `cargo clippy --release --tests --bins --lib -- -D warnings` et `rustfmt --edition 2024 --check <fichiers touchés>` doivent être propres.
- Mesure : **même session**, rapport de **ratios** (nps, qualité à nœuds constant), jamais de seuil absolu.
- Chaque tâche se termine par une suite verte et un commit unique, en français.

## Hors périmètre

- **NNUE / tensorisation / mapping d'actions** (D1) : le pipeline « logits → softmax → prior » de la phase 4 est prêt, mais l'entraînement et le service sont un programme à part.
- **Table de transposition (B3)** : le Zobrist incrémental existe, mais le contrat de valeur transposée en zero-sum n'est pas défini. **Avant** de l'implémenter, mesurer le taux de transposition réel (sonde) — hors de cette phase.
- **Réutilisation d'arbre (B2)**, **tree-parallel + virtual loss (B1)**, **bruit de Dirichlet (C1)**, **FPU (A3)**, **réglage de `c_puct` (A4)** : hors de cette phase.
- **Élargir l'éval** (mobilité, sécurité du roi, taper — C3) : la phase 5 ne touche qu'à la *feuille*, pas au contenu de l'éval.
- **Modifier le générateur** : `GenKind::CapturesOnly` est déjà là et suffit. Pas de tri MVV-LVA dans le générateur (la phase 4 l'a posé : en MCTS l'ordre ne compte qu'une fois par nœud).

## Le point qui tranche la conception

La valeur de feuille est **la valeur de la position pour le trait**. Deux sémantiques sont possibles pour la « résolution des captures » :

1. **sans stand-pat** : la valeur est celle de la *meilleure chaîne de captures* ;
2. **avec stand-pat** : la valeur est le **max** entre l'éval statique (le trait joue un coup calme) et la meilleure chaîne de captures.

C'est la sémantique **standard de la quiescence** (stand-pat), et c'est celle retenue : le trait peut toujours refuser l'échange. Elle a une conséquence importante et **assumée** : la valeur de feuille regarde **un coup plus loin** que la position. C'est exactement l'effet horizon que la quiescence corrige en alpha-beta, et c'est la raison d'être de la phase — mais cela signifie que l'arbre **double compte** partially les tactiques qu'il explorerait de lui-même si le budget le permettait. La phase mesure donc l'effet **net**, pas théorique.

## Structure des fichiers

| Fichier | Rôle | Tâche |
|---|---|---|
| `src/mcts/qsearch.rs` | **nouveau** : `qvalue` (quiescence captures-seulement, stand-pat) | 1 |
| `src/mcts.rs` | `leaf_value` paramétré, `Limits.leaf_q`, worker, `with_leaf_q` | 1, 2 |
| `src/uci.rs` | option `LeafQ` ; **correctif du budget temps** | 3 |
| `tests/mcts_phase5.rs` | **nouveau** : sémantique de `qvalue`, leaf, budget temps, déterminisme | 1, 2, 4 |
| `tests/mcts_phase3.rs`, `tests/mcts_phase4.rs` | inchangés (le défaut `leaf_q = 0` doit les laisser verts) | 4 |
| `examples/leaf_probe.rs` | **nouveau** : sonde d'erreur de feuille (diagnostic) | 1 |
| `examples/mcts_probe.rs`, `examples/mcts_tactics.rs` | option `leaf_q` dans les sondes | 5 |
| `docs/mcts/phase5-2026-10-22.md` | **nouveau** : rapport de mesure | 5 |

## Tâches

### Tâche 1 — `qvalue` (le cœur de la phase)

- [ ] Créer `src/mcts/qsearch.rs` :
  - `pub fn qvalue(pos: &Position, depth: u32) -> i32` — negamax captures-seulement, **stand-pat** (`eval_for_mover`), valeur **pour le trait**, en centipions ;
  - profondeur 0 ⇒ l'éval statique ; pas de capture disponible ⇒ l'éval statique ;
  - aucune allocation : `movegen::generate_captures` renvoie une `MoveList` de pile, `make`/`unmake` par capture.
- [ ] Tests unitaires dans le module : position à capture gratuite (la valeur monte), échange Pair (la valeur reste équilibrée), position calme (valeur == éval statique), profondeur 0 (identique à l'éval).
- [ ] Regrouper l'existant `examples/leaf_probe.rs` sur `qvalue` (plus de doublon de recherche) et le committer comme sonde.

### Tâche 2 — `leaf_value` paramétré

- [ ] `pub fn leaf_value(pos: &Position, hist: &[u64], leaf_q: u32) -> f32` : terminalité **d'abord** (exact, jamais remplacé), puis `qvalue` si `leaf_q > 0`, sinon l'éval statique.
- [ ] `Limits.leaf_q: u32` (défaut `0`), `Worker.leaf_q`, passage dans `search_once` (chemin mono **et** parallèle).
- [ ] `Search::with_leaf_q(n)` (constructeur fluent).
- [ ] Mettre à jour **tous** les littéraux `Limits { .. }` du dépôt (tests, exemples, bancs).

### Tâche 3 — option UCI + correctif du budget temps

- [ ] Option UCI `LeafQ` (entier, défaut `0`), parsée et passée dans `Limits`.
- [ ] **Correctif du budget temps** (`parse_go`) :
  - le budget ne doit **jamais** dépasser le temps restant moins une marge (le plancher de 50 ms est la cause des 158/200 adjugations) ;
  - une **marge** de sécurité est réservée pour l'aller-retour UCI et la première allocation de l'arène (l'overshoot mesuré est de ~9 ms) ;
  - le chemin `movetime` reçoit la même marge.
- [ ] Test : `go wtime 20` ⇒ le moteur répond **en moins de 20 ms** (plus de flag possible).

### Tâche 4 — filet de tests

- [ ] `tests/mcts_phase5.rs` : sémantique de `qvalue`, `leaf_value` (terminalité prioritaire), **déterminisme** (même graine ⇒ même coup avec `leaf_q > 0`), légalité des coups, budget de nœuds exact, **parité `leaf_q = 0`** (le chemin par défaut est bit-à-bit le même qu'en phase 4).
- [ ] `cargo test --release` : les suites phases 3 et 4 passent **sans modification**.

### Tâche 5 — mesure

- [ ] Débit : `mcts_probe` avec `leaf_q ∈ {0, 1, 2, 4}` (coût de la feuille sur le chemin chaud).
- [ ] Erreur de feuille : `leaf_probe` (avant/après).
- [ ] **Échelle Elo** : candidat = phase 5 (`leaf_q = d`), référence = phase 4 gelée, nœuds-fixes 2k/10k/40k + TC 8+0.08, avec le contrôle « phase 4 forcé `Policy=uct` » comme témoin.
- [ ] Rapport `docs/mcts/phase5-2026-10-22.md`, `ELO_LOG.md`, `README.md`.

## Gates

| Gate | Critère |
|---|---|
| G1 | `leaf_q = 0` est **bit-à-bit** le chemin de la phase 4 (différentiel phase 3 + tests phase 4 verts sans modification) |
| G2 | `qvalue` a la sémantique annoncée (stand-pat, valeur du trait) — tests unitaires |
| G3 | le moteur ne se fait **plus** flaguer en TC court (budget temps borné par le temps restant) |
| G4 | le débit ne se dégrade pas de plus de ~40 % à `leaf_q = 1` (sinon le rapport le dit et le réglage par défaut reste 0) |
| G5 | verdict Elo sur l'échelle nœuds-fixes **et** temps-fixe : accepté (H1) ou **reverté** avec le motif écrit dans le rapport |

## Risques

- **Coût de la feuille** : la quiescence est sur le chemin chaud (une évaluation par nœud). Le coût doit être mesuré (G4) ; s'il est prohibitif, le réglage par défaut reste `0` et le rapport documente le négatif.
- **Double comptage** : la feuille regarde un coup plus loin ; l'arbre explorerait de lui-même certaines de ces tactiques. L'effet **net** est mesuré, pas supposé.
- **Biais de l'éval** : l'éval statique (matériel + PST) reste la feuille de la quiescence ; une chaîne d'échanges qui s'évalue mal ne sont pas corrigées.
- **Un résultat négatif est un résultat** : si l'Elo ne suit pas, la phase est **revertée** et le rapport le dit — comme pour la PUCT formelle de la phase 4.
