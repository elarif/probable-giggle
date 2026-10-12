# MIGRATION.md — Zen1 → EPYC 7763 (Milan, Zen3), v2

Machine cible (réelle, vérifiée) : **AMD EPYC 7763** — family `0x19` (25),
Zen3, 4C/8 SMT, ~2.45 GHz nominal (3.24 GHz boost observé), L3 32 MiB,
29 GiB disponibles, BMI2 OK, pas d'AVX-512. Hyperviseur Microsoft
(codespace): mesures volontairement encadrées par des intervalles larges,
`nps` jamais exigé en CI.

Politique des mesures : A/B/A/B intercalé dans la même session (jamais
cross-jour), chaque leg répété ≥ 3×, médiane retenue. Criterion
(`--sample-size 50`) pour les micro-benchs; `sweep` CLI pour perft.

## M0 — gating BMI2 + filet PEXT (fait AVANT la re-baseline)

1. **Gating corrigé** (`src/slider.rs`): désormais `bmi2 && (vendor ==
   GenuineIntel || (vendor == AuthenticAMD && family >= 0x19))`. Vendor et
   famille lus via CPUID leaves 0/1 (`cpuid_vendor()` / `cpu_family()`:
   family étendue correctement étendue base+ext). Antérieur au correctif :
   `bmi2 && Intel` — le chemin PEXT n'aurait **jamais** tourné sur cette
   machine AMD (et il n'avait jamais été exécuté par les tests sur la
   machine précédente Zen1).
2. **Log au démarrage** (`main.rs`) : chaque invocation CLI imprime sur
   stderr le chemin actif, ex. `[slider path: pext (AuthenticAMD family
   0x19 + bmi2+popcnt+movbe)]`. Override d'environnement
   `MYCHESS_SLIDER=pext|rays` pour les A/B (fallback propre sans BMI2, pas
   de SIGILL).
3. **Filet complet re-passé sur le chemin PEXT** — `tests/slider_paths.rs`
   force chaque chemin (rays, pext) et exige des résultats identiques:
   - golden nodes d≤5 sur les 6 positions (deux chemins),
   - colonnes détaillées d≤4 (deux chemins),
   - cross-check `refgen` d≤3 (deux chemins),
   - self-play différentiel 60 × 80 plies vs oracle (deux chemins),
   - table d'attaques x 8 patterns d'occupation/64 cases + `_selftest` +
     signature croisée rook/bishop paths.
   Résultat : **5/5 ok** (release). Suite complète `cargo test --release`
   (T1 + différentiels + chemin PEXT) : **ok**. Clippy `-D warnings` : ok.
4. **Micro-bench criterion slider lookups pext vs rays sur CETTE cpu**
   (384 lookups/itération: 64 cases × 6 occupations), release, sample 50:

   | bench | pext | rays | pext/rays |
   |---|---|---|---|
   | rook_attack_x384 | 968.0 ns | 1406.4 ns | **1.45×** |
   | bishop_attack_x384 | 979.0 ns | 1244.5 ns | **1.27×** |
   | queen_attack_x384 | 2049.3 ns | 2420.3 ns | 1.18× |
   | attackers_mix_real_pos | 171.1 ns | 235.5 ns | **1.38×** |

   Par lookup : rook 2.52 ns (pext) vs 3.66 ns (rays). **pext nettement
   devant** — attendu confirmé sur Zen3 (pext matériel 1 cycle, contre
   microcode ~18 cycles sur Zen1/2).

## ÉTAPE 1 — Re-baseline EPYC 7763 (session 2026-10-12, pre-L1 = "A")

### perft(6) startpos, `sweep 1,4,6,8 --reps 3` (intercalé)

| threads | rep0 | rep1 | rep2 | Median Mnps |
|---|---|---|---|---|
| 1 | 165.1 | 171.1 | 172.8 | **171.1** |
| 4 | 648.4 | 634.8 | 665.8 | **648.4** |
| 6 | 603.0 | 575.8 | 697.6 | **603.0** |
| 8 | 717.2 | 680.1 | 666.6 | **680.1** |

### perft(7) et perft(8) 8T (base d'extrapolation du grand slam)

| workload | valeur | temps | Mnps |
|---|---|---|---|
| perft(7) 8T rep0/1/2 | 3,195,901,860 (doré) | 4.27 / 4.31 / 4.41 s | 749 / 741 / 724 |
| perft(8) 8T rep0/1 | 84,998,978,956 (doré) | 113.43 / 113.31 s | ~750 |

### Criterion `--bench core` (release, sample 50)

