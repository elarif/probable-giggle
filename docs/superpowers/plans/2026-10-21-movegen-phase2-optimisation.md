# Phase 2 — Optimisation de `src/movegen.rs` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rendre `src/movegen.rs` plus rapide par appel, à API publique inchangée, en supprimant le `memset` de `MoveList` et en ajoutant une terminaison bon marché (`has_legal_move`), puis mesurer contre `motors`.

**Architecture:** Le générateur de production n'est pas réécrit : `generate` garde sa passe de légalité fusionnée (`build_legality` → `checkers` / `danger` / `pinned` + `pin_lines`). On attaque trois coûts mesurés : l'initialisation de `MoveList` (512 octets remis à zéro à chaque appel), l'absence de test de terminaison sans matérialisation de liste, et la génération en échec qui paie toutes les classes de pièces alors que le masque `allowed` est minuscule. Le portage `src/gct/` est supprimé : `motors` et la table publiée Grand Chess Tree suffisent comme oracles externes, et `src/refgen.rs` reste l'oracle interne.

**Tech Stack:** Rust edition 2024, MSRV 1.97 (`rust-toolchain.toml`), `rayon`, criterion 0.5 (dev), `microbench` 0.5 (dev).

## Global Constraints

- Rust edition 2024, toolchain épinglée `1.97.0` par `rust-toolchain.toml`. Ne pas la changer.
- `u64::isolate_lowest_one` est obligatoire à la place de `x & x.wrapping_neg()` (lints du dépôt).
- **API publique inchangée** pour `generate`, `generate_all`, `generate_captures`, `count_moves`, `attackers_to`, `compute_pins`, et pour `MoveList::{new, push, len, is_empty, get, as_slice, iter}`. Les ajouts sont autorisés (`has_legal_move`), les changements de signature existante ne le sont pas.
- `unsafe` autorisé **uniquement** dans `impl MoveList` de `src/movegen.rs`, chaque bloc avec un commentaire `// SAFETY:` qui énonce l'invariant. Aucun autre `unsafe` n'est introduit.
- `Position`, `src/types.rs`, `src/tables.rs`, `src/slider.rs`, `src/position.rs` sont **gelés** : aucune modification.
- Comportement observable identique : pour toute position légale, l'ensemble des coups émis par `generate(pos, _, GenKind::All)` ne change pas, `count_moves` ne change pas, et le perft détaillé ne change pas d'un seul nœud.
- `cargo clippy --all-targets` et `cargo fmt -- --check` sont cassés sur `main` (`examples/first_case.rs`, `examples/d9_motif_shard.rs`). Utiliser `cargo clippy --release --tests --bins --lib -- -D warnings`, `cargo clippy --release --benches -- -D warnings` et `rustfmt --edition 2024 --check <fichiers touchés>`.
- Mesure : criterion A/B dans la même session, machine au repos, au moins deux jambes, médianes. Variance locale ±20 %. Lire des **ratios**, jamais des seuils absolus de nps.
- Chaque tâche se termine par une suite verte et un commit unique en français.

## Hors périmètre

- Aucune modification de `src/mcts.rs`, `src/memo.rs`, `src/uci.rs`. `has_legal_move` n'est **pas** câblé au MCTS dans ce plan.
- Aucune spécialisation pour perft : pas de comptage en bulk à profondeur 2, pas de `perft` réécrit.
- Aucune refonte de l'arène `Node`/`Edge` du MCTS, aucun parallélisme MCTS, aucun NNUE. Ce sont les leviers de la phase 3 (`docs/analyse_movegen_mcts.md` §7).
- Aucune optimisation de `src/gct/` : le module est supprimé.

## Plafond attendu, à dire dès maintenant

Les gains de ce plan portent sur le **coût par appel du générateur**, qui n'est qu'une fraction d'une simulation MCTS. Le `memset` représente ~15 ns sur les 110 ns d'un `generate(All)` sur la position initiale, soit ~14 % de l'appel. Si une simulation coûte ~2 µs au total, le gain de bout en bout est de l'ordre du pourcent. Ce plan rend le générateur meilleur ; il ne multiplie pas les simulations par deux. Les leviers qui multiplient les simulations sont hors périmètre et nommés ci-dessus.

---

## Structure des fichiers

| Fichier | Rôle | Tâche |
|---|---|---|
| `src/gct/{mod,attacks,legality,generate,perft}.rs` | **supprimés** | 1 |
| `tests/gct_{attacks,equivalence,legality,perft}.rs` | **supprimés** | 1 |
| `tests/gct_oracle.rs` | **renommé** `tests/perft_publie.rs`, conserve l'ancre externe | 1 |
| `examples/gctperft.rs` | **supprimé** | 1 |
| `benches/gct.rs` | **supprimé** | 1 |
| `src/lib.rs` | retirer `pub mod gct;` | 1 |
| `tools/compare_movegen.sh` | retirer le bras `gct`, garder `mychess` vs `motors` | 1 |
| `src/movegen.rs` | `MoveList` sans `memset` ; `has_legal_move` ; évasions | 2, 3, 4 |
| `benches/movegen.rs` | nouveau : `movelist_new`, `has_legal_move`, positions en échec | 2, 3, 4 |
| `tests/movegen_terminaison.rs` | nouveau : différentiel `has_legal_move` | 3 |
| `docs/movegen/phase2-2026-10-21.md` | rapport de mesure | 5 |

---

### Task 1: Retirer `src/gct/` en préservant l'ancre externe

**Files:**
- Delete: `src/gct/mod.rs`, `src/gct/attacks.rs`, `src/gct/legality.rs`, `src/gct/generate.rs`, `src/gct/perft.rs`
- Delete: `tests/gct_attacks.rs`, `tests/gct_equivalence.rs`, `tests/gct_legality.rs`, `tests/gct_perft.rs`
- Delete: `examples/gctperft.rs`, `benches/gct.rs`
- Rename: `tests/gct_oracle.rs` → `tests/perft_publie.rs`
- Modify: `src/lib.rs:6`, `Cargo.toml` (bloc `[[bench]] name = "gct"`), `tools/compare_movegen.sh`

**Interfaces:**
- Consumes: rien.
- Produces: suite sans `mychess::gct` ; `tests/perft_publie.rs` valide `mychess::perft::perft_detailed` contre la table publiée (inchangé sur le fond).

**Note de conception, à lire avant d'agir.** `tests/gct_oracle.rs` **ne dépend pas** de `src/gct/` : il importe seulement `mychess::perft::perft_detailed` et `mychess::position::Position`. C'est déjà un test de la table publiée contre le générateur de production. Il survit donc à la suppression et devient la seule ancre externe. Ne pas le supprimer, ne pas l'affaiblir : c'est ce qui empêche `motors` et `mychess` de se donner mutuellement raison sur une règle mal comprise.

- [ ] **Step 1: Constater la base avant de couper**

