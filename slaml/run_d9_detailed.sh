#!/bin/sh
# GATE 2.5 M4 (fond de tâche, non bloquant): startpos d9 --detailed,
# memo OFF — derniere ligne CPW non reclamee: la table detaillee d9.
# Binaire = tag vcore-1.0 (core frozen). Chaine de preuve identique a d9:
# log du chemin actif archive dans le meme log.
cd /workspaces/probable-giggle
echo "=== GATE 2.5 M4: startpos d9 --detailed (memo off, T1 table columns) ===" > slaml/d9_detailed.log
echo "start: $(date -Is), host: $(uname -n), cores: $(nproc)" >> slaml/d9_detailed.log
# L'invocation imprime elle-meme "[slider path: ...]" sur stderr (main.rs M0)
# -> chaine de preuve du chemin actif archivee dans le meme log.
./target/release/mychess perft "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1" 9 --detailed >> slaml/d9_detailed.log 2>&1
RC=$?
echo "end: $(date -Is), exit=$RC" >> slaml/d9_detailed.log