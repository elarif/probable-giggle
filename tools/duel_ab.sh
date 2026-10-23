#!/usr/bin/env bash
# Duel **AB vs AB** : le même binaire, deux jeux d'options différents.
#
# C'est l'instrument de la phase AB8 (« AB8b » du plan
# `docs/ab/plan-heuristiques-2026-10-23.md`) : décider si une
# heuristique d'élagage est un **gain de force**, et pas seulement un
# gain de débit.
#
# Le duel de la phase AB (`tools/tournoi_ab_vs_mcts.sh`) oppose l'alpha-beta
# au MCTS ; celui-ci oppose **deux alpha-beta**, ce qui isole
# exactement l'effet de l'option testée : même moteur, même évaluation,
# même budget de temps, une seule option qui change.
#
# Usage :
#   tools/duel_ab.sh [--rounds N] [--a "AbNullMove=1"] [--b "AbNullMove=0"]
#                    [--out LOG] [--concurrency N] [--seed N] [--time MS]
#
# Exemple :
#   tools/duel_ab.sh --rounds 20 --a "AbNullMove=1" --b "AbNullMove=0" \
#       --out prt/rr/ab-nullmove.log
set -euo pipefail

ROUNDS=20
CONCURRENCY=5
SEED=20261023
TIME_MS=200
TIMEMARGIN=200
OPT_A=""
OPT_B=""
OUT="prt/rr/ab-duel.log"
PGN=""
MYCHESS="${MYCHESS:-target/release/mychess}"
BOOK="${BOOK:-/tmp/openings_src/eco_lichess.epd}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --rounds) ROUNDS="$2"; shift 2 ;;
        --concurrency) CONCURRENCY="$2"; shift 2 ;;
        --seed) SEED="$2"; shift 2 ;;
        --time) TIME_MS="$2"; shift 2 ;;
        --timemargin) TIMEMARGIN="$2"; shift 2 ;;
        --a) OPT_A="$2"; shift 2 ;;
        --b) OPT_B="$2"; shift 2 ;;
        --out) OUT="$2"; shift 2 ;;
        --pgn) PGN="$2"; shift 2 ;;
        *) echo "option inconnue : $1" >&2; exit 2 ;;
    esac
done

# **fastchess lit `st=` en SECONDES** (et non en millisecondes :
# `fastchess -help` : « st=N - Movetime in seconds »). Passer 200
# directement donnerait 200 s par coup, soit un facteur 1000 — un duel
# qui ne finit pas et un journal qui ment sur le budget.
ST_SEC="$(awk -v ms="$TIME_MS" 'BEGIN { printf "%.3f", ms / 1000 }')"

if ! command -v fastchess >/dev/null 2>&1; then
    echo "fastchess est introuvable (il est compilé dans ~/.local/bin — cf. ELO_LOG.md)" >&2
    exit 2
fi
if [[ ! -x "$MYCHESS" ]]; then
    echo "le moteur $MYCHESS n'existe pas (cargo build --release)" >&2
    exit 2
fi
if [[ ! -f "$BOOK" ]]; then
    echo "le livre $BOOK est introuvable (tools/gen_epd_openings.py peut le reconstruire)" >&2
    exit 2
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
OUT_DIR="$(dirname "$OUT")"
mkdir -p "$OUT_DIR"
# Un chemin absolu est utilisé tel quel ; un chemin relatif est relatif
# à la racine du dépôt.
abs_path() {
    case "$1" in
        /*) echo "$1" ;;
        *) echo "$ROOT/$1" ;;
    esac
}
OUT_ABS="$(abs_path "$OUT")"
BOOK_ABS="$(abs_path "$BOOK")"
MYCHESS_ABS="$(abs_path "$MYCHESS")"

# Les options passées au moteur : `AbNullMove=1` devient
# `option.AbNullMove=1` pour fastchess.
opts_to_fastchess() {
    local spec="$1"
    if [[ -z "$spec" ]]; then
        echo ""
        return
    fi
    local out=""
    local IFS=' '
    for kv in $spec; do
        out+=" option.${kv}"
    done
    echo "${out# }"
}

A_OPTS="$(opts_to_fastchess "$OPT_A")"
B_OPTS="$(opts_to_fastchess "$OPT_B")"

{
    echo "# duel AB vs AB — $(date -Is)"
    echo "# A : ${OPT_A:-<defaut>}"
    echo "# B : ${OPT_B:-<defaut>}"
    echo "# book : ${BOOK} (ordre aleatoire, graine ${SEED})"
    echo "# rondes : ${ROUNDS} x 2 couleurs par paire"
    echo "# temps : ${TIME_MS} ms par coup (fastchess st=${ST_SEC}s, timemargin=${TIMEMARGIN} ms) ; adjudication : aucune"
    echo "# hote : $(uname -srm) — $(nproc) coeurs"
    echo "# command : fastchess ${A_OPTS} vs ${B_OPTS}"
} | tee "$OUT_ABS"

PGN_ARGS=()
if [[ -n "$PGN" ]]; then
    PGN_ABS="$(abs_path "$PGN")"
    mkdir -p "$(dirname "$PGN_ABS")"
    PGN_ARGS=(-pgnout "file=$PGN_ABS" notation=san append=false)
fi

cd "$OUT_DIR"
fastchess \
    "${PGN_ARGS[@]}" \
    -engine name=a cmd="$MYCHESS_ABS" args="uci" option.Search=alpha ${A_OPTS} \
    -engine name=b cmd="$MYCHESS_ABS" args="uci" option.Search=alpha ${B_OPTS} \
    -each proto=uci "st=${ST_SEC}" "timemargin=${TIMEMARGIN}" \
    -openings file="$BOOK_ABS" format=epd order=random \
    -rounds "$ROUNDS" -games 2 -concurrency "$CONCURRENCY" -srand "$SEED" \
    2>&1 | tee -a "$OUT_ABS"