Run:
```bash
cargo test --release 2>&1 | grep -E "^test result" | grep -oE "[0-9]+ passed" | awk -F' ' '{p+=$1} END {print "passed:", p}'
for t in gct_attacks gct_equivalence gct_legality gct_oracle gct_perft; do
  printf '%s: ' "$t"
  cargo test --release --test "$t" 2>&1 | grep -E "^test result" | grep -oE "^test result: ok\. [0-9]+"
done
```
Expected: `passed: 82`, puis `gct_attacks: 4`, `gct_equivalence: 20`, `gct_legality: 6`, `gct_oracle: 3`, `gct_perft: 2`. Total supprimé = 4 + 20 + 6 + 2 = 32, donc **50 tests attendus après la tâche**. Si ces nombres diffèrent, arrêter et le signaler.

- [ ] **Step 2: Vérifier que l'oracle conservé est bien découplé**

Run:
```bash
grep -n "^use " tests/gct_oracle.rs
```
Expected: exactement deux `use`, `mychess::perft::perft_detailed` et `mychess::position::Position`. Aucune mention de `mychess::gct`. Si une dépendance à `gct` apparaît, elle a été introduite depuis la rédaction et le renommage demande un ajustement — le signaler.

- [ ] **Step 3: Supprimer le module et ses tests**

```bash
git rm -r src/gct tests/gct_attacks.rs tests/gct_equivalence.rs tests/gct_legality.rs tests/gct_perft.rs examples/gctperft.rs benches/gct.rs
```
Puis retirer la ligne `pub mod gct;` de `src/lib.rs` :

```rust
pub mod perft;
```

(Contexte : la déclaration de modules de `src/lib.rs` se termine par `pub mod gct;` juste après `pub mod perft;`. Retirer uniquement la ligne `pub mod gct;`.)

- [ ] **Step 4: Renommer l'ancre et laver ses références mortes**

```bash
git mv tests/gct_oracle.rs tests/perft_publie.rs
```

Dans `tests/perft_publie.rs`, remplacer l'en-tête et le commentaire qui cite le fichier supprimé. L'en-tête actuel commence par :

```rust
//! Validation contre la table de perft détaillée Grand Chess Tree.
```

Le remplacer par :

```rust
//! Validation du générateur de production contre la table de perft détaillée
//! Grand Chess Tree — l'ancre externe du dépôt.
//!
//! Les goldens de `tests/perft/gct/` stockent les colonnes GCT **verbatim**.
//! La conversion vers la convention de `mychess` est faite ici, jamais dans
//! les fichiers — c'est ce qui rend l'oracle indépendant.
//!
//! Ce test ne dépend d'aucun autre générateur : il compare `movegen` à des
//! nombres publiés. `src/gct/` a été supprimé (phase 2) ; `motors` et
//! `src/refgen.rs` couvrent l'axe inter-implémentations.
```

Et remplacer le commentaire de duplication qui cite trois fichiers, dont un supprimé :

```rust
// Duplication VOLONTAIRE avec `tests/perft_golden.rs` et `tests/gct_perft.rs` :
```

par :

```rust
// Duplication VOLONTAIRE avec `tests/perft_golden.rs` :
```

