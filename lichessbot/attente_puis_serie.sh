#!/bin/sh
# Attend que le bridge soit libre (0 partie), puis lance une série de défis.
# usage: attente_puis_serie.sh [args du defi_serie.py]
cd /workspaces/probable-giggle
while :; do
    N=$(/workspaces/probable-giggle/lichessbot/venv/bin/python /workspaces/probable-giggle/lichessbot/playing_count.py)
    case "$N" in
        0) break ;;
        -1) sleep 10 ;;
        *) echo "bridge occupé ($N) — attente fin de partie..." ;;
    esac
    sleep 15
done
exec /workspaces/probable-giggle/lichessbot/venv/bin/python /workspaces/probable-giggle/lichessbot/defi_serie.py "$@"