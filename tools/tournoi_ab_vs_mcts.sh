#!/usr/bin/env bash
# tools/tournoi_ab_vs_mcts.sh — le duel **contrôlé** AB vs MCTS.
#
# Usage :
#   tools/tournoi_ab_vs_mcts.sh [--st 0.2] [--rounds 20] [--concurrency 5]
#                               [--book fichier.epd] [--seed N]
#                               [--out prt/rr/ab-vs-mcts.log]
#
# Le point de ce tournoi est qu'il est **contrôlé** : les deux moteurs
# sont le **même binaire**, avec la **même évaluation**
# (`eval::eval_for_mover`), le **même core gelé** et le **même budget de
# temps**. La seule différence est l'algorithme de recherche :
#
#   ab    `setoption name Search value alpha`  (phase AB : negamax + TT)
#   mcts  `setoption name Search value mcts`   (le MCTS du projet)
#
# C'est la seule expérience qui permette d'attribuer un écart de force à
# l'**algorithme** et non à l'évaluation — toutes les autres mesures du
# projet passent par Stockfish, à 950 Elo de distance
# (`docs/elo/README.md`).
#
# `timemargin` : tolérance au dépassement (une itération un peu trop
# longue ne doit pas faire adjuger la partie).
# Adjudication : **aucune** (ni `-resign`, ni `-draw`) — même règle que le
# tournoi du 2026-10-22.

set -euo pipefail

ST=0.2
ROUNDS=20
CONCURRENCY=5
BOOK=""
SEED=20261022
NOISE=0
OUT="prt/rr/ab-vs-mcts.log"
PGN=""
MYCHESS="${MYCHESS:-target/release/mychess}"
TIMEMARGIN=200

usage() {
    sed -n '2,22p' "$0" >&2
    exit 2
}

while [ $# -gt 0 ]; do
    case "$1" in
        --st)         ST="${2:?}"; shift 2 ;;
        --rounds)     ROUNDS="${2:?}"; shift 2 ;;
        --concurrency) CONCURRENCY="${2:?}"; shift 2 ;;
        --book)       BOOK="${2:?}"; shift 2 ;;
        --seed)       SEED="${2:?}"; shift 2 ;;
        --out)        OUT="${2:?}"; shift 2 ;;
        --pgn)        PGN="${2:?}"; shift 2 ;;
        --timemargin) TIMEMARGIN="${2:?}"; shift 2 ;;
        -h|--help)    usage ;;
        *) echo "option inconnue : $1" >&2; usage ;;
    esac
done

if [ -z "$BOOK" ]; then
    for cand in /tmp/openings_src/eco_lichess.epd \
                openings/eco_lichess.epd \
                openings/builtin_min.epd; do
        [ -f "$cand" ] && { BOOK="$cand"; break; }
    done
fi
[ -n "$BOOK" ] || { echo "aucun livre d'ouvertures" >&2; exit 2; }
[ -x "$MYCHESS" ] || { echo "mychess introuvable : $MYCHESS" >&2; exit 2; }
command -v fastchess >/dev/null || { echo "fastchess absent du PATH" >&2; exit 2; }

mkdir -p "$(dirname "$OUT")"

# `fastchess` **réécrit** un `config.json` dans le répertoire courant : ce
# doit être le répertoire du journal, jamais la racine du dépôt (ce
# fichier y est versionné, et il n'est pas un livrable de la mesure).
OUT_ABS="$(cd "$(dirname "$OUT")" && pwd)/$(basename "$OUT")"
OUT_DIR="$(dirname "$OUT_ABS")"
MYCHESS_ABS="$(cd "$(dirname "$MYCHESS")" && pwd)/$(basename "$MYCHESS")"
if [ -n "${BOOK:-}" ] && [ "${BOOK:0:1}" != "/" ]; then
    BOOK_ABS="$(cd "$(dirname "$BOOK")" && pwd)/$(basename "$BOOK")"
else
    BOOK_ABS="$BOOK"
fi

# L'en-tête porte le protocole complet : un journal sans son en-tête
# n'est pas une preuve.
{
    echo "# duel controle AB vs MCTS (meme binaire, meme evaluation)"
    echo "# budget : movetime ${ST}s par coup, identique pour les deux"
    echo "# timemargin : ${TIMEMARGIN} ms ; adjudication : aucune"
    echo "# book : ${BOOK} (ordre aleatoire, graine ${SEED})"
    echo "# rondes : ${ROUNDS} x 2 couleurs par paire"
    echo "# bruit de la valeur : ${NOISE} cp (0 = valeur parfaite)"
    echo "# ab = Search=alpha (negamax + table de transposition)"
    echo "# mcts = Search=mcts (le MCTS du projet)"
    echo "# date : $(date -Is)"
} > "$OUT"

PGN_ARGS=()
[ -n "$PGN" ] && PGN_ARGS=(-pgnout "file=$PGN" notation=uci)

(
cd "$OUT_DIR"
fastchess \
    "${PGN_ARGS[@]}" \
    -engine name=ab cmd="$MYCHESS_ABS" args="uci" option.Search=alpha option.EvalNoise="$NOISE" \
    -engine name=mcts cmd="$MYCHESS_ABS" args="uci" option.Search=mcts option.EvalNoise="$NOISE" \
    -each proto=uci "st=$ST" "timemargin=$TIMEMARGIN" \
    -openings file="$BOOK_ABS" format=epd order=random \
    -rounds "$ROUNDS" -games 2 -concurrency "$CONCURRENCY" -srand "$SEED" \
    >> "$OUT_ABS" 2>&1 < /dev/null
)

echo "journal : $OUT"
grep -c "^Finished game" "$OUT" || true