(Le reste du paragraphe, qui explique la convention `checks = checks + mates` et l'interdiction de factoriser, reste valable et doit être conservé.)

- [ ] **Step 5: Retirer le bras `gct` du harnais**

Dans `tools/compare_movegen.sh`, supprimer :
- la ligne de construction `cargo build --release --quiet --example gctperft || die "..."` ;
- l'affectation `GCTPERFT="${ROOT}/target/release/examples/gctperft"` ;
- toute fonction ou appel mesurant `gctperft`.

Le script doit encore mesurer deux choses : `mychess perft <fen> <depth>` et `motors` (`position fen …` puis `go perft <n> threads 1`).

Run:
```bash
bash -n tools/compare_movegen.sh && grep -c "gctperft" tools/compare_movegen.sh || echo "0 reference"
```
Expected: `0 reference`.

- [ ] **Step 6: Retirer l'entrée `[[bench]]` de Cargo.toml**

Supprimer le bloc :

```toml
[[bench]]
name = "gct"
harness = false
```

- [ ] **Step 7: Vérifier que la suite est verte et que l'ancre tient toujours**

Run:
```bash
cargo test --release 2>&1 | grep -E "^test result" | grep -oE "[0-9]+ passed" | awk -F' ' '{p+=$1} END {print "passed:", p}'
cargo test --release --test perft_publie 2>&1 | tail -4
cargo clippy --release --tests --bins --lib -- -D warnings 2>&1 | tail -2
cargo clippy --release --benches -- -D warnings 2>&1 | tail -2
```
Expected: `passed: 50` ; `perft_publie` 3 tests verts ; les deux clippy sans sortie d'erreur.

La preuve que l'ancre n'a pas été affaiblie : `perft_publie` doit encore exercer 15 lignes à d0–d4 et 6 lignes à d5–d6 (les deux tests profonds restent `#[ignore]`). Vérifier avec :

```bash
cargo test --release --test perft_publie -- --list
```
Expected: trois entrées, dont deux marquées `: test` normal.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor(movegen): retirer le portage gct, garder la table publiee comme ancre"
```

---

### Task 2: `MoveList` sans `memset`

**Files:**
- Modify: `src/movegen.rs:1-53`
- Create: `benches/movegen.rs`
- Modify: `Cargo.toml` (entrée `[[bench]] name = "movegen"`)

**Interfaces:**
- Consumes: rien.
- Produces: `MoveList::new()` reste `pub fn new() -> MoveList` ; `push`, `len`, `is_empty`, `get`, `as_slice`, `iter`, `Default` gardent leur signature. Nouveau : `MoveList::new()` ne touche plus la mémoire des 512 octets du tampon.

**Pourquoi c'est sûr.** `[MaybeUninit<Move>; N]` est un type dont **toutes** les valeurs sont valides : `MaybeUninit::uninit().assume_init()` sur ce type est l'idiome documenté par la bibliothèque standard pour obtenir un tableau non initialisé sans `unsafe` supplémentaire. `Move` est `Copy` et sans destructeur, donc rien à `drop`.

- [ ] **Step 1: Écrire les tests de comportement**

Ajouter à la fin de `src/movegen.rs` :

```rust
#[cfg(test)]
mod tests_movelist {
    use super::*;

    #[test]
    fn une_liste_neuve_est_vide() {
        let l = MoveList::new();
        assert_eq!(l.len(), 0);
        assert!(l.is_empty());
        assert_eq!(l.as_slice().len(), 0);
        assert_eq!(l.iter().count(), 0);
    }

    #[test]
    fn default_est_vide() {
        let l = MoveList::default();
        assert!(l.is_empty());
    }

    #[test]
    fn push_puis_get_rend_les_coups_dans_l_ordre() {
        let mut l = MoveList::new();
        let a = Move::new(8, 16, FLAG_QUIET);
        let b = Move::new(1, 18, FLAG_CAPTURE);
        let c = Move::new(48, 56, FLAG_PROMO_Q);
        l.push(a);
        l.push(b);
        l.push(c);
        assert_eq!(l.len(), 3);
        assert_eq!(l.get(0), a);
        assert_eq!(l.get(1), b);
        assert_eq!(l.get(2), c);
        assert_eq!(l.as_slice(), &[a, b, c]);
        assert_eq!(l.iter().collect::<Vec<_>>(), vec![a, b, c]);
    }

    #[test]
    fn la_capacite_est_de_256_et_le_remplissage_complet_est_lisible() {
        let mut l = MoveList::new();
        for i in 0..256u16 {
            l.push(Move(i));
        }
        assert_eq!(l.len(), 256);
        assert_eq!(l.get(0), Move(0));
        assert_eq!(l.get(255), Move(255));
        assert_eq!(l.as_slice().len(), 256);
    }

    #[test]
    fn une_liste_reutilisee_repart_de_zero() {
        let mut l = MoveList::new();
        l.push(Move::new(0, 8, FLAG_QUIET));
        let mut l2 = MoveList::new();
        assert!(l2.is_empty());
        l2.push(Move::new(1, 9, FLAG_QUIET));
        assert_eq!(l2.len(), 1);
    }
}
```

Vérifier que `Move::new` et les constantes `FLAG_*` sont accessibles : `src/movegen.rs` fait déjà `use crate::types::*;`. `Move(i)` est le constructeur tuple public utilisé par `tests/differential.rs:57`.

- [ ] **Step 2: Lancer les tests pour vérifier qu'ils passent déjà**

Run: `cargo test --lib tests_movelist 2>&1 | tail -8`
Expected: 5 tests passent. Ils doivent passer **avant** le changement : ils décrivent le comportement à préserver, pas le nouveau. C'est voulu — le contrat est inchangé, seule la façon de l'atteindre change.

- [ ] **Step 3: Ajouter le banc qui mesure ce qu'on attaque**

Créer `benches/movegen.rs` :

```rust
//! Coût par appel du générateur de production.
//!
//! Criterion, pas de seuil absolu : on lit des ratios dans une même session.

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use mychess::movegen::{MoveList, count_moves, generate};
use mychess::position::Position;
use mychess::types::GenKind;

const FENS: [(&str, &str); 4] = [
    (
        "startpos",
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    ),
    (
        "kiwipete",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    ),
    ("pos3", "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1"),
    ("sje", "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10"),
];

/// Coût d'une liste neuve, isolé. `black_box` empêche le compilateur de
/// supprimer l'appel : sans lui, `MoveList::new()` sans `memset` se réduit à
/// une constante et la mesure ne veut rien dire.
fn bench_movelist_new(c: &mut Criterion) {
    let mut g = c.benchmark_group("movelist_new");
    g.bench_function("new", |b| {
        b.iter(|| std::hint::black_box(MoveList::new()));
    });
    g.finish();
}

fn bench_generate(c: &mut Criterion) {
    let mut g = c.benchmark_group("generate");
    for (name, fen) in FENS {
        let pos = Position::from_fen(fen).unwrap();
        g.bench_function(format!("all/{name}"), |b| {
            b.iter(|| {
                let mut l = MoveList::new();
                generate(&pos, &mut l, GenKind::All);
                std::hint::black_box(l.len())
            });
        });
        g.bench_function(format!("count/{name}"), |b| b.iter(|| count_moves(&pos)));
    }
    g.finish();
}

criterion_group!(benches, bench_movelist_new, bench_generate);
criterion_main!(benches);
```

`BatchSize` n'est pas utilisé : retirer cet import. Ajouter dans `Cargo.toml`, après le bloc `[[bench]] name = "slider"` :

```toml
[[bench]]
name = "movegen"
harness = false
```

- [ ] **Step 4: Mesurer la base, deux jambes**

Run:
```bash
cargo bench --bench movegen -- --save-baseline avant 2>&1 | grep -E "movelist_new|time:" | head -20
cargo bench --bench movegen -- --save-baseline avant2 2>&1 | grep -E "movelist_new|time:" | head -20
```
Expected: `movelist_new/new` autour de **10–20 ns** (le `memset` de 512 octets). Noter les deux médianes, on s'en sert au Step 6. Si la valeur dépasse 40 ns, la mesure est polluée : relancer au repos.

- [ ] **Step 5: Remplacer le tampon par `MaybeUninit`**

Remplacer le haut de `src/movegen.rs` (l'import et la structure, lignes 1–53) par :

```rust
//! Legal move generation with checkmask / pinmask, in bulk where possible.

use crate::position::Position;
use crate::slider::{between, bishop_attacks, rook_attacks};
use crate::tables::{
    DIR_POS, FILE_A, FILE_H, KING_ATT, KNIGHT_ATT, PAWN_ATTACKERS, RANK_3, RANK_6, RANK_8, RAYS,
};
use crate::types::*;
use std::mem::MaybeUninit;

/// Nombre maximal de coups légaux dans une position : 218.
pub const MAX_MOVES: usize = 218;
/// Capacité du tampon. On garde 256 pour rester au-dessus de `MAX_MOVES`
/// même si l'appelant pousse à la main, comme le font les tests.
const CAPACITY: usize = 256;

/// Fixed-capacity move list on the stack: no allocation in the hot path, and
/// no zeroing either — only the first `len` entries are ever read.
///
/// The buffer is `MaybeUninit` so that `new()` costs nothing: a real caller
/// fills the list from scratch every call, and paying 512 bytes of `memset`
/// per node was ~14 % of `generate`'s latency on the opening position.
pub struct MoveList {
    moves: [MaybeUninit<Move>; CAPACITY],
    len: u8,
}

impl MoveList {
    #[inline]
    pub fn new() -> MoveList {
        MoveList {
            // SAFETY: `[MaybeUninit<Move>; CAPACITY]` accepts every bit pattern,
            // including uninitialised memory, so `assume_init` on the *array of
            // MaybeUninit* is sound and does not read anything.
            moves: unsafe { MaybeUninit::<[MaybeUninit<Move>; CAPACITY]>::uninit().assume_init() },
            len: 0,
        }
    }
    #[inline(always)]
    pub fn push(&mut self, m: Move) {
        debug_assert!((self.len as usize) < CAPACITY);
        if (self.len as usize) < CAPACITY {
            self.moves[self.len as usize].write(m);
            self.len += 1;
        }
    }
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len as usize
    }
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    #[inline(always)]
    pub fn get(&self, i: usize) -> Move {
        assert!(i < self.len as usize, "MoveList::get hors bornes");
        // SAFETY: `i < len`, and the first `len` entries were written by `push`.
        unsafe { self.moves[i].assume_init() }
    }
    pub fn as_slice(&self) -> &[Move] {
        // SAFETY: the first `len` entries are initialised, contiguous, and
        // `Move` is `Copy` with no padding, so the slice layout matches.
        unsafe { std::slice::from_raw_parts(self.moves.as_ptr().cast::<Move>(), self.len as usize) }
    }
    pub fn iter(&self) -> impl Iterator<Item = Move> + '_ {
        self.as_slice().iter().copied()
    }
}

