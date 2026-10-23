#!/usr/bin/env bash
# Les trois duels AB-vs-AB de la phase AB8b, en **SPRT** (verdict binaire).
#
# Protocole : celui du projet, **gelé** (`test.sh`, phase 4/5) —
# `SPRT elo0=0 elo1=5 alpha=0.05 beta=0.05`. Le verdict est binaire :
# « le candidat (A) est-il meilleur que la référence (B) ? »
#   * acceptée  : A est meilleur d'au moins 5 Elo, avec 95 % de confiance ;
#   * rejetée   : A n'est pas meilleur (l'écart est sous 5 Elo ou négatif),
#                 avec 95 % de confiance.
#
# Le contrôle (`AbNullMove=1` contre `AbNullMove=1`) est **indispensable** :
# les deux moteurs sont identiques, donc le SPRT **doit** rejeter. S'il
# acceptait, l'instrument serait cassé (option mal transmise, biais de
# couleur, protocole faux) et les deux autres duels ne vaudraient rien.
#
# 200 rondes x 2 couleurs = 400 parties maximum ; le SPRT s'arrête avant
# si les bornes sont atteintes.
#
# Les trois duels sont joués **en séquence** : un contrôle temps-fixe
# (200 ms) est sensible à la charge de la machine, et un duel concurrent
# fausserait le verdict (cf. docs/ab/phase8-2026-10-23.md §4.1).
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

ROUNDS=200
SEED=20261023
COMMON=(--rounds "$ROUNDS" --sprt --seed "$SEED" --concurrency 5 --time 200)

echo "=== 1/3 — contrôle (AbNullMove=1 contre AbNullMove=1) ==="
tools/duel_ab.sh "${COMMON[@]}" \
    --a "AbNullMove=1" --b "AbNullMove=1" \
    --out prt/rr/ab-controle-sprt.log

echo "=== 2/3 — null-move (AbNullMove=1 contre AbNullMove=0) ==="
tools/duel_ab.sh "${COMMON[@]}" \
    --a "AbNullMove=1" --b "AbNullMove=0" \
    --out prt/rr/ab-nullmove-sprt.log --pgn prt/rr/ab-nullmove-sprt.pgn

echo "=== 3/3 — LMR (AbLmr=1 contre AbLmr=0) ==="
tools/duel_ab.sh "${COMMON[@]}" \
    --a "AbLmr=1" --b "AbLmr=0" \
    --out prt/rr/ab-lmr-sprt.log --pgn prt/rr/ab-lmr-sprt.pgn

echo "=== les trois duels sont joués ==="
