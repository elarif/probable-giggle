# SELFPLAY v2 — plan corrigé

> **Plan canonique** depuis le 2026-10-14 (intégré depuis `selfplay_critique.md`,
> remplace `selfplay_v1.md` archivé).

**Principe de la révision** : trois changements structurels (arbitrage stratégique explicite avant tout ; chiffrage CPU/NPS en phase P-1 ; provenance+confiance des labels comme concept de première classe) et une série de corrections ponctuelles (exploration racine, IS, gates en pente, ML ops). Ce qui était bon est conservé tel quel : le cadrage §0, S11b, la discipline SPRT, la règle de plafonnement — mais redéfinie sur une unité stable.

## Diff v1 → v2 (traçabilité)

| Correction | Motif | Section |
|---|---|---|
| P-1 chiffrage NPS/enveloppe CPU avant tout | le budget nœuds/coup devient une *conséquence* du chiffrage, pas une donnée | §1 |
| Décision stratégique self-play vs distillation arbitrée à SP-4, canary S14 en P3 | le levier « capacité » admis comme dominant ne peut pas attendre P6 | §0, §6 |
| Schéma buffer : `played`, `src`, `conf`, `targets{}` | delta incalculable sous température ; tag `src` exigé par S11b ; extensibilité WDL | §4 |
| Nouveau S17 — exploration racine (Dirichlet α=0.3, ε=0.25, τ chiffré) | sinon collapse circulaire visits↔priors garanti en S4 | §3 |
| S9 + correction importance-sampling | sinon le batch dérive vers les positions aberrantes | §3 |
| S10 déclassé hors chemin critique | effet marginal aux racines, distribution shift sans decay | §3 |
| S16 v2 : ladder local de moteurs étalonnés, métrique en **pente** | Maia/lichess = ancre de style bruitée ; l'offset absolu ne mesure rien | §3, §6 |
| GEN-CONFIG versionné par génération | reproductibilité + comparabilité des gates | §5 |
| Règle de plafond redéfinie : ΔElo / doublement de **positions uniques** à config fixe | « ×2 » était ambigu et devenait non comparable linéaire→MLP | §6 |
| Gate diversité chiffré (nulles, unicité, entropie) | seul axe majeur sans métrique en v1 | §6 |
| Texel : holdout externe + biais gen-0 déclaré | le corpus gen-0 est auto-distribué ; le biais commence à P1 | §6 |
| Adjudication dé-circularisée (conf 0.4 + re-vérification) | res auto-référencé injecte l'eval dans le label | §4 |
| S12 : labels côté fort uniquement | les z asymétriques sont biaisés par le budget | §3 |
| S8 : escalade ×10→×100 si incohérence re-check | les positions à haute entropie sont celles où ×10 ne suffit pas | §3 |
| S13 monté en P3 en post-traitement offline | labels endgame exacts tôt, zéro probing en ligne | §6 |
| Pré-gate ML (gradient check, overfit-1-batch, split par partie, EMA, sizing) | risque bugs silencieux + overfit en Rust pur | §6 |

---

## 0. Décision stratégique préalable (écrite, signée, datée)

Avant P-1, répondre par écrit dans le repo à : **« le self-play est-il une contrainte du projet ou un moyen ? »**