impl Default for MoveList {
    fn default() -> Self {
        Self::new()
    }
}
```

Deux points de vigilance, à ne pas improviser :

1. `push` garde un `if` **en plus** du `debug_assert`. Sans lui, pousser plus de 256 coups écrirait hors bornes en release. Le `if` se replie au prédicat déjà évalué par le `debug_assert` en debug, et coûte une comparaison prédite en release.
2. `MAX_MOVES` est public et documenté mais **non utilisé** pour dimensionner le tampon : le passer à 218 économiserait 76 octets de pile et casserait le test `la_capacite_est_de_256`. Laisser 256.

- [ ] **Step 6: Vérifier comportement, suite et gain**

Run:
```bash
cargo test --lib tests_movelist 2>&1 | tail -4
cargo test --release 2>&1 | grep -E "^test result" | grep -oE "[0-9]+ passed" | awk -F' ' '{p+=$1} END {print "passed:", p}'
cargo bench --bench movegen -- --baseline avant 2>&1 | grep -E "movelist_new|generate/all|change:" | head -12
```
Expected: 5 tests verts ; `passed: 55` ; `movelist_new/new` passe de 10–20 ns à **≤ 2 ns** (changement attendu : forte amélioration, hors bruit) ; `generate/all/startpos` gagne **10–20 %**. Si `generate` ne bouge pas alors que `movelist_new` s'effondre, c'est que le compilateur avait déjà supprimé le `memset` : le dire dans le rapport de tâche, ne pas maquiller le chiffre.

- [ ] **Step 7: Vérifier que le perft est identique au nœud près**

Run:
```bash
cargo run --release --quiet -- perft "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1" 5
cargo run --release --quiet -- perft "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1" 4
```
Expected: `4865609` puis `4085603`. Ce sont les constantes publiées ; tout écart signifie que `push`/`get`/`as_slice` a un défaut.

- [ ] **Step 8: Commit**

```bash
git add src/movegen.rs benches/movegen.rs Cargo.toml
git commit -m "perf(movegen): MoveList sans memset, tampon MaybeUninit"
```

---

### Task 3: `has_legal_move` par non-vacuité de bitboards

**Files:**
- Modify: `src/movegen.rs` (nouvelle fonction publique après `count_moves`)
- Create: `tests/movegen_terminaison.rs`
- Modify: `benches/movegen.rs` (groupe `terminaison`)

**Interfaces:**
- Consumes: `Legality` / `build_legality` (privés, dans le même fichier), `ep_legal_ray`, `castling` rights publics de `Position`.
- Produces: `pub fn has_legal_move(pos: &Position) -> bool` — vrai dès qu'au moins un coup légal existe, ne remplit aucune liste.

**Pourquoi une implémentation séparée plutôt qu'un `generate` écourté.** Écourté demande de faire remonter un signal d'arrêt à travers `generate` et ses six sous-générateurs, ce qui change des signatures internes et duplique la logique d'émission. La non-vacuité s'écrit à part, sans émission, et se valide par différentiel contre `generate` — deux chemins distincts, donc un oracle réel au sens de la leçon du dépôt (« un oracle n'est indépendant que s'il dérive sa réponse d'une autre source »).

- [ ] **Step 1: Écrire les tests de terminaison, avec leurs cas limites**

Créer `tests/movegen_terminaison.rs` :

```rust
//! `has_legal_move` contre `generate` : deux chemins, même verdict.
//!
//! L'oracle est `generate`, qui matérialise la liste. Les positions sont
//! dérivées de parcours réels (perft et auto-jeu), pas écrites à la main :
//! les FEN inventées de la phase 1 se sont révélées fausses deux fois.

use mychess::movegen::{MoveList, generate, has_legal_move};
use mychess::position::Position;
use mychess::types::GenKind;

fn fen_legaux(fen: &str) -> bool {
    let pos = Position::from_fen(fen).unwrap();
    let mut l = MoveList::new();
    generate(&pos, &mut l, GenKind::All);
    !l.is_empty()
}

#[test]
fn mat_et_pat() {
    // Mat de l'imbecile : les blancs n'ont aucun coup.
    assert!(!has_legal_move(
        &Position::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3")
            .unwrap()
    ));
    // Pat : roi a8, dame c7, roi blanc c6 — les blancs sont étouffés.
    let pat = "k7/2Q5/2K5/8/8/8/8/8 b - - 0 1";
    assert!(!has_legal_move(&Position::from_fen(pat).unwrap()));
    assert!(!fen_legaux(pat));
}

#[test]
fn double_echec_ne_laisse_que_le_roi() {
    let pos = Position::from_fen("4k3/8/8/8/8/2n5/1q6/K7 w - - 0 1").unwrap();
    let mut l = MoveList::new();
    generate(&pos, &mut l, GenKind::All);
    assert!(l.iter().all(|m| m.from() == 0));
    assert_eq!(has_legal_move(&pos), !l.is_empty());
}

#[test]
fn les_deux_chemins_concordent_sur_tout_l_arbre_de_kiwipete() {
    let fens = [
        ("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1", 3u32),
        ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 4),
        ("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1", 4),
        ("r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10", 3),
    ];
    let mut checked = 0u32;
    for (fen, depth) in fens {
        let mut pos = Position::from_fen(fen).unwrap();
        walk(&mut pos, depth, &mut checked);
    }
    assert!(checked > 10_000, "corpus trop maigre : {checked} positions");
}

