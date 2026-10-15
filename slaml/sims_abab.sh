#!/bin/sh
# ==========================================================================
# slaml/sims_abab.sh — sims/s A/B/A/B (checklist A1, MESURE POST-M4).
# usage: sims_abab.sh <v1_bin> <a1_bin> [nodes] [pairs] [fen]
#
#   A = v1 baseline (tag vmcts-v1 binary : playouts aléatoires)
#   B = A1 (HEAD binary : eval-leaf + calibration nommée)
#
# Interleaved A/B/A/B, same session (ELO_LOG rule: jamais cross-jour).
# Each run: fresh `uci/isready/ucinewgame`, `position <fen>`, `go nodes N`,
# sims/s parsed from the engine's own `info ... nodes {} nps {} time {}`
# line (nodes = playouts budget honnêtement décompté par Search::run).
# First run of each side is a warmup, logged but EXCLUDED from the medians.
# ==========================================================================
set -u
cd /workspaces/probable-giggle || exit 2
V1=${1:?usage: sims_abab.sh <v1_bin> <a1_bin> [nodes] [pairs] [fen]}
A1=${2:?usage: sims_abab.sh <v1_bin> <a1_bin> [nodes] [pairs] [fen]}
NODES=${3:-10000}
PAIRS=${4:-2}
FEN=${5:-"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1"}
LOG=slaml/sims_abab_$(date -u +%Y%m%dT%H%M%SZ).log

{
    echo "=== sims/s A/B/A/B (post-M4): A=v1 ($V1) | B=A1 ($A1) ==="
    echo "go nodes=$NODES, pairs=$PAIRS (A/B/A/B interleave), fen: $FEN"
    echo "start: $(date -Is), host: $(uname -n), cores: $(nproc)"
} > "$LOG"

run_one() { # $1=bin $2=tag $3=round
    out=$( (
        printf 'uci\nisready\nucinewgame\nposition %s\ngo nodes %s\n' \
            "fen $FEN" "$NODES"
        sleep 8
        printf 'quit\n'
    ) | "$1" 2>/dev/null )
    info=$(printf '%s\n' "$out" | grep '^info ' | tail -1)
    nps=$(printf '%s ' "$info" | sed -n 's/.* nps \([0-9.]*\) .*/\1/p')
    ms=$(printf '%s ' "$info" | sed -n 's/.* time \([0-9]*\) .*/\1/p')
    n=$(printf '%s ' "$info" | sed -n 's/.* nodes \([0-9]*\) .*/\1/p')
    echo "$3|$2|$n|$nps|$ms|$info" >> "$LOG"
    echo "$3|$2|nodes=$n sims/s=$nps ms=$ms"
}

echo "-- warmup (excluded from medians)"
run_one "$V1" v1 warmup >/dev/null
run_one "$A1" a1 warmup >/dev/null

p=1
while [ "$p" -le "$PAIRS" ]; do
    run_one "$V1" v1 "A$p"
    run_one "$A1" a1 "B$p"
    p=$((p + 1))
done

median() { # $1=tag
    grep "|$1|" "$LOG" | grep -v '|warmup|' | awk -F'|' '{print $5}' |
        sort -n | awk '{a[NR]=$1} END {print a[int((NR+1)/2)]}'
}
mv1=$(median v1)
mb1=$(median a1)
{
    echo ""
    echo "medians: v1=$mv1 sims/s | A1=$mb1 sims/s | ratio A1/v1=$(awk "BEGIN{printf \"%.0f\", $mb1/$mv1}")x"
    echo "attendu (checklist): v1 ~10-15k sims/s, A1 ~1M sims/s (ordre de grandeur)"
    echo "end: $(date -Is)"
} >> "$LOG"
echo "median v1=$mv1 A1=$mb1 (log: $LOG)"