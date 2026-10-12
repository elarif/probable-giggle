# ELO_LOG.md — journal match/SPRT (futur)

hardware: **AMD EPYC 7763** (Milan, Zen3 family `0x19`, 4C/8SMT, ~2.45 GHz,
BMI2, pas d'AVX-512) — hyperviseur partagé : les mesures nps vivent dans
MIGRATION.md avec des fourchettes larges, le CI n'exige JAMAIS de seuil nps.

Équipes de test prévues (phase MCTS) :
- fastchess SPRT concurrency = **4–6** (4 cœurs physiques : ne pas saturer
  les 8 SMT pour ne pas biaiser les temps de réflexion),
- CPU-only → **Tier D** (pas de GPU dans la chaîne),
- A/B/A/B intercalé même session, jamais cross-jour.

## Entrées

*(aucune partie jouée à ce jour — le cœur n'a ni recherche ni évaluation ;
ce journal accueillera SPRT/fix-ply/Match dès la phase MCTS)*

| date | version | adversaire | conditions | résultat |
|---|---|---|---|---|
| — | — | — | — | — |