/// Parcourt tous les coups légaux jusqu'à `depth` et compare les deux chemins
/// à chaque nœud.
fn walk(pos: &mut Position, depth: u32, checked: &mut u32) {
    let mut l = MoveList::new();
    generate(pos, &mut l, GenKind::All);
    let a = !l.is_empty();
    let b = has_legal_move(pos);
    assert_eq!(
        a,
        b,
        "desaccord has_legal_move={b} generate={a} fen={}",
        pos.to_fen()
    );
    *checked += 1;
    if depth == 0 {
        return;
    }
    for mv in l.iter() {
        let u = pos.make(mv);
        walk(pos, depth - 1, checked);
        pos.unmake(mv, u);
    }
}
```

Vérifier que `Position::to_fen` existe ; sinon remplacer le message d'échec par les compteurs seuls, sans ajouter de méthode à `Position` (gelé).

- [ ] **Step 2: Lancer le test pour le voir échouer**

Run: `cargo test --release --test movegen_terminaison 2>&1 | tail -12`
Expected: échec de compilation `cannot find function has_legal_move in module mychess::movegen`. C'est le rouge attendu.

- [ ] **Step 3: Implémenter**

Ajouter dans `src/movegen.rs`, après `count_moves_impl` :

```rust
/// Vrai dès qu'il existe au moins un coup légal. Ne remplit aucune liste.
///
/// Contract: a **legal** position is required, as for `generate`. On illegal
/// entry (absent king) the answer is `false`, defined and silent.
///
/// Correctness rests on three facts, each of which is the reason a piece
/// class can be answered without enumerating its targets:
///
/// 1. a non-pinned, non-king piece is legal wherever it is pseudo-legal, so
///    `targets & !us & allowed != 0` decides the class;
/// 2. a pinned piece must stay on its pin line, so its targets are
///    intersected with `line_of(from)` — and that intersection is already a
///    subset of `allowed` when in check;
/// 3. castling is legal only with no checker, and its whole answer is a
///    bitboard test on `occ` and `danger`.
pub fn has_legal_move(pos: &Position) -> bool {
    let us = pos.side;
    let them = us.opp();
    let occ = pos.occ();
    let us_bb = pos.color_bb(us);
    let them_bb = pos.color_bb(them);
    let empty = !occ;
    let ctx = build_legality(pos, us);
    let ksq = ctx.ksq;
    if ksq == NO_SQUARE {
        return false;
    }

    // 1. Roi : une seule case d'arrivée valide suffit.
    if KING_ATT[(ksq & 63) as usize] & !us_bb & !ctx.danger & !ctx.them_king != 0 {
        return true;
    }

    let ncheck = ctx.checkers.count_ones();
    if ncheck >= 2 {
        return false; // double échec : seul le roi pouvait sauver, il n'a rien
    }

    let allowed = if ncheck == 1 {
        let c = ctx.checkers.trailing_zeros() as u8;
        (between(ksq, c) | ctx.checkers) & !ctx.them_king
    } else {
        !ctx.them_king
    };
    let pinned = ctx.pinned;
    let pawns = pos.bb_piece[PAWN] & us_bb;

    // 2. Cavaliers, fous, tours, dames : non cloués d'abord.
    let mut sliders = (pos.bb_piece[KNIGHT]
        | pos.bb_piece[BISHOP]
        | pos.bb_piece[ROOK]
        | pos.bb_piece[QUEEN])
        & us_bb
        & !pinned;
    while sliders != 0 {
        let from = sliders.trailing_zeros() as u8;
        sliders &= sliders - 1;
        let bit = 1u64 << from;
        let targets = if bit & pos.bb_piece[KNIGHT] != 0 {
            KNIGHT_ATT[from as usize]
        } else {
            let mut t = 0u64;
            if bit & pos.bb_piece[BISHOP] != 0 {
                t |= bishop_attacks(occ, from);
            }
            if bit & pos.bb_piece[ROOK] != 0 {
                t |= rook_attacks(occ, from);
            }
            if bit & pos.bb_piece[QUEEN] != 0 {
                t |= bishop_attacks(occ, from) | rook_attacks(occ, from);
            }
            t
        };
        if targets & !us_bb & allowed != 0 {
            return true;
        }
    }

    // 3. Pions non cloués : poussées, doubles, captures, promotions, ep.
    if gen_pawns_has_legal(pos, us, pawns & !pinned, allowed, empty) {
        return true;
    }

    // 4. Pièces clouées : cibles restreintes à leur ligne d'épingle.
    for i in 0..ctx.npins as usize {
        let from = ctx.pin_sq[i];
        let bit = 1u64 << from;
        if pawns & bit != 0 {
            // Les pions cloués sont traités par le chemin bulk : leur ligne
            // d'épingle peut être orthogonale ou diagonale, et une poussée
            // d'une case n'est légale que si elle reste sur la ligne. On
            // délègue pour ne pas dupliquer cette géométrie ici.
            if gen_pawns_pinned_has_legal(pos, us, from, allowed & ctx.pin_lines[i], empty) {
                return true;
            }
            continue;
        }
        let targets = if bit & pos.bb_piece[KNIGHT] != 0 {
            KNIGHT_ATT[from as usize]
        } else {
            let mut t = 0u64;
            if bit & pos.bb_piece[BISHOP] != 0 {
                t |= bishop_attacks(occ, from);
            }
            if bit & pos.bb_piece[ROOK] != 0 {
                t |= rook_attacks(occ, from);
            }
            if bit & pos.bb_piece[QUEEN] != 0 {
                t |= bishop_attacks(occ, from) | rook_attacks(occ, from);
            }
            if bit & pos.bb_piece[KING] != 0 {
                0
            }
            t
        };
        if targets & !us_bb & allowed & ctx.pin_lines[i] != 0 {
            return true;
        }
    }

    // 5. Roi cloué n'existe pas ; reste le roque, sans échec seulement.
    if ncheck == 0 && has_castle(pos, us, occ) {
        return true;
    }

    false
}
```

Les trois helpers appelés existent ou sont triviaux à écrire ; les ajouter juste avant `has_legal_move` :

```rust
/// Pions non cloués : vrai si une poussée, une double, une capture, une
/// promotion ou une prise en passant est légale. Aucune émission.
fn gen_pawns_has_legal(pos: &Position, us: Color, pawns: u64, allowed: u64, empty: u64) -> bool {
    if pawns == 0 {
        return false;
    }
    let up: u8 = if us == Color::White { 8 } else { 8 };
    let (push1, push2_rank, cap_a, cap_b) = if us == Color::White {
        (pawns << up, RANK_3, (pawns << 7) & !FILE_H, (pawns << 9) & !FILE_A)
    } else {
        (pawns >> up, RANK_6, (pawns >> 9) & !FILE_H, (pawns >> 7) & !FILE_A)
    };
    let them = pos.color_bb(us.opp());
    let quiet1 = push1 & empty;
    if quiet1 & allowed != 0 {
        return true;
    }
    let quiet2 = (quiet1 & push2_rank) << up;
    let quiet2 = if us == Color::White {
        (quiet1 & push2_rank) << 8
    } else {
        (quiet1 & push2_rank) >> 8
    };
    if quiet2 & empty & allowed != 0 {
        return true;
    }
    if (cap_a | cap_b) & them & allowed != 0 {
        return true;
    }
    // Prise en passant : la géométrie est déjà écrite une fois dans
    // `ep_legal_ray`, on la réutilise au lieu de la réécrire.
    let ep = pos.ep_square();
    if ep != NO_EP {
        if let Some(cap_sq) = pos.ep_capture_square(us) {
            let attackers = PAWN_ATTACKERS[us.opp().idx()][ep as usize] & pawns;
            if attackers != 0 && (1u64 << ep) & allowed != 0 {
                let from = attackers.trailing_zeros() as u8;
                let occ_after = (pos.occ() & !(1u64 << cap_sq)) | (1u64 << ep);
                if ep_legal_ray(pos, us.opp(), from, ep, pos.king_sq(us), occ_after) {
                    return true;
                }
            }
        }
    }
    false
}

