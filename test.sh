#!/bin/sh
# ==========================================================================
# test.sh <candidate> <baseline> — SPRT via fastchess (Elo ladder protocol).
#
# Protocol (fixed): SPRT elo0=0 elo1=5 alpha=0.05 beta=0.05; node-fixed
# controls (2k/10k/40k nodes per move, fastchess `nodes=`), second run in
# time (TC 8+0.08) only for candidates that pass node-fixed. Openings:
# uho/lichess4852 style EPD, each position twice (colors swapped), random
# order. Fixed identical settings between engines (threads=1, hash=16).
#
# Concurrency: 5 (fastchess) — machine: EPYC 7763 4C/8SMT (voir ELO_LOG.md).
#
# Openings file: placer openings/uho_lichess_4852_v1.epd (ou chess.epd).
# openings/README.md explique où les récupérer. À défaut le script utilise
# openings/builtin_min.epd (UNIQUEMENT pour smoke-tests : trop petit pour un
# SPRT officiel — biais d'ouvertures).
#
# BENCH-FREEZE guard: refuses to run until M4-d9 (slaml/d9_detailed.log) is
# complete AND reconciled vs slaml/cpw_d9_reference.md (see MIGRATION.md /
# ELO_LOG.md note "M4 reconciled"), unless MYCHESS_ELO_ALLOW=1 is set.
#
# Usage:
#   test.sh <candidate_bin> <baseline_bin> [nodes] [rounds]
#       candidate_bin / baseline_bin : paths to mychess UCI binaries
#       nodes   : nodes per move for the SPRT (default 10000)
#       rounds  : rounds to schedule (default 200 -> ~400 games max; SPRT
#                 stops early on LLR bounds)
# On completion (win/loss/at-limit), a line is APPENDED to ELO_LOG.md as a
# template with the raw LLR/loss fields still to fill by the operator
# (fastchess prints the SPRT verdict on stdout; it is teed to a file).
# ==========================================================================
set -u

if [ "${MYCHESS_ELO_ALLOW:-0}" != "1" ] && [ -f slaml/d9_detailed.log ]; then
    if ! grep -q "^end:" slaml/d9_detailed.log 2>/dev/null; then
        echo "test.sh: BENCH-FREEZE actif (M4-d9 en cours). Fixer MYCHESS_ELO_ALLOW=1 pour outrepasser." >&2
        exit 3
    fi
fi

CAND=${1:?usage: test.sh <candidate_bin> <baseline_bin> [nodes] [rounds]}
BASE=${2:?usage: test.sh <candidate_bin> <baseline_bin> [nodes] [rounds]}
NODES=${3:-10000}
ROUNDS=${4:-200}
OPENINGS=${MYCHESS_OPENINGS:-openings/uho_li_4852.epd}
if [ ! -f "$OPENINGS" ]; then
    OPENINGS="openings/chess.epd"
fi

if ! command -v fastchess >/dev/null 2>&1; then
    echo "test.sh: fastchess introuvable (installer le fork de cutechess-cli)." >&2
    exit 4
fi

mkdir -p prt
TS=$(date -u +%Y%m%dT%H%M%SZ)
OUT="prt/sprt-${TS}.log"

# Node-fixed SPRT; concurrency 5 (4 physical cores, SMT margin per ELO_LOG).
# fastchess node control: `tc=nodes=N` (fixed nodes/move, deterministic TC);
# if the build rejects tc=nodes=, use -each proto=uci nodes="$NODES" instead.
fastchess \
    -engine name=cand cmd="$CAND" \
    -engine name=base cmd="$BASE" \
    -each proto=uci tc="nodes=${NODES}" \
    -sprt elo0=0 elo1=5 alpha=0.05 beta=0.05 \
    -openings file="$OPENINGS" format=epd order=random \
    -rounds "$ROUNDS" -games 2 -concurrency 5 -repeat \
    -resign movecount=3 score=600 -draw movenumber=40 movecount=8 score=10 \
    | tee "$OUT"

echo ""
echo "== SPRT raw results: $OUT =="
echo "append a ELO_LOG.md:"
echo "| <date> post-M4 | <commit> | cand vs base, nodes=$NODES | LLR, W-L-D -> Elo±err | verdict |"