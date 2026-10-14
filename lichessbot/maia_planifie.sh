#!/bin/sh
# Séquence maia planifiée : maia1/maia5/maia9 ont un quota quotidien de
# bot-vs-bot (100 parties/jour/bot) qui se réouvre ~07:15-07:31 UTC.
# Ce script dort jusqu'à 07:25 UTC 2026-10-14 puis enchaîne les défis
# (un seul jeu à la fois, le bridge joue). Re-lançable : si la fenêtre est
# passée il tire immédiatement.
TARGET_EPOCH=$(date -u -d "2026-10-14T07:25:00Z" +%s)
NOW=$(date +%s)
if [ "$TARGET_EPOCH" -le "$NOW" ]; then
    echo "fenêtre déjà ouverte, lancement immédiat"
else
    SLEEP=$((TARGET_EPOCH - NOW))
    echo "dodo ${SLEEP}s (jusqu'à 07:25 UTC) puis défis maia1/5/9"
    sleep "$SLEEP"
fi
exec /workspaces/probable-giggle/lichessbot/venv/bin/python /workspaces/probable-giggle/lichessbot/defi_bots.py 300 3 maia1 maia5 maia9