/// Pion cloué : une poussée ou une capture qui reste sur la ligne d'épingle.
fn gen_pawns_pinned_has_legal(
    pos: &Position,
    us: Color,
    from: u8,
    line: u64,
    empty: u64,
) -> bool {
    let bit = 1u64 << from;
    let up: i32 = if us == Color::White { 8 } else { -8 };
    let forward = (from as i32 + up) as u8;
    if (1u64 << forward) & empty & line != 0 {
        return true;
    }
    let them = pos.color_bb(us.opp());
    let caps: u64 = if us == Color::White {
        ((bit << 7) & !FILE_H) | ((bit << 9) & !FILE_A)
    } else {
        ((bit >> 9) & !FILE_H) | ((bit >> 7) & !FILE_A)
    };
    if caps & them & line != 0 {
        return true;
    }
    // Prise en passant d'un pion cloué : la ligne doit couvrir la case
    // d'arrivée *et* la capture ne doit pas exposer le roi sur la colonne.
    let ep = pos.ep_square();
    if ep != NO_EP && (1u64 << ep) & line != 0 {
        if let Some(cap_sq) = pos.ep_capture_square(us) {
            if caps & (1u64 << ep) != 0 {
                let occ_after = (pos.occ() & !(1u64 << cap_sq)) | (1u64 << ep);
                if ep_legal_ray(pos, us.opp(), from, ep, pos.king_sq(us), occ_after) {
                    return true;
                }
            }
        }
    }
    false
}

