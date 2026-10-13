#!/bin/sh
# Réconciliation M4-d9 : le log du run détaillé vs la table CPW stockée.
# usage: sh slaml/reconcile.sh  → exit 0 = réconcilié exactement ; 1 = divergence.
# (aucune mesure : lecture de logs uniquement ; peut tourner pendant le run,
#  il répond simplement "log incomplet" tant que la table n'est pas imprimée.)
cd /workspaces/probable-giggle || exit 2

LOG=slaml/d9_detailed.log
REF=slaml/cpw_d9_reference.md

[ -f "$LOG" ] || { echo "reconcile: $LOG absent"; exit 2; }
grep -q "^end:" "$LOG" || { echo "reconcile: run M4 encore en cours (pas de ligne end:)"; exit 2; }

get_log() { awk -v k="$1" '$1 == k {print $NF; exit}' "$LOG"; }
get_ref() {
    case "$1" in
        nodes) rk="Nodes" ;;
        captures) rk="Captures" ;;
        ep) rk="E.p." ;;
        castles) rk="Castles" ;;
        promotions) rk="Promotions" ;;
        checks) rk="Checks" ;;
        discovery_checks) rk="Discovery Checks" ;;
        double_checks) rk="Double Checks" ;;
        mates) rk="Checkmates" ;;
    esac
    grep -F "| $rk |" "$REF" | head -1 | cut -d'|' -f3 | tr -d ' ,'
}

fail=0
total=0
for k in nodes captures ep castles promotions checks discovery_checks double_checks mates; do
    l_num=$(get_log "$k")
    r=$(get_ref "$k")
    total=$((total+1))
    if [ "$l_num" = "$r" ]; then
        echo "OK   $k = $l_num"
    else
        echo "DIFF $k: log='$l_num' cpw='$r'"
        fail=$((fail+1))
    fi
done
echo "---- réconciliation: $((total-fail))/$total colonnes"
[ "$fail" = 0 ] && exit 0 || exit 1