#!/usr/bin/env bash
# Duel **valeur apprise vs référence gelée** (phase NN, tranche N4).
#
# C'est le **gate de force** de la phase NN : l'erreur d'évaluation peut
# être meilleure et le moteur être **plus faible** — c'est exactement ce
# qui est arrivé au null-move (phase AB8 : −71 Elo). Le seul juge est le
# duel.
#
# Le protocole est **le protocole gelé** du dépôt :
#   * SPRT `elo0=0 elo1=5 alpha=0.05 beta=0.05` (bornes LLR ±2,94) ;
#   * 400 parties, 2 couleurs par paire ;
#   * un **contrôle** obligatoire avant (même binaire, mêmes options des
#     deux côtés) : il mesure le biais du protocole et prouve que le
#     résultat n'est pas un artefact.
#
# L'**A** est le moteur avec la valeur apprise (`EvalNN=1`), le **B** est
# la référence gelée (l'évaluation statique, `EvalNN=0`). Le B est le
# *même binaire* : la seule différence est l'option, donc la mesure est
# propre.
#
# Usage :
#   tools/duel_nn.sh [--rounds N] [--time MS] [--seed N] [--concurrency N]
#                    [--net FICHIER] [--search mcts|alpha] [--out LOG]
#                    [--pgn FICHIER] [--sprt] [--controle]
#
#   --controle  remplace le duel par le **contrôle** (A et B ont les
#               mêmes options) : c'est la mesure qui doit être plate.
set -euo pipefail

ROUNDS=200
CONCURRENCY=5
SEED=20261023
TIME_MS=200
TIMEMARGIN=200
SPRT=""
SPRT_ARGS=()
NET="${NET:-models/value_v1.bin}"
SEARCH="mcts"
OUT="prt/rr/nn-duel.log"
PGN=""
CONTROLE=0
MYCHESS="${MYCHESS:-target/release/mychess}"
# Les deux binaires du duel : par défaut le meme (une seule option
# change) ; ils peuvent differer pour mesurer un **correctif** du
# moteur (ex. : le correctif du RNG).
BIN_A="${BIN_A:-$MYCHESS}"
BIN_B="${BIN_B:-$MYCHESS}"
BOOK="${BOOK:-/tmp/openings_src/eco_lichess.epd}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --rounds) ROUNDS="$2"; shift 2 ;;
        --concurrency) CONCURRENCY="$2"; shift 2 ;;
        --seed) SEED="$2"; shift 2 ;;
        --time) TIME_MS="$2"; shift 2 ;;
        --timemargin) TIMEMARGIN="$2"; shift 2 ;;
        --net) NET="$2"; shift 2 ;;
        --bin-a) BIN_A="$2"; shift 2 ;;
        --bin-b) BIN_B="$2"; shift 2 ;;
        --search) SEARCH="$2"; shift 2 ;;
        --out) OUT="$2"; shift 2 ;;
        --pgn) PGN="$2"; shift 2 ;;
        --sprt) SPRT_ARGS=(-sprt elo0=0 elo1=5 alpha=0.05 beta=0.05); SPRT="elo0=0 elo1=5 alpha=0.05 beta=0.05"; shift ;;
        --controle) CONTROLE=1; shift ;;
        *) echo "option inconnue : $1" >&2; exit 2 ;;
    esac
done

# **fastchess lit `st=` en SECONDES** (et non en millisecondes).
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
abs_path() {
    case "$1" in
        /*) echo "$1" ;;
        *) echo "$ROOT/$1" ;;
    esac
}
OUT_ABS="$(abs_path "$OUT")"
BOOK_ABS="$(abs_path "$BOOK")"
MYCHESS_ABS="$(abs_path "$BIN_A")"
MYCHESS_B_ABS="$(abs_path "$BIN_B")"
NET_ABS="$(abs_path "$NET")"

if [[ "$CONTROLE" -eq 1 ]]; then
    A_OPTS="option.Search=${SEARCH}"
    B_OPTS="option.Search=${SEARCH}"
    LABEL="controle (A et B identiques)"
else
    A_OPTS="option.Search=${SEARCH} option.EvalNN=1 option.EvalNNFile=${NET_ABS}"
    B_OPTS="option.Search=${SEARCH} option.EvalNN=0"
    LABEL="valeur apprise (EvalNN=1) contre reference gelee (EvalNN=0)"
fi

{
    echo "# duel NN — $(date -Is)"
    echo "# nature : ${LABEL}"
    echo "# reseau : ${NET_ABS}"
    echo "# binaires : A = ${MYCHESS_ABS} — B = ${MYCHESS_B_ABS}"
    echo "# book : ${BOOK} (ordre aleatoire, graine ${SEED})"
    echo "# rondes : ${ROUNDS} x 2 couleurs par paire"
    echo "# temps : ${TIME_MS} ms par coup (fastchess st=${ST_SEC}s, timemargin=${TIMEMARGIN} ms) ; adjudication : aucune"
    echo "# sprt : ${SPRT:-<aucun — Elo estime, verdict non binaire>}"
    echo "# hote : $(uname -srm) — $(nproc) coeurs"
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
    -engine name=a cmd="$MYCHESS_ABS" args="uci" ${A_OPTS} \
    -engine name=b cmd="$MYCHESS_B_ABS" args="uci" ${B_OPTS} \
    -each proto=uci "st=${ST_SEC}" "timemargin=${TIMEMARGIN}" \
    -openings file="$BOOK_ABS" format=epd order=random \
    -rounds "$ROUNDS" -games 2 -concurrency "$CONCURRENCY" -srand "$SEED" \
    "${SPRT_ARGS[@]}" \
    2>&1 | tee -a "$OUT_ABS"