- Si *contrainte* (indépendance de l'engine, originalité, licence) : l'arbitrage coût/Elo est déjà fait, S14 reste une ancillaire sur têtes séparées (S11b-iv), et le plafond self-play est accepté comme donnée du problème.
- Si *moyen* : la boucle est en compétition continue avec la distillation, et le **canary S14** (P3, §6) fournit le ratio ΔElo / heure-CPU des deux pistes. À SP-4, décision documentée : poursuite boucle, pivot distillation, ou hybride (têtes séparées).

Sans cette décision, P6 « si plafond » consomme tout le pipeline avant d'utiliser le levier reconnu comme le plus gros. La v1 éludait ; la v2 l'impose.

## 1. P-1 — chiffrage du budget réel (0.25 j, bloquant)

1. **NPS baseline** : benchmark 100 positions fixes × 3 graines → nœuds/s/cœur (eval linéaire + MCTS). Noté dans ELO_LOG.
2. **Prototype MLP** (2 h, *sans training*) : forward pass du net S5 candidat sur 10⁴ positions → NPS estimé post-P4. C'est ce chiffre qui fixe le budget nœuds/coup post-P4 (probablement 2–4k au lieu de 10k — documenter, ne pas nier).
3. **Enveloppe CPU** : 8 cœurs ≈ 690k s CPU/j ; mesurer la consommation réelle M4-d9 + bridge sur une semaine → **budget self-play disponible** (chiffre écrit, pas implicite).
4. **Allocation de boucle** (point de départ, ajustée à SP-3) : génération 70 % / mining+re-cherche 12 % / ancres 8 % / training 10 %.
5. **Unité de plafond v2** : *ΔElo par doublement du corpus cumulé de positions uniques (zobrist) à GEN-CONFIG fixe*. Seuil de changement de levier : < 10–20 Elo sur deux doubles. Tout changement de GEN-CONFIG re-baseline.

> Conséquence honnête à écrire dans ELO_LOG : les gens MLP auront des labels racine *plus bruités* que les gens linéaires (budget/coup réduit pour tenir l'enveloppe). La compensation prévue : S8 escalade, S6/S7, TB. Si la compensation échoue → levier capacité ≠ levier bruit, diagnostic SP-3.

## 2. Cadrage (repris de v1, un ajout)

Les trois causes (boucle fermée / capacité / bruit de labels) et l'aggravateur diversité sont conservés. **Ajout v2 : le biais de boucle fermée est actif dès gen-0** — le corpus fondateur est distribué par l'eval qu'il va retuner. Toute mesure P1–P3 doit être relue avec cette étiquette.

Chaque cause reçoit désormais sa **mesure de succès** (pas seulement ses remèdes) :

| Cause | Mesure de succès |
|---|---|
| Boucle fermée | pente Elo_ext(gen)/Elo_int(gen) ≥ 0.6 sur ≥ 4 gens (S16 v2) |
| Capacité | ΔElo/double-corpus ≥ 20 à config fixe |
| Bruit de labels | écart val-loss entre labels bruts et labels re-cherchés (holdout S8) — décroissant across gens |
| Diversité | gate DIV (§6 P0) |

## 3. Inventaire révisé

Conservés inchangés : S1, S2, S3, S4, S5, S11, **S11b (intégralement)**, S13 (déplacé), S15 (+ métriques).

Marqués avec delta :

| # | Correction v2 |
|---|---|
| S6 | λ du mix z/valeur **calibré chaque gen sur val set** (grille 0 / 0.15 / 0.25 / 0.4), pas fixé à 0.25 |
| S8 | re-cherche ×10, **escalade ×100 si \|Q_recheck − Q_orig\| > 0.1** (plafond : 3 % des positions/mining round) ; le relabel écrase avec conf 0.9 |
| S9 | p_i ∝ (regret_i + 0.01)^0.6 ; **poids IS** w_i ∝ (N·p_i)^−β, β : 0.4→1 en fin de gen, clip 3× ; priorité des positions *non* re-cherchées cappée (regret biaisé → 0) |
| S10 | **déclassé** hors chemin critique (option post-P5). Conditions de réactivation : decay 0.98/gen sur ΣQ, Q̄ref = moyenne value des coups frères, application limitée aux positions vues ≥ 50× (openings/transpositions — stats buffer à l'appui) |
| S12 | tag `src=asym` ; **labels pris côté fort uniquement** ; côté faible : diversité d'ouvertures seulement, exclu du replay value/policy |
| S14 | **canary dès P3** (5–10 % CPU) : SF profondeur fixe sur 50k positions stratifiées du corpus → mini-distill → ΔElo/h-CPU vs boucle, input de la décision SP-4. Question licence tranchée **par écrit** avant (outputs de recherche vs poids) |
| S16 | **v2 = ladder local d'ancrage** : 3–4 adversaires fixes (snapshot gen-0 gelé, 1–2 moteurs externes à Elo CCRL connu, SF à skill-level fixe), TC/nœuds fixes, ≥ 200 parties/mesure, openings UHO. Maia/lichess = **ancre secondaire de style**, jamais de gate |

**Nouveaux :**

| # | Idée | Détail |
|---|---|---|
| S17 | **Exploration racine** (bloquant pour S4) | Dirichlet(α=0.3), ε=0.25, racine seule, actif pendant toute la génération d'entraînement. Température : τ=1 coups ≤ 20, décroissance linéaire vers τ=0.05 au coup 40. Visits stockées = distribution brute (→ cible S4 à τ=1, propre) |
| S18 | **ML ops minimum** (pré-gate P4) | split train/val **par partie** ; gradient check numérique (10 paramètres) ; overfit-1-batch (loss → ~0) ; EMA poids (decay 0.999) = poids du générateur n+1 ; fenêtre replay 5 gens ; sizing : params ≤ positions_uniques_entraînables / 20 en première approx, grossissement justifié par courbe val uniquement ; cache d'éval par nœud MCTS |

## 4. Buffer v2 — schéma et politique de confiance des labels

```json
{"fen":"...","gen":3,"played":"d7d5","z":1,"res":0.5,
 "src":"self","conf":0.6,"budget":10000,"gencfg":"sha8...",
 "moves":[{"u":"e2e4","q":0.13,"n":4120}],
 "delta":0.02,"entrop":1.84,
 "targets":{},
 "relabel":{"by":"search10x","q":0.05,"budget":100000}}
```

Changements : `played` (obligatoire — sinon `delta` incalculable sous τ), `src`, `conf`, `targets{}` extensible (WDL, moves-left à P4 sans migration de format), `gencfg` (hash de la GEN-CONFIG), `relabel` (écrasement tracé).

**Hiérarchie de confiance par provenance** (à calibrer sur le holdout S8) :

| src | conf | usage policy |
|---|---|---|
| `tb` (syzygy exact) | 1.00 | oui (coups TB) |
| `search100x` | 0.95 | oui |
| `search10x` | 0.90 | oui |
| `teacher` (calibré) | 0.70 | non — têtes séparées (S11b-iv) |
| `self` (partie à terminaison) | 0.60 | oui |
| `asym` côté fort | 0.50 | oui |
| `self` **adjudiqué** | 0.40 | oui, recalibré par WDL en P4 |
| `asym` côté faible | — | exclu du replay |
| `exploit` | hérite du relabel | **non** (S11b-ii), value seule |

**Poids effectif dans la loss** = conf_src × w_IS(S9) × 0.9^(gen_actuelle − gen).

**Adjudication dé-circularisée** : draw-adjudication → res=0.5, conf 0.4 ; win/loss-adjudication → z, conf 0.4, et la position d'adjudication entre en file de re-vérification ×10 (priorité basse). En P4, seuils de resign recalés sur WDL (P(loss) ≥ 0.9). Fin de la circularité eval→res→eval.

## 5. GEN-CONFIG — configuration de génération versionnée

Un fichier hashé par gen, référencé dans chaque ligne du buffer et dans ELO_LOG :

```
cpuct, nœuds/coup, τ-schedule, dirichlet (α, ε), loi du cap-randomization,
seuils d'adjudication, hash du pool EPD, hash net/poids
```

- Le **cap-randomization** (S15) est chiffré : cap ~ log-uniforme [500, budget].
- Distinction explicite **mode génération** (exploration : S17 actif) vs **mode mesure** (force max : τ=0, pas de Dirichlet, SPRT `test.sh`). v1 mélangeait les deux.
- Toute mesure Elo interne reste sous la discipline v1 : SPRT seul juge, jamais de multi-améliorations entre legs, mention « post-M4 ».

## 6. Phases et gates v2

```
P-1 CHIFFRAGE (0.25 j)   §1 — NPS linéaire mesuré, NPS MLP estimé,
                         enveloppe CPU écrite, unité de plafond figée
  GATE SP-(-1) : enveloppe écrite ; budget nœuds/coup gen-0 dérivé du chiffre

P0 INFRA (1 j)           buffer v2 + logging racine (Q/n/Δ/entropie/played) ;
                         S17 exploration ; gate DIV : positions uniques/loggées
                         ≥ 85 %, baseline taux de nulles + entropie coups 1–12
                         enregistrées ; ladder d'ancrage S16 v2 installé
  GATE SP-0 : round-trip buffer OK ; corpus gen-0 ≥ 5×10⁴ pos ;
              première mesure d'ancre dans ELO_LOG ; métriques DIV au vert

P1 TEXEL (0.75 j)        corpus = gen-0 + holdout externe (suites de test +
                         parties d'ancre) ; positions calmes (post-quiescence,
                         sans échec) ; split par partie
  GATE SP-1 : SPRT multi-TC (2 cadences) cand>base ;
              val holdout externe > val corpus gen-0 (sinon biais déclaré) ;
              ELO_LOG : "Texel v1 sur corpus auto-distribué, refit prévu gen-3"

P2 PUCT+PRIORS           inchangé. GATE : SPRT vs A1.5.

P3 LOOPY (1.5 j setup)   boucle 24 h séquencée selon allocation §1 ;
                         S8 v2 (escalade ×100) ; S9-IS ; S13 offline
                         (post-traitement ≤5 hommes → labels exacts, jamais
                         en ligne) ; S6 λ calibré ; canary S14 (5–10 % CPU)
  GATE SP-3 : ΔElo/double-corpus ≥ 20 sur 2 doubles, à config fixe ;
              DIV au vert ; ancre : pente ext/int ≥ 0.6 (≥ 4 points) ;
              canary S14 chiffré → input SP-4

P4 NET (3–5 j)           S5+S7+S4. PRÉ-GATE SP-4a (S18, bloquant) : gradient
                         check, overfit-1-batch, courbe val décroissante,
                         split par partie, EMA branché
  GATE SP-4 : SPRT net vs P3 ; priors appris ≥ priors SEE/history (SPRT) ;
              DÉCISION STRATÉGIQUE documentée (boucle vs S14, cf. §0)

P5 LIGUE (1.5 j)         S11/S11b + exploitant à force garantie (SF depth
                         limitée ou snapshot ×10) ; S12 v2
  GATE SP-5 v2 : exploitant d'abord VALIDÉ sur témoin (bat gen n-1 ≥ 60 %) —
                 prouve qu'il fonctionne — puis winrate vs courant < 55 %

P6 ANCRES (0.5 j + cont.) TB offline étendu si pertinent ; S14 selon décision
                         SP-4 ; rapport d'incohérence licence tranché et écrit
  GATE SP-6 v2 : pente Elo_ext/Elo_int ≥ 0.6 sur les 4 dernières gens
                 (jamais |écart| < seuil — l'offset de pool n'est pas un biais)
```

**Règle de plafond v2** (remplace toutes les occurrences v1) : *si ΔElo par doublement de positions uniques à GEN-CONFIG fixe < 10–20 sur deux doubles → changement de levier (capacité / mining / enseignant selon canary), jamais « plus de parties ».*

**Gate DIV** (continu, dès P0) : alarme si taux de nulles +15 points vs baseline gen-0 sur 2 gens, ou unicité < 85 % → actions S15 ordonnées : turnover EPD → τ↑ → cap médian ↓.

## 7. Garde-fous v2

- Tous les garde-fous v1 conservés (CPU partagé / taskset, draw-death, Δ sous-estimé et sa re-cherche, SPRT seul verdict).
- **Ajout — poids du générateur** : le net qui génère la gen n+1 est l'EMA (S18), jamais le dernier batch (stabilité inter-gens).
- **Ajout — labels d'exploit** : conf héritée du relabel uniquement ; toute ligne `src=exploit` sans `relabel` est droppée à l'ingestion.
- **Ajout — reproductibilité** : seeds, GEN-CONFIG hash, et version des poids archivés par gen (rejouabilité d'une gen).
- **Ajout — le bug le plus cher** : avant P4, un test statistique dirichlet/τ : entropie empirique des distributions racine générées ∈ fourchette attendue (sinon l'exploration est morte en silence et S4 distille un collapse).

## 8. Immédiat

1. Rédiger la décision §0 (½ h, bloque tout le reste).
2. P-1 : benchmark NPS + proto MLP forward-only + mesure conso M4/bridge (0.25 j).
3. P0 : buffer v2 + S17 + métriques DIV + ladder ancre local (1 j).
4. P1 Texel avec holdout externe → première entrée ELO_LOG « post-M4 ».
5. Lancer le canary S14 en fond dès que P3 tourne.

**Ce qui change en pratique** : la v2 coûte ~1–1.5 j humain de plus que la v1 (P-1, ML ops, ladder local), et supprime du chemin critique une idée (S10) et un gate mal dessiné (SP-5/SP-6 v1). En échange, le plan devient réfutable : chaque cause du plafond a sa mesure, le budget est un nombre, et la décision la plus coûteuse (S14) est prise sur données au premier plafond plutôt que par épuisement au dernier.