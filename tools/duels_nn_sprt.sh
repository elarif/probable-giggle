#!/usr/bin/env bash
# Les duels de la phase NN, en **SPRT** (verdict binaire) — le protocole
# **gelé** du dépôt (`elo0=0 elo1=5 alpha=0.05 beta=0.05`).
#
# Trois duels, joués **en séquence** (un contrôle à budget temps fixe est
# sensible à la charge de la machine ; un duel concurrent fausserait le
# verdict — cf. `docs/ab/phase8-2026-10-23.md` §4.1) :
#
#   1. **le contrôle** : le moteur contre lui-même (`EvalNN=0` des deux
#      côtés). Les deux moteurs sont identiques, donc le SPRT **doit**
#      rejeter. S'il acceptait, l'instrument serait cassé et les deux
#      autres duels ne vaudraient rien ;
#   2. **la valeur apprise** : `EvalNN=1` contre `EvalNN=0` — même
#      binaire, une seule option change. C'est le **gate de force** de
#      la phase ;
#   3. **le correctif du RNG** : le moteur au générateur corrigé contre
#      le moteur d'avant le correctif. C'est un **changement de
#      moteur**, donc il se mesure comme tel (le générateur du MCTS
#      avait une période de quatre — voir `docs/nn/phase-nn-2026-10-23.md`).
#
# 200 rondes x 2 couleurs = 400 parties maximum ; le SPRT s'arrête avant
# si les bornes sont atteintes.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

ROUNDS=200
SEED=20261023
NET="${NET:-models/value_v1.bin}"
AVANT="${AVANT:-/tmp/mychess-avant-fix-rng}"
# Le **moteur** du duel : l'alpha-beta, comme dans les duels AB8b. Les
# parties du MCTS a 200 ms/coup durent ~85 s (le moteur joue long), ce
# qui rendrait le duel inutilisable en temps Machine ; l'alpha-beta
# joue des parties decisives en quelques secondes, et c'est aussi
# l'instrument de mesure du projet.
SEARCH="${SEARCH:-alpha}"
COMMON=(--rounds "$ROUNDS" --sprt --seed "$SEED" --concurrency 5 --time 200 --search "$SEARCH")

echo "=== 1/3 — contrôle (le moteur contre lui-même) ==="
tools/duel_nn.sh "${COMMON[@]}" --controle \
    --out prt/rr/nn-controle-sprt.log

echo "=== 2/3 — la valeur apprise (EvalNN=1 contre EvalNN=0) ==="
tools/duel_nn.sh "${COMMON[@]}" --net "$NET" \
    --out prt/rr/nn-valeur-sprt.log --pgn prt/rr/nn-valeur-sprt.pgn

if [[ -x "$AVANT" ]]; then
    echo "=== 3/3 — le correctif du RNG (après contre avant) ==="
    tools/duel_nn.sh "${COMMON[@]}" --controle \
        --bin-a target/release/mychess --bin-b "$AVANT" \
        --out prt/rr/nn-rng-sprt.log
else
    echo "=== 3/3 — le correctif du RNG : binaire d'avant absent ($AVANT) — duel saute ==="
fi

echo "=== les duels sont joués ==="
