#!/bin/sh
# GATE 2.5 M4-par (relance checkpoint-resumable): startpos d9 --detailed.
# Deux runs antérieurs tués par redémarrage du workspace sans progression
# persistée (slaml/d9_detailed.log.killed-20261013T0816, -20261013T1812).
# Cette relance sharding le root comme le runner d9detailed validé (split=2
# → sous-arbres d7 via perft_detailed, même fold) MAIS checkpointe chaque
# sous-arbre terminé dans slaml/d9_shards.ckpt : un restart relance = resume
# (shard==serial validé d5/d6/d7 + kill/resume testé avant lancement).
cd /workspaces/probable-giggle
echo "=== GATE 2.5 M4-par: startpos d9 --detailed (root-split, memo off, checkpointed) ===" > slaml/d9_detailed.log
echo "start: $(date -Is), host: $(uname -n), cores: $(nproc)" >> slaml/d9_detailed.log
./target/release/examples/d9detailed_shard "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1" 9 8 2 slaml/d9_shards.ckpt >> slaml/d9_detailed.log 2>&1
RC=$?
echo "end: $(date -Is), exit=$RC" >> slaml/d9_detailed.log