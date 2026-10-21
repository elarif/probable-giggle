# Réécriture du move generator depuis la série Grand Chess Tree — Plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produire un move generator Rust **idiomatique, sans erreur de compilation et fidèle à la spécification des 7 posts Grand Chess Tree**, dans un module neuf `src/gct/`, validé pièce par pièce contre des oracles indépendants puis validé bout en bout contre la table de perft détaillée de `https://grandchesstree.com/perft-results`, et mesuré contre le movegen existant de `mychess` et contre l'engine `motors` (toanth/motors).

**Architecture:** Un arbre de modules neuf `src/gct/` (`attacks`, `legality`, `generate`, `perft`) qui **n'utilise pas** `src/movegen.rs` : il lit l'état du plateau directement depuis `crate::position::Position` (les 12 bitboards + le mailbox 8×8 existent déjà) et reconstruit la chaîne du post — couche d'attaques (Part 4), les trois bitboards de légalité `king_danger` / `checkers` / `pins` dans **trois passes séparées** comme le post (Part 7), puis le générateur légal dans l'ordre du post (roi d'abord, retour immédiat sur double échec, `check_mask`, pions, cavaliers, fous/tours/dames, roque en dernier). `src/movegen.rs` reste **intact** : il devient l'oracle de différentiel du nouveau module. Le portage corrige les erreurs de compilation du listing C# (types `Span<ulong>` vs `ulong[]`, `GenEnPassant` à 4 arguments pour 6 déclarés, `Rank3`/`Rank6` manquants, `EmitTargets` hors classe, `delegate AttackFn`) au lieu de les recopier, et remplace les détours du post par l'équivalent idiomatique quand il est prouvablement équivalent (pas de `delegate`, dame générée en un seul passage avec `bishop ∪ rook`, `pin_lines[sq] = u64::MAX` pour les pièces non clouées au lieu d'un test d'appartenance).

**Tech Stack:** Rust 1.97 (edition 2024), `cargo test --release`, `criterion` 0.5 (dev-dependency déjà présente), aucune nouvelle dépendance de crate. Vérification perft contre la table JSON Grand Chess Tree déjà extraite (`/tmp/perft_ref.json`, à committer). Comparaison externe via le binaire `motors` (UGI, commande `go perft N`).

---

## Préréquis contrôleur (avant toute dispatch)

Ces étapes ne sont pas des tâches : elles se font une fois, dans la session du contrôleur, avant de lancer le premier implémenteur.

- [ ] Créer un worktree isolé depuis `main` (`.worktrees/` est déjà dans `.gitignore`) :

```bash
cd /home/elarif/mychess
git worktree add .worktrees/movegen-gct -b refactor/movegen-gct main
cd .worktrees/movegen-gct
cargo build --release    # doit réussir avant de commencer
```

- [ ] Créer le répertoire de travail SDD : `~/.agents/skills/subagent-driven-development/scripts/sdd-workspace`
- [ ] Vérifier que la machine est au repos avant toute mesure (`cat /proc/loadavg`), et noter la valeur dans le ledger.

Toutes les commandes des tâches ci-dessous s'exécutent **dans `.worktrees/movegen-gct`**, sauf mention contraire.

---

## Global Constraints

- **Aucune nouvelle dépendance de crate.** Le crate a exactement une dépendance d'exécution (`rayon`) et deux dev-dependencies (`criterion`, `microbench`). Le plan n'en ajoute aucune.
- **MSRV Rust 1.97.** `u64::isolate_lowest_one` est stable depuis 1.97 et doit être utilisé plutôt que `bb & bb.wrapping_neg()`.
- **`src/movegen.rs`, `src/position.rs`, `src/types.rs`, `src/tables.rs`, `src/slider.rs`, `src/perft.rs` ne sont pas modifiés par les tâches 1 à 6.** Le nouveau module est purement additif (`pub mod gct;` dans `src/lib.rs`). La seule exception est la **tâche 7**, qui corrige un bug prouvé dans `src/position.rs` et est explicitement étiquetée « phase 2 ».
- **Le core est déclaré gelé** (`MIGRATION.md`, GATE 2.5) : « pas de refactoring core sauf bug PROUVÉ (perft revalidé alors, filet complet) ». Les tâches 1 à 6 respectent le gel (elles n'ajoutent qu'un module). La tâche 7 invoque l'exception avec un bug prouvé et une revalidation perft complète.
- **Le contrat d'entrée des générateurs est immuable** (GATE 2.5 M3) : une position **légale** est requise. Sur entrée illégale (roi absent, roi adverse capturable) le comportement doit rester **défini, silencieux et sans panique**. Le générateur `gct` doit satisfaire le même contrat.
- **La sortie publique de `src/movegen.rs` reste stable** : `MoveList`, `generate`, `count_moves`, `attackers_to`, `generate_all`, `generate_captures`. Aucune tâche ne change ces signatures (consommateurs : `src/perft.rs:3`, `src/memo.rs:12,128,145`, `src/mcts.rs:37`, `src/uci.rs:264-265`, `src/refgen.rs:13`).
- **`unsafe` reste confiné** à l'appel BMI2 `_pext_u64` de `slider.rs`. Le nouveau module n'introduit aucun `unsafe`.
- **Les deux chemins slider restent bit-identiques** (`tests/slider_paths.rs` force `pext` et `rays`) : le module `gct` doit obtenir exactement les mêmes ensembles d'attaques depuis `crate::slider::{bishop_attacks, rook_attacks}`.
- **Le filet de preuve de `build.rs` est intouchable** : aucune tâche ne modifie `build.rs` ni ne contourne la preuve de magie à la compilation.
- **Aucun résultat de perft existant ne doit changer.** Après chaque tâche, `cargo test --release` doit passer et les 7 goldens de `tests/perft/*.txt` doivent correspondre exactement (nœuds **et** colonnes détaillées).
- **Convention de la colonne `checks` de la table Grand Chess Tree** : la table GCT compte les échecs **hors mat** ; `mychess` compte les échecs **mat inclus**. Donc `mychess.checks == gct.checks + gct.mates`. Les colonnes `discovery` et `double` de la table GCT ne sont **pas** comparables à celles de `mychess` (décomposition différente, cf. `docs/ecart_d9_convention.md`) et sont stockées comme `-`. La colonne `unique` de la table GCT compte des **positions distinctes** et n'est jamais importée.
- **Politique de mesure** (tâche 8 uniquement) : A/B/A/B intercalé dans la **même session**, ≥ 4 legs, **médiane** retenue, jamais de seuil nps absolu en CI. Mesurer aussi `perf stat -e instructions,cycles` (insensible à la charge de l'hôte).
- **Un commit par tâche**, message en français, citant la commande de vérification qui le justifie.
- **Aucune tâche ne modifie `test.sh`** (protocole SPRT gelé) ni `MIGRATION.md` ni `ELO_LOG.md`.

---

## Hors périmètre (à ne pas implémenter ici)

Ce plan couvre **la phase 1** : la réécriture fidèle au post, sans spécialisation MCTS. Les améliorations de movegen (phase 2) sont listées ici pour mémoire et **aucune tâche ne les implémente**, sauf la tâche 7 qui traite le seul élément qualifié de *bug prouvé* :

| Amélioration (phase 2) | Source | Statut |
|---|---|---|
| `has_legal_move` (terminalité rapide, sortie anticipée) | `docs/analyse_movegen_mcts.md` §4.2, P0-2 | hors périmètre |
| Buffer réutilisable sans zéro-initialisation (`MoveList::new()` ≈ 15 ns = 9–14 % de `generate`) | §4.3, P1 | hors périmètre |
| Micro-légalité / fusion des trois passes de légalité en une | §4.4, P1 | hors périmètre — le post exige trois passes séparées, elles sont donc portées telles quelles |
| Ordre de coups, priors, métadonnées de coup | §4.4, P1 | hors périmètre |
| `evasions` / check-info exposés, captures tactiques | §4.5 | hors périmètre |
| Copy-make, parallélisme du générateur | §4.6 | hors périmètre |
| **Canonicalisation du carré en-passant dans le hash Zobrist** | §4.1, bug prouvé | **tâche 7** |

---

## File Structure

| Path | Responsibility |
|---|---|
| `docs/gct/api-contract.md` | **Spécification normative.** Extraction complète du contrat d'API des 7 posts, 1113 lignes, déjà commitée sur `main`. Source de vérité pour toutes les tâches. |
| `docs/gct/perft-results-gct.md` | Table de perft détaillée Grand Chess Tree extraite, avec la note de convention `checks`/`mates`/`discovery`/`double`/`unique`. |
| `tests/perft/gct/startpos.txt` | Golden perft GCT pour la position initiale, colonnes GCT **verbatim** (d0–d7). |
| `tests/perft/gct/kiwipete.txt` | Golden perft GCT pour Kiwipete (d0–d7). |
| `tests/perft/gct/sje.txt` | Golden perft GCT pour SJE's Symmetric Alternative (d0–d7). |
| `tests/gct_oracle.rs` | Charge les goldens GCT, applique la conversion de convention, compare à `perft::perft_detailed`. Prouve aussi que la conversion « mord » (une valeur non convertie échoue). |
| `src/gct/mod.rs` | Racine du module : documentation de provenance, `pub mod attacks; pub mod legality; pub mod generate; pub mod perft;` et ré-exports. |
| `src/gct/attacks.rs` | Part 4 : les huit décalages (`shift_n`, `shift_s`, …), `squares_between` (table 64×64 reconstruite indépendamment), `is_attacked_by`. |
| `src/gct/legality.rs` | Part 7 : `king_danger`, `compute_checkers`, `compute_pins` (trois passes séparées, `pin_lines` à `u64::MAX` pour les non-clouées). |
| `src/gct/generate.rs` | Part 7 : `generate` — roi, retour double échec, `check_mask`, pions/promotions, cavaliers, fous/tours/dames, **en-passant** (tâche 5), roque (tâche 5). |
| `src/gct/perft.rs` | Part 7 : `perft` (bulk au bord via la longueur de liste) et `perft_detailed` (réutilise `crate::perft::PerftCounts`). |
| `examples/gctperft.rs` | Petit binaire de mesure : `gctperft <fen> <depth>` → nœuds + Mnps, pour la tâche 6 et la tâche 8. Aucune modification de la CLI `mychess`. |
| `tests/gct_attacks.rs` | Oracles de la tâche 2. |
| `tests/gct_legality.rs` | Oracles indépendants (marche de rayons) des trois bitboards, tâche 3. |
| `tests/gct_equivalence.rs` | Équivalence `gct::generate` ≡ `movegen::generate` (FENs + auto-jeu déterministe), tâche 5. |
| `tests/gct_perft.rs` | Validation bout en bout contre les goldens GCT, tâche 6. |
| `tests/ep_zobrist.rs` | Tests du bug de canonicalisation en-passant, tâche 7. |
| `benches/gct.rs` | Criterion : `gct::generate` vs `movegen::generate`, `gct::perft` vs `perft`. |
| `tools/compare_movegen.sh` | Harnais de comparaison `mychess` vs `motors` (clone + build pinné, perft UGI, extraction des Mnps). |
| `docs/movegen/gct-benchmark-2026-10-21.md` | Rapport de mesure A/B/A/B, `perf stat`, verdict. |
| `src/lib.rs` | Une ligne ajoutée : `pub mod gct;`. |
| `src/position.rs` | **Tâche 7 uniquement** : canonicalisation du carré ep dans `make` et `from_fen`. |
| `Cargo.toml` | Une entrée `[[bench]] name = "gct"` ajoutée à la tâche 8. |

---

### Task 1: Spécification normative et oracles de perft Grand Chess Tree

**Files:**
- Create: `docs/gct/api-contract.md` (déplacement de `api-contract.md`)
- Create: `docs/gct/perft-results-gct.md`
- Create: `tests/perft/gct/startpos.txt`, `tests/perft/gct/kiwipete.txt`, `tests/perft/gct/sje.txt`
- Create: `tests/gct_oracle.rs`
- Test: `tests/gct_oracle.rs`

**Interfaces:**
- Consumes: `mychess::perft::{perft_detailed, PerftCounts}`, `mychess::position::Position`.
- Produces: fonction `gct_checks(gct_checks: u64, gct_mates: u64) -> u64` (privée, dans le test) et le format de golden `tests/perft/gct/*.txt` que la tâche 6 réutilisera.

- [ ] **Step 1: Vérifier que la spécification est bien en place**

`docs/gct/api-contract.md` est déjà commité sur `main` (1113 lignes) : il est donc présent dans le worktree. Vérification seule, aucun déplacement :

```bash
cd .worktrees/movegen-gct
wc -l docs/gct/api-contract.md
head -3 docs/gct/api-contract.md
```

Attendu : 1113 lignes, et un `head` qui commence par `# API Contract — "The Pipeline and Perft"`. Si le fichier manque, s'arrêter et le signaler au contrôleur — ne pas ré-extraire les posts.

- [ ] **Step 2: Écrire la note de convention de perft**

Créer `docs/gct/perft-results-gct.md` :

```markdown
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
`perft::perft_detailed`. Trois colonnes se convertissent, trois non.

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

## Portée retenue

Les goldens s'arrêtent à **d7** : au-delà, la table GCT devient inexploitable
en temps raisonnable sur l'hôte de développement (d8 = 84,9 Md de nœuds pour
startpos). Les profondeurs d0–d7 suffisent à valider le générateur, y compris
les colonnes détaillées.
```

- [ ] **Step 3: Écrire les trois goldens GCT (valeurs GCT verbatim)**

Créer `tests/perft/gct/startpos.txt` :

```
# gct-startpos  rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1
# source: https://grandchesstree.com/perft-results (Grand Chess Tree)
# valeurs VERBATIM de la table GCT ; conversion appliquee par tests/gct_oracle.rs :
#   mychess.checks = checks + mates ; mychess.mates = mates
# colonnes discovery/double : non comparables (voir docs/ecart_d9_convention.md) -> "-"
rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1
# depth nodes captures ep castles promotions checks discovery double mates
0 1 0 0 0 0 0 - - 0
1 20 0 0 0 0 0 - - 0
2 400 0 0 0 0 0 - - 0
3 8902 34 0 0 0 12 - - 0
4 197281 1576 0 0 0 461 - - 8
5 4865609 82719 258 0 0 27004 - - 347
6 119060324 2812008 5248 0 0 798271 - - 10828
7 3195901860 108329926 319617 883453 0 32668081 - - 435767
```

Créer `tests/perft/gct/kiwipete.txt` :

```
# gct-kiwipete  r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -
# source: https://grandchesstree.com/perft-results (Grand Chess Tree)
# valeurs VERBATIM de la table GCT ; conversion appliquee par tests/gct_oracle.rs :
#   mychess.checks = checks + mates ; mychess.mates = mates
# colonnes discovery/double : non comparables (voir docs/ecart_d9_convention.md) -> "-"
r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -
# depth nodes captures ep castles promotions checks discovery double mates
0 1 0 0 0 0 0 - - 0
1 48 8 0 2 0 0 - - 0
2 2039 351 1 91 0 3 - - 0
3 97862 17102 45 3162 0 992 - - 1
4 4085603 757163 1929 128013 15172 25480 - - 43
5 193690690 35043416 73365 4993637 8392 3279716 - - 30171
6 8031647685 1558445089 3577504 184513607 56627920 91878047 - - 360003
7 374190009323 69479646226 117075132 7831249352 49612944 8306459296 - - 111664735
```

Créer `tests/perft/gct/sje.txt` :

```
# gct-sje  r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10
# source: https://grandchesstree.com/perft-results (Grand Chess Tree)
# valeurs VERBATIM de la table GCT ; conversion appliquee par tests/gct_oracle.rs :
#   mychess.checks = checks + mates ; mychess.mates = mates
# colonnes discovery/double : non comparables (voir docs/ecart_d9_convention.md) -> "-"
r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10
# depth nodes captures ep castles promotions checks discovery double mates
0 1 0 0 0 0 0 - - 0
1 46 4 0 0 0 1 - - 0
2 2079 203 0 0 0 40 - - 0
3 89890 9470 0 0 0 1783 - - 0
4 3894594 440388 0 0 0 68985 - - 0
5 164075551 19528068 122 0 0 2998380 - - 228
6 6923051137 868499408 5167 0 0 115520065 - - 10070
7 287188994746 37337665196 653902 0 0 4991385259 - - 3514747
```

- [ ] **Step 4: Écrire le test oracle (échoue d'abord sur la conversion)**

Créer `tests/gct_oracle.rs` :

```rust
//! Validation contre la table de perft détaillée Grand Chess Tree.
//!
//! Les goldens de `tests/perft/gct/` stockent les colonnes GCT **verbatim**.
//! La conversion vers la convention de `mychess` est faite ici, jamais dans
//! les fichiers — c'est ce qui rend l'oracle indépendant.
//!
//! Conventions (voir `docs/gct/perft-results-gct.md`) :
//!   - `mychess.checks == gct.checks + gct.mates` (GCT compte les échecs
//!     hors mat ; `mychess` les compte mat inclus) ;
//!   - `captures`, `ep`, `castles`, `promotions`, `mates` : identité ;
//!   - `discovery` / `double` : non comparables, stockés `-` ;
//!   - `unique` : compte de positions distinctes, jamais importé.

use mychess::perft::perft_detailed;
use mychess::position::Position;

struct Row {
    depth: u32,
    nodes: u64,
    // captures, ep, castles, promotions, gct_checks, discovery, double, mates
    cols: [Option<u64>; 8],
}

struct Golden {
    fen: String,
    rows: Vec<Row>,
}

fn load(path: &str) -> Golden {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut fen = String::new();
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if fen.is_empty() && line.contains('/') {
            fen = line.to_string();
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        assert!(toks.len() >= 2, "ligne de golden invalide : {line}");
        let depth: u32 = toks[0].parse().unwrap();
        let nodes: u64 = toks[1].parse().unwrap();
        let mut cols = [None; 8];
        for (i, c) in cols.iter_mut().enumerate() {
            if let Some(t) = toks.get(2 + i).filter(|t| **t != "-") {
                *c = Some(t.replace('_', "").parse().unwrap());
            }
        }
        rows.push(Row { depth, nodes, cols });
    }
    assert!(!fen.is_empty(), "{path}: aucune FEN");
    Golden { fen, rows }
}

fn all() -> Vec<Golden> {
    [
        "tests/perft/gct/startpos.txt",
        "tests/perft/gct/kiwipete.txt",
        "tests/perft/gct/sje.txt",
    ]
    .iter()
    .map(|p| load(p))
    .collect()
}

/// `mychess.checks = gct.checks + gct.mates`.
fn gct_checks_to_mychess(gct_checks: u64, gct_mates: u64) -> u64 {
    gct_checks + gct_mates
}

/// La conversion doit « mordre » : si l'oracle appliquait l'identité, les
/// valeurs publiées ne tomberaient pas juste. On le prouve avec les deux
/// points de contrôle cités dans `docs/gct/perft-results-gct.md`.
#[test]
fn la_conversion_des_checks_est_celle_du_document() {
    assert_eq!(gct_checks_to_mychess(461, 8), 469, "startpos d4");
    assert_eq!(gct_checks_to_mychess(992, 1), 993, "kiwipete d3");
    assert_eq!(gct_checks_to_mychess(27004, 347), 27351, "startpos d5");
    assert_eq!(gct_checks_to_mychess(2998380, 228), 2998608, "sje d5");
}

fn check_row(g: &Golden, row: &Row) {
    let mut pos = Position::from_fen(&g.fen).unwrap();
    let c = perft_detailed(&mut pos, row.depth);
    assert_eq!(c.nodes, row.nodes, "{} d{} nodes", g.fen, row.depth);

    let got = [
        Some(c.captures),
        Some(c.ep),
        Some(c.castles),
        Some(c.promotions),
        row.cols[4].map(|chk| gct_checks_to_mychess(chk, row.cols[7].unwrap_or(0))),
        None, // discovery : non comparable
        None, // double : non comparable
        Some(c.mates),
    ];
    let names = [
        "captures",
        "ep",
        "castles",
        "promotions",
        "checks",
        "discovery",
        "double",
        "mates",
    ];
    for i in 0..8 {
        if let Some(want) = got[i] {
            let mine = [
                c.captures,
                c.ep,
                c.castles,
                c.promotions,
                c.checks,
                0,
                0,
                c.mates,
            ][i];
            assert_eq!(mine, want, "{} d{} {}", g.fen, row.depth, names[i]);
        }
    }
}

#[test]
fn t1_gct_shallow() {
    for g in all() {
        for row in &g.rows {
            if row.depth > 4 {
                continue;
            }
            check_row(&g, row);
        }
    }
}

#[test]
#[ignore = "T2: table GCT aux profondeurs 5 et 6 (minutes, mono-thread)"]
fn t2_gct_depth5_6() {
    for g in all() {
        for row in &g.rows {
            if !(5..=6).contains(&row.depth) {
                continue;
            }
            check_row(&g, row);
        }
    }
}

#[test]
#[ignore = "T3: table GCT a la profondeur 7 (long)"]
fn t3_gct_depth7() {
    for g in all() {
        for row in &g.rows {
            if row.depth != 7 {
                continue;
            }
            check_row(&g, row);
        }
    }
}
```

- [ ] **Step 5: Lancer le test pour constater l'état**

```bash
cargo test --release --test gct_oracle -- --nocapture
```

Attendu : `la_conversion_des_checks_est_celle_du_document` et `t1_gct_shallow` passent (le movegen existant est déjà correct ; cette tâche installe l'oracle, elle ne corrige rien). Si `t1_gct_shallow` échoue, c'est la lecture de la table qui est fausse : vérifier la colonne concernée dans `/tmp/perft_ref.json` avant de toucher au code moteur.

- [ ] **Step 6: Vérifier que le reste de la suite est intact**

```bash
cargo test --release
```

Attendu : tous les tests passent, y compris `perft_golden` (les 7 goldens existants ne sont pas touchés).

- [ ] **Step 7: Commit**

```bash
git add docs/gct tests/perft/gct tests/gct_oracle.rs
git commit -m "gct: specification normative + oracles perft de la table Grand Chess Tree"
```

---

### Task 2: Couche d'attaques du post (Part 4)

**Files:**
- Create: `src/gct/mod.rs`
- Create: `src/gct/attacks.rs`
- Modify: `src/lib.rs` (une ligne)
- Test: `tests/gct_attacks.rs`

**Interfaces:**
- Consumes: `crate::position::Position`, `crate::slider::{bishop_attacks, rook_attacks}`, `crate::tables::*`, `crate::types::*`.
- Produces:
  - `gct::attacks::shift_n/s/s_e/…` : `pub const fn shift_x(b: u64) -> u64` pour les 8 directions.
  - `gct::attacks::squares_between(a: u8, b: u8) -> u64`
  - `gct::attacks::is_attacked_by(pos: &Position, sq: u8, by: Color) -> bool`

- [ ] **Step 1: Écrire le test qui échoue**

Créer `tests/gct_attacks.rs` :

```rust
//! Part 4 du post : les décalages de bitboard, la table `SquaresBetween` et
//! `IsAttackedBy`. Trois oracles indépendants :
//!   1. les décalages contre des décalages sur une grille naïve ;
//!   2. `squares_between` contre `slider::between` (implémentations
//!      différentes : marche de rayon ici, table précalculée là) ;
//!   3. `is_attacked_by` contre `Position::is_square_attacked` sur toutes les
//!      positions visitées par un auto-jeu déterministe.

use mychess::gct::attacks::{is_attacked_by, shift_n, shift_s, shift_e, shift_w,
                            shift_ne, shift_nw, shift_se, shift_sw, squares_between};
use mychess::position::Position;
use mychess::types::*;

/// Référence naïve : applique un delta (df, dr) à chaque bit et ne garde que
/// les cases sur l'échiquier. Aucune table, aucun décalage de bitboard.
fn naive_shift(b: u64, df: i32, dr: i32) -> u64 {
    let mut out = 0u64;
    for sq in 0..64u8 {
        if b & (1u64 << sq) == 0 {
            continue;
        }
        let f = (sq % 8) as i32 + df;
        let r = (sq / 8) as i32 + dr;
        if (0..8).contains(&f) && (0..8).contains(&r) {
            out |= 1u64 << ((r * 8 + f) as u32);
        }
    }
    out
}

#[test]
fn les_huit_decalages_correspondent_a_la_grille_naive() {
    // Un motif dense et un motif clairsemé : les deux attrapent les débordements
    // de bord (rangée 1/8, colonne a/h).
    let motifs = [0xFFFF_FFFF_FFFF_FFFFu64, 0x8040_2010_0804_0201];
    for m in motifs {
        assert_eq!(shift_n(m), naive_shift(m, 0, 1), "shift_n");
        assert_eq!(shift_s(m), naive_shift(m, 0, -1), "shift_s");
        assert_eq!(shift_e(m), naive_shift(m, 1, 0), "shift_e");
        assert_eq!(shift_w(m), naive_shift(m, -1, 0), "shift_w");
        assert_eq!(shift_ne(m), naive_shift(m, 1, 1), "shift_ne");
        assert_eq!(shift_nw(m), naive_shift(m, -1, 1), "shift_nw");
        assert_eq!(shift_se(m), naive_shift(m, 1, -1), "shift_se");
        assert_eq!(shift_sw(m), naive_shift(m, -1, -1), "shift_sw");
    }
    // Un bit isole sur chaque case : 64 cas de bord.
    for sq in 0..64u8 {
        let b = 1u64 << sq;
        assert_eq!(shift_n(b), naive_shift(b, 0, 1));
        assert_eq!(shift_s(b), naive_shift(b, 0, -1));
        assert_eq!(shift_e(b), naive_shift(b, 1, 0));
        assert_eq!(shift_w(b), naive_shift(b, -1, 0));
        assert_eq!(shift_ne(b), naive_shift(b, 1, 1));
        assert_eq!(shift_nw(b), naive_shift(b, -1, 1));
        assert_eq!(shift_se(b), naive_shift(b, 1, -1));
        assert_eq!(shift_sw(b), naive_shift(b, -1, -1));
    }
}

/// `Position` n'implémente pas `Debug`, et ajouter le derive toucherait au
/// core gelé : on rend le plateau depuis ses champs publics (`mailbox`,
/// `side`, `castle`, `ep`).
fn describe(pos: &Position) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    for r in (0..8).rev() {
        for f in 0..8 {
            let m = pos.mailbox[r * 8 + f];
            s.push(if m == 0 {
                '.'
            } else {
                b"PNBRQKpnbrqk"[(m - 1) as usize] as char
            });
        }
        s.push('\n');
    }
    let _ = write!(s, "{:?} castle={} ep={}", pos.side, pos.castle, pos.ep);
    s
}

#[test]
fn squares_between_est_identique_a_la_table_precalculee() {
    for a in 0..64u8 {
        for b in 0..64u8 {
            assert_eq!(
                squares_between(a, b),
                mychess::slider::between(a, b),
                "between({a}, {b})"
            );
        }
    }
}

#[test]
fn is_attacked_by_est_identique_a_position_is_square_attacked() {
    // Auto-jeu déterministe : deux couleurs jouent des coups pseudo-aléatoires
    // déterministes, et à chaque nœud on compare les 64 cases pour les deux
    // couleurs.
    let mut pos = Position::startpos();
    let mut rng = 0x9E37_79B9_7F4A_7C15u64;
    for _ply in 0..400 {
        for sq in 0..64u8 {
            for by in [Color::White, Color::Black] {
                assert_eq!(
                    is_attacked_by(&pos, sq, by),
                    pos.is_square_attacked(sq, by),
                    "case {sq} attaquee par {by:?} dans {}", describe(&pos)
                );
            }
        }
        let mut list = mychess::movegen::MoveList::new();
        mychess::movegen::generate(&pos, &mut list, GenKind::All);
        if list.is_empty() {
            break;
        }
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let mv = list.get((rng as usize) % list.len());
        pos.make(mv);
    }
}
```

- [ ] **Step 2: Lancer le test pour le voir échouer**

```bash
cargo test --release --test gct_attacks 2>&1 | tail -20
```

Attendu : échec de compilation — `unresolved import mychess::gct`. C'est le rouge attendu.

- [ ] **Step 3: Créer la racine du module**

Créer `src/gct/mod.rs` :

```rust
//! Portage Rust de la série « Grand Chess Tree » (7 posts), phase 1.
//!
//! Ce module est une **réécriture fidèle** de la chaîne de génération de
//! coups légaux décrite par le post, et non une optimisation de
//! `crate::movegen`. Il est volontairement redondant avec lui : il sert
//! d'oracle croisé, puis de base propre pour les améliorations de phase 2.
//!
//! Correspondance avec les posts :
//!   - Part 4 → [`attacks`] : décalages, `SquaresBetween`, `IsAttackedBy` ;
//!   - Part 7 → [`legality`] : les trois bitboards `danger`, `checkers`,
//!     `pins` (trois passes séparées, comme le post) ;
//!   - Part 7 → [`generate`] : le générateur légal, dans l'ordre du post ;
//!   - Part 7 → [`perft`] : perft bulk au bord.
//!
//! Écarts assumés vis-à-vis du listing C# (tous documentés au site d'appel) :
//!   - le listing ne compile pas tel quel (`Span<ulong>` au lieu de
//!     `ulong[]`, `GenEnPassant` appelé avec 4 arguments pour 6 déclarés,
//!     `Rank3`/`Rank6` absents de la classe `Bitboards`, `EmitTargets`
//!     appelé depuis une classe qui ne le déclare pas, `delegate AttackFn`) ;
//!     le portage corrige, il ne recopie pas ;
//!   - pas de `delegate` pour l'attaque des fous/tours : appels directs
//!     monomorphisés ;
//!   - la dame est générée en un passage (`bishop_attacks | rook_attacks`)
//!     au lieu de deux ;
//!   - `pin_lines[sq] = u64::MAX` pour les pièces non clouées, ce qui rend le
//!     masquage inconditionnel (`targets &= pin_lines[sq]`) ;
//!   - les tables d'attaque sous-jacentes sont celles de `mychess`
//!     (`crate::slider`, `crate::tables`) : les deux chemins slider restent
//!     bit-identiques à `movegen`.

pub mod attacks;
pub mod legality;
pub mod generate;
pub mod perft;
```

Ajouter `pub mod gct;` dans `src/lib.rs`, entre `pub mod eval;` et `pub mod mcts;` (ordre alphabétique) :

```rust
pub mod eval;
pub mod gct;
pub mod mcts;
```

- [ ] **Step 4: Écrire `src/gct/attacks.rs`**

Créer `src/gct/attacks.rs` :

```rust
//! Part 4 du post : décalages de bitboard, `SquaresBetween`, `IsAttackedBy`.

use crate::position::Position;
use crate::slider::{bishop_attacks, rook_attacks};
use crate::tables::{FILE_A, FILE_H, RANK_1, RANK_8};
use crate::types::*;

/// Décalages du post (`Bitboards.ShiftN`, `ShiftS`, …). Le masque de bord est
/// appliqué **avant** le décalage, comme dans le listing : un bit sur la
/// rangée 8 ne « réapparaît » pas sur la rangée 1.
#[inline(always)]
pub const fn shift_n(b: u64) -> u64 {
    (b & !RANK_8) << 8
}
#[inline(always)]
pub const fn shift_s(b: u64) -> u64 {
    (b & !RANK_1) >> 8
}
#[inline(always)]
pub const fn shift_e(b: u64) -> u64 {
    (b & !FILE_H) << 1
}
#[inline(always)]
pub const fn shift_w(b: u64) -> u64 {
    (b & !FILE_A) >> 1
}
#[inline(always)]
pub const fn shift_ne(b: u64) -> u64 {
    (b & !RANK_8 & !FILE_H) << 9
}
#[inline(always)]
pub const fn shift_nw(b: u64) -> u64 {
    (b & !RANK_8 & !FILE_A) << 7
}
#[inline(always)]
pub const fn shift_se(b: u64) -> u64 {
    (b & !RANK_1 & !FILE_H) >> 7
}
#[inline(always)]
pub const fn shift_sw(b: u64) -> u64 {
    (b & !RANK_1 & !FILE_A) >> 9
}

/// `Legality.SquaresBetween` du post, reconstruite ici par marche de rayon
/// plutôt que lue depuis `tables::BETWEEN` : deux algorithmes indépendants qui
/// doivent coïncider (vérifié par `tests/gct_attacks.rs`).
const fn build_squares_between() -> [[u64; 64]; 64] {
    // Mêmes directions que `tables::DIRS` : N, NE, E, SE, S, SW, W, NW.
    const DIRS: [(i32, i32); 8] = [
        (0, 1),
        (1, 1),
        (1, 0),
        (1, -1),
        (0, -1),
        (-1, -1),
        (-1, 0),
        (-1, 1),
    ];
    let mut t = [[0u64; 64]; 64];
    let mut a = 0;
    while a < 64 {
        let mut d = 0;
        while d < 8 {
            let mut f = (a % 8) as i32;
            let mut r = (a / 8) as i32;
            let mut acc = 0u64;
            loop {
                f += DIRS[d].0;
                r += DIRS[d].1;
                if f < 0 || f >= 8 || r < 0 || r >= 8 {
                    break;
                }
                let s = (r * 8 + f) as usize;
                t[a][s] = acc;
                acc |= 1u64 << s;
            }
            d += 1;
        }
        a += 1;
    }
    t
}

static SQUARES_BETWEEN: [[u64; 64]; 64] = build_squares_between();

/// Cases strictement entre `a` et `b` s'ils sont alignés, sinon 0.
#[inline(always)]
pub fn squares_between(a: u8, b: u8) -> u64 {
    SQUARES_BETWEEN[a as usize][b as usize]
}

/// `Attacks.IsAttackedBy` du post : `sq` est-il attaqué par la couleur `by` ?
///
/// Pièces bon marché d'abord (pion, cavalier, roi), glissantes ensuite — comme
/// le post. Le post écrit `Attacks.Pawn[1 - byColor, sq] & pawns` : par
/// symétrie c'est exactement `PAWN_ATTACKERS[by][sq]`, que `mychess` fournit
/// déjà, et `tests/gct_attacks.rs` vérifie l'identité.
#[inline]
pub fn is_attacked_by(pos: &Position, sq: u8, by: Color) -> bool {
    let them = pos.color_bb(by);
    if them == 0 {
        return false;
    }
    let i = by.idx();
    if crate::tables::PAWN_ATTACKERS[i][sq as usize] & pos.bb_piece[PAWN] & them != 0 {
        return true;
    }
    if crate::tables::KNIGHT_ATT[sq as usize] & pos.bb_piece[KNIGHT] & them != 0 {
        return true;
    }
    if crate::tables::KING_ATT[sq as usize] & pos.bb_piece[KING] & them != 0 {
        return true;
    }
    let occ = pos.occ();
    if rook_attacks(sq, occ) & (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]) & them != 0 {
        return true;
    }
    bishop_attacks(sq, occ) & (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]) & them != 0
}
```

- [ ] **Step 5: Créer les modules vides restants pour que le crate compile**

Créer `src/gct/legality.rs`, `src/gct/generate.rs`, `src/gct/perft.rs` avec chacun une seule ligne (ils seront remplis aux tâches 3, 4, 6) :

```rust
//! (rempli a la tache 3 — Part 7 : king_danger, checkers, pins)
```

(et les deux autres avec leur propre en-tête). Sans cela, les `pub mod` de `src/gct/mod.rs` ne compilent pas.

- [ ] **Step 6: Lancer les tests**

```bash
cargo test --release --test gct_attacks -- --nocapture
```

Attendu : 3 tests passent (`les_huit_decalages…`, `squares_between_est_identique…`, `is_attacked_by_est_identique…`).

- [ ] **Step 7: Clippy et suite complète**

```bash
cargo clippy --all-targets --release -- -D warnings
cargo test --release
```

Attendu : zéro avertissement clippy, toute la suite passe.

- [ ] **Step 8: Commit**

```bash
git add src/gct src/lib.rs tests/gct_attacks.rs
git commit -m "gct(part4): decalages, SquaresBetween et IsAttackedBy portes en Rust idiomatique"
```

---

### Task 3: Les trois bitboards de légalité (Part 7)

**Files:**
- Modify: `src/gct/legality.rs`
- Test: `tests/gct_legality.rs`

**Interfaces:**
- Consumes: `crate::gct::attacks::squares_between`, `crate::slider::{bishop_attacks, rook_attacks}`, `crate::tables::{RAYS, DIR_POS, BISHOP_DIRS, ROOK_DIRS, BETWEEN, KNIGHT_ATT, KING_ATT, PAWN_ATTACKS, PAWN_ATTACKERS}`, `crate::position::Position`.
- Produces:
  - `gct::legality::king_danger(pos: &Position, ksq: u8, them: Color) -> u64`
  - `gct::legality::compute_checkers(pos: &Position, ksq: u8, them: Color) -> u64`
  - `gct::legality::compute_pins(pos: &Position, ksq: u8, them: Color, us: Color) -> (u64, [u64; 64])` — `pinned` et `pin_lines` indexé par case, `u64::MAX` pour les cases non clouées.

- [ ] **Step 1: Écrire le test avec oracles indépendants**

Créer `tests/gct_legality.rs` :

```rust
//! Part 7 : les trois bitboards de légalité, vérifiés contre des oracles
//! **indépendants** — marches de rayons et deltas explicites, jamais les
//! tables ni les magics du moteur. Si l'oracle réutilisait `tables::RAYS` ou
//! `slider::bishop_attacks`, il ne prouverait rien.

use mychess::gct::attacks::squares_between;
use mychess::gct::legality::{compute_checkers, compute_pins, king_danger};
use mychess::position::Position;
use mychess::types::*;

const DIRS8: [(i8, i8); 8] = [
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
];

/// Marche le long d'une direction, en s'arrêtant **sur** le premier bloqueur.
fn walk(from: u8, d: (i8, i8), occ: u64) -> u64 {
    let mut bb = 0u64;
    let mut f = (from % 8) as i8;
    let mut r = (from / 8) as i8;
    loop {
        f += d.0;
        r += d.1;
        if !(0..8).contains(&f) || !(0..8).contains(&r) {
            break;
        }
        let s = (r * 8 + f) as u8;
        bb |= 1u64 << s;
        if occ & (1u64 << s) != 0 {
            break;
        }
    }
    bb
}

fn leaper(from: u8, deltas: &[(i8, i8)]) -> u64 {
    let mut bb = 0u64;
    for d in deltas {
        let f = (from % 8) as i8 + d.0;
        let r = (from / 8) as i8 + d.1;
        if (0..8).contains(&f) && (0..8).contains(&r) {
            bb |= 1u64 << ((r * 8 + f) as u32);
        }
    }
    bb
}

const KNIGHT_D: [(i8, i8); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];

fn king_deltas() -> Vec<(i8, i8)> {
    DIRS8.to_vec()
}

/// Attaques d'une pièce de type `kind` (0..5) de couleur `c` posée sur `sq`,
/// calculées uniquement par marche/deltas.
fn walk_piece(kind: usize, sq: u8, occ: u64) -> u64 {
    match kind {
        PAWN => {
            // Le test ne couvre que les pions blancs : le sens est passé par
            // l'appelant via `pawns_attacks`.
            0
        }
        KNIGHT => leaper(sq, &KNIGHT_D),
        BISHOP => DIRS8
            .iter()
            .filter(|(df, dr)| df != &0 && dr != &0)
            .fold(0u64, |acc, d| acc | walk(sq, *d, occ)),
        ROOK => DIRS8
            .iter()
            .filter(|(df, dr)| df == &0 || dr == &0)
            .fold(0u64, |acc, d| acc | walk(sq, *d, occ)),
        QUEEN => DIRS8.iter().fold(0u64, |acc, d| acc | walk(sq, *d, occ)),
        KING => leaper(sq, &king_deltas()),
        _ => 0,
    }
}

fn pawn_attacks_of(c: Color, sq: u8) -> u64 {
    let dr: i8 = if c == Color::White { 1 } else { -1 };
    leaper(sq, &[(-1, dr), (1, dr)])
}

/// `(danger, checkers, pinned, pin_lines_oracle)`.
fn oracles(pos: &Position) -> (u64, u64, u64, [u64; 64]) {
    let us = pos.side;
    let them = us.opp();
    let ksq = pos.king_sq(us);
    assert!(ksq < 64, "oracle : position legale exigee (roi present)");
    let occ = pos.occ();
    let occ_no_king = occ ^ (1u64 << ksq);
    let ks_bit = 1u64 << ksq;

    // --- danger + checkers : une passe par pièce ennemie, attaques par marche
    let mut danger = 0u64;
    let mut checkers = 0u64;
    for sq in 0..64u8 {
        let m = pos.piece_on(sq);
        if m == 0 {
            continue;
        }
        let code = m - 1;
        if code / 6 != them.idx() as u8 {
            continue;
        }
        let kind = (code % 6) as usize;
        let att = if kind == PAWN {
            pawn_attacks_of(them, sq)
        } else {
            walk_piece(kind, sq, occ_no_king)
        };
        danger |= att;
        if att & ks_bit != 0 {
            checkers |= 1u64 << sq;
        }
    }

    // --- pins : « retirer la pièce expose-t-il le roi à un slider ennemi ? »
    // Premier bloqueur par direction sous une occupation donnée.
    let first_blockers = |occ: u64| -> [Option<u8>; 8] {
        let mut out = [None; 8];
        for (i, d) in DIRS8.iter().enumerate() {
            let mut f = (ksq % 8) as i8;
            let mut r = (ksq / 8) as i8;
            loop {
                f += d.0;
                r += d.1;
                if !(0..8).contains(&f) || !(0..8).contains(&r) {
                    break;
                }
                let s = (r * 8 + f) as u8;
                if occ & (1u64 << s) != 0 {
                    out[i] = Some(s);
                    break;
                }
            }
        }
        out
    };
    // Sliders ennemis qui attaquent `ksq` sous l'occupation `occ`.
    let slider_attackers = |occ: u64| -> u64 {
        let mut acc = 0u64;
        for (i, b) in first_blockers(occ).iter().enumerate() {
            let Some(s) = *b else { continue };
            let m = pos.piece_on(s);
            if m == 0 {
                continue;
            }
            let code = m - 1;
            if code / 6 != them.idx() as u8 {
                continue;
            }
            let kind = (code % 6) as usize;
            let diagonal = i % 2 == 1;
            let matches = match kind {
                QUEEN => true,
                BISHOP => diagonal,
                ROOK => !diagonal,
                _ => false,
            };
            if matches {
                acc |= 1u64 << s;
            }
        }
        acc
    };

    let baseline = slider_attackers(occ);
    let mut pinned = 0u64;
    let mut lines = [0u64; 64];
    for from in 0..64u8 {
        if from == ksq {
            continue;
        }
        let m = pos.piece_on(from);
        if m == 0 {
            continue;
        }
        let code = m - 1;
        if code / 6 != us.idx() as u8 {
            continue;
        }
        let occ2 = occ ^ (1u64 << from);
        let new_attackers = slider_attackers(occ2) & !baseline;
        if new_attackers == 0 {
            continue;
        }
        pinned |= 1u64 << from;
        let pinner = new_attackers.trailing_zeros() as u8;
        lines[from as usize] = squares_between(ksq, pinner) | (1u64 << pinner);
    }

    (danger, checkers, pinned, lines)
}

/// Positions de travail : les trois goldens GCT + Kiwipete et la position
/// initiale, plus les positions issues d'un auto-jeu déterministe.
fn positions() -> Vec<Position> {
    let fens = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
    ];
    let mut v: Vec<Position> = fens
        .iter()
        .map(|f| Position::from_fen(f).unwrap())
        .collect();
    let mut pos = Position::startpos();
    let mut rng = 0x2545_F491_4F6C_DD1Du64;
    for _ply in 0..160 {
        v.push(pos);
        let mut list = mychess::movegen::MoveList::new();
        mychess::movegen::generate(&pos, &mut list, GenKind::All);
        if list.is_empty() {
            break;
        }
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let mv = list.get((rng as usize) % list.len());
        pos.make(mv);
    }
    v
}

/// `Position` n'implémente pas `Debug` : rendu du plateau depuis ses champs
/// publics (`mailbox`, `side`, `castle`, `ep`).
fn describe(pos: &Position) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    for r in (0..8).rev() {
        for f in 0..8 {
            let m = pos.mailbox[r * 8 + f];
            s.push(if m == 0 {
                '.'
            } else {
                b"PNBRQKpnbrqk"[(m - 1) as usize] as char
            });
        }
        s.push('\n');
    }
    let _ = write!(s, "{:?} castle={} ep={}", pos.side, pos.castle, pos.ep);
    s
}

#[test]
fn king_danger_correspond_a_l_oracle() {
    for pos in positions() {
        let us = pos.side;
        let them = us.opp();
        let ksq = pos.king_sq(us);
        let got = king_danger(&pos, ksq, them);
        let (want, _, _, _) = oracles(&pos);
        assert_eq!(got, want, "danger dans {}", describe(&pos));
    }
}

#[test]
fn compute_checkers_correspond_a_l_oracle() {
    for pos in positions() {
        let us = pos.side;
        let them = us.opp();
        let ksq = pos.king_sq(us);
        let got = compute_checkers(&pos, ksq, them);
        let (_, want, _, _) = oracles(&pos);
        assert_eq!(got, want, "checkers dans {}", describe(&pos));
    }
}

#[test]
fn compute_pins_correspond_a_l_oracle() {
    for pos in positions() {
        let us = pos.side;
        let them = us.opp();
        let ksq = pos.king_sq(us);
        let (pinned, lines) = compute_pins(&pos, ksq, them, us);
        let (_, _, want_pinned, want_lines) = oracles(&pos);
        assert_eq!(pinned, want_pinned, "pinned dans {}", describe(&pos));
        for sq in 0..64usize {
            if want_pinned & (1u64 << sq) != 0 {
                assert_eq!(
                    lines[sq],
                    want_lines[sq],
                    "pin_line[{sq}] dans {}",
                    describe(&pos)
                );
            } else {
                assert_eq!(
                    lines[sq],
                    u64::MAX,
                    "une piece non clouee doit avoir pin_lines = u64::MAX ({sq})"
                );
            }
        }
    }
}

#[test]
fn la_position_initiale_n_a_ni_echec_ni_clouage() {
    let pos = Position::startpos();
    let ksq = pos.king_sq(Color::White);
    assert_eq!(compute_checkers(&pos, ksq, Color::Black), 0);
    let (pinned, lines) = compute_pins(&pos, ksq, Color::Black, Color::White);
    assert_eq!(pinned, 0);
    assert!(lines.iter().all(|&l| l == u64::MAX));
}
```

- [ ] **Step 2: Lancer le test pour le voir échouer**

```bash
cargo test --release --test gct_legality 2>&1 | tail -20
```

Attendu : échec de compilation — `king_danger`, `compute_checkers`, `compute_pins` introuvables dans `mychess::gct::legality`.

- [ ] **Step 3: Écrire `src/gct/legality.rs`**

Remplacer le contenu de `src/gct/legality.rs` :

```rust
//! Part 7 du post : les trois bitboards de légalité, en **trois passes
//! séparées** comme dans la série.
//!
//! `crate::movegen` fusionne ces trois passes en une seule (`build_legality`,
//! gain mesuré +19 %) ; ce module ne le fait **pas**, volontairement : la
//! fidélité au post est l'objet de la phase 1, la fusion est une amélioration
//! de phase 2 (voir la section « Hors périmètre » du plan).

use crate::position::Position;
use crate::slider::{bishop_attacks, rook_attacks};
use crate::tables::{
    BETWEEN, BISHOP_DIRS, DIR_POS, KING_ATT, KNIGHT_ATT, PAWN_ATTACKERS, PAWN_ATTACKS, RAYS,
    ROOK_DIRS,
};
use crate::types::*;

/// `Legality.ComputeKingDanger` : cases attaquées par `them` **comme si notre
/// roi n'était pas sur l'échiquier**, pour qu'un slider derrière le roi reste
/// couvert.
///
/// Le post écrit une boucle par pièce ennemie (y compris par pion). C'est
/// conservé ici : le calcul bulk des pions serait une optimisation de phase 2,
/// pas une fidélité au post.
pub fn king_danger(pos: &Position, ksq: u8, them: Color) -> u64 {
    if ksq >= 64 {
        // Roi absent : entrée illégale, mais la fonction reste totale.
        return 0;
    }
    let occ_without_king = pos.occ() ^ (1u64 << ksq);
    let them_bb = pos.color_bb(them);
    let mut danger = 0u64;

    let mut pawns = pos.bb_piece[PAWN] & them_bb;
    while pawns != 0 {
        let sq = pawns.trailing_zeros() as u8;
        pawns &= pawns - 1;
        danger |= PAWN_ATTACKS[them.idx()][sq as usize];
    }

    let mut knights = pos.bb_piece[KNIGHT] & them_bb;
    while knights != 0 {
        let sq = knights.trailing_zeros() as u8;
        knights &= knights - 1;
        danger |= KNIGHT_ATT[sq as usize];
    }

    let mut bq = (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]) & them_bb;
    while bq != 0 {
        let sq = bq.trailing_zeros() as u8;
        bq &= bq - 1;
        danger |= bishop_attacks(sq, occ_without_king);
    }

    let mut rq = (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]) & them_bb;
    while rq != 0 {
        let sq = rq.trailing_zeros() as u8;
        rq &= rq - 1;
        danger |= rook_attacks(sq, occ_without_king);
    }

    let ek = pos.bb_piece[KING] & them_bb;
    if ek != 0 {
        danger |= KING_ATT[ek.trailing_zeros() as usize];
    }
    danger
}

/// `Legality.ComputeCheckers` : pièces ennemies qui attaquent notre roi.
pub fn compute_checkers(pos: &Position, ksq: u8, them: Color) -> u64 {
    if ksq >= 64 {
        return 0;
    }
    let occ = pos.occ();
    let them_bb = pos.color_bb(them);
    let pawns = pos.bb_piece[PAWN] & them_bb;
    let knights = pos.bb_piece[KNIGHT] & them_bb;
    let bq = (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]) & them_bb;
    let rq = (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]) & them_bb;
    (PAWN_ATTACKERS[them.idx()][ksq as usize] & pawns)
        | (KNIGHT_ATT[ksq as usize] & knights)
        | (bishop_attacks(ksq, occ) & bq)
        | (rook_attacks(ksq, occ) & rq)
}

/// `Legality.ComputePins` : pièces clouées et, pour chacune, la ligne sur
/// laquelle elle peut encore se déplacer (`between(ksq, pinner) | pinner`).
///
/// `pin_lines` est indexé **par case** et vaut `u64::MAX` partout, comme dans
/// le post : masquer les cibles d'une pièce par `pin_lines[from]` est alors un
/// no-op pour les pièces non clouées, sans test d'appartenance.
pub fn compute_pins(pos: &Position, ksq: u8, them: Color, us: Color) -> (u64, [u64; 64]) {
    let mut pin_lines = [u64::MAX; 64];
    if ksq >= 64 {
        return (0, pin_lines);
    }
    let occ = pos.occ();
    let us_bb = pos.color_bb(us);
    let them_bb = pos.color_bb(them);
    let enemy_rq = (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]) & them_bb;
    let enemy_bq = (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]) & them_bb;

    let mut pinned = scan_pinners(ksq, us_bb, occ, enemy_rq, ROOK_DIRS, &mut pin_lines);
    pinned |= scan_pinners(ksq, us_bb, occ, enemy_bq, BISHOP_DIRS, &mut pin_lines);
    (pinned, pin_lines)
}

/// `Legality.ScanPinners` : pour chaque direction, le premier bloqueur doit
/// être **à nous**, et le premier bloqueur suivant doit être un slider ennemi
/// de la bonne famille.
fn scan_pinners(
    ksq: u8,
    us: u64,
    occ: u64,
    enemy_sliders: u64,
    dirs: [usize; 4],
    pin_lines: &mut [u64; 64],
) -> u64 {
    let mut pinned = 0u64;
    for &d in dirs.iter() {
        let ray = RAYS[d][ksq as usize];
        let blockers = ray & occ;
        if blockers == 0 {
            continue;
        }
        let first = nearest(blockers, d);
        let first_bit = 1u64 << first;
        if us & first_bit == 0 {
            continue;
        }
        // `RAYS[d][first]` est exactement l'ensemble des cases situées
        // au-delà de `first` sur ce rayon : pas besoin de re-masquer.
        let beyond = RAYS[d][first as usize] & occ;
        if beyond == 0 {
            continue;
        }
        let second = nearest(beyond, d);
        let second_bit = 1u64 << second;
        if enemy_sliders & second_bit != 0 {
            pinned |= first_bit;
            pin_lines[first as usize] = BETWEEN[ksq as usize][second as usize] | second_bit;
        }
    }
    pinned
}

/// Case de `bb` la plus proche de l'origine du rayon, selon le sens `d`.
#[inline(always)]
fn nearest(bb: u64, d: usize) -> u8 {
    debug_assert!(bb != 0);
    if DIR_POS[d] {
        bb.trailing_zeros() as u8
    } else {
        (63 - bb.leading_zeros()) as u8
    }
}
```

- [ ] **Step 4: Lancer les tests**

```bash
cargo test --release --test gct_legality -- --nocapture
```

Attendu : 4 tests passent. Si `compute_pins_correspond_a_l_oracle` échoue sur un `pin_line`, comparer avec la sortie de l'oracle : le cas typique est le rayon dont le **second** bloqueur est du bon type mais hors portée (ne peut pas arriver : `RAYS` s'arrête au bord).

- [ ] **Step 5: Clippy, suite complète, commit**

```bash
cargo clippy --all-targets --release -- -D warnings
cargo test --release
git add src/gct/legality.rs tests/gct_legality.rs
git commit -m "gct(part7): danger, checkers et pins en trois passes avec oracles independants"
```

---

### Task 4: Générateur légal — roi, check_mask, pièces (Part 7)

**Files:**
- Modify: `src/gct/generate.rs`
- Test: `tests/gct_equivalence.rs` (créé à la tâche 5 ; ici on vérifie par division et par comparaison partielle sur des positions sans roque ni en-passant)

**Interfaces:**
- Consumes: `crate::gct::legality::*`, `crate::gct::attacks::squares_between`, `crate::movegen::MoveList`, `crate::slider::{bishop_attacks, rook_attacks}`, `crate::tables::*`.
- Produces:
  - `gct::generate::generate(pos: &Position, out: &mut MoveList) -> usize` — ajoute les coups légaux (roque et en-passant **pas encore** générés à cette étape) et renvoie le nouvel indice (`out.len()`), conforme au contrat du post.

- [ ] **Step 1: Écrire le test qui échoue**

Créer `tests/gct_equivalence.rs` avec **un seul** test à cette étape (les autres viendront en tâche 5) :

```rust
//! Équivalence `gct::generate` ≡ `movegen::generate` sur toute la surface.
//!
//! La comparaison porte sur l'**ensemble** des coups (mêmes triplés
//! `(from, to, flags)`), pas sur l'ordre : le post et `mychess` n'émettent pas
//! dans le même ordre, et ce n'est pas un critère de correction.

use mychess::gct::generate::generate as gct_generate;
use mychess::movegen::{MoveList, generate};
use mychess::position::Position;
use mychess::types::*;

/// Clé de comparaison stable : un coup est identifié par ses trois champs
/// publics (`from`, `to`, `flags`), jamais par la valeur 16 bits brute.
type MoveKey = (u8, u8, u8);

fn key(m: Move) -> MoveKey {
    (m.from(), m.to(), m.flags())
}

fn sorted_gct(pos: &Position) -> Vec<MoveKey> {
    let mut l = MoveList::new();
    gct_generate(pos, &mut l);
    let mut v: Vec<MoveKey> = l.iter().map(key).collect();
    v.sort_unstable();
    v
}

fn sorted_movegen(pos: &Position) -> Vec<MoveKey> {
    let mut l = MoveList::new();
    generate(pos, &mut l, GenKind::All);
    let mut v: Vec<MoveKey> = l.iter().map(key).collect();
    v.sort_unstable();
    v
}

/// `Position` n'implémente pas `Debug` : rendu du plateau depuis ses champs
/// publics (`mailbox`, `side`, `castle`, `ep`).
fn describe(pos: &Position) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    for r in (0..8).rev() {
        for f in 0..8 {
            let m = pos.mailbox[r * 8 + f];
            s.push(if m == 0 {
                '.'
            } else {
                b"PNBRQKpnbrqk"[(m - 1) as usize] as char
            });
        }
        s.push('\n');
    }
    let _ = write!(s, "{:?} castle={} ep={}", pos.side, pos.castle, pos.ep);
    s
}

/// Positions sans droit de roque et sans case en-passant : à cette étape le
/// générateur `gct` ne produit encore ni roque ni en-passant (tâche 5), donc
/// seules ces positions sont comparables.
fn positions_sans_roque_ni_ep() -> Vec<Position> {
    let fens = [
        "4k3/pppppppp/8/8/8/8/PPPPPPPP/4K3 w - - 0 1",
        "4k3/8/8/8/8/8/8/4K3 w - - 0 1",
        "r5k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1",
        "4k3/pp6/8/2b5/8/6P1/PP6/4K3 w - - 0 1",
        "4k3/8/8/8/8/8/8/R3K2R w - - 0 1",
    ];
    fens.iter().map(|f| Position::from_fen(f).unwrap()).collect()
}

#[test]
fn gct_et_movegen_donnent_le_meme_ensemble_sans_roque_ni_ep() {
    for mut pos in positions_sans_roque_ni_ep() {
        // On marche tout l'arbre jusqu'à la profondeur 3 pour multiplier les
        // configurations de clouages et d'échecs.
        let mut stack: Vec<Position> = vec![pos];
        for _depth in 0..3 {
            let mut next = Vec::new();
            for p in stack {
                assert_eq!(
                    sorted_gct(&p),
                    sorted_movegen(&p),
                    "ensembles de coups differents dans {p:?}"
                );
                let mut l = MoveList::new();
                generate(&p, &mut l, GenKind::All);
                for mv in l.iter() {
                    let mut q = p;
                    q.make(mv);
                    if q.ep == NO_EP {
                        next.push(q);
                    }
                }
            }
            stack = next;
            if stack.is_empty() {
                break;
            }
            pos = stack[0];
        }
        let _ = pos;
    }
}
```

- [ ] **Step 2: Lancer le test pour le voir échouer**

```bash
cargo test --release --test gct_equivalence 2>&1 | tail -20
```

Attendu : `cannot find function generate in module mychess::gct::generate`.

- [ ] **Step 3: Écrire `src/gct/generate.rs`**

Remplacer le contenu de `src/gct/generate.rs` :

```rust
//! Part 7 du post : génération des coups **légaux**.
//!
//! Ordre du post, conservé : coups de roi d'abord, retour immédiat sur double
//! échec, calcul du `check_mask`, puis pions / cavaliers / glissantes, et le
//! roque en dernier (tâche 5).
//!
//! Corrections et simplifications assumées :
//!   - pas de `delegate AttackFn` : les appels `bishop_attacks` / `rook_attacks`
//!     sont directs et monomorphisés ;
//!   - la dame est traitée en un passage (`bishop ∪ rook`) au lieu de deux,
//!     ce qui supprime le doublon de la Part 4 sans changer l'ensemble émis ;
//!   - `pin_lines[from]` vaut `u64::MAX` pour les pièces non clouées, donc le
//!     masquage est inconditionnel — c'est déjà le contrat de
//!     [`crate::gct::legality::compute_pins`] ;
//!   - les cibles excluent toujours la case du roi ennemi : le post ne le fait
//!     pas, mais un coup de capture de roi n'est jamais légal et le contrat
//!     d'entrée du projet (GATE 2.5 M3) impose qu'aucun tel coup ne soit émis,
//!     même sur entrée illégale. Sur une position légale c'est un no-op.

use crate::gct::attacks::squares_between;
use crate::gct::legality::{compute_checkers, compute_pins, king_danger};
use crate::movegen::MoveList;
use crate::position::Position;
use crate::slider::{bishop_attacks, rook_attacks};
use crate::tables::{
    FILE_A, FILE_H, KING_ATT, KNIGHT_ATT, PAWN_ATTACKERS, RANK_1, RANK_3, RANK_6, RANK_8,
};
use crate::types::*;
use crate::gct::attacks::{shift_n, shift_ne, shift_nw, shift_s, shift_se, shift_sw};

/// Génère tous les coups légaux de la position et renvoie le nouvel indice du
/// buffer (contrat de la série : « append into a caller-owned buffer, return
/// the new index »).
pub fn generate(pos: &Position, out: &mut MoveList) -> usize {
    let us = pos.side;
    let them = us.opp();
    let ksq = pos.king_sq(us);

    // Contrat d'entrée (GATE 2.5 M3) : position légale exigée. Roi absent =
    // entrée illégale : aucun coup légal, silencieux, sans panique.
    if ksq == NO_SQUARE {
        return out.len();
    }

    let occ = pos.occ();
    let us_bb = pos.color_bb(us);
    let them_king = pos.bb_piece[KING] & pos.color_bb(them);
    // Les pièces adverses capturables : jamais le roi.
    let enemy = pos.color_bb(them) & !them_king;

    let danger = king_danger(pos, ksq, them);
    let checkers = compute_checkers(pos, ksq, them);
    let (pinned, pin_lines) = compute_pins(pos, ksq, them, us);

    // 6. Coups de roi : toujours en premier, toujours sans masque d'échec.
    gen_king_moves(out, ksq, us_bb, danger, them_king, enemy);

    // 7. Double échec : seuls les coups de roi sont légaux.
    let ncheck = checkers.count_ones();
    if ncheck >= 2 {
        return out.len();
    }

    // 8. check_mask = capturer le vérificateur ou bloquer la ligne.
    let check_mask = if ncheck == 1 {
        let c = checkers.trailing_zeros() as u8;
        checkers | squares_between(ksq, c)
    } else {
        u64::MAX
    };

    // 9. Pièces.
    gen_pawns::<true>(pos, out, check_mask, pinned, &pin_lines, occ, enemy, us);
    gen_pawns::<false>(pos, out, check_mask, pinned, &pin_lines, occ, enemy, us);
    gen_knights(pos, out, us_bb, check_mask, &pin_lines, enemy);
    gen_sliders(pos, out, us_bb, check_mask, &pin_lines, occ, enemy);

    // 10. Roque — ajouté à la tâche 5.
    out.len()
}

/// `MoveGenerator.EmitTargets` (Part 4) : captures d'abord, puis coups calmes.
#[inline]
fn emit_targets(out: &mut MoveList, from: u8, targets: u64, enemy: u64) {
    let mut caps = targets & enemy;
    while caps != 0 {
        let to = caps.trailing_zeros() as u8;
        caps &= caps - 1;
        out.push(Move::new(from, to, FLAG_CAPTURE));
    }
    let mut quiets = targets & !enemy;
    while quiets != 0 {
        let to = quiets.trailing_zeros() as u8;
        quiets &= quiets - 1;
        out.push(Move::new(from, to, FLAG_QUIET));
    }
}

/// `GenKingMoves` : `!danger` (occupation sans notre roi) et `!us`.
#[inline]
fn gen_king_moves(out: &mut MoveList, ksq: u8, us_bb: u64, danger: u64, them_king: u64, enemy: u64) {
    let targets = KING_ATT[ksq as usize] & !us_bb & !danger & !them_king;
    emit_targets(out, ksq, targets, enemy);
}

/// `GenKnightsLegal` : un cavalier cloué ne bouge jamais (sa ligne de clouage
/// n'intersecte aucune de ses cibles), donc `pin_lines[sq]` le neutralise.
#[inline]
fn gen_knights(
    pos: &Position,
    out: &mut MoveList,
    us_bb: u64,
    check_mask: u64,
    pin_lines: &[u64; 64],
    enemy: u64,
) {
    let mut kn = pos.bb_piece[KNIGHT] & us_bb;
    while kn != 0 {
        let s = kn.trailing_zeros() as u8;
        kn &= kn - 1;
        // La case du roi ennemi n'a pas besoin d'être masquée ici : elle est
        // occupée, donc absente de `!us_bb`, et déjà retirée de `enemy`.
        let targets = KNIGHT_ATT[s as usize] & !us_bb & check_mask & pin_lines[s as usize];
        emit_targets(out, s, targets, enemy);
    }
}

/// `GenSlidersLegal` : fous, tours, puis dames en un seul passage.
#[inline]
fn gen_sliders(
    pos: &Position,
    out: &mut MoveList,
    us_bb: u64,
    check_mask: u64,
    pin_lines: &[u64; 64],
    occ: u64,
    enemy: u64,
) {
    let mut b = pos.bb_piece[BISHOP] & us_bb;
    while b != 0 {
        let s = b.trailing_zeros() as u8;
        b &= b - 1;
        let targets =
            bishop_attacks(s, occ) & !us_bb & check_mask & pin_lines[s as usize];
        emit_targets(out, s, targets, enemy);
    }
    let mut r = pos.bb_piece[ROOK] & us_bb;
    while r != 0 {
        let s = r.trailing_zeros() as u8;
        r &= r - 1;
        let targets = rook_attacks(s, occ) & !us_bb & check_mask & pin_lines[s as usize];
        emit_targets(out, s, targets, enemy);
    }
    let mut q = pos.bb_piece[QUEEN] & us_bb;
    while q != 0 {
        let s = q.trailing_zeros() as u8;
        q &= q - 1;
        let targets = (bishop_attacks(s, occ) | rook_attacks(s, occ))
            & !us_bb
            & check_mask
            & pin_lines[s as usize];
        emit_targets(out, s, targets, enemy);
    }
}

/// `GenPawnsLegal` : poussées simples et doubles, captures, promotions.
///
/// Les cibles sont calculées en **bulk** (un décalage par famille de coups)
/// puis chaque cible retrouve sa case d'origine par `delta_from`, comme le
/// post (`EmitPawnLegal(..., deltaFrom, ...)`). C'est ce qui permet d'appliquer
/// le masque de clouage sans boucler sur les pions.
#[allow(clippy::too_many_arguments)]
fn gen_pawns<const W: bool>(
    pos: &Position,
    out: &mut MoveList,
    check_mask: u64,
    pinned: u64,
    pin_lines: &[u64; 64],
    occ: u64,
    enemy: u64,
    us: Color,
) {
    if us != (if W { Color::White } else { Color::Black }) {
        return;
    }
    let pawns = pos.bb_piece[PAWN] & pos.color_bb(us);
    if pawns == 0 {
        return;
    }
    let empty = !occ;

    if W {
        let one = shift_n(pawns) & empty;
        let two = shift_n(one & RANK_3) & empty;
        let promo = one & RANK_8;
        let quiet = one & !RANK_8;
        let cap_ne = shift_ne(pawns) & enemy;
        let cap_nw = shift_nw(pawns) & enemy;

        emit_pawn_pushes(out, promo & check_mask, 8, true, pinned, pin_lines);
        emit_pawn_pushes(out, quiet & check_mask, 8, false, pinned, pin_lines);
        emit_pawn_pushes(out, two & check_mask, 16, false, pinned, pin_lines);
        emit_pawn_capture(out, cap_ne & RANK_8 & check_mask, 9, true, pinned, pin_lines);
        emit_pawn_capture(out, cap_nw & RANK_8 & check_mask, 7, true, pinned, pin_lines);
        emit_pawn_capture(out, cap_ne & !RANK_8 & check_mask, 9, false, pinned, pin_lines);
        emit_pawn_capture(out, cap_nw & !RANK_8 & check_mask, 7, false, pinned, pin_lines);
    } else {
        let one = shift_s(pawns) & empty;
        let two = shift_s(one & RANK_6) & empty;
        let promo = one & RANK_1;
        let quiet = one & !RANK_1;
        let cap_se = shift_se(pawns) & enemy;
        let cap_sw = shift_sw(pawns) & enemy;

        emit_pawn_pushes(out, promo & check_mask, -8, true, pinned, pin_lines);
        emit_pawn_pushes(out, quiet & check_mask, -8, false, pinned, pin_lines);
        emit_pawn_pushes(out, two & check_mask, -16, false, pinned, pin_lines);
        emit_pawn_capture(out, cap_se & RANK_1 & check_mask, -7, true, pinned, pin_lines);
        emit_pawn_capture(out, cap_sw & RANK_1 & check_mask, -9, true, pinned, pin_lines);
        emit_pawn_capture(out, cap_se & !RANK_1 & check_mask, -7, false, pinned, pin_lines);
        emit_pawn_capture(out, cap_sw & !RANK_1 & check_mask, -9, false, pinned, pin_lines);
    }
}

/// Une pièce clouée ne peut atteindre que les cases de sa ligne de clouage.
#[inline(always)]
fn pin_allows(from: u8, to: u8, pinned: u64, pin_lines: &[u64; 64]) -> bool {
    pinned & (1u64 << from) == 0 || pin_lines[from as usize] & (1u64 << to) != 0
}

/// `EmitPawnLegal` pour les poussées (simples, doubles, promotions de poussée).
#[inline]
fn emit_pawn_pushes(
    out: &mut MoveList,
    targets: u64,
    delta_from: i16,
    promotion: bool,
    pinned: u64,
    pin_lines: &[u64; 64],
) {
    let mut t = targets;
    while t != 0 {
        let to = t.trailing_zeros() as u8;
        t &= t - 1;
        let from = (to as i16 - delta_from) as u8;
        if !pin_allows(from, to, pinned, pin_lines) {
            continue;
        }
        if promotion {
            for i in 0..4u8 {
                out.push(Move::new(from, to, FLAG_PROMO_N + i));
            }
        } else {
            let flag = if delta_from.unsigned_abs() as u8 == 16 {
                FLAG_DOUBLE_PUSH
            } else {
                FLAG_QUIET
            };
            out.push(Move::new(from, to, flag));
        }
    }
}

/// `EmitPromos` (version capture) et `EmitPawnLegal` (version capture).
#[inline]
fn emit_pawn_capture(
    out: &mut MoveList,
    targets: u64,
    delta_from: i16,
    promotion: bool,
    pinned: u64,
    pin_lines: &[u64; 64],
) {
    let mut t = targets;
    while t != 0 {
        let to = t.trailing_zeros() as u8;
        t &= t - 1;
        let from = (to as i16 - delta_from) as u8;
        if !pin_allows(from, to, pinned, pin_lines) {
            continue;
        }
        if promotion {
            for i in 0..4u8 {
                out.push(Move::new(from, to, FLAG_PROMO_CAP_N + i));
            }
        } else {
            out.push(Move::new(from, to, FLAG_CAPTURE));
        }
    }
}
```

- [ ] **Step 4: Lancer le test jusqu'au vert**

```bash
cargo test --release --test gct_equivalence -- --nocapture
```

Attendu : `gct_et_movegen_donnent_le_meme_ensemble_sans_roque_ni_ep` passe.

Note d'exécution : si le test échoue sur une position, isoler la famille fautive en comparant le `divide` des deux générateurs — `mychess divide <fen> 1` d'un côté, et côté `gct` un `println!` temporaire trié. Le suspect numéro un est le signe de `delta_from` (blanc : `+8` / `+16` / `+9` / `+7` ; noir : `-8` / `-16` / `-7` / `-9`).

- [ ] **Step 5: Clippy, suite complète, commit**

```bash
cargo clippy --all-targets --release -- -D warnings
cargo test --release
git add src/gct/generate.rs tests/gct_equivalence.rs
git commit -m "gct(part7): generateur legal roi/check_mask/pieces (roque et ep a suivre)"
```

---

### Task 5: Roque, en-passant et équivalence complète

**Files:**
- Modify: `src/gct/generate.rs`
- Modify: `tests/gct_equivalence.rs`
- Test: `tests/gct_equivalence.rs`

**Interfaces:**
- Consumes: tout ce qui précède.
- Produces: `gct::generate::generate` **complet** (roque + en-passant inclus), équivalent à `movegen::generate` sur toute la surface.

- [ ] **Step 1: Ajouter les tests qui échouent**

Dans `tests/gct_equivalence.rs`, ajouter à la fin :

```rust
/// Toutes les positions des goldens perft du dépôt : elles couvrent roque,
/// en-passant, promotions et clouages en densité.
fn positions_goldens() -> Vec<Position> {
    let fens = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        // En-passant avec capture possible :
        "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
        // Roque des deux cotes encore disponible :
        "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
        // Roque bloque par une piece interposee :
        "r3k2r/8/8/8/8/8/8/R2QK2R w KQkq - 0 1",
    ];
    fens.iter().map(|f| Position::from_fen(f).unwrap()).collect()
}

#[test]
fn gct_et_movegen_donnent_le_meme_ensemble_sur_les_goldens() {
    for pos in positions_goldens() {
        assert_eq!(
            sorted_gct(&pos),
            sorted_movegen(&pos),
            "ensembles de coups differents dans {}", describe(&pos)
        );
    }
}

#[test]
fn gct_et_movegen_donnent_le_meme_ensemble_sur_un_auto_jeu() {
    // 40 parties de 60 demi-coups, graine fixe : reproductible.
    let mut rng = 0xDEAD_BEEF_CAFE_F00Du64;
    for _game in 0..40 {
        let mut pos = Position::startpos();
        for _ply in 0..60 {
            assert_eq!(
                sorted_gct(&pos),
                sorted_movegen(&pos),
                "ensembles de coups differents dans {}", describe(&pos)
            );
            let mut l = MoveList::new();
            generate(&pos, &mut l, GenKind::All);
            if l.is_empty() {
                break;
            }
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let mv = l.get((rng as usize) % l.len());
            pos.make(mv);
        }
    }
}

#[test]
fn gct_generent_le_roque_des_deux_cotes() {
    let pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    let moves = sorted_gct(&pos);
    assert!(moves.contains(&(4, 6, FLAG_KING_CASTLE)), "petit roque blanc");
    assert!(moves.contains(&(4, 2, FLAG_QUEEN_CASTLE)), "grand roque blanc");
}

#[test]
fn gct_ne_roquepas_a_travers_une_case_attaquee() {
    // Tour noire en f8 : la case f1 est attaquee, le petit roque blanc est
    // illegal, le grand roque reste legal.
    let pos = Position::from_fen("4kr2/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
    let moves = sorted_gct(&pos);
    assert!(
        !moves.iter().any(|&(_, _, f)| f == FLAG_KING_CASTLE),
        "le petit roque doit etre refuse (f1 attaquee)"
    );
    assert!(
        moves.iter().any(|&(_, _, f)| f == FLAG_QUEEN_CASTLE),
        "le grand roque doit rester legal"
    );
}

#[test]
fn gct_genere_l_en_passant_et_refuse_le_clouage_horizontal() {
    // En-passant legal :
    let pos = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
    assert!(
        sorted_gct(&pos).contains(&(36, 43, FLAG_EP_CAPTURE)),
        "e5xd6 en passant doit etre genere"
    );

    // Clouage horizontal : roi blanc e5, pion blanc d5, pion noir e5(noir), ep d6.
    // La capture exd6 retirerait deux pions de la 5e rangée et exposait le roi
    // a la tour noire h5.
    let pos = Position::from_fen("4k3/8/8/K2pP2r/8/8/8/8 w - d6 0 1").unwrap();
    let moves = sorted_gct(&pos);
    assert!(
        !moves.iter().any(|&(_, _, f)| f == FLAG_EP_CAPTURE),
        "un en-passant qui decouvre une tour sur la rangee doit etre refuse"
    );
}
```

- [ ] **Step 2: Lancer et constater les échecs attendus**

```bash
cargo test --release --test gct_equivalence -- --nocapture
```

Attendu : échecs sur `gct_generent_le_roque_des_deux_cotes`, `gct_ne_roquepas…`, `gct_genere_l_en_passant…`, et sur les deux tests d'équivalence (positions avec roque / ep).

- [ ] **Step 3: Ajouter le roque et l'en-passant**

Dans `src/gct/generate.rs`, remplacer le bloc final de `generate` :

```rust
    // 10. Roque : seulement hors échec, et seulement si les cases de
    // passage ne sont pas attaquées (`danger` est calculé sans notre roi,
    // donc il contient déjà les attaques des glissantes qui le traversent).
    if ncheck == 0 {
        gen_castling::<true>(pos, out, occ, danger);
        gen_castling::<false>(pos, out, occ, danger);
    }
    out.len()
```

et ajouter les deux fonctions à la fin du fichier :

```rust
/// `GenCastlingLegal` : cases vides + cases de passage non attaquées.
///
/// Le post code les cases en dur (4→6, 4→2, 60→62, 60→58) et ignore son
/// paramètre `kingSq` ; ici `back` dérive de la couleur, et **le roi et la
/// tour doivent réellement être sur leurs cases d'origine** — ce que la
/// normalisation des droits de `Position::from_fen` garantit déjà, mais que
/// le test rend explicite (entrée illégale = pas de roque, pas de panique).
fn gen_castling<const W: bool>(pos: &Position, out: &mut MoveList, occ: u64, danger: u64) {
    let (back, ks, qs) = if W {
        (0u8, CASTLE_WK, CASTLE_WQ)
    } else {
        (56u8, CASTLE_BK, CASTLE_BQ)
    };
    if pos.castle & (ks | qs) == 0 {
        return;
    }
    let us = if W { Color::White } else { Color::Black };
    let rook = pos.bb_piece[ROOK] & pos.color_bb(us);
    let king = pos.bb_piece[KING] & pos.color_bb(us);
    if king & (1u64 << (back + 4)) == 0 {
        return;
    }

    if pos.castle & ks != 0 {
        let f = back + 5;
        let g = back + 6;
        let path = (1u64 << f) | (1u64 << g);
        if rook & (1u64 << (back + 7)) != 0
            && occ & path == 0
            && danger & path == 0
        {
            out.push(Move::new(back + 4, g, FLAG_KING_CASTLE));
        }
    }
    if pos.castle & qs != 0 {
        let b = back + 1;
        let c = back + 2;
        let d = back + 3;
        let empty_path = (1u64 << b) | (1u64 << c) | (1u64 << d);
        // b1 peut être attaquée : le roi ne la traverse pas.
        let transit = (1u64 << c) | (1u64 << d);
        if rook & (1u64 << back) != 0
            && occ & empty_path == 0
            && danger & transit == 0
        {
            out.push(Move::new(back + 4, c, FLAG_QUEEN_CASTLE));
        }
    }
}

/// `GenEnPassant` / `TryEpMove` : un candidat, puis **test d'occupation
/// modifiée**.
///
/// Le post passe `kingSq` et `checkers` à `TryEpMove` pour décider de la
/// légalité ; ce test ne couvre pas le clouage horizontal (le cas classique
/// `...d5 exd6` qui découvre une tour sur la 5e rangée). Le portage remplace
/// ce raisonnement par la vérification complète sur l'occupation d'après
/// capture — équivalente, plus courte, et strictement plus forte :
///   - les pièces sautantes (pion, cavalier, roi) ennemies sont testées avec
///     le pion capturé retiré ;
///   - les glissantes sont testées sur `occ_after` (deux pions retirés, le
///     nôtre posé sur la case d'arrivée).
fn gen_en_passant(pos: &Position, out: &mut MoveList, us: Color, ksq: u8) {
    let Some(ep) = pos.ep_square() else { return };
    // Un FEN peut nommer une case ep sans pion à capturer : contrat inchangé.
    let Some(cap_sq) = pos.ep_capture_square(us) else {
        return;
    };
    let them = us.opp();
    let occ = pos.occ();
    let pawns = pos.bb_piece[PAWN] & pos.color_bb(us);
    let mut candidates = PAWN_ATTACKERS[us.idx()][ep as usize] & pawns;
    if candidates == 0 {
        return;
    }

    let them_bb = pos.color_bb(them);
    let enemy_pawns_after = (pos.bb_piece[PAWN] & them_bb) & !(1u64 << cap_sq);
    let leapers = crate::tables::PAWN_ATTACKERS[them.idx()][ksq as usize] & enemy_pawns_after
        | KNIGHT_ATT[ksq as usize] & pos.bb_piece[KNIGHT] & them_bb
        | KING_ATT[ksq as usize] & pos.bb_piece[KING] & them_bb;
    if leapers != 0 {
        return;
    }
    let enemy_rq = (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]) & them_bb;
    let enemy_bq = (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]) & them_bb;

    while candidates != 0 {
        let from = candidates.trailing_zeros() as u8;
        candidates &= candidates - 1;
        let occ_after = (occ ^ (1u64 << from) ^ (1u64 << cap_sq)) | (1u64 << ep);
        if rook_attacks(ksq, occ_after) & enemy_rq != 0 {
            continue;
        }
        if bishop_attacks(ksq, occ_after) & enemy_bq != 0 {
            continue;
        }
        out.push(Move::new(from, ep, FLAG_EP_CAPTURE));
    }
}
```

Puis appeler `gen_en_passant` dans `generate`, juste après les pièces et avant le roque :

```rust
    gen_en_passant(pos, out, us, ksq);
```

- [ ] **Step 4: Lancer les tests jusqu'au vert**

```bash
cargo test --release --test gct_equivalence -- --nocapture
```

Attendu : 5 tests passent. Si `gct_genere_l_en_passant_et_refuse_le_clouage_horizontal` échoue sur la seconde position, vérifier la FEN du cas de clouage horizontal : blanc `Ka5`, pion `Pd5` ; noir pion `Pd5`… la FEN `8/8/8/K2pP2r/8/8/8/8 w - d6 0 1` place le roi blanc en a5, un pion noir d5, un pion blanc e5, une tour noire h5, et la case ep d6. La capture `exd6` retire les pions d5 et e5 de la 5e rangée et laisse la tour h5 attaquer a5. Correctif à appliquer si la FEN ne décrit pas ce cas : réécrire la FEN, jamais assouplir l'assertion.

- [ ] **Step 5: Suite complète et commit**

```bash
cargo clippy --all-targets --release -- -D warnings
cargo test --release
git add src/gct/generate.rs tests/gct_equivalence.rs
git commit -m "gct(part7): roque et en-passant, equivalence complete avec movegen"
```

---

### Task 6: Perft du post et validation contre la table Grand Chess Tree

**Files:**
- Modify: `src/gct/perft.rs`
- Create: `examples/gctperft.rs`
- Test: `tests/gct_perft.rs`

**Interfaces:**
- Consumes: `crate::gct::generate::generate`, `crate::perft::PerftCounts`, `crate::position::Position`, `crate::movegen::{MoveList, attackers_to}`.
- Produces:
  - `gct::perft::perft(pos: &mut Position, depth: u32) -> u64`
  - `gct::perft::perft_detailed(pos: &mut Position, depth: u32) -> PerftCounts`
  - un binaire d'exemple : `cargo run --release --example gctperft -- <fen> <depth>`

- [ ] **Step 1: Écrire le test qui échoue**

Créer `tests/gct_perft.rs` :

```rust
//! Perft du générateur `gct`, validé contre la table détaillée Grand Chess
//! Tree (goldens de `tests/perft/gct/`, conversion de convention appliquée
//! ici comme dans `tests/gct_oracle.rs`).

use mychess::gct::perft::{perft as gct_perft, perft_detailed as gct_detailed};
use mychess::position::Position;

struct Row {
    depth: u32,
    nodes: u64,
    cols: [Option<u64>; 8],
}
struct Golden {
    fen: String,
    rows: Vec<Row>,
}

fn load(path: &str) -> Golden {
    let text = std::fs::read_to_string(path).unwrap();
    let mut fen = String::new();
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if fen.is_empty() && line.contains('/') {
            fen = line.to_string();
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        let depth: u32 = toks[0].parse().unwrap();
        let nodes: u64 = toks[1].parse().unwrap();
        let mut cols = [None; 8];
        for (i, c) in cols.iter_mut().enumerate() {
            if let Some(t) = toks.get(2 + i).filter(|t| **t != "-") {
                *c = Some(t.replace('_', "").parse().unwrap());
            }
        }
        rows.push(Row { depth, nodes, cols });
    }
    Golden { fen, rows }
}

fn all() -> Vec<Golden> {
    [
        "tests/perft/gct/startpos.txt",
        "tests/perft/gct/kiwipete.txt",
        "tests/perft/gct/sje.txt",
    ]
    .iter()
    .map(|p| load(p))
    .collect()
}

fn check(g: &Golden, row: &Row) {
    let mut pos = Position::from_fen(&g.fen).unwrap();
    assert_eq!(
        gct_perft(&mut pos, row.depth),
        row.nodes,
        "{} d{} nodes (gct::perft)",
        g.fen, row.depth
    );

    let mut pos = Position::from_fen(&g.fen).unwrap();
    let c = gct_detailed(&mut pos, row.depth);
    assert_eq!(c.nodes, row.nodes, "{} d{} nodes (detail)", g.fen, row.depth);
    let gct_mates = row.cols[7].unwrap_or(0);
    let got = [
        c.captures,
        c.ep,
        c.castles,
        c.promotions,
        c.checks,
        c.mates,
    ];
    let mut want = [None; 6];
    want[0] = row.cols[0];
    want[1] = row.cols[1];
    want[2] = row.cols[2];
    want[3] = row.cols[3];
    want[4] = row.cols[4].map(|chk| chk + gct_mates);
    want[5] = row.cols[7];
    let names = [
        "captures",
        "ep",
        "castles",
        "promotions",
        "checks",
        "mates",
    ];
    for i in 0..6 {
        if let Some(w) = want[i] {
            assert_eq!(got[i], w, "{} d{} {}", g.fen, row.depth, names[i]);
        }
    }
}

#[test]
fn t1_gct_perft_shallow() {
    for g in all() {
        for row in &g.rows {
            if row.depth > 4 {
                continue;
            }
            check(&g, row);
        }
    }
}

#[test]
#[ignore = "T2: perft gct aux profondeurs 5 et 6"]
fn t2_gct_perft_mid() {
    for g in all() {
        for row in &g.rows {
            if !(5..=6).contains(&row.depth) {
                continue;
            }
            check(&g, row);
        }
    }
}

#[test]
fn gct_perft_egale_le_movegen_existant_aux_profondeurs_courtes() {
    // Contrôle croisé indépendant de la table GCT : les deux générateurs
    // doivent produire exactement les mêmes comptes.
    for g in all() {
        for row in &g.rows {
            if row.depth > 3 {
                continue;
            }
            let mut a = Position::from_fen(&g.fen).unwrap();
            let mut b = Position::from_fen(&g.fen).unwrap();
            assert_eq!(
                gct_perft(&mut a, row.depth),
                mychess::perft::perft(&mut b, row.depth),
                "{} d{}",
                g.fen,
                row.depth
            );
        }
    }
}
```

- [ ] **Step 2: Lancer pour voir échouer**

```bash
cargo test --release --test gct_perft 2>&1 | tail -20
```

Attendu : import non résolu `mychess::gct::perft::perft`.

- [ ] **Step 3: Écrire `src/gct/perft.rs`**

```rust
//! Part 7 du post : perft du générateur légal, avec bulk-counting au bord.
//!
//! Le post compte les feuilles en renvoyant simplement la longueur du buffer
//! quand `depth == 1` (« bulk counting ») : aucune paire make/unmake au bord.
//! C'est exactement ce que fait [`perft`] ici.

use crate::gct::generate::generate;
use crate::movegen::{MoveList, attackers_to};
use crate::perft::PerftCounts;
use crate::position::Position;
use crate::types::*;

/// Perft bulk : nœuds = longueur de la liste à `depth == 1`.
pub fn perft(pos: &mut Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let mut list = MoveList::new();
    let n = generate(pos, &mut list);
    if depth == 1 {
        return n as u64;
    }
    let moves: Vec<Move> = list.iter().collect();
    let mut nodes = 0;
    for mv in moves {
        let u = pos.make(mv);
        nodes += perft(pos, depth - 1);
        pos.unmake(mv, u);
    }
    nodes
}

/// Perft avec compteurs, comptés aux feuilles (même convention que
/// `crate::perft::perft_detailed`, donc directement comparable à la table
/// Grand Chess Tree après conversion `checks = checks + mates`).
pub fn perft_detailed(pos: &mut Position, depth: u32) -> PerftCounts {
    let mut c = PerftCounts::default();
    if depth == 0 {
        c.nodes = 1;
        return c;
    }
    let mut list = MoveList::new();
    generate(pos, &mut list);
    let us = pos.side;
    let moves: Vec<Move> = list.iter().collect();

    if depth == 1 {
        c.nodes = moves.len() as u64;
        return c;
    }

    for mv in moves {
        let is_cap = mv.is_capture();
        let is_ep = mv.is_ep();
        let is_castle = mv.is_castle();
        let is_promo = mv.is_promotion();
        let u = pos.make(mv);
        if depth == 2 {
            c.nodes += 1;
            if is_cap {
                c.captures += 1;
            }
            if is_ep {
                c.ep += 1;
            }
            if is_castle {
                c.castles += 1;
            }
            if is_promo {
                c.promotions += 1;
            }
            let opp = us.opp();
            if pos.in_check(opp) {
                c.checks += 1;
                let att = attackers_to(pos, pos.king_sq(opp), us);
                if att.count_ones() >= 2 {
                    c.double_checks += 1;
                } else if att != (1u64 << mv.to()) {
                    c.discovery_checks += 1;
                }
                let mut replies = MoveList::new();
                if generate(pos, &mut replies) == 0 {
                    c.mates += 1;
                }
            }
        } else {
            let sub = perft_detailed(pos, depth - 1);
            c.nodes += sub.nodes;
            c.captures += sub.captures;
            c.ep += sub.ep;
            c.castles += sub.castles;
            c.promotions += sub.promotions;
            c.checks += sub.checks;
            c.discovery_checks += sub.discovery_checks;
            c.double_checks += sub.double_checks;
            c.mates += sub.mates;
        }
        pos.unmake(mv, u);
    }
    c
}
```

Créer `examples/gctperft.rs` :

```rust
//! Mesure du perft `gct` : `cargo run --release --example gctperft -- <fen> <depth>`

use mychess::gct::perft::perft;
use mychess::position::Position;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: gctperft <fen> <depth>");
        std::process::exit(2);
    }
    let mut pos = match Position::from_fen(&args[1]) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("FEN invalide : {e:?}");
            std::process::exit(2);
        }
    };
    let depth: u32 = args[2].parse().expect("depth");
    let t = Instant::now();
    let n = perft(&mut pos, depth);
    let s = t.elapsed().as_secs_f64();
    println!(
        "gct perft({depth}) = {n}  [{:.2}s, {:.1} Mnodes/s]",
        s,
        n as f64 / s / 1e6
    );
}
```

Note : `perft_detailed` utilise `depth == 2` comme niveau de comptage, parce que `depth == 1` est déjà traité en bulk. La convention de comptage reste « à la feuille » : on marque les coups qui *mènent* à une feuille.

- [ ] **Step 4: Lancer les tests**

```bash
cargo test --release --test gct_perft -- --nocapture
```

Attendu : `t1_gct_perft_shallow` et `gct_perft_egale_le_movegen_existant…` passent.

- [ ] **Step 5: Valider en profondeur (T2, optionnel mais exigé pour le rapport)**

```bash
cargo test --release --test gct_perft -- --ignored --nocapture
cargo run --release --example gctperft -- "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -" 4
```

Attendu : `gct perft(4) = 4085603` (valeur GCT Kiwipete d4) et les tests `ignored` passent.

- [ ] **Step 6: Clippy, suite complète, commit**

```bash
cargo clippy --all-targets --release -- -D warnings
cargo test --release
git add src/gct/perft.rs tests/gct_perft.rs examples/gctperft.rs
git commit -m "gct(part7): perft bulk + validation contre la table Grand Chess Tree"
```

---

### Task 7: Canonicalisation du carré en-passant (bug prouvé, phase 2 P0)

**Files:**
- Modify: `src/position.rs`
- Test: `tests/ep_zobrist.rs`

**Interfaces:**
- Consumes: `crate::position::{Position, NO_EP}`, `crate::tables::PAWN_ATTACKERS`, `crate::types::*`.
- Produces: `Position::ep` **canonique** — la case ep n'est conservée que si un pion adverse peut effectivement y capturer. Aucune signature publique ne change.

**Justification du gel** (`MIGRATION.md` : « pas de refactoring core sauf bug PROUVÉ ») : `make` écrit la case ep après **tout** double pas, même sans pion adverse adjacent ; le hash Zobrist inclut le fichier ep. Deux positions identiques (même plateau, même trait, mêmes droits) atteintes l'une par un double pas, l'autre par une poussée simple sur deux temps, ont donc **deux hashs différents**. C'est un bug de qualité de hash : recherche de répétition et future table de transposition traitent des positions identiques comme distinctes. Le bug est prouvé par le test de l'étape 1, qui doit échouer avant le correctif et passer après.

- [ ] **Step 1: Écrire le test qui échoue**

Créer `tests/ep_zobrist.rs` :

```rust
//! Le carré en-passant doit être **canonique** : conservé seulement si une
//! capture en-passant est réellement disponible.
//!
//! Bug prouvé : `make` pose la case ep après tout double pas, même sans pion
//! adverse adjacent, et le hash Zobrist inclut le fichier ep. La même position
//! atteinte autrement a donc un hash différent.

use mychess::position::Position;
use mychess::types::*;

fn sq(name: &str) -> u8 {
    square_from_uci(name.as_bytes()).expect("case valide").index()
}

#[test]
fn double_pas_sans_pion_adjacent_ne_laisse_pas_de_case_ep() {
    let mut pos = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
    let u = pos.make(Move::new(sq("e2"), sq("e4"), FLAG_DOUBLE_PUSH));
    assert_eq!(
        pos.ep, NO_EP,
        "aucun pion noir en d4/f4 : la case ep doit etre effacee"
    );
    pos.unmake(Move::new(sq("e2"), sq("e4"), FLAG_DOUBLE_PUSH), u);
    assert_eq!(pos.ep, NO_EP, "unmake doit restaurer la case ep d'origine");
}

#[test]
fn double_pas_avec_pion_adjacent_conserve_la_case_ep() {
    let mut pos = Position::from_fen("4k3/8/8/8/3p4/8/4P3/4K3 w - - 0 1").unwrap();
    let u = pos.make(Move::new(sq("e2"), sq("e4"), FLAG_DOUBLE_PUSH));
    assert_eq!(pos.ep, sq("e3"), "un pion noir en d4 peut capturer e3");
    assert_eq!(pos.ep_capture_square(Color::Black), Some(sq("e4")));
    pos.unmake(Move::new(sq("e2"), sq("e4"), FLAG_DOUBLE_PUSH), u);
}

#[test]
fn deux_chemins_vers_la_meme_position_donnent_le_meme_hash() {
    // Position A : le pion blanc arrive en e4 par un double pas (aucun pion
    // noir en d4/f4 -> la case ep est un fantome).
    let mut a = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
    a.make(Move::new(sq("e2"), sq("e4"), FLAG_DOUBLE_PUSH));

    // Position B : la meme position, construite par FEN (aucune case ep).
    let b = Position::from_fen("4k3/8/8/8/4P3/8/8/4K3 b - - 0 1").unwrap();

    assert_eq!(
        a.zobrist, b.zobrist,
        "deux positions identiques doivent hasher identiquement"
    );
    assert_eq!(a.recompute_zobrist(), a.zobrist);
    assert_eq!(b.recompute_zobrist(), b.zobrist);
}

#[test]
fn une_fen_avec_case_ep_fantome_est_canonisee() {
    // Case ep e3 annoncee sans pion blanc en d4/f4 : rien ne peut capturer.
    let p = Position::from_fen("4k3/8/8/8/4P3/8/8/4K3 b - e3 0 1").unwrap();
    assert_eq!(p.ep, NO_EP, "case ep sans captureur : canonisee a l'entree");
    assert_eq!(p.recompute_zobrist(), p.zobrist);

    // Et quand un captureur existe, la case est conservee.
    let q = Position::from_fen("4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1").unwrap();
    assert_eq!(q.ep, sq("e3"));
    assert_eq!(q.recompute_zobrist(), q.zobrist);
}

#[test]
fn la_canonisation_ne_change_aucun_compte_de_perft() {
    // Les deux positions sont identiques pour le movegen : meme arbre.
    let fens = [
        "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
        "4k3/8/8/4P3/8/8/8/4K3 w - d6 0 1",
        "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
    ];
    for fen in fens {
        let mut a = Position::from_fen(fen).unwrap();
        let mut b = Position::from_fen(fen).unwrap();
        for d in 0..=4u32 {
            assert_eq!(
                mychess::perft::perft(&mut a, d),
                mychess::perft::perft(&mut b, d),
                "{fen} d{d}"
            );
        }
    }
}
```

- [ ] **Step 2: Lancer pour voir le rouge**

```bash
cargo test --release --test ep_zobrist -- --nocapture
```

Attendu : `double_pas_sans_pion_adjacent_ne_laisse_pas_de_case_ep` et `deux_chemins_vers_la_meme_position_donnent_le_meme_hash` et `une_fen_avec_case_ep_fantome_est_canonisee` échouent. `la_canonisation_ne_change_aucun_compte_de_perft` passe déjà (il documente l'invariant à préserver).

- [ ] **Step 3: Implémenter la canonicalisation dans `make`**

Dans `src/position.rs`, remplacer le bloc :

```rust
        if self.ep != NO_EP {
            zob ^= z.ep_file[(self.ep & 7) as usize];
        }
        self.ep = if flags == FLAG_DOUBLE_PUSH {
            ((from + to) / 2) as u8
        } else {
            NO_EP
        };
        if self.ep != NO_EP {
            zob ^= z.ep_file[(self.ep & 7) as usize];
        }
```

par :

```rust
        if self.ep != NO_EP {
            zob ^= z.ep_file[(self.ep & 7) as usize];
        }
        // Canonicalisation : la case ep n'est conservee que si un pion adverse
        // peut effectivement y capturer. Sans cela, un double pas sans
        // captureur laisse une case « fantome » dans le hash Zobrist, et deux
        // positions identiques atteintes autrement hashent differemment
        // (repetition et transposition ratees). Le pion mobile est deja pose
        // sur `to` a ce stade, donc l'ensemble des captureurs est exact.
        self.ep = if flags == FLAG_DOUBLE_PUSH {
            let candidate = ((from + to) / 2) as u8;
            if self.has_ep_capturer(candidate, us) {
                candidate
            } else {
                NO_EP
            }
        } else {
            NO_EP
        };
        if self.ep != NO_EP {
            zob ^= z.ep_file[(self.ep & 7) as usize];
        }
```

`us` est l'index de couleur du pion mobile (`0` = blanc, `1` = noir), déjà calculé plus haut dans `make` (`let us = (code / 6) as usize;`). Ajouter la méthode dans `impl Position` :

```rust
    /// Un pion adverse peut-il capturer en passant sur `ep` ?
    ///
    /// Sert à canonicaliser la case ep : une case annoncée sans captureur
    /// n'a aucun effet légal, mais polluerait le hash Zobrist.
    #[inline(always)]
    fn has_ep_capturer(&self, ep: u8, us: usize) -> bool {
        let them = 1 - us;
        let idx = them * 6 + PAWN;
        debug_assert!(ep < 64);
        crate::tables::PAWN_ATTACKERS[them][ep as usize]
            & crate::zobrist_index_pawn(them)
            & self.bb_color[them]
            != 0
            && self.bb_piece[PAWN] & self.bb_color[them] != 0
            && (crate::tables::PAWN_ATTACKERS[them][ep as usize]
                & self.bb_piece[PAWN]
                & self.bb_color[them])
                != 0
            && {
                let _ = idx;
                true
            }
    }
```

Remplacer cette ébauche par la version simple (c'est celle-ci qui est attendue) :

```rust
    /// Un pion adverse peut-il capturer en passant sur `ep` ?
    ///
    /// Sert à canonicaliser la case ep : une case annoncée sans captureur
    /// n'a aucun effet légal, mais polluerait le hash Zobrist.
    #[inline(always)]
    fn has_ep_capturer(&self, ep: u8, us: usize) -> bool {
        let them = 1 - us;
        if ep >= 64 {
            return false;
        }
        crate::tables::PAWN_ATTACKERS[them][ep as usize]
            & self.bb_piece[PAWN]
            & self.bb_color[them]
            != 0
    }
```

- [ ] **Step 4: Canonicaliser aussi à l'entrée FEN**

Dans `from_fen`, juste avant `pos.zobrist = pos.recompute_zobrist();`, insérer :

```rust
        // Meme canonicalisation qu'a la sortie de `make` : une FEN peut
        // annoncer une case ep sans pion capable de capturer.
        if pos.ep != NO_EP {
            let them = if pos.side == Color::White { 1 } else { 0 };
            let capturers = crate::tables::PAWN_ATTACKERS[them][pos.ep as usize]
                & pos.bb_piece[PAWN]
                & pos.bb_color[them];
            if capturers == 0 {
                pos.ep = NO_EP;
            }
        }
        pos.zobrist = pos.recompute_zobrist();
```

`pos.side` est la couleur au trait, donc `them = 1 - side_idx` est bien la couleur qui peut capturer. Utiliser `pos.side.opp().idx()` plutôt que l'arithmétique :

```rust
            let them = pos.side.opp().idx();
```

- [ ] **Step 5: Lancer les tests**

```bash
cargo test --release --test ep_zobrist -- --nocapture
cargo test --release
```

Attendu : les 5 tests de `ep_zobrist` passent, **et toute la suite passe** — en particulier `tests/malformed_fen.rs` (une case ep sans pion reste sans effet), `tests/differential.rs` (intégrité du Zobrist incrémental), et `tests/perft_golden.rs` (aucun compte de nœuds ne change).

- [ ] **Step 6: Revalidation perft explicite**

```bash
cargo test --release --test perft_golden -- --nocapture
cargo run --release -- --perft-help 2>/dev/null || true
cargo run --release -- perft "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1" 6
```

Attendu : `perft(6) = 119060324`, et les 7 goldens passent.

- [ ] **Step 7: Commit**

```bash
git add src/position.rs tests/ep_zobrist.rs
git commit -m "fix(position): canonicaliser la case ep dans le hash (bug prouve) + revalidation perft"
```

---

### Task 8: Mesure — `gct` vs `movegen` vs `motors`

**Files:**
- Create: `benches/gct.rs`
- Modify: `Cargo.toml` (une entrée `[[bench]]`)
- Create: `tools/compare_movegen.sh`
- Create: `docs/movegen/gct-benchmark-2026-10-21.md`
- Test: exécution manuelle des commandes ci-dessous

**Interfaces:**
- Consumes: `mychess::gct::generate::generate`, `mychess::gct::perft::perft`, `mychess::movegen::{generate, count_moves}`, `mychess::perft::perft`.
- Produces: un rapport chiffré et reproductible, aucune API publique nouvelle.

- [ ] **Step 1: Écrire le bench criterion**

Créer `benches/gct.rs` :

```rust
//! Comparaison directe des deux generateurs et des deux perfts.
//!
//! Criterion, pas de seuil absolu : on lit des **ratios** dans une meme
//! session, machine au repos.

use criterion::{criterion_group, criterion_main, Criterion};
use mychess::gct::generate::generate as gct_generate;
use mychess::gct::perft::perft as gct_perft;
use mychess::movegen::{MoveList, count_moves, generate};
use mychess::perft::perft;
use mychess::position::Position;
use mychess::types::GenKind;

const FENS: [(&str, &str); 4] = [
    (
        "startpos",
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    ),
    (
        "kiwipete",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -",
    ),
    (
        "pos3",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    ),
    (
        "sje",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    ),
];

fn bench_generate(c: &mut Criterion) {
    let mut g = c.benchmark_group("generate");
    for (name, fen) in FENS {
        let pos = Position::from_fen(fen).unwrap();
        // `MoveList::len` est un champ prive : on construit une liste neuve a
        // chaque iteration. C'est exactement ce que fait un appelant reel, et
        // cela inclut donc le cout de `MoveList::new()` (~15 ns), identique
        // pour les deux implementations — le ratio reste juste.
        g.bench_function(format!("gct/{name}"), |b| {
            b.iter(|| {
                let mut l = MoveList::new();
                gct_generate(&pos, &mut l)
            });
        });
        g.bench_function(format!("movegen/{name}"), |b| {
            b.iter(|| {
                let mut l = MoveList::new();
                generate(&pos, &mut l, GenKind::All);
                l.len()
            });
        });
    }
    g.finish();
}

fn bench_count(c: &mut Criterion) {
    let mut g = c.benchmark_group("count");
    for (name, fen) in FENS {
        let pos = Position::from_fen(fen).unwrap();
        g.bench_function(format!("gct/{name}"), |b| {
            b.iter(|| {
                let mut l = MoveList::new();
                gct_generate(&pos, &mut l)
            });
        });
        g.bench_function(format!("movegen_count_moves/{name}"), |b| {
            b.iter(|| count_moves(&pos))
        });
    }
    g.finish();
}

fn bench_perft(c: &mut Criterion) {
    let mut g = c.benchmark_group("perft");
    let startpos = Position::from_fen(FENS[0].1).unwrap();
    let kiwipete = Position::from_fen(FENS[1].1).unwrap();
    g.bench_function("gct/startpos_d4", |b| {
        b.iter(|| {
            let mut p = startpos;
            gct_perft(&mut p, 4)
        });
    });
    g.bench_function("movegen/startpos_d4", |b| {
        b.iter(|| {
            let mut p = startpos;
            perft(&mut p, 4)
        });
    });
    g.bench_function("gct/kiwipete_d3", |b| {
        b.iter(|| {
            let mut p = kiwipete;
            gct_perft(&mut p, 3)
        });
    });
    g.bench_function("movegen/kiwipete_d3", |b| {
        b.iter(|| {
            let mut p = kiwipete;
            perft(&mut p, 3)
        });
    });
    g.finish();
}

criterion_group!(benches, bench_generate, bench_count, bench_perft);
criterion_main!(benches);
```

Ajouter dans `Cargo.toml`, après le bloc `[[bench]] name = "slider"` :

```toml
[[bench]]
name = "gct"
harness = false
```

- [ ] **Step 2: Vérifier que le bench compile et tourne**

```bash
cargo bench --bench gct -- --warm-up-time 1 --measurement-time 3 2>&1 | tail -40
```

Attendu : les trois groupes produisent des chiffres. Noter les médianes.

- [ ] **Step 3: Écrire le harnais de comparaison avec `motors`**

Créer `tools/compare_movegen.sh` :

```bash
#!/usr/bin/env bash
# Comparaison de debit perft : mychess (movegen) / mychess (gct) / motors.
#
# motors est clone dans un cache stable (pas /tmp : trois redemarrages l'ont
# deja efface) et epingle a un commit precis, pour que la comparaison reste
# reproductible.
set -euo pipefail

MOTORS_REPO="${MOTORS_REPO:-https://github.com/toanth/motors}"
MOTORS_COMMIT="${MOTORS_COMMIT:-fdb697373e03d21e26a254dea0f8613a596ca630}"
CACHE="${HOME}/.cache/mychess"
MOTORS_DIR="${CACHE}/motors"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "== build mychess =="
cargo build --release --quiet
MYCHESS="${ROOT}/target/release/mychess"

if [[ ! -d "$MOTORS_DIR/.git" ]]; then
    mkdir -p "$CACHE"
    git clone --quiet "$MOTORS_REPO" "$MOTORS_DIR"
fi
git -C "$MOTORS_DIR" fetch --quiet origin "$MOTORS_COMMIT" || true
git -C "$MOTORS_DIR" checkout --quiet "$MOTORS_COMMIT"
echo "== build motors @ ${MOTORS_COMMIT:0:7} =="
cargo build --release --quiet --manifest-path "${MOTORS_DIR}/Cargo.toml" -p motors
MOTORS="${MOTORS_DIR}/target/release/motors"

POSITIONS=(
  "startpos|rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1|6"
  "kiwipete|r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -|5"
  "sje|r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10|5"
)

printf '\n%-10s %5s %14s %12s %12s %12s\n' \
    "position" "depth" "nodes" "mychess" "gct" "motors"
printf '%.0s-' {1..70}; printf '\n'

for entry in "${POSITIONS[@]}"; do
    IFS='|' read -r name fen depth <<< "$entry"

    my_out=$("$MYCHESS" perft "$fen" "$depth" 2>/dev/null)
    my_nodes=$(sed -E 's/.*= ([0-9]+).*/\1/' <<< "$my_out")
    my_mnps=$(sed -E 's/.*, ([0-9.]+) Mnodes\/s.*/\1/' <<< "$my_out")

    gct_out=$(cargo run --release --quiet --example gctperft -- "$fen" "$depth")
    gct_nodes=$(sed -E 's/.*= ([0-9]+).*/\1/' <<< "$gct_out")
    gct_mnps=$(sed -E 's/.*, ([0-9.]+) Mnodes\/s.*/\1/' <<< "$gct_out")

    motors_out=$(printf 'ugi\nposition fen %s\ngo perft %s\nquit\n' "$fen" "$depth" \
                 | "$MOTORS" | grep -E 'Nodes searched')
    motors_nodes=$(sed -E 's/.*: *([0-9]+).*/\1/' <<< "$motors_out")
    # motors n'affiche que les Mnps dans sa ligne "Finished perft" : on la
    # recupere separement.
    motors_mnps=$(printf 'ugi\nposition fen %s\ngo perft %s\nquit\n' "$fen" "$depth" \
                 | "$MOTORS" | grep -E 'Finished perft' | sed -E 's/.*\(([0-9.]+)Mnps\).*/\1/')

    if [[ "$my_nodes" != "$gct_nodes" || "$my_nodes" != "$motors_nodes" ]]; then
        echo "ECHEC: comptes de noeuds divergents pour $name d$depth" >&2
        echo "  mychess=$my_nodes gct=$gct_nodes motors=$motors_nodes" >&2
        exit 1
    fi

    printf '%-10s %5s %14s %12s %12s %12s\n' \
        "$name" "$depth" "$my_nodes" "$my_mnps" "$gct_mnps" "$motors_mnps"
done

echo
echo "Mnodes/s, une seule execution par cellule : pour un verdict,"
echo "utiliser l'A/B/A/B de docs/movegen/gct-benchmark-2026-10-21.md."
```

Rendre exécutable :

```bash
chmod +x tools/compare_movegen.sh
```

- [ ] **Step 4: Lancer le harnais**

```bash
cat /proc/loadavg
tools/compare_movegen.sh
```

Attendu : trois lignes, comptes de nœuds **identiques** dans les trois colonnes (startpos d6 = 119060324, kiwipete d5 = 193690690, sje d5 = 164075551). Toute divergence est un échec de génération, pas un problème de mesure. Noter `loadavg` : si la charge dépasse ~1.5, relancer plus tard.

- [ ] **Step 5: Mesure A/B/A/B et `perf stat`**

```bash
# 4 legs alternes, machine au repos, memes binaires.
for leg in 1 2 3 4; do
  echo "--- leg $leg ---"
  ./target/release/mychess perft "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -" 5
  cargo run --release --quiet --example gctperft -- "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -" 5
done

# Compteurs materiels (insensibles a la charge) sur le perft gct.
perf stat -e instructions,cycles,cache-misses,L1-dcache-load-misses \
  cargo run --release --quiet --example gctperft -- \
  "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -" 4 2>&1 | tail -20
perf stat -e instructions,cycles,cache-misses,L1-dcache-load-misses \
  ./target/release/mychess perft \
  "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq -" 4 2>&1 | tail -20
```

Attendu : 4 mesures par implémentation, médiane retenue, et les compteurs `instructions`/`cycles` pour les deux. Consigner les chiffres bruts, jamais un seuil.

- [ ] **Step 6: Écrire le rapport**

Créer `docs/movegen/gct-benchmark-2026-10-21.md` avec cette structure, **les chiffres mesurés remplis** (aucun champ laissé vide, aucun chiffre inventé) :

```markdown
# Movegen : portage GCT vs `movegen` vs `motors` — mesures du 2026-10-21

## Protocole

- Hôte : Ryzen 5 1600X (Zen 1), hyperviseur partagé ; `loadavg` relevé avant chaque série.
- A/B/A/B alterné, 4 legs, médiane retenue. Aucun seuil absolu.
- `perf stat -e instructions,cycles,cache-misses,L1-dcache-load-misses`.
- `motors` épinglé à `fdb697373e03d21e26a254dea0f8613a596ca630`.

## Correction des sorties

Trois implémentations, mêmes nœuds partout (sinon le harnais échoue) :

| position | depth | nœuds | mychess | gct | motors |
|---|---|---|---|---|---|
| startpos | | | | | |
| kiwipete | | | | | |
| sje | | | | | |

## Coût par nœud (criterion, médianes)

| bench | gct | movegen | ratio |
|---|---|---|---|
| generate/startpos | | | |
| generate/kiwipete | | | |
| generate/sje | | | |
| perft/startpos_d4 | | | |
| perft/kiwipete_d3 | | | |

## Compteurs matériels

| implémentation | instructions / nœud | cycles / nœud | L1-dcache-load-misses |
|---|---|---|---|
| mychess | | | |
| gct | | | |
| motors | | | |

## Lecture

Trois passes de légalité séparées, une boucle par pièce ennemie dans
`king_danger`, un `MoveList` réutilisé : le portage `gct` **doit** être plus
lent que `movegen`, qui a fusionné les passes (+19 %) et bulk-calcule les
pions. Le chiffre attendu est donc un écart **en faveur de `movegen`** ; un
`gct` plus rapide signalerait une erreur de mesure.

L'écart avec `motors` est le vrai objectif : `motors` calcule `danger` en bulk
(Kogge-Stone), sans table d'attaques, et c'est la cible de la phase 2.

## Verdict

- Exactitude : les trois implémentations donnent les mêmes comptes sur les
  trois positions, validés contre la table Grand Chess Tree (nodes **et**
  colonnes détaillées, conversion `checks = checks + mates`).
- Performance : `gct` = ____ % de `movegen` ; `movegen` = ____ % de `motors`.
- Décision : `src/movegen.rs` reste le générateur de production. `src/gct/`
  devient la base de travail de la phase 2 (bulk `danger`, buffer réutilisable,
  `has_legal_move`).
```

- [ ] **Step 7: Vérification finale complète**

```bash
cargo clippy --all-targets --release -- -D warnings
cargo test --release
cargo test --release --test gct_perft -- --ignored
git status --short
```

Attendu : clippy propre, tous les tests passent, `git status` ne montre que les fichiers de la tâche.

- [ ] **Step 8: Commit**

```bash
git add benches/gct.rs Cargo.toml tools/compare_movegen.sh docs/movegen/gct-benchmark-2026-10-21.md
git commit -m "gct: benches criterion + harnais de comparaison motors + rapport de mesure"
```

---

## Self-Review

### 1. Couverture de la spécification

Chaque exigence du post (et chaque correction annoncée du listing) est couverte par une tâche :

| Exigence / artefact du post | Couvert par |
|---|---|
| Part 1 — contrat de perft (`generate → make → recurse → unmake`, feuilles seules) | Task 6 (`gct::perft::perft`) |
| Part 2 — plateau (12 bitboards + mailbox 8×8), LERF | **Réutilisé** : `crate::position::Position` a déjà exactement cette forme. Aucune tâche ne le réécrit (le gel du core l'interdit). |
| Part 3 — encodage `flag << 12 \| from << 6 \| to` | **Réutilisé** : `crate::types::Move` a exactement ce bit layout (`Move::new(from, to, flags)` = `(from) \| (to<<6) \| (flags<<12)`), avec les 16 mêmes flags. |
| Part 4 — `Bitboards.ShiftN…ShiftSW` | Task 2 (`shift_n` … `shift_sw`), testés contre la grille naïve |
| Part 4 — `Attacks.Knight/King/Pawn/Ray` | **Réutilisé** : `crate::tables` fournit les mêmes tables, bit-identiques, en `const fn` |
| Part 4 — `Attacks.IsAttackedBy` | Task 2 |
| Part 4 — `Rank3` / `Rank6` manquants dans le listing | Task 4 : `RANK_3` / `RANK_6` sont utilisés depuis `crate::tables` (le bug d'extraction disparaît) |
| Part 4 — génération pseudo-légale | **Non portée** : sans objet dans le module, dont la raison d'être est le générateur **légal** du Part 7 ; `crate::refgen` joue déjà le rôle d'oracle pseudo-légal |
| Part 5 — `MakeMove` / `UnmakeMove` / `UndoInfo` / `CastlingRightsMask` | **Réutilisé** : `Position::make` / `unmake` / `CASTLE_MASK_LUT` font déjà exactement ces 11 étapes |
| Part 5 — `RebuildOccupancy` (recalcul complet de l'occupation) | **Non porté** : `mychess` maintient l'occupation de façon incrémentale. Recopier `RebuildOccupancy` serait une régression (voir `docs/analyse_movegen_mcts.md` §5). |
| Part 6 — magics + `PEXT` | **Réutilisé** : `crate::slider` fournit `rook_attacks` / `bishop_attacks` (magic généré **et prouvé** à la compilation, dispatch PEXT/magic). Aucune tâche ne réimplémente les magics. |
| Part 7 — `ComputeKingDanger` | Task 3 |
| Part 7 — `ComputeCheckers` | Task 3 |
| Part 7 — `ComputePins` + `ScanPinners` + `pinLines = ulong.MaxValue` | Task 3 |
| Part 7 — `SquaresBetween` (64×64) | Task 2 (reconstruite indépendamment) |
| Part 7 — ordre : roi d'abord, retour double échec, `check_mask`, pièces, roque | Task 4 + Task 5 |
| Part 7 — `GenKnightsLegal`, `GenSlidersLegal`, `GenPawnsLegal`, `EmitPromos` | Task 4 |
| Part 7 — `GenCastlingLegal` | Task 5 |
| Part 7 — `GenEnPassant` / `TryEpMove` | Task 5 |
| Part 7 — `delegate AttackFn` | Supprimé (appels directs) — documenté en tête de `src/gct/generate.rs` |
| Part 7 — dame générée deux fois (Part 4) / une fois (Part 7) | Task 4 : une seule passe `bishop ∪ rook` |
| Part 7 — erreurs de compilation du listing (`Span<ulong>`, `GenEnPassant` 4/6 args, `EmitTargets` hors classe, `Rank3`/`Rank6`, `kingSq` inutilisé) | Corrigées, pas recopiées — Task 2/4/5, documentées en tête de `src/gct/mod.rs` |
| Perft numbers de la série (startpos d1–d6, Kiwipete d1–d5) | Task 1 (goldens GCT, plus profonds) + Task 6 (validation) |
| Table détaillée Grand Chess Tree | Task 1 (oracle) + Task 6 (validation `gct` contre table) |
| Comparaison avec `motors` | Task 8 |

Écart assumé et documenté : les Parts 2, 3, 5 (make/unmake) et 6 (magics) **ne sont pas réécrites** — `mychess` possède déjà des versions supérieures à celles des posts (occupation incrémentale, magics prouvés à la compilation, pas de `RebuildOccupancy`). Réécrire ces couches violerait le gel du core sans gain mesurable. La réécriture porte donc sur ce que le post apporte réellement : la **chaîne de génération de coups légaux**, du Part 4 au Part 7.

### 2. Balayage des placeholders

- « TBD » / « TODO » / « à implémenter plus tard » : **aucun**. Chaque tâche nomme des fichiers exacts, des signatures exactes et du code complet.
- « Ajouter la gestion d'erreur appropriée » : **aucun**. Les deux seules fonctions faillibles (`Position::from_fen`, `args[2].parse()`) ont un chemin d'erreur explicite (`expect` documenté ou message + `exit(2)`).
- « Écrire des tests pour ce qui précède » : **aucun**. Le code de test est donné intégralement.
- « Similaire à la tâche N » : **aucun**. Chaque tâche répète le code dont elle a besoin.
- **Aucun conditionnel laissé ouvert** : les deux seuls cas qui en demandaient un (champ `MoveList::len` privé dans le bench, paramètre `_promo_rank` inutilisé dans `emit_pawn_capture`) ont été corrigés dans le code du plan, pas renvoyés à l'implémenteur.
- **Un conditionnel de diagnostic** : Task 4 Step 4 et Task 5 Step 4 indiquent quoi faire si un test échoue (signe de `delta_from`, FEN de clouage horizontal) et interdisent explicitement d'assouplir l'assertion. Ce sont des chemins de réparation, pas des trous.
- **Un chiffre à remplir** : le rapport de Task 8 contient des cellules vides, mais elles sont remplies **par la mesure** des étapes 4 et 5, pas par l'auteur du plan. Aucun chiffre n'est inventé dans le plan.

### 3. Cohérence des types

| Nom | Défini dans | Utilisé par | Cohérent |
|---|---|---|---|
| `gct::attacks::shift_n/s/e/w/ne/nw/se/sw` | Task 2 | Task 4 (`gen_pawns`) | oui — `pub const fn(u64) -> u64` |
| `gct::attacks::squares_between` | Task 2 | Task 3 (oracle de pins), Task 4 (`check_mask`) | oui — `fn(u8, u8) -> u64` |
| `gct::attacks::is_attacked_by` | Task 2 | Task 2 (test) seulement | oui |
| `gct::legality::king_danger` | Task 3 | Task 4, Task 5 | oui — `(&Position, u8, Color) -> u64` |
| `gct::legality::compute_checkers` | Task 3 | Task 4 | oui — `(&Position, u8, Color) -> u64` |
| `gct::legality::compute_pins` | Task 3 | Task 4 | oui — `(&Position, u8, Color, Color) -> (u64, [u64; 64])` |
| `gct::generate::generate` | Task 4 (partiel), Task 5 (complet) | Task 6 (perft), Task 8 (bench) | oui — `(&Position, &mut MoveList) -> usize` |
| `gct::perft::perft` | Task 6 | Task 6, Task 8 | oui — `(&mut Position, u32) -> u64` |
| `gct::perft::perft_detailed` | Task 6 | Task 6 | oui — `(&mut Position, u32) -> PerftCounts` |
| `PerftCounts` | existant, `src/perft.rs:30` | Task 6 | oui — champs `nodes/captures/ep/castles/promotions/checks/discovery_checks/double_checks/mates` |
| `MoveList` | existant, `src/movegen.rs:11` | Task 4, 5, 6, 8 | oui — `new/push/len/is_empty/get/as_slice/iter` ; `len` **privé**, d'où l'usage de `MoveList::new()` dans les benches |
| `MoveKey = (u8, u8, u8)` | Task 4 (test) | Task 4, Task 5 (tests) | oui — `m.from()`, `m.to()`, `m.flags()` sont tous `pub const fn` |
| `NO_EP` / `NO_SQUARE` | existant, `src/types.rs` | Task 4, 5, 7 | oui — les deux valent `64` |
| `FLAG_*` | existant, `src/types.rs:190-203` | Task 4, 5, 7 | oui — `FLAG_QUIET/DoublePush/KingCastle/QueenCastle/Capture/EpCapture/PromoN…PromoCapQ` |
| `Position::has_ep_capturer` | Task 7 | `make`, `from_fen` | oui — `(&self, u8, usize) -> bool`, privée |
| `examples/gctperft.rs` | Task 6 | Task 8 (harnais + `perf stat`) | oui — impression `gct perft(D) = N  [X.XXs, Y.Y Mnodes/s]`, parsée par sed |

Incohérence détectée et corrigée pendant la rédaction :

1. **`MoveList::len` est privé** — la première version du bench écrivait `l.len = 0`. Corrigé en construisant une liste neuve par itération (le coût de `MoveList::new()`, ~15 ns, est inclus dans les deux bras, donc le ratio reste juste).
2. **`Move::0` est `pub` mais la convention du dépôt interdit d'y toucher hors `src/types.rs`** — la première version de `MoveKey` comparait `m.0`. Corrigé en `(from, to, flags)`.
3. **`gen_pawns` recevait `us` alors que `W` le détermine** — la première version comparait `us` à une constante *à l'intérieur* de la boucle. Laissé tel quel mais en tête de fonction (un seul `if`, replié à la compilation), pour garder les deux paramètres explicites à l'appel.
4. **`emit_pawn_capture` portait un paramètre `_promo_rank` inutilisé** — vestige du `RANK_8`/`RANK_1` du post, déjà appliqué par l'appelant (`cap_ne & RANK_8`). Supprimé, les quatre sites d'appel sont alignés sur une seule ligne.
5. **`gen_knights` appelait un `enemy_is_king_hole()` qui retournait `0`** — masque décoratif. Supprimé ; un commentaire explique pourquoi la case du roi ennemi n'a pas besoin d'être retirée là (elle est occupée, donc absente de `!us_bb`, et déjà absente de `enemy`).
6. **`Position` n'implémente pas `Debug`** — sept assertions de test écrivaient `{pos:?}` et ne compilaient donc pas. Ajouter le derive toucherait au core gelé. Corrigé : un helper `describe(&pos)` (rendu du plateau depuis `mailbox`, `side`, `castle`, `ep`, tous publics) est fourni dans chacun des trois fichiers de test concernés (`tests/gct_attacks.rs`, `tests/gct_legality.rs`, `tests/gct_equivalence.rs`).

Leçon pour l'exécuteur : le code C# de source a été lu intégralement, mais **le code Rust de ce plan n'a pas été compilé**. Il doit être traité comme un premier jet à corriger contre le compilateur, pas comme du code fini. Les endroits les plus susceptibles de demander un ajustement sont : les conversions de signe dans `emit_pawn_pushes` / `emit_pawn_capture` (`i16` → `u8`), l'ordre des arguments de `Move::new(from, to, flags)`, et les appels `let Some(...) = ... else { ... }` (let-else, stable depuis 1.65, disponible en 1.97).
