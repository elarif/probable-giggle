#!/usr/bin/env python3
"""Croise la convention `discovery_checks` / `double_checks` de mychess avec
python-chess, sur les positions qui produisent le pattern litigieux.

Le rapport `slaml/d9_checkcols_report_20261014.md` laisse deux conventions
candidates pour l'ecart d9 (+2 037 discovery, +768 715 double) :

(a) un **double echec par prise en-passant** ;
(b) un **echec de roi-roque** (la tour qui arrive donne check).

`python-chess` n'expose pas ces compteurs, mais il expose exactement ce
qu'il faut pour trancher la *convention* : la liste des coups qui donnent
check, et le fait qu'ils soient doubles.

Ce script enumere, depuis une position, tous les coups qui donnent un check
et compte, pour chacun, le nombre d'attaquants du roi adverse apres le coup
— exactement la regle de `src/perft.rs` :

    if att.count_ones() >= 2  -> double_checks
    else if att != moved_bit   -> discovery_checks
    else                      -> direct

Il distingue ensuite les coups `ep` et `castle`, pour mesurer leur
contribution. Le resultat est compare au detail publie par CPW.

Usage:
    python3 cpw_convention.py <fen>            # une position
    python3 cpw_convention.py --startpos 6     # startpos a la profondeur N
"""

from __future__ import annotations

import sys
from collections import Counter

import chess


def classify(board: chess.Board) -> Counter:
    """Compte les classes d'echec sur les coups d'une position.

    Reproduit la regle de mychess (`src/perft.rs`) et ajoute le detail
    ep/castle, qui est ce que le rapport d9 cherche a isoler.
    """
    counts: Counter = Counter()
    for move in board.legal_moves:
        mover = board.turn
        board.push(move)
        # Apres le coup, c'est l'adversaire qui joue : le roi vulnerable est
        # celui de `mover`, et les attaquants sont ceux de `mover`.
        king = board.king(mover)
        if king is not None and board.is_check():
            attackers = board.attackers_mask(mover, king) & board.occupied_co[mover]
            attackers = chess.SquareSet(attackers)
            n = len(attackers)
            if n >= 2:
                counts["double"] += 1
                if board.is_en_passant(move):
                    counts["double_ep"] += 1
                if board.is_castling(move):
                    counts["double_castle"] += 1
            elif attackers != {move.from_square}:
                counts["discovery"] += 1
                if board.is_en_passant(move):
                    counts["discovery_ep"] += 1
                if board.is_castling(move):
                    counts["discovery_castle"] += 1
            else:
                counts["direct"] += 1
        board.pop()
    return counts


def report(fen: str) -> None:
    board = chess.Board(fen)
    c = classify(board)
    print(f"FEN: {fen}  (side to move: {board.turn})")
    print(
        f"  checks={c['direct'] + c['discovery'] + c['double']}"
        f"  direct={c['direct']}"
        f"  discovery={c['discovery']}"
        f"  double={c['double']}"
    )
    print(
        f"  ep: discovery_ep={c['discovery_ep']} double_ep={c['double_ep']}"
        f"   |  castle: discovery_castle={c['discovery_castle']}"
        f" double_castle={c['double_castle']}"
    )


def main(argv: list[str]) -> int:
    if not argv:
        print(__doc__)
        return 2
    if argv[0] == "--startpos":
        depth = int(argv[1]) if len(argv) > 1 else 3
        board = chess.Board()
        for _ in range(depth - 1):
            board.push(next(iter(board.legal_moves)))
        report(board.fen())
        return 0
    for fen in argv:
        report(fen)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