/// Roque : appelé seulement hors échec.
fn has_castle(pos: &Position, us: Color, occ: u64) -> bool {
    let (king_to, rook_from, path_empty, path_safe) = if us == Color::White {
        (CASTLE_WK, 7u8, 0b0110_0000u64, 0b0110_0000u64)
    } else {
        (CASTLE_BK, 63u8, 0b0110_0000_0000_0000_0000_0000_0000_0000u64,
         0b0110_0000_0000_0000_0000_0000_0000_0000u64)
    };
    let _ = (king_to, rook_from, path_empty, path_safe);
    // Le roque n'est presque jamais le seul coup possible ; il est donc
    // testé en dernier et par le chemin générique, qui est déjà écrit et
    // vérifié. Coût nul sur les positions sans droits.
    if pos.castle == 0 {
        return false;
    }
    let mut l = MoveList::new();
    gen_castle(pos, &mut l, us, occ);
    !l.is_empty()
}
```

**Attention, ce bloc n'est pas à recopier tel quel.** Deux endroits sont volontairement laissés au implémenteur, et le brief les signale plutôt que de faire semblant :

1. `gen_pawns_has_legal` contient une ligne morte (`let up: u8 = if us == Color::White { 8 } else { 8 };`) et un `quiet2` calculé deux fois. Écrire la version propre : un seul `up`, un seul `quiet2`. Ne pas laisser de code mort.
2. `has_castle` ci-dessus n'implémente pas le test bitboard annoncé dans la doc de `has_legal_move` — il délègue à `gen_castle` sur une liste jetable. C'est **acceptable et volontaire** (le roque est rare et coûteux seulement quand des droits existent), mais alors la doc de `has_legal_move` doit dire que le roque est le seul cas matérialisé, au lieu de prétendre à un test de bitboards. Corriger la doc, ou implémenter le test direct — au choix, mais les deux doivent concorder. C'est exactement le défaut « doc qui promet ce que le code ne fait pas » relevé sur `gen_castle` en phase 1 : ne pas le reproduire.

Autres points à vérifier au moment d'écrire, car le brief ne peut pas les deviner :
- `pos.ep_square()` et `pos.ep_capture_square(us)` existent (`src/position.rs`), ainsi que `pos.king_sq(us)` et `pos.castle` — mais `king_sq` a un `debug_assert!` sur un roi absent, d'où le `if ksq == NO_SQUARE { return false; }` en tête. Garder cet ordre.
- `PAWN_ATTACKERS[us.opp().idx()][ep]` donne les pions du **camp au trait** qui attaquent `ep`. Vérifier la convention dans `src/tables.rs` avant de s'y fier : en phase 1 c'est `PAWN_ATTACKERS[c][sq]` = cases d'où un pion de couleur `c` attaque `sq`.
- `ep_legal_ray(pos, them, from, ep, ksq, occ)` prend `them` = couleur de l'adversaire du camp au trait (le camp qui a double-poussé).
- `RANK_3` / `RANK_6` sont déjà importés en tête de `src/movegen.rs`.

- [ ] **Step 4: Faire passer les tests et le différentiel**

Run:
```bash
cargo test --release --test movegen_terminaison 2>&1 | tail -6
cargo test --release --test differential 2>&1 | tail -4
cargo test --release 2>&1 | grep -E "^test result" | grep -oE "[0-9]+ passed" | awk -F' ' '{p+=$1} END {print "passed:", p}'
```
Expected: 3 tests verts dans `movegen_terminaison` (dont >10 000 positions comparées), `differential` vert, `passed: 58`.

Si le différentiel signale un désaccord, **ne pas ajuster le test** : le bug est dans `has_legal_move`. Les trois candidats classiques sont la poussée double (le rang intermédiaire doit être vide), la prise en passant (masquage de la pièce capturée sur la colonne) et le pion cloué sur une colonne (poussée légale alors qu'elle reste sur la ligne mais expose le roi — non, justement : sur une colonne, une poussée reste sur la ligne, donc elle est légale).

- [ ] **Step 5: Ajouter le banc de terminaison**

Dans `benches/movegen.rs`, ajouter :

```rust
fn bench_terminaison(c: &mut Criterion) {
    let mut g = c.benchmark_group("terminaison");
    for (name, fen) in FENS {
        let pos = Position::from_fen(fen).unwrap();
        g.bench_function(format!("has_legal_move/{name}"), |b| {
            b.iter(|| std::hint::black_box(has_legal_move(&pos)))
        });
    }
    // Position de mat : le pire cas, aucune sortie anticipée possible.
    let mat = Position::from_fen(
        "rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3",
    )
    .unwrap();
    g.bench_function("has_legal_move/mat", |b| {
        b.iter(|| std::hint::black_box(has_legal_move(&mat)))
    });
    g.finish();
}
```

Ajouter `has_legal_move` à `use mychess::movegen::{...}` et `bench_terminaison` à `criterion_group!`.

Run: `cargo bench --bench movegen -- terminaison --quick 2>&1 | grep -E "time:|terminaison" | head -12`
Expected: `has_legal_move/startpos` nettement **sous** `generate/all/startpos` (les pièces non clouées sortent au premier succès) ; `has_legal_move/mat` du même ordre que `generate`, puisque aucune sortie anticipée n'est possible. C'est le résultat attendu, pas un échec.

- [ ] **Step 6: Commit**

```bash
git add src/movegen.rs tests/movegen_terminaison.rs benches/movegen.rs
git commit -m "feat(movegen): has_legal_move par non-vacuite de bitboards"
```

---

### Task 4: Évasions — n'itérer que les cases de sortie d'échec

**Files:**
- Modify: `src/movegen.rs` (chemin en échec de `generate`)
- Modify: `benches/movegen.rs` (positions en échec)

**Interfaces:**
- Consumes: `build_legality`, `Legality::line_of`, `allowed`.
- Produces: aucune signature nouvelle. `generate` inchangé vu de l'extérieur.

**Règle de décision, à appliquer sans négocier.** Cette tâche est la seule du plan dont le gain n'est pas acquis d'avance. Elle se termine par l'une de deux issues, toutes deux acceptables :

- **garder** si le ratio mesuré sur le groupe `echec` est ≥ 1,15× en faveur du nouveau chemin, sur deux jambes ;
- **revert** si le ratio est < 1,15× ou si la variance entre jambes dépasse 20 %.

Un revert documenté est un succès de la tâche. Ne pas conserver du code qui ne gagne pas.

- [ ] **Step 1: Construire le corpus en échec et le banc**

Il faut des positions **en échec**, et elles doivent être obtenues par jeu réel, pas inventées. Ajouter à `benches/movegen.rs` :

```rust
/// Positions en échec, extraites par auto-jeu déterministe : beaucoup
/// d'échecs, et le camp au trait a peu de coups.
fn positions_en_echec(graines: &[(&str, u32)]) -> Vec<Position> {
    let mut out = Vec::new();
    for (fen, n) in graines {
        let mut pos = Position::from_fen(fen).unwrap();
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..*n {
            let mut l = MoveList::new();
            generate(&pos, &mut l, GenKind::All);
            if l.is_empty() {
                break;
            }
            let mut checkers = 0u32;
            if !pos.. /* voir ci-dessous */ false {
                checkers += 0;
            }
            let _ = checkers;
            let n_moves = l.len() as u64;
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let pick = (state % n_moves) as usize;
            let mv = l.get(pick);
            let u = pos.make(mv);
            if pos.is_in_check() {
                out.push(pos);
            }
            let _ = u;
            pos.unmake(mv, u);
            // On rejoue le coup pour avancer la partie.
            let u2 = pos.make(mv);
            let _ = u2;
            let _ = u2;
        }
    }
    out
}
```

**Ce bloc est un brouillon avec du code mort (`checkers`, `pos..`, doubles `make`).** Il est là pour dire précisément ce que le corpus doit être — positions en échec issues d'un parcours déterministe — pas pour être recopié. Écrire la version propre :

1. avancer la partie d'un coup tiré d'un PRNG à graine fixe ;
2. si le camp au trait est en échec **après** le coup, empiler une copie de la position ;
3. continuer jusqu'à `n` coups ou jusqu'au blocage.

Le test de « en échec » doit venir d'une API existante. Vérifier laquelle est disponible : `pos.is_in_check()` existe peut-être déjà, sinon `movegen::attackers_to(&pos, pos.king_sq(pos.side), pos.side.opp()) != 0` répond à la question sans ajouter de méthode à `Position` (gelé). Utiliser celle qui existe ; ne pas modifier `src/position.rs`.

Puis le banc :

```rust
fn bench_echec(c: &mut Criterion) {
    let positions = positions_en_echec(&[
        ("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1", 60),
        ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 60),
    ]);
    assert!(positions.len() >= 20, "corpus en echec trop maigre: {}", positions.len());
    let mut g = c.benchmark_group("echec");
    g.bench_function("generate", |b| {
        b.iter_batched(
            || positions.clone(),
            |ps| {
                let mut n = 0usize;
                for p in &ps {
                    let mut l = MoveList::new();
                    generate(p, &mut l, GenKind::All);
                    n += l.len();
                }
                std::hint::black_box(n)
            },
            criterion::BatchSize::SmallInput,
        )
    });
    g.finish();
}
```

`iter_batched` avec un `clone()` du `Vec` domine la mesure si le corpus est gros : garder le corpus petit (20–60 positions) et vérifier que le temps mesuré est très supérieur au clone. Si ce n'est pas le cas, mesurer avec un `for` simple dans `b.iter` et un index tournant, sans cloner.

- [ ] **Step 2: Mesurer avant, deux jambes**

Run:
```bash
cargo bench --bench movegen -- echec --save-baseline avant_echec 2>&1 | grep -E "time:|echec" | head -6
cargo bench --bench movegen -- echec --save-baseline avant_echec2 2>&1 | grep -E "time:|echec" | head -6
```
Expected: deux médianes proches à 20 % près. Si elles divergent de plus, la machine n'est pas au repos : relancer.

- [ ] **Step 3: Ajouter le chemin inversé dans `generate`**

Dans `generate`, juste après le calcul de `allowed` et avant le bloc pions, insérer la bifurcation. Le principe : en échec simple, `allowed` contient au plus deux cases (la case du donneur d'échec et, s'il est glissant, la case d'interposition). Plutôt que d'énumérer les cibles de chaque pièce et de les masquer, on énumère les cases de `allowed` et on demande « quelle pièce à moi peut y aller ».

```rust
    // En échec simple, `allowed` fait au plus deux cases : on itère les
    // cases de sortie au lieu des pièces. Le masque `allowed` est déjà
    // appliqué par le chemin générique, mais celui-ci paie un parcours de
    // toutes les pièces même quand une seule peut parer.
    if ncheck == 1 {
        let mut evasions = allowed;
        while evasions != 0 {
            let to = evasions.trailing_zeros() as u8;
            evasions &= evasions - 1;
            let tbit = 1u64 << to;

            // Captures et calmes des pièces non clouées qui atteignent `to`.
            let mut attackers = (KNIGHT_ATT[to as usize] & pos.bb_piece[KNIGHT])
                | (KING_ATT[to as usize] & 0) // le roi est déjà traité
                | (bishop_attacks(occ, to) & (pos.bb_piece[BISHOP] | pos.bb_piece[QUEEN]))
                | (rook_attacks(occ, to) & (pos.bb_piece[ROOK] | pos.bb_piece[QUEEN]));
            attackers &= us_bb;
            while attackers != 0 {
                let from = attackers.trailing_zeros() as u8;
                attackers &= attackers - 1;
                let line = ctx.line_of(from);
                if line != 0 && line & tbit == 0 {
                    continue; // pièce clouée hors de sa ligne
                }
                let cap = (them_bb >> to) & 1 != 0;
                out.push(Move::new(
                    from,
                    to,
                    if cap { FLAG_CAPTURE } else { FLAG_QUIET },
                ));
            }
        }
        // Pions, roque et ep restent au chemin existant : leur géométrie est
        // déjà écrite et masquée par `allowed`.
        ...
        return;
    }
