# SELFPLAY v3 — plan corrigé

> **Plan canonique** depuis le 2026-10-20 (révision v3, issue de la revue
> critique du document v2 ; remplace `selfplay_v1.md` archivé et la v2 du
> 2026-10-14).

**Principe de la v2** (conservé) : trois changements structurels (arbitrage stratégique explicite avant tout ; chiffrage CPU/NPS en phase P-1 ; provenance+confiance des labels comme concept de première classe) et une série de corrections ponctuelles (exploration racine, IS, gates en pente, ML ops). Ce qui était bon est conservé tel quel : le cadrage §0, S11b, la discipline SPRT, la règle de plafonnement.

**Principe de la v3** (cette révision) : le plan ne peut pas mesurer ce qu'il ne sait pas mesurer. Quatre corrections, toutes de même nature — **aligner le seuil sur l'instrument qui le mesure** :

1. l'**unité de plafond** passe de « doublement de corpus » à « ΔElo/h-CPU » (le doublement n'est pas une unité transitive) ;
2. le **SPRT gelé** (`elo0=0/elo1=5`) est un test de qualité binaire : un **estimateur de magnitude** (PR-SPRT) est ajouté, sinon le premier plafonnement est indécidable ;
3. le **holdout externe** est rendu **strictement disjoint** du corpus d'entraînement (il ne peut pas être à la fois juge et apprenant) ;
4. les **estimations d'Elo non mesurées** (S1/S2/S5) sont reclassées en « hypothèses à falsifier » et ne peuvent plus entrer dans un gate.

S'y ajoutent les précisions techniques S17 (cible policy = `softmax(Q/τ)`, pas les visites brutes), S8 (seuil d'escalade calibré sur la distribution de |ΔQ|), le schéma du buffer (`schema` versionné ; relation `z`/`res`/cible explicite), l'enveloppe CPU honnête (691 200 s/j = *plafond*, pas budget) et le chiffrage du **coût des mesures** lui-même.

## Diff v2 → v3 (traçabilité)

