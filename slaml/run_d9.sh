#!/bin/sh
# L6 grand slam: startpos d9, 8 threads, T3 path (perft_parallel via sweep).
cd /workspaces/probable-giggle
echo "=== L6 grand slam startpos d9 (sweep 8 threads, T3 path) ===" > slaml/d9.log
echo "start: $(date -Is), host: $(uname -n), cores: $(nproc)" >> slaml/d9.log
START=$(date +%s)
./target/release/mychess sweep "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1" 9 "8" --reps 1 >> slaml/d9.log 2>&1
RC=$?
END=$(date +%s)
echo "end: $(date -Is), exit=$RC, elapsed=$((END-START))s" >> slaml/d9.log
