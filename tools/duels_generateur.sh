#!/usr/bin/env bash
# Les trois matchs de la phase « générateur » (2026-10-23).
#
# 1. le **gate** : l'alpha-beta **après** contre l'alpha-beta **avant**
#    (le binaire d'origine, figé dans /tmp/mychess-avant-movegen) ;
# 2. l'**ancre externe** : l'alpha-beta après contre **CAPS-LiTE**
#    (`toanth/motors`, moteur Rust > 3000 Elo, générateur pseudo-légal
#    + test de légalité par coup) ;
# 3. le **MCTS** contre CAPS-LiTE — la comparaison demandée entre les
#    deux algorithmes du dépôt et le générateur de référence.
#
# Protocole : `movetime` (le seul budget commun — CAPS n'implémente
# pas `go nodes`), livre ECO lichess, 200 rondes x 2 couleurs.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

NEW="target/release/mychess"
OLD="/tmp/mychess-avant-movegen"
CAPS="/tmp/motors/target/release/motors"
BOOK="${BOOK:-/tmp/openings_src/eco_lichess.epd}"
OUT="prt/rr"
mkdir -p "$OUT"

if [[ ! -f "$BOOK" ]]; then
    echo "livre d'ouvertures absent : $BOOK — génération"
    tools/gen_epd_openings.py > "$BOOK" 2>/dev/null || true
fi

echo "=== 1/3 — le gate : l'alpha-beta après contre l'alpha-beta avant ==="
fastchess -pgnout file="$OUT/movegen-gate.pgn" notation=san append=false \
    -engine name=apres cmd="$NEW" args=uci option.Search=alpha \
    -engine name=avant cmd="$OLD" args=uci option.Search=alpha \
    -each proto=uci st=0.200 timemargin=200 \
    -openings file="$BOOK" format=epd order=random \
    -rounds 200 -games 2 -concurrency 10 -srand 20261023 \
    > "$OUT/movegen-gate.log" 2>&1
echo "gate : $(grep -E '^Elo:|^Games:' "$OUT/movegen-gate.log" | tr '\n' ' ')"

echo "=== 2/3 — l'ancre externe : l'alpha-beta après contre CAPS-LiTE ==="
fastchess -pgnout file="$OUT/movegen-ab-vs-caps.pgn" notation=san append=false \
    -engine name=mychess cmd="$NEW" args=uci option.Search=alpha \
    -engine name=caps cmd="$CAPS" args=uci \
    -each proto=uci st=0.200 timemargin=200 \
    -openings file="$BOOK" format=epd order=random \
    -rounds 200 -games 2 -concurrency 10 -srand 20261023 \
    > "$OUT/movegen-ab-vs-caps.log" 2>&1
echo "ab vs caps : $(grep -E '^Elo:|^Games:' "$OUT/movegen-ab-vs-caps.log" | tr '\n' ' ')"

echo "=== 3/3 — le MCTS contre CAPS-LiTE ==="
fastchess -pgnout file="$OUT/movegen-mcts-vs-caps.pgn" notation=san append=false \
    -engine name=mychess cmd="$NEW" args=uci option.Search=mcts \
    -engine name=caps cmd="$CAPS" args=uci \
    -each proto=uci st=0.100 timemargin=200 \
    -openings file="$BOOK" format=epd order=random \
    -rounds 200 -games 2 -concurrency 10 -srand 20261023 \
    > "$OUT/movegen-mcts-vs-caps.log" 2>&1
echo "mcts vs caps : $(grep -E '^Elo:|^Games:' "$OUT/movegen-mcts-vs-caps.log" | tr '\n' ' ')"

echo "=== les trois matchs sont joués ==="