| Correction | Motif | Section |
|---|---|---|
| Unité de plafond : ΔElo/**h-CPU** (et non doublement de corpus) | le coût CPU d'un doublement n'est pas constant ; comparer des ΔElo entre stades de corpus n'est pas transitif | §1.5, §6 |
| Deux instruments : SPRT qualité (gelé) + **PR-SPRT** magnitude | le SPRT `elo0=0/elo1=5` est binaire ; il ne peut pas mesurer un seuil de 10–20 Elo, ni étayer « +150 Elo » | §5.1, §6 |
| Holdout externe **disjoint** du corpus d'entraînement | un holdout qui entre dans le corpus ne peut plus juger | §6 (P1/SP-1) |
| Estimations d'Elo reclassées en **hypothèses falsifiables** | « +150–300 Elo » (S1) est 30× la résolution de l'instrument ; le document était chiffré partout ailleurs | §7.1 |
| S17 : cible policy = `softmax(Q/τ)`, **pas** les visites brutes | à τ=1 la distribution de visites est une loi de Monte-Carlo : on propagerait le bruit, pas la force | §3.1 |
| S8 : seuil d'escalade **calibré** (quantile de \|ΔQ\|), pas 0.1 fixé | 0.1 sur Q∈[-1,1] déclencherait partout ; le plafond 3 % serait mort-né | §3.2 |
| Buffer : champ `schema` + règle d'arbitrage `z`/`res`/cible | le format n'était pas versionné ; la relation z/res/cible d'entraînement était implicite | §4 |
| Enveloppe CPU : 691 200 s/j = **plafond**, budget réel mesuré | le plan supposait 100 % des 8 cœurs, en coexistant avec le bridge et le canary | §1.3 |
| Chiffrage du **coût des mesures** dans P-1 | 7 gates SPRT sur 8 cœurs partagés peuvent coûter plus cher que l'entraînement | §1.3, §7 |
| SP-1 : plus de « SPRT multi-TC » (protocole gelé) | un second TC change la variance et invalide la comparaison avec ELO_LOG | §6 (P1) |
| Réserve explicite sur la mesure « boucle fermée » (Elo_ext/Elo_int) | deux Elo sur deux protocoles distincts : le ratio de pentes n'est pas interprétable en l'état | §2, §6 (SP-0) |
| `relabel` : résultat de l'adjudication explicité (`res=±1`) | l'adjudication « win/loss → z » laissait `z` et `res` redondants | §4 |

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

*Note v3* : si la réponse est « moyen », l'arbitrage se fait sur **ΔElo/h-CPU** (canary S14) — la même unité que la règle de plafond (§1.5). Les deux chiffres sont donc directement comparables, ce qui n'était pas le cas avec la v2 (ΔElo/double-corpus vs ΔElo/h-CPU). Si la réponse est « contrainte », la règle de plafond reste applicable mais son seuil n'a pas d'arbitrage concurrent à feed : on plafonne sur le levier interne disponible.

> ✅ **DÉCISION PRISE le 2026-10-21 : la réponse est « MOYEN ».**
> Le self-play est un moyen d'obtenir de l'Elo, pas une contrainte du projet
> (le core est gelé et le MCTS générique ; le projet serait cohérent,
> mesurable et mergable sans la moindre ligne de self-play).
> Conséquences : l'unité d'arbitrage est **ΔElo/h-CPU**, le **canary S14 est
> bloquant pour SP-4** (à lancer dès P3), et le raisonnement circulaire
> (« le self-play est bon parce que c'est du self-play ») est explicitement
> exclu.
> Décision complète, argumentée et datée : **`DECISION_SELFPLAY.md`** (fichier
> racine). Révisable uniquement par un nouveau document daté dans ce fichier,
> avec les données du canary.

## 1. P-1 — chiffrage du budget réel (0.25 j, bloquant)

1. **NPS baseline** : benchmark 100 positions fixes × 3 graines → nœuds/s/cœur (eval linéaire + MCTS). Noté dans ELO_LOG.
2. **Prototype MLP** (2 h, *sans training*) : forward pass du net S5 candidat sur 10⁴ positions → NPS estimé post-P4. C'est ce chiffre qui fixe le budget nœuds/coup post-P4 (probablement 2–4k au lieu de 10k — documenter, ne pas nier).
3. **Enveloppe CPU** : mesurer la consommation réelle M4-d9 + bridge sur une semaine → **budget self-play disponible** (chiffre écrit, pas implicite).
   - *Plafond théorique* : 8 cœurs × 86400 s = **691 200 s CPU/j**. Ce n'est **pas** le budget : le bridge lichess (S16) et le canary S14 (5–10 %) doivent cohabiter, et les runs self-play passent hors fenêtres de défis (`attente_puis_serie.sh`). Le budget réel est typiquement **60–75 % du plafond** — à mesurer, pas à supposer.
   - *Le coût des mesures Elo lui-même* doit être chiffré dans la même enveloppe. Sept gates SPRT sur 8 cœurs partagés peuvent coûter plus cher que l'entraînement qu'ils mesurent. P-1 chiffre aussi : parties/heure mesure (fastchess), coût CPU d'un leg SPRT, et le nombre de legs par gate.
4. **Allocation de boucle** (point de départ, ajustée à SP-3) : génération 70 % / mining+re-cherche 12 % / ancres 8 % / training 10 %. *Somme = 100 % du budget self-play disponible (point 3), pas du plafond théorique.*
5. **Unité de plafond v3** : *ΔElo par heure-CPU de self-play, à GEN-CONFIG fixe*.
   - **Pourquoi pas le doublement de corpus** : un doublement de positions uniques n'est pas un intervalle de temps constant. Le coût CPU d'un nouveau doublement **augmente** avec la taille du corpus (les positions uniques deviennent plus rares à trouver à mesure que le moteur rejoue les mêmes), donc comparer des ΔElo entre des stades de corpus différents est une comparaison non transitive. Le doublement reste une **description** de l'étape (« on a doublé le corpus entre la gen n et n+1 »), pas l'**unité** du seuil.
   - *Unité déjà maîtrisée ailleurs dans ce document* : §3 S14 utilise exactement ΔElo/h-CPU pour l'arbitrage self-play vs distillation. La règle de plafond doit utiliser la même unité, sinon deux chiffres du même document ne sont pas comparables.
   - **Seuil de changement de levier** : < 10–20 Elo/h-CPU sur deux étapes consécutives à GEN-CONFIG fixe. Tout changement de GEN-CONFIG re-baseline.
   - *Contrainte de mesure* : voir §6 « Protocole de mesure » — le SPRT `test.sh` (elo0=0/elo1=5) est un test de *qualité* binaire ; il ne donne pas la magnitude. Le seuil de 10–20 exige un estimateur de magnitude dédié (cf. §6, PR-SPRT).

> Conséquence honnête à écrire dans ELO_LOG : les gens MLP auront des labels racine *plus bruités* que les gens linéaires (budget/coup réduit pour tenir l'enveloppe). La compensation prévue : S8 escalade, S6/S7, TB. Si la compensation échoue → levier capacité ≠ levier bruit, diagnostic SP-3.

## 2. Cadrage (repris de v1, un ajout)

Les trois causes (boucle fermée / capacité / bruit de labels) et l'aggravateur diversité sont conservés. **Ajout v2 : le biais de boucle fermée est actif dès gen-0** — le corpus fondateur est distribué par l'eval qu'il va retuner. Toute mesure P1–P3 doit être relue avec cette étiquette.

Chaque cause reçoit désormais sa **mesure de succès** (pas seulement ses remèdes) :

| Cause | Mesure de succès |
|---|---|
| Boucle fermée | pente Elo_ext(gen)/Elo_int(gen) ≥ 0.6 sur ≥ 4 gens (S16 v2) — cf. réserve sur la commensurabilité (§6) |
| Capacité | ΔElo/h-CPU ≥ 20 à GEN-CONFIG fixe (unité v3, §1.5) |
| Bruit de labels | écart val-loss entre labels bruts et labels re-cherchés (holdout S8) — décroissant across gens |
| Diversité | gate DIV (§6 P0) |

> **Réserve sur la mesure « boucle fermée »** : `Elo_ext` et `Elo_int` ne sont pas sur une échelle commune contrôlable — `Elo_ext` dépend du pool d'ouvertures, du TC et du style de l'adversaire ; `Elo_int` dépend d'un autre pool. Le *ratio de deux pentes* ainsi construites n'a pas d'interprétation directe. Deux corrections possibles, à trancher en SP-0 :
> (a) **ancre unique** : un seul adversaire dont on mesure les deux Elo (interne sur le ladder local, externe sur le même pool) → le ratio est alors interprétable ;
> (b) **écart en Elo absolu** (l'ancrage v1 : |Elo lichess − Elo interne| < 150) sur un protocole gelé, avec seuil de *biais* et non de *pente*.
> La v3 ne tranche pas : elle garde la pente, ajoute la réserve, et mandate le arbitrage en SP-0. **SP-0 doit trancher (a) ou (b) avant SP-3**, sinon le gate « pente ≥ 0.6 » n'est pas actionnable.

## 3. Inventaire révisé

Conservés inchangés : S1, S2, S3, S4, S5, S11, **S11b (intégralement)**, S13 (déplacé), S15 (+ métriques).

Marqués avec delta :

| # | Correction (v2, ajustée v3) |
|---|---|
| S6 | λ du mix z/valeur **calibré chaque gen sur val set** (grille 0 / 0.15 / 0.25 / 0.4), pas fixé à 0.25 |
| S8 | re-cherche ×10, **escalade ×100 si \|Q_recheck − Q_orig\| > seuil** (seuil **calibré**, §3.2 ; plafond : 3 % des positions/mining round) ; le relabel écrase avec conf 0.9 |
| S9 | p_i ∝ (regret_i + 0.01)^0.6 ; **poids IS** w_i ∝ (N·p_i)^−β, β : 0.4→1 en fin de gen, clip 3× ; priorité des positions *non* re-cherchées cappée (regret biaisé → 0) |
| S10 | **déclassé** hors chemin critique (option post-P5). Conditions de réactivation : decay 0.98/gen sur ΣQ, Q̄ref = moyenne value des coups frères, application limitée aux positions vues ≥ 50× (openings/transpositions — stats buffer à l'appui) |
| S12 | tag `src=asym` ; **labels pris côté fort uniquement** ; côté faible : diversité d'ouvertures seulement, exclu du replay value/policy |
| S14 | **canary dès P3** (5–10 % CPU) : SF profondeur fixe sur 50k positions stratifiées du corpus → mini-distill → ΔElo/h-CPU vs boucle, input de la décision SP-4. Question licence tranchée **par écrit** avant (outputs de recherche vs poids) |
| S16 | **ladder local d'ancrage** : 3–4 adversaires fixes (snapshot gen-0 gelé, 1–2 moteurs externes à Elo CCRL connu, SF à skill-level fixe), TC/nœuds fixes, ≥ 200 parties/mesure, openings UHO. Maia/lichess = **ancre secondaire de style**, jamais de gate |

**Nouveaux :**

| # | Idée | Détail |
|---|---|---|
| S17 | **Exploration racine** (bloquant pour S4) | Dirichlet(α=0.3), ε=0.25, racine seule, actif pendant toute la génération d'entraînement. Température : τ=1 coups ≤ 20, décroissance linéaire vers τ=0.05 au coup 40. **Cible policy S4 = `softmax(Q/τ)` sur les valeurs de recherche, PAS les visites brutes** (cf. ci-dessous) |
| S18 | **ML ops minimum** (pré-gate P4) | split train/val **par partie** ; gradient check numérique (10 paramètres) ; overfit-1-batch (loss → ~0) ; EMA poids (decay 0.999) = poids du générateur n+1 ; fenêtre replay 5 gens ; sizing : params ≤ positions_uniques_entraînables / 20 en première approx, grossissement justifié par courbe val uniquement ; cache d'éval par nœud MCTS |

### 3.1 Précision S17 — la cible policy de S4 n'est pas la distribution de visites

La v2 écrivait « Visits stockées = distribution brute (→ cible S4 à τ=1, propre) ». C'est faux et c'est un piège : à τ=1, la distribution de visites est une **loi de Monte-Carlo**, plate sur les coups visités et de variance élevée. S'en servir comme cible de distillation policy ne « propage » pas la force de recherche — ça propage son bruit.

**Règle retenue** :

- la **cible policy** de S4 est `softmax(Q/τ)` sur les valeurs de recherche `Q` des coups racine (et non le ratio de visites) ;
- les **visites** sont stockées séparément (elles servent à S8/S9 : regret, frontière, entropie) mais ne sont pas la cible policy ;
- le τ utilisé pour la cible policy est celui du **mode mesure** (τ=0 → politique quasi-greedy), pas celui du mode génération — sinon on distille la politique bruitée qu'on cherche précisément à remplacer.

Conséquence de design : le buffer doit donc stocker **les `Q` par coup racine** (déjà prévu : champ `moves[].q`) *et* les visites *et* le τ applicable. Le champ `q` de la v2 est donc.load-bearing : sans lui, S4 n'est pas implémentable.

### 3.2 Précision S8 — le seuil d'escalade doit être calibré, pas fixé

Le seuil « escalade ×100 si |Q_recheck − Q_orig| > 0.1 » est posé a priori alors que `Q ∈ [-1,1]` (convention `eval_normalized`). Un seuil de 0.1 est **très permissif** : presque toutes les positions à regret non nul le déclencheront, et le plafond « 3 % des positions/mining round » serait atteint dès le premier round.

**Règle retenue** : le seuil d'escalade est **calibré sur la distribution réelle de |ΔQ|** mesurée sur un lot de positions du corpus, de la même façon que λ de S6 (« grille 0 / 0.15 / 0.25 / 0.4 », §3). Le seuil est un **quantile** de cette distribution (proposition : q90), pas une constante. Le plafond 3 % reste le garde-fou ; si le seuil calibré le rend inatteignable, c'est le plafond qui doit être relevé — pas le seuil handmade.

## 4. Buffer v3 — schéma et politique de confiance des labels

```json
{"schema":3,"fen":"...","gen":3,"played":"d7d5","z":1,"res":0.5,
 "src":"self","conf":0.6,"budget":10000,"gencfg":"sha8...",
 "moves":[{"u":"e2e4","q":0.13,"n":4120}],
 "delta":0.02,"entrop":1.84,
 "targets":{},
 "relabel":{"by":"search10x","q":0.05,"budget":100000}}
```

Changements : `played` (obligatoire — sinon `delta` incalculable sous τ), `src`, `conf`, `targets{}` extensible (WDL, moves-left à P4 sans migration de format), `gencfg` (hash de la GEN-CONFIG), `relabel` (écrasement tracé).

**Versionnement du schéma** : champ `"schema"` (entier) en tête de chaque ligne, lu par le lecteur. `gencfg` versionne la *configuration de génération* ; `schema` versionne la *forme du buffer*. Les deux sont nécessaires : un changement de `gencfg` re-baseline les gates (mesures non comparables), un changement de `schema` impose une migration du buffer existant. Règle : `schema` **n'est jamais** changé sur un buffer existant — on écrit un nouveau fichier, on migrate, et on note la migration dans ELO_LOG.

**Rôle de `z` / `res` / la cible d'entraînement** (précisé — la v2 laissait cette relation implicite) :

| champ | signifié | qui l'utilise |
|---|---|---|
| `z` | résultat de partie ∈ {-1, 0, +1}, point de vue du joueur **à la racine** | cible *value* de S6 (composante « résultat de partie ») |
| `res` | score **adjudiqué** ∈ [-1, +1] (0.5 = nulle adjugée) | remplace `z` quand l'adjudication a été appliquée ; cible value de S6 sinon |
| `q` (par coup) | valeur de recherche du coup, convention `eval_normalized` ∈ [-1,1] | **cible policy de S4** (softmax(Q/τ), §3.1) ; base du regret S8/S9 |

Règle d'arbitrage unique et non ambiguë : **la cible value est `res` s'il existe, sinon `z`** ; `z` n'est jamais utilisé quand `res` est présent. L'adjudication (draw → 0.5, win/loss → ±1) écrase donc `z` et laisse `z` comme trace de la partie d'origine — c'est ce qui rend la trace `conf` non ambiguë.

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

**Adjudication dé-circularisée** : draw-adjudication → res=0.5, conf 0.4 ; win/loss-adjudication → res=±1, conf 0.4, et la position d'adjudication entre en file de re-vérification ×10 (priorité basse). En P4, seuils de resign recalés sur WDL (P(loss) ≥ 0.9). Fin de la circularité eval→res→eval.

## 5. GEN-CONFIG — configuration de génération versionnée

Un fichier hashé par gen, référencé dans chaque ligne du buffer et dans ELO_LOG :

```
cpuct, nœuds/coup, τ-schedule, dirichlet (α, ε), loi du cap-randomization,
seuils d'adjudication, hash du pool EPD, hash net/poids
```

- Le **cap-randomization** (S15) est chiffré : cap ~ log-uniforme [500, budget].
- Distinction explicite **mode génération** (exploration : S17 actif) vs **mode mesure** (force max : τ=0, pas de Dirichlet, SPRT `test.sh`). v1 mélangeait les deux.
- Toute mesure Elo interne reste sous la discipline v1 : SPRT seul juge, jamais de multi-améliorations entre legs, mention « post-M4 ».

## 5.1 Protocole de mesure — deux instruments distincts

C'est la correction la plus structurante de la v3, et elle manquait à la v2.

Le protocole `test.sh` est **gelé** : `-sprt elo0=0 elo1=5 alpha=0.05 beta=0.05`, nœuds fixes, `-games 2`, `-repeat`, `-concurrency 5`, une 2ᵉ passe en temps (TC 8+0.08) pour les seuls candidats qui passent la passe nœuds-fixes. C'est un **test de qualité** : il répond « le candidat est-il meilleur que la base ? » avec un LLR et un verdict.

Il **ne** répond **pas** « de combien ? ». Un SPRT à `elo1=5` est calibré pour un effet réel d'environ 5 Elo ; son verdict est binaire. Or :

- le seuil de plafond est de **10–20 Elo/h-CPU** (§1.5) ;
- l'estimation S1 « Texel +150–300 Elo » (v1, non vérifiée) est **30×** la résolution de l'instrument.

Conséquence : avec le seul `test.sh`, on ne peut pas distinguer « ΔElo = 12, on plafonne » de « ΔElo = 3, bruit », et on ne peut pas étayer une annonce de « +150 Elo ».

**Deux instruments, deux rôles** :

| instrument | rôle | configuration | sortie |
|---|---|---|---|
| **SPRT qualité** (existant, `test.sh`) | « mieux ou pas ? » — verdict binaire pour ELO_LOG et les gates de passage | inchangé : `elo0=0 elo1=5` | LLR + verdict |
| **estimateur de magnitude** (nouveau, `PR-SPRT`) | « de combien ? » — exigé par les seuils de plafond et par tout chiffrage d'Elo annoncé | même protocole parties, mais sortie des **scores** et LLR continu → Elo **estimé ± IC** | Elo ± intervalle |

Règles d'usage :

1. **Le protocole `test.sh` reste gelé.** Aucun changement de `elo0/elo1/alpha/beta`, aucun « multi-TC » : ajouter un second TC change la variance du test et invalide la comparaison avec les entrées ELO_LOG existantes. Le gate SP-1 est donc reformulé (§6) pour ne pas exiger de multi-TC.
2. `PR-SPRT` **réutilise** les mêmes parties que le SPRT (même pool d'ouvertures, mêmes settings) et n'en diffère que par la sortie ; il ne double pas le coût des parties.
3. Tout chiffre d'Elo annoncé dans ELO_LOG porte son **intervalle de confiance**, jamais une point-estimation nue.
4. Le seuil de plafond (10–20 Elo/h-CPU) est comparé à l'**IC bas** de l'estimation, pas à sa moyenne : « on plafonne » signifie « le IC bas est sous le seuil », ce qui est la lecture conservative.

> PR-SPRT est une **brique de mesure**, pas une brique de self-play : elle doit exister avant P3, sinon le premier plafonnement est indécidable. Coût estimé : 0,5 j (script d'analyse du log fastchess + calcul LLR/IC, pas de changement moteur).

## 6. Phases et gates v3

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

P1 TEXEL (0.75 j)        corpus d'entraînement = gen-0 (holdout externe
                         EXCLU, cf. réserve) ; positions calmes
                         (post-quiescence, sans échec) ; split par partie
  GATE SP-1 : SPRT qualité (test.sh, protocole gelé) cand>base ;
              PR-SPRT : Elo estimé ± IC (l'IC bas sert de base au seuil) ;
              val holdout externe ≥ val corpus gen-0 (sinon biais déclaré) ;
              ELO_LOG : "Texel v1 sur corpus auto-distribué, refit prévu gen-3"

P2 PUCT+PRIORS           inchangé. GATE : SPRT vs A1.5.

P3 LOOPY (1.5 j setup)   boucle 24 h séquencée selon allocation §1 ;
                         S8 v2 (escalade ×100, seuil calibré §3.2) ; S9-IS ;
                         S13 offline (post-traitement ≤5 hommes → labels
                         exacts, jamais en ligne) ; S6 λ calibré ;
                         canary S14 (5–10 % CPU)
  GATE SP-3 : ΔElo/h-CPU ≥ 20 (IC bas) sur 2 étapes, à GEN-CONFIG fixe ;
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

**Règle de plafond v3** (remplace toutes les occurrences v1/v2) : *si ΔElo par heure-CPU de self-play à GEN-CONFIG fixe est < 10–20 (comparé à l'IC bas de PR-SPRT) sur deux étapes consécutives → changement de levier (capacité / mining / enseignant selon canary), jamais « plus de parties ».*

> Le « doublement de positions uniques » reste une **description** d'étape (« entre la gen n et n+1, le corpus a doublé ») mais n'est plus l'**unité** du seuil : le coût CPU d'un doublement n'est pas constant (les positions uniques deviennent plus rares à trouver), donc comparer des ΔElo entre stades de corpus différents n'est pas transitif. Voir §1.5.

**Gate DIV** (continu, dès P0) : alarme si taux de nulles +15 points vs baseline gen-0 sur 2 gens, ou unicité < 85 % → actions S15 ordonnées : turnover EPD → τ↑ → cap médian ↓.

**PR-SPRT** (0.5 j, à faire **avant P3**) : script d'analyse qui transforme le log fastchess en Elo estimé ± IC. Il ne modifie pas `test.sh` et ne change aucun protocole ; il lit les mêmes parties et produit une magnitude là où le SPRT ne produit qu'un verdict. Sans lui, le premier plafonnement est indécidable (§5.1).

**Réserve sur le holdout externe (P1/SP-1)** : le holdout sert à (a) mesurer la généralisation et (b) signaler un corpus biaisé. Il doit donc être **strictement disjoint** du corpus d'entraînement. La formulation v2 « corpus = gen-0 + holdout externe » est supprimée : un holdout qui entre dans le corpus ne peut plus jouer son rôle de juge. Le gate SP-1 compare `val(holdout externe)` à `val(corpus d'entraînement)` — c'est une **comparaison de généralisation**, pas une appartenance.

## 7. Garde-fous v3

- Tous les garde-fous v1 conservés (CPU partagé / taskset, draw-death, Δ sous-estimé et sa re-cherche, SPRT seul verdict).
- **Ajout — poids du générateur** : le net qui génère la gen n+1 est l'EMA (S18), jamais le dernier batch (stabilité inter-gens).
- **Ajout — labels d'exploit** : conf héritée du relabel uniquement ; toute ligne `src=exploit` sans `relabel` est droppée à l'ingestion.
- **Ajout — reproductibilité** : seeds, GEN-CONFIG hash, et version des poids archivés par gen (rejouabilité d'une gen).
- **Ajout — le bug le plus cher** : avant P4, un test statistique dirichlet/τ : entropie empirique des distributions racine générées ∈ fourchette attendue (sinon l'exploration est morte en silence et S4 distille un collapse).
- **Ajout — coût des mesures** : chaque gate qui déclenche une mesure Elo doit annoncer son coût CPU avant de s'engager (§1.3). Un gate dont la mesure coûte plus que l'étape qu'il évalue est un gate mal dimensionné.

## 7.1 Hypothèses d'Elo à falsifier (non des faits)

Les chiffres suivants viennent de v1 et n'ont **jamais été mesurés**. Ils ne doivent pas être cités comme des attentes, et chacun a une mesure qui l'infirme :

| hypothèse | source | mesure qui l'infirme | statut |
|---|---|---|---|
| Texel (S1) : **+150–300 Elo** | v1 | PR-SPRT sur le refit, IC bas | à vérifier en P1 |
| Boucle (S2) : pente **+30–80 Elo/×2** | v1 | PR-SPRT, ΔElo/h-CPU | à vérifier en P3 |
| MLP (S5) : **+100–200 Elo** puis plateau | v1 | PR-SPRT net vs P3 | à vérifier en P4 |
| Réduction coût d'échantillon S8 : **~×10** | v1 | écart val-loss brut vs re-cherché (holdout S8) | à vérifier en P3 |
| Budget nœuds/coup post-P4 : **2–4k** | v1 | NPS mesuré (proto MLP, §1.2) | à vérifier en P1 |

> Règle : aucun de ces nombres n'entre dans un gate, un engagement de planning ou une annonce ELO_LOG avant d'avoir été mesuré. Ils servent uniquement à dimensionner les efforts (un refit de 389 paramètres a un effet de l'ordre de quelques Elo, pas de centaines).

## 8. Immédiat

1. Rédiger la décision §0 (½ h, bloque tout le reste).
2. P-1 : benchmark NPS + proto MLP forward-only + mesure conso M4/bridge + **coût des mesures** (0.25 j).
3. **PR-SPRT** (0.5 j) : l'instrument de magnitude, avant tout gate de plafond.
4. P0 : buffer v2 (avec `schema` + relation `z`/`res` explicite) + S17 (cible policy = softmax(Q/τ)) + métriques DIV + ladder ancre local (1 j).
5. P1 Texel avec holdout externe **disjoint** → première entrée ELO_LOG « post-M4 ».
6. Lancer le canary S14 en fond dès que P3 tourne.

**Ce qui change en pratique** : la v2 coûte ~1–1.5 j humain de plus que la v1 (P-1, ML ops, ladder local), et supprime du chemin critique une idée (S10) et un gate mal dessiné (SP-5/SP-6 v1). En échange, le plan devient réfutable : chaque cause du plafond a sa mesure, le budget est un nombre, et la décision la plus coûteuse (S14) est prise sur données au premier plafond plutôt que par épuisement au dernier.