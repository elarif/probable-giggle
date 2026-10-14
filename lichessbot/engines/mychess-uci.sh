#!/bin/sh
# wrapper lichess-bot : mychess est un binaire multi-subcommand, il faut la
# sous-commande uci. Pointe sur le binaire release du workspace (A1 HEAD).
exec /workspaces/probable-giggle/target/release/mychess uci "$@"