```

**Ce bloc est un squelette, pas du code livrable.** Trois raisons précises :

1. `| (KING_ATT[to as usize] & 0)` est une ligne morte qui ne fait rien : la supprimer.
2. Il n'émet **pas les promotions** (un pion qui va sur la dernière rangée doit produire quatre coups) et il oublie les pions, dont les captures ne passent pas par `bishop_attacks`. Si le chemin inversé est retenu, la partie pions doit être soit réutilisée depuis `gen_pawns_bulk` avec `allowed` restreint à `evasions`, soit écrite ici avec `push_promotions`.
3. L'appel à `ctx.line_of(from)` est linéaire en `npins`, ce qui est acceptable (≤ 8) mais doit être constaté, pas supposé.

Le chemin le plus sûr, et celui à essayer en premier : **ne remplacer que la partie cavaliers/glissantes** par l'itération inverse, et laisser pions, ep et roque au chemin existant, qui est déjà masqué par `allowed`. C'est ce que le squelette ci-dessus suggère, et c'est aussi le plus petit diff possible.

- [ ] **Step 4: Vérifier l'équivalence exacte, puis mesurer**

Run:
```bash
cargo test --release --test differential 2>&1 | tail -4
cargo test --release --test perft_publie 2>&1 | tail -4
cargo test --release 2>&1 | grep -E "^test result" | grep -oE "[0-9]+ passed" | awk -F' ' '{p+=$1} END {print "passed:", p}'
cargo bench --bench movegen -- echec --baseline avant_echec 2>&1 | grep -E "time:|change:|echec" | head -8
```
Expected: `differential` et `perft_publie` verts (l'équivalence coup par coup est non négociable) ; `passed: 58` ; ratio sur `echec` lu dans la ligne `change:`.

- [ ] **Step 5: Appliquer la règle de décision**

- Ratio ≥ 1,15× sur les deux jambes : garder le code, passer au Step 6.
- Ratio < 1,15× ou variance > 20 % : `git checkout -- src/movegen.rs benches/movegen.rs` pour tout rendre, **sauf** le corpus et le banc `echec` s'ils sont utiles à la phase 3. Documenter dans le rapport de tâche : chiffres obtenus, raison du revert.

- [ ] **Step 6: Commit (gardé ou revert documenté)**

```bash
git add -A
git commit -m "perf(movegen): generer les evasions en iterant les cases de sortie"
```
ou, en cas de revert, un commit qui n'ajoute que le banc :
```bash
git add benches/movegen.rs
git commit -m "bench(movegen): corpus en echec, mesure du chemin d evasion"
```

---

### Task 5: Mesure finale contre `motors` et rapport

**Files:**
- Create: `docs/movegen/phase2-2026-10-21.md`
- Modify: `tools/compare_movegen.sh` (nouvelles cibles si nécessaire)

**Interfaces:**
- Consumes: `benches/movegen.rs`, `tools/compare_movegen.sh`, les deux rapports de phase 1 (`docs/movegen/gct-benchmark-2026-10-21.md`).
- Produces: rapport chiffré avec verdict keep/revert par optimisation.

- [ ] **Step 1: Relire la base de phase 1 pour ne pas comparer deux sessions**

Run:
```bash
grep -n "kiwipete d5\|0.564\|Mnps" docs/movegen/gct-benchmark-2026-10-21.md | head -12
```
Expected: retrouver les valeurs de référence de la phase 1 (`movegen` ≈ 302 Mnps en perft kiwipete d5 dans le harnais). Elles ne servent **que** de point de repère ; la comparaison qui compte est celle mesurée dans **cette** session.

- [ ] **Step 2: Mesurer les trois débits, deux jambes chacun**

Run:
```bash
bash tools/compare_movegen.sh 2>&1 | tee /tmp/phase2-leg1.txt | tail -20
bash tools/compare_movegen.sh 2>&1 | tee /tmp/phase2-leg2.txt | tail -20
```
Expected: deux jeux de trois nombres (mychess mono-thread, mychess 8 threads si le harnais le fait, motors mono-thread). Les comptes de nœuds doivent correspondre exactement aux constantes publiées ; un écart signifie un défaut du harnais, pas des moteurs.

- [ ] **Step 3: Écrire le rapport**

Créer `docs/movegen/phase2-2026-10-21.md` avec cette structure :

```markdown
# Phase 2 : optimisation du movegen — mesures du 2026-10-21

## Protocole
[Commandes exactes, deux jambes, machine, charge moyenne constatée]

## Ce qui a changé
| Optimisation | Avant | Après | Ratio | Verdict |
|---|---|---|---|---|

## Coût par appel (criterion)
| bench | jambe 1 | jambe 2 | médiane |
|---|---|---|---|

## Terminaison
[has_legal_move vs generate, y compris le cas mat]

## Perft contre motors
[mono-thread, un facteur par position, avec la réserve SIMD]

## Ce qui n'a pas été fait, et pourquoi
[Phase 3 : arène MCTS, parallélisme, NNUE — cf. analyse §7]

## Limites
[Variance locale, charge machine, comparaison non iso-technique]
```

Chaque chiffre du rapport doit venir d'une commande effectivement exécutée. Aucune extrapolation.

- [ ] **Step 4: Commit**

```bash
git add docs/movegen/phase2-2026-10-21.md tools/compare_movegen.sh
git commit -m "docs(movegen): rapport de mesure de la phase 2"
```

---

## Auto-évaluation du plan

**Couverture.** `memset` → tâche 2. Terminaison bon marché → tâche 3. Évasions → tâche 4. Mesure → tâches 2, 3 et 5. Suppression de `gct/` → tâche 1. API inchangée → contrainte globale, vérifiée par les tests existants non modifiés. `unsafe` confiné → tâche 2 uniquement.

**Placeholders.** Les tâches 1, 2 et 5 sont entièrement spécifiées, code compris. Les tâches 3 et 4 contiennent des blocs explicitement étiquetés comme brouillons, avec la liste de ce qui est faux dedans et la façon de le corriger. C'est un choix assumé : le code de `has_legal_move` et de l'itération inverse dépend de conventions (`PAWN_ATTACKERS`, `ep_legal_ray`, `line_of`) dont une transcription aveugle produirait du code faux, ce qui est précisément le mode d'échec dominant de la phase 1 — huit défauts de plan y ont été trouvés, dont sept étaient des FEN ou des énoncés faux. Un brouillon honnêtement étiqueté vaut mieux qu'un listing qui a l'air fini et ne compile pas.

**Cohérence des types.** `MoveList::{new,push,len,is_empty,get,as_slice,iter}` identiques entre les tâches 2 et 3. `has_legal_move(&Position) -> bool` défini une fois, utilisé en 3 et 5. `MAX_MOVES` déclaré et volontairement non utilisé — signalé dans le texte.
