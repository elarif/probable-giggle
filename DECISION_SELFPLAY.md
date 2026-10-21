# Décision stratégique §0 — le self-play est-il une contrainte du projet ou un moyen ?

> **Statut** : tranchée, écrite, datée — 2026-10-21.
> **Porte** : SELFPLAY.md §0 exige cette réponse **avant P-1** (chiffrage CPU/NPS,
> bloquant pour tout le reste du pipeline).
> **Portée** : cette décision ne se périme pas au premier plafonnement ; elle
> fixe l'**unité d'arbitrage** et l'**interdiction de raisonnement circulaire**.

## La question, telle que §0 la pose

> « le self-play est-il une contrainte du projet ou un moyen ? »

Deux réponses possibles, et les conséquences ne se ressemblent pas :

- **contrainte** : le self-play est une propriété du projet (indépendance de
  l'engine, originalité, licence) — on le fait parce qu'il le faut ; le
  plafonnement est une donnée du problème, et on change de levier *interne*
  quand il est atteint.
- **moyen** : le self-play est un *outil* pour obtenir de l'Elo — il est en
  compétition continue avec la distillation, et on compare les deux pistes en
  coût par Elo.

## La réponse : **moyen**

**Le self-play est un moyen d'obtenir de l'Elo, pas une contrainte du projet.**

### Pourquoi (l'argument décisif)

Le projet est un moteur d'échecs écrit de zéro. Aucun élément de son cahier
des charges n'impose le self-play :

- Le **core** (position, movegen, Zobrist, perft) est gelé depuis GATE 2.5
  (`vcore-1.0`) et ne dépend en rien d'un pipeline d'entraînement. Il serait
  identique sans la moindre ligne de self-play.
- Le **MCTS** (v1, tag `vmcts-v1`) est un moteur de jeu générique. Il consomme
  un contrat d'éval ; il n'a aucune opinion sur d'où viennent les poids.
- Le **ladder** A1→A2→A3→… (voir README) est une suite d He's improvements
  *de moteur*, pas de méthode d'entraînement. Chaque étape est mesurable par
  SPRT sans corpus, sans buffer, sans réseau.
- Le **bridge lichess** (S16) mesure l'Elo *externe* du moteur. Il ne dépend
  que du binaire UCI, pas de la boucle.

Autrement dit : si le self-play disparaissait demain, le projet resterait
cohérent, mesurable, et mergable. Ce qui le définit, c'est
« moteur d'échecs original, CPU-only, mesuré » — pas « moteur entraîné par
self-play ». La réponse ne peut donc pas être *contrainte*.

**Corollaire** : la question n'est pas « le self-play est-il utile ? » mais
« est-il le *meilleur* moyen, par heure-CPU, d'obtenir l'Elo que ce projet
vise ? ». C'est exactement la question que §0 formule en *moyen*.

### Ce que la réponse implique, concrètement

1. **L'unité d'arbitrage est ΔElo / heure-CPU** (§1.5, v3). La même unité que
   celle du canary S14 (§3, P3) et que celle de la règle de plafond (§6). Deux
   chiffres du même document doivent être comparables ; ils le sont maintenant.
2. **Le canary S14 est obligatoire dès P3**, sur 5–10 % du CPU : il mesure le
   ΔElo/h-CPU de la boucle face à celui de la distillation, sur le même
   protocole, avec le même instrument (PR-SPRT).
3. **La décision SP-4 est un arbitrage chiffré, pas un rituel** : poursuite de
   la boucle, pivot vers la distillation, ou hybride (têtes séparées, S11b-iv).
   Les trois sont admis ; aucun n'est le default.
4. **La règle de plafond reste applicable** (§6) : < 10–20 Elo/h-CPU sur deux
   étapes consécutives à GEN-CONFIG fixe → changement de levier (capacité /
   mining / enseignant), **jamais « plus de parties »**. Mais la réponse
   « moyen » ajoute une seule chose à la v2 : ce changement de levier a un
   **arbitre externe** (le canary), au lieu d'être un arbitrage interne.
5. **Le canary ne peut pas attendre le plafond.** C'était l'objection de fond à
   la v1, et c'est la raison pour laquelle §0 est bloquant. Penser « on
   verra au plafond » est précisément l'erreur que la décision « moyen »
   interdit : elle ferait consommer tout le pipeline avant d'utiliser le levier
   reconnu comme le plus gros.

### Ce que la réponse nAutorise PAS

- **Pas de raisonnement circulaire.** « Le self-play est bon parce que c'est
  du self-play » est la définition de la réponse *contrainte* ; on l'écarte
  explicitement. Le pipeline doit produire un nombre, pas une conviction.
- **Pas de raccourci par les estimations non mesurées.** Les chiffres « +150 à
  +300 Elo » (S1/S2/S5) sont reclassés en « hypothèses à falsifier » (§7.1) et
  ne peuvent plus entrer dans un gate. Ils ne sont pas des arguments en
  faveur du self-play.
- **Pas de gel de l'arbitrage.** Si la décision est révisée (ex. : le canary
  montre que la distillation gagne d'un facteur 2 en ΔElo/h-CPU), elle doit
  être **ré-écrite, datée, et consignée** dans ce fichier — pas tacitement
  appliquée.

## Conséquences sur le chemin critique (inchangé, mais hiérarchisé)

La réponse « moyen » **ne change pas la liste des gates** : elle change
**l'ordre d'importance** et rend deux items bloquants qu'ils ne l'étaient pas.

| item | avant | après |
|---|---|---|
| PR-SPRT (§5.1) | recommandé | **bloquant** — sans instrument de magnitude, l'arbitrage « moyen » n'a pas de commune mesure |
| canary S14 (P3) | prévu | **bloquant pour SP-4**, à lancer dès que P3 tourne (§8.6) |
| boucle fermée : ancre unique (a) vs écart absolu (b) | à trancher en SP-0 | **inchangé** — mais le tranchement doit précéder SP-3, sinon le gate « pente ≥ 0.6 » n'est pas actionnable |
| §0 | bloquant | **tranché** par ce document |

## Pièces jointes

- `SELFPLAY.md` §0, §1, §1.5, §3 (S14), §5.1, §6, §7, §8 — le plan canonique.
- `MIGRATION.md` — GATE 2.5 (core gelé, `vcore-1.0`), l'état M4.
- `ELO_LOG.md` — l'état des instruments de mesure au 2026-10-21.
- `docs/superpowers/plans/2026-10-20-selfplay-p0-p1-instrumentation.md` — le plan
  d'instrumentation (P0/P1) en cours.

## Ce que cette décision ne fait pas

Elle ne chiffre rien : l'enveloppe CPU, le NPS mesuré, le coût des mesures
SPRT et le ratio ΔElo/h-CPU restent à produire en **P-1** (§1) et au canary
S14 (P3). La décision fixe l'**unité** et l'**arbitre** ; elle ne fournit pas
les chiffres. Les produire est le travail suivant, et il commence par
l'instrument de mesure (PR-SPRT), dont l'implémentation est en cours sur la
branche `feature/selfplay-instrumentation`.

---

*Décision prise le 2026-10-21. Révisable uniquement par un nouveau document
daté dans ce fichier, avec les données du canary S14.*
