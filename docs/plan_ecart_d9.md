# Plan de localisation de l'écart d9 — `discovery_checks` / `double_checks`

> **Statut** : plan d'exécution, écrit le 2026-10-21.
> **Verrou** : gate M4-d9 (BENCH-FREEZE) — aucune mesure Elo tant que l'écart
> n'est pas tranché (`slaml/d9_checkcols_report_20261014.md`, §Conséquence).
> **Porte** : la réconciliation est à 7/9 ; les deux colonnes divergentes
> alimentent les priors A2, donc elles doivent être justifiées avant tout
> refit.

---

## 1. Ce qui est établi (ne pas refaire)

| fait | preuve |
|---|---|
| L'écart est **reproductible** | deux runs d9 complets, sur deux hôtes différents, deux chemins slider (pext sur EPYC, magic sur Ryzen), avec et sans checkpoint → **mêmes chiffres au nœud près** |
| L'écart est **strictement localisé** | `Nodes`, `Captures`, `E.p.`, `Castles`, `Promotions`, `Checks`, `Mates` sont **exacts** ; la somme des `Checks` est identique des deux côtés (36 095 901 903) |
| L'écart est **nul jusqu'à d8** | d7 : 0/0 · d8 : `double` **exact** (147 215), `discovery` à **4** près · d9 : +2 037 / +768 715 |
| L'écart est donc **un seuil de profondeur**, pas une dérive | il n'apparaît qu'à d9 |

| colonne | mychess | CPW | écart |
|---|---|---|---|
| `discovery_checks` | 37 103 750 | 37 101 713 | **+2 037** |
| `double_checks` | 6 315 946 | 5 547 231 | **+768 715** |

**Conséquence de la ligne `Checks` exacte** : l'ensemble « feuilles avec ≥1
échec » est *le même* des deux côtés. Ce qui diverge est la **classification**
de cet ensemble en `direct` / `discovery` / `double`. C'est une question de
**convention**, pas de movegen.

---

## 2. L'hypothèse à tester, et pourquoi elle est forte

**H1 — le motif en cause est la promotion.**

La promotion la plus rapide depuis la startpos est à **9 plies**
(1.e4 d5 2.exd5 3.d6 4.d7 5.d8=Q). Donc :

- d7 (7 plis) : `promotions = 0`
- d8 (8 plis) : `promotions = 0`
- **d9 (9 plis) : `promotions = 17 334 376` — la première profondeur qui
  contient des promotions**

C'est exactement la frontière observée : l'écart est nul à d8 et saute à d9.

Le mécanisme plausible : une promotion peut **révéler** un échec (la pièce
qui partait bloquait une ligne) **et** le promoteur attaque le roi →
`att.count_ones() >= 2` → `double_checks` chez mychess. La convention CPW
compterait autrement.

Rapport de grandeur : 768 715 / 17 334 376 = **4,43 %** des promotions.

**H2 (secondaire) — l'écart `discovery` de 2 037 a une autre origine.** Il
existe déjà 4 fois à d8 : ce n'est pas le même motif que le `double`. Deux
hypothèses : (a) la même famille de motif (promotion) mais classé `discovery`
plutôt que `double` dans certains cas ; (b) un motif distinct, lui aussi
nouveau à d9.

**H3 (à ne pas oublier) — une convention CPW qui exclut le promoteur.**
CPW compte peut-être les attaquants *sans* le pion promu, ou compte un
échec « double » seulement si les deux attaquants sont de types différents.
C'est la version « convention » du verdict, à distinguer d'un vrai bug.

---

## 3. Plan d'exécution

Chaque étape a un **livrable vérifiable** et un **critère d'arrêt**. Les
étapes 1 à 4 sont indépendantes de la machine longue (elles tournent en
secondes) ; seule l'étape 5 est longue.

### Étape 1 — Décomposition d9 par motif (EN COURS, ~3,5 h)

**Outil** : `examples/d9_motif_shard.rs` (shardé, checkpointe par lots).
**Commande** :

```sh
./target/release/examples/d9_motif_shard \
  "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1" 9 12 2 \
  /tmp/motif_d9.ckpt
```

**Sortie** : `checks`, `direct`, `discovery`, `double`, `discovery_ep`,
`discovery_castle`, `double_ep`, `double_castle`, `mates`, `ep`.

**Critère d'arrêt** : si `double_ep + discovery_ep ≈ 768 715 + 2 037`, la
répartition est dans le motif ep et H1 tombe. Si `double_ep` vaut ~0 et
`double` porte tout l'écart, le motif ep est innocent et il faut passer à
l'étape 2 (promotion).

> Note : ce runner ne décompose pas encore *par promotion*. C'est l'objet
> de l'étape 2.

### Étape 2 — Décomposition d9 par promotion (APRES l'étape 1)

**Modification** : ajouter au runner deux compteurs — les checks dont le
coup joué est une promotion, ventilés par classe (`promo_double`,
`promo_discovery`). Un coup est une promotion si `mv.is_promotion()`.

**Critère d'arrêt** : si `promo_double ≈ 768 715`, **H1 est confirmée** et la
cause est localisée : un double-échec par promotion.

### Étape 3 — Extraction d'un contre-exemple minimal (le cœur du diagnostic)

Une fois le motif identifié (disons la promotion), il faut **un seul
exemple** de la position fautive, pas un décompte.

