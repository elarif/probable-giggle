#!/bin/sh
# GATE 2.5 M4 relance (parallèle root-split détaillé) : startpos d9 --detailed.
# Le runner examples/d9detailed.rs consomme le core FROZEN via API publique
# (perft_detailed + rayon), équivalent ensembliste au mono-thread (validé
# d4/d5 par==serial + colonnes dorées T1 avant lancement).
cd /workspaces/probable-giggle
echo "=== GATE 2.5 M4-par: startpos d9 --detailed (root-split, memo off) ===" > slaml/d9_detailed.log
echo "start: $(date -Is), host: $(uname -n), cores: $(nproc)" >> slaml/d9_detailed.log
./target/release/examples/d9detailed "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1" 9 8 2 >> slaml/d9_detailed.log 2>&1
RC=$?
echo "end: $(date -Is), exit=$RC" >> slaml/d9_detailed.log