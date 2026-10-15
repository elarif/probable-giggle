# ARCHIVÉ v1 (2026-10-14) — remplacé par SELFPLAY.md v2 (plan corrigé ;
# diff v1→v2 tracé dans la section « Diff v1 → v2 » du plan canonique).
# Conservé pour traçabilité des concepts référencés par la v2 (S#, gates v1).

# SELFPLAY.md — pipeline d'entraînement self-play (plan)

Planification de la chaîne self-play → labels → refit → ladder, à l'échelle
CPU-only (8 cœurs). Chaque idée est notée **S#** ; le planning en phases
**P#** avec gates mesurables. Les mesures Elo suivent la politique
MIGRATION.md (A/B/A/B même session, ≥3 legs, médiane) et le garde-fou
BENCH-FREEZE `test.sh` (toute mesure post-M4, mention « post-M4 »
obligatoire dans ELO_LOG.md).

## 0. Les trois causes du plafond self-play (cadrage)

| Cause | Symptôme | Remèdes (S#) |
|---|---|---|
| Biais de boucle fermée — le moteur apprend à battre son double | l'Elo self-play monte, l'Elo externe stagne | S11 exploiteurs, S16 ancre externe, S8 re-cherche |
| Capacité figée — la forme d'eval sature | ΔElo/×2 > budget < 10, replafonne après refit | S5 net value+policy, S14 enseignant externe |
| Bruit/insuffisance des labels — résultats de parties bruyants | lenteur de convergence, dérive endgame | S6 mix value/résultat, S8 mining, S9 replay priorisé, S13 tablebase, S10 regret-matching |
| ( aggravateur ) Effondrement de diversité d'ouvertures | corpus redondant, carrefour d'ouvertures | S15 pool EPD + cap randomization + température |

## 1. Inventaire des idées

| # | Idée | Vide comblé | Effet attendu | Coût | Prérequis |
|---|---|---|---|---|---|
| S1 | **Texel tuning** — refit des poids eval linéaire (`eval.rs`) par régression sigmoid(eval)↔résultat sur positions calmes | labels → vérité terminale | +150–300 Elo | ~0.5 j | corpus S16... non: P0 buffer |
| S2 | **Boucle loopy** — self-play → refit → self-play plus fort → refit | consolidation | pente +30–80 Elo/×2 | continu | S1 |
| S3 | **PUCT A2 + priors** (`Child::prior`, hook `prior(pos, mv)`) — pas de l'entraînement mais le levier B_eff dominant | efficacité du playout | +Elo à nœuds fixes | ladder déjà tracé | — |
| S4 | **Distillation policy** — visits MCTS → cible policy → priors appris | réutilise le self-play (2 gradients/partie) | gros (B_eff ↓) | avec S5 | corpus visits loggés |
| S5 | **MLP value+policy** (~10⁵–10⁶ params, CPU) remplaçant l'eval linéaire | capacité | +100–200 puis nouveau plateau | ~2–3 j | corpus + framework minimal (Rust pur) |
| S6 | **Value target mixte** (lc0) — target = 75% résultat de partie + 25% valeur sous-arbre (lissage) | bruit labels | dizaines d'Elo | trivial | S5 (ou refit supervisé) |
| S7 | **Têtes auxiliaires** — WDL trichotomie (nulle≠perte), moves-left | calibration endgame, draw-awareness | dizaines d'Elo | faible | S5 |
| S8 | **Frontier mining** — log à chaque coup Δ=Q(best)−Q(choisi) et entropie des visites ; positions-frontière re-cherchées ×budget → labels propres | allocation du calcul de label ; bruit | réduction ~×10 du coût échantillon (ratio KataGo) | ~1 j | logging Q/n par racine |
| S9 | **Replay priorisé** — fréquence d'échantillonnage ∝ regret (ou TD-error) | même que S8, côté batch | idem S8 | ~0.5 j | buffer indexé |
| S10 | **RM distillation** — accumulateur `FEN → {mv: ΣQ}` ; cible policy = mixture regret-matching `p ∝ Σmax(0, Q̄ᵢ−Q̄ref)` (esprit CFR tabulaire sur estimés de recherche) | bruit + boucle fermée | cumul inter-générations | ~1 j | buffer hash-addressable (zobrist en place) |
| S11 | **Ligue d'exploiteurs** — pool de snapshots + « exploitants » budget ×N dont la mission = battre le net courant ; chaque victoire = échantillon prioritaire | boucle fermée | casse les paliers longs | ~1.5 j | harness parties (fastchess déjà là) |
| S11b | **Hygiène de ligue** (anti-Goodhart) — l'exploiteur sonde, il n'enseigne pas : (i) quotas replay ≤ 10-15 %/exploiteur, égalitaires, jamais ∝ winrate ; (ii) des parties exploit on ne garde que les positions perdues, re-étiquetées par SA recherche ×10 (S8) ou tablebase (S13), tag `src=exploit`, exclus des cibles policy ; (iii) un exploitateur holdout (jamais entraîné dessus) = mesure de généralisation ; (iv) enseignants (S14) sur têtes séparées calibrées, jamais cible policy simultanée ; (v) détecteur de dérive de style (profil openings/longueurs/nulles vs humains) ; (vi) rotation si un exploitateur > 80 % des deltas | façonnage vers SF/lc0/Reckless (dérive de distribution, écho d'archi lc0, conflit de conventions Q) | préserve la validité de la ligue | ~0.5 j | S11 |
| S12 | **Self-play asymétrique** — budget par côté inégal (ex. 10k vs 1k) → diversité de défenses à réaliser | diversité / diversité | moyen | trivial | — |
| S13 | **Ancrage tablebase** — positions 3–4(-5) hommes syzygy injectées avec labels EXACTS dans le replay | sémantique Q, endgame | stabilise les paliers | ~0.5 j | syzygy téléchargé (3-4-5 ≈ 1 Go) |
| S14 | **Enseignant externe** — distiller des scores de recherche profonde (SF) ou des evals de nets publics (lc0 datas dispo, filiation GPL) | capacité | gros mais hors self-play | ~1 j + heures CPU | binaire SF / datas |
| S15 | **Diversité d'ouvertures** — pool EPD élargi (`uho_lichess_4852_v1.epd` en place), playout-cap randomization (jeux échantillonnés à cap aléatoire ≤ budget), température decay | diversité | anti-draw-death | trivial | — |
| S16 | **Ancre Elo externe** — parties bridge lichess (Maia etc.) = mesure hors distribution ; tracer Elo interne vs Elo lichess ; écart > ~150 = biais boucle fermée manifeste | mesure honnête | anti-auto-satisfaction | zéro (bridge existant) | — |

## 2. Format du buffer self-play (un seul format pour toutes les idées)

`selfplay/buffer/gen-YYYYMMDD.jsonl` (gzip/zst optionnel), une ligne par
position *jouée* :

```json
{"fen":"...","z":1,"res":0.5,"budget":10000,"gen":1,
 "moves":[{"u":"e2e4","q":0.13,"n":4120},...],
 "delta":0.02,"entrop":1.84,"regret_tag":0}
```

- `z` résultat de partie {-1,0,1}, `res` score adjudiqué (resign/draw via
  config fastchess — seuils existants).
- `moves` : les coups de racine effectivement visités (q ∈ [-1,1], convention
  `eval_normalized`), `n` visits ; alimente S1, S4, S8, S9, S10.
- `delta` = Q₁−Q_best_choisi ; `entrop` = entropie de la distribution de
  visite ; `regret_tag` = 1 si position re-cherchée à budget étendu (S8).
- Dédup/`FEN→cumul` (S10) via zobrist déjà en place.
- Volume : ~10k nœuds/partie ⇒ ~60 positions/partie × ~1 Mo/1000 lignes ;
  ~10⁴ parties/jour = ~1 Go/j compressé. Prévoir rotation par gen.

## 3. Planning — phases et gates

> Règle de plafonnement (décision partout) : **si ΔElo/×2 < 10–20 sur deux
> doubles consécutifs, changer de levier** (capacité ou mining ou
> enseignant — jamais « plus de parties »).

```
P0 INFRA (0.5–1 j)     buffer self-play + logging Q/n/Δ/entropie à la racine
                       (branche dans le mode self-play de mcts.rs) ;
                       corpus zéro = self-play eval courante, ~10⁴ parties
  GATE SP-0 : buffer valide (round-trip), corpus ≥ 5×10⁴ positions

P1 S1 TEXEL (0.5 j)    refit poids eval linéaire (positions calmes, z)
  GATE SP-1 : SPRT cand>base à nœuds fixes (test.sh) → entrée ELO_LOG

P2 S3 PUCT+PRIORS      (ladder existant — priorités heuristiques)
  GATE : SPRT vs A1.5 — verdict seul juge

P3 S8+S9+S6 LOOPY (1 j setup, puis continu)
                       boucle 24 h : génération → mining frontier (×10 sur
                       les {Δ≈0, entropie haute}) → replay ∝ regret →
                       refit → génération n+1
  GATE SP-3 : ΔElo/×2 ≥ 30 sur 2 itérations, sinon lever = S5

P4 S5+S7+S4 NET        MLP value+policy (CPU, Rust pur), WDL + moves-left,
                       priors = distillation visits → hook prior()
  GATE SP-4 : SPRT net vs P3 ; priors appris ≥ priors SEE/history

P5 S11+S12 LIGUE       exploitants (budget ×10, snapshots), self-play
                       asymétrique, mining des failles en batch
  GATE SP-5 : winrate exploitant < 55 % contre le net courant

P6 S13+S14+S16 ANCRES  tablebase 3-4-5 dans le replay ; distillation
                       enseignant si plafond ; ancre lichess continue
  GATE SP-6 : |Elo lichess − Elo interne| < 150 (sinon biais S11/S8 à revoir)
```

S10, S12, S15 restent des accélérateurs transverses à brancher quand la
phase le demande (S10 dès que P3 tourne ; S15 dès P0 ; S12 en P5).

## 4. Garde-fous / risques

- **CPU partagé** : M4-d9 (8 cœurs) et le bridge lichess cohabitent. Les
  runs self-play/large passent après réconciliation M4 et hors fenêtres de
  défis (`attente_puis_serie.sh` pattern) ; quota de cœurs par `taskset`
  si besoin.
- **Draw-death** : S15 obligatoire (pool EPD + cap randomization) ;
  adjudication resign/draw active pour la qualité de `res`.
- **Sur-estimation self-play** : S16 systématique — toute génération joue
  ses parties d'ancre (Maia/lc0) avant d'être déclarée « gén. n+1 ».
- **Δ sous-estimé** : la re-cherche ×10 n'est pas optionnelle ; elle est le
  correctif de biais du mining (sinon le regret mesuré → 0 par
  construction).
- **Corpus vs ladder** : le SPRT `test.sh` reste le seul verdict Elo
  interne ; aucun changement d'option d'engine entre deux legs (interdit
  multi-améliorations, comme le ladder).

## 5. Immédiat (dès BENCH-FREEZE levé)

1. P0 buffer + logging racine (~0.5 j), corpus gen-0.
2. P1 Texel → première entrée ELO_LOG « post-M4 » de la phase self-play.
3. Brancher S15 (pool UHO complet) sur les runs fastchess + self-play.