**Méthode** :
1. Rejouer les 400 sous-arbres racine de d9 (le split du runner) et, pour
   chacun, descendre jusqu'à la **première** feuille qui produit le motif
   litigieux.
2. Enregistrer la FEN de cette feuille, le coup joué, et les deux
   attaquants.
3. Vérifier cette position à la main et avec `python-chess`.

**Outil** : un second runner, ou un mode `--first-case` du runner d9 par
motif : il s'arrête à la première occurrence et affiche la FEN.

**Livrable** : une FEN + un coup, reproduisant le désaccord de convention.

### Étape 4 — Arbitrage de la convention (python-chess comme troisième partie)

**Problème** : `python-chess` n'expose pas les compteurs `discovery` /
`double`. Il faut donc écrire l'**oracle** de la convention CPW.

`examples/cpw_convention.py` existe déjà (il reproduit la règle de mychess et
sépare ep / roque). Il faut y ajouter les variantes de convention
candidates, et les exécuter sur le contre-exemple de l'étape 3 :

| variante | règle | mychess | CPW |
|---|---|---|---|
| V0 (actuelle) | `n_attackants >= 2` → double | — | ? |
| V1 | le promoteur ne compte pas comme attaquant | ? | ? |
| V2 | double = 2 attaquants de **types différents** | ? | ? |
| V3 | un échec « double » exige deux attaquants **non-promotion** | ? | ? |

**Critère d'arrêt** : une variante qui, appliquée au contre-exemple, donne
le verdict CPW **et** qui, appliquée à l'ensemble d9, reproduit exactement
5 547 231 / 37 101 713. Une variante qui ne reproduit pas les deux colonnes
n'est pas la bonne.

> Pour cela, il faut pouvoir **re-compter d9 avec une variante**. Le
> runner de l'étape 2 est le véhicule : on lui ajoute un sélecteur de
> convention.

### Étape 5 — Recompte d9 sous la convention candidate (long, ~4 h)

Rejouer d9 avec la convention V* retenue, et vérifier que les 9 colonnes
sortent **exactement** comme CPW. C'est la preuve que la convention est la
bonne, pas une coïncidence sur un échantillon.

### Étape 6 — Verdict et levée du gel

Trois issues possibles, et il faut les distinguer :

| verdict | condition | action |
|---|---|---|
| **A — convention CPW** | une variante reproduit exactement les 9 colonnes | documenter la convention dans `MIGRATION.md`, l'exempter explicitement dans `reconcile.sh` (avec le motif exact), **lever le BENCH-FREEZE** |
| **B — bug mychess** | aucune convention ne reproduit les 9 colonnes, et un contre-exemple montre un classement *objectivement* faux | corriger `src/perft.rs`, re-run d9, re-reconcilier à 9/9 |
| **C — indécidable** | le contre-exemple montre que les deux conventions sont défendables et qu'aucune ne reproduit exactement | consigner l'indécidabilité, ne pas lever le gel, trancher avec l'auteur de la table CPW |

Dans les cas A et B, le **rapport** `slaml/d9_checkcols_report_20261014.md` est
mis à jour (il annonce un plan en 4 étapes : micro-FEN ep, bisect d9, FEN
dédiés d5–d6, verdict) — les étapes 1–3 ci-dessus en sont l'exécution.

---

## 4. Contraintes

- **Aucun changement de comportement du core** pendant la localisation :
  `src/perft.rs` est un **lecteur** de compteurs, il n'affecte ni le movegen
  ni les résultats de perft en nombre de nœuds. Toute modification de la
  règle de classification est un changement de *convention*, à traiter
  dans le cadre du verdict (étape 6), pas en douce.
- **Le perft en nœuds ne doit pas bouger** : `cargo test --release` doit
  rester à 39/39, et les 6 goldens exacts.
- **Budget CPU** : l'étape 1 occupe 12 cœurs pendant ~3,5 h. Ne pas lancer
  d'autre charge lourde en parallèle (le run `t2_parallel_deep` de la
  session parallèle a déjà écrasé le premier d9).
- **Traçabilité** : chaque étape produit un log versionné dans `slaml/`, et
  chaque conclusion est écrite dans `MIGRATION.md` (pas seulement dans un
  rapport de diagnostic).

---

## 5. Chemins de sortie si l'hypothèse H1 est fausse

Si l'étape 1 montre que `double_ep ≈ 0` **et** que l'écart n'est pas dans
les promotions (étape 2), les candidats restants sont, par ordre de
décroissance du rapport de grandeur :

1. **double-échec par roque** (`double_castle`) — le roi dégage, la tour
   arrive : deux attaquants. Le rapport le liste comme candidat (b).
2. **double-échec par capture** — la pièce capturée bloquait une ligne, et
   l'attaquant capturé en est un autre.
3. **une convention sur le *nombre* d'attaquants** (V2 ci-dessus), sans
   motif particulier — mais alors l'écart serait aussi présent à d7/d8, ce
   qui est exclu.

Le critère discriminant est toujours le même : **le motif doit être absent
à d8 et présent à d9**. Toute hypothèse qui l'explique est un motif
nécessitant 9 plies — la promotion en est l'exemple canonique, le roque en
est un autre (dégager le roi, joiner la tour et roquer : ≥ 9 plies depuis la
startpos).