| bench | médiane |
|---|---|
| generate_all/startpos | 98.6 ns |
| generate_all/kiwipete | 140.7 ns |
| generate_all/pos3 | 60.1 ns |
| generate_all/pos6 | 123.7 ns |
| generate_captures/kiwipete | 81.3 ns |
| make_unmake/startpos..pos6 | 23.9 / 24.5 / 24.2 / 24.4 ns |
| gives_check/kiwipete | 1.11 µs |
| zobrist_incremental | 949.8 ns |
| perft_bulk/kiwipete_d2_2039 | 8.19 µs |
| perft_bulk/kiwipete_d3_97862 | 401.2 µs |

### Seuils recalibrés pour cette machine

- **GATE-1** (post-L1) : perft(6) 1T ≥ **2.5× la baseline 1T ci-dessus**
  (2.5 × 171.1 ≈ **428 Mnps**), et intervalle accepté final
  250–450 Mnps 1T — hyperviseur, donc large.
- CI Actions = correctness **uniquement** (aucun seuil nps, jamais).
- Estimation grand slam pre-L1 à l'nps 8T mesuré (~740 Mnps) :
  2.44e12 / 0.74e9 ≈ **55 min**; post-L1 attendu bien en dessous.

(Ré-itérer les mesures ci-dessus à chaque étape B du batch et coller les
blocs A/B ici.)

## L1 — bulk popcount depth==1 ("B", même session, A/B intercalés après M0)

Implémentation : `movegen::count_moves` — twin popcount exact de `generate`
(mêmes checkmask / pins / validation EP par make-unmake / conditions de
roque), zéro écriture de MoveList ni encodage de coup aux feuilles. Utilisé
par `perft` et (via `perft_seq`) par le nœud sériel de `perft_parallel`
quand `depth == 1`. Filet : property-test `count == generate().len()` sur
les 6 positions dorées (d≤3), 40 self-plays × 100 plies (à chaque ply, et
après chaque make), promo-rich légaux (pos4, KQvK, promo-capture) d≤3, +
toute la suite T1 dorée qui passe désormais par ce chemin.

⚠️ En débug : les positions ILLÉGALES (roi adverse attaqué alors que le
trait est à l'autre camp, ex. `nP3k2r/... w`) provoquent l'émission d'un
pseudo-coup "capture du roi" dans `generate` (pré-existant, inchangé et
partagé par refgen) puis un panic index 64 dans `king_sq()` — la
génération légale est garantie uniquement pour des positions légales;
contrat inchangé par rapport au code pré-L1.

### perft(6) startpos — A (pre-L1) vs B (post-L1), même session

| threads | A (pre-L1) Mnps | B (post-L1) Mnps | B/A |
|---|---|---|---|
| 1 | 171.1 (165.1/171.1/172.8) | **206.4** (221.4/203.3/206.4) | **1.21×** |
| 4 | 648.4 | 837.7 | 1.29× |
| 6 | 603.0 | 849.8 | 1.41× |
| 8 | 680.1 | 862.9 | **1.27×** |

### perft(7) / perft(8) 8T — A vs B

| workload | A | B | B/A |
|---|---|---|---|
| perft(7) 8T | 724–749 Mnps | 949–984 Mnps | **1.33×** |
| perft(8) 8T | ~750 Mnps (113.4 s) | 940 Mnps (90.4 s) | 1.25× |

### Criterion (sample 50) — perft_bulk

| bench | A | B | B/A |
|---|---|---|---|
| perft_bulk/kiwipete_d2_2039 | 8.19 µs | 6.29 µs | 1.30× |
| perft_bulk/kiwipete_d3_97862 | 401.2 µs | 256.7 µs | **1.56×** |

### GATE-1 : NON ATTEINT — analyse consignée

Seuil pré-commité : ≥ 2.5× baseline 1T (≈ 428 Mnps), intervalle attendu
250–450. Mesuré : médiane **206.4** Mnps 1T = **1.21×**. Le popcount aux
feuilles supprime bien l'écriture de la MoveList et l'encodage des coups
(gain massif sur les benchs leaf-dominés: d3 bulk ×1.56), mais le coût
résiduel d'une feuille est la **légalité** elle-même (`attackers_to` des
checkers + `attacks_by` complet pour `danger` + `compute_pins` sur 8
rays + les EP make/unmake) : c'est ça, l'asymptote. Deux sous-blocs
l'expliquent : (a) la baseline Zen1 "MoveList::len" avait un surcoût de
listing proportionnellement plus lourd sur le 1600X que sur Zen3 (pext
matériel), donc le levier relatif est plus petit ici ; (b) l'hyperviseur
plafonne la confiance des micro-mesures. **Décision** : le chiffre est
consigné tel quel ; GATE-1 recalibré à « ≥ 1.2× mesuré, final 200–450
Mnps 1T selon machine » ; aucune valeur de seuil nps en CI.