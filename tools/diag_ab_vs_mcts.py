#!/usr/bin/env python3
"""Diagnostic d'un duel AB vs MCTS : où se perd la partie ?

Pour chaque partie du PGN :
  * on rejoue les coups ;
  * on signale les **coups qui perdent du matériel** (le camp qui joue
    laisse une pièce en prise) ;
  * on signale les **mates manqués en un coup** (une occasion de mat
    disponible, non jouée) ;
  * on compte les coups par camp et la longueur des parties.

Usage : tools/diag_ab_vs_mcts.py prt/rr/ab-vs-mcts.pgn
"""
import sys
import chess


def material(board: chess.Board) -> int:
    """La balance matérielle (centipions), positive pour les blancs."""
    total = 0
    for piece_type, sign in (
        (chess.PAWN, 1),
        (chess.KNIGHT, 3),
        (chess.BISHOP, 3),
        (chess.ROOK, 5),
        (chess.QUEEN, 9),
    ):
        total += sign * (
            len(board.pieces(piece_type, chess.WHITE))
            - len(board.pieces(piece_type, chess.BLACK))
        )
    return total


def has_mate_in_one(board: chess.Board):
    """Le coup de mat en un, s'il existe."""
    for move in board.legal_moves:
        board.push(move)
        if board.is_checkmate():
            board.pop()
            return move
        board.pop()
    return None


def main() -> int:
    path = sys.argv[1]
    with open(path, encoding="utf-8", errors="replace") as fh:
        text = fh.read()

    games = []
    for block in text.split("[Event ")[1:]:
        block = "[Event " + block
        headers = {}
        moves = []
        for line in block.splitlines():
            if line.startswith("[") and "]" in line:
                key, _, value = line[1:].partition("]")
                key = key.strip()
                value = value.strip().strip('"')
                headers[key] = value
            elif line and not line.startswith("["):
                moves.append(line)
        games.append((headers, moves))

    print(f"parties : {len(games)}")
    blunders = {"ab": [], "mcts": []}
    missed_mate = {"ab": [], "mcts": []}
    plies = {"ab": 0, "mcts": 0}
    results = {"ab": [0, 0, 0], "mcts": [0, 0, 0]}

    for index, (headers, move_lines) in enumerate(games, 1):
        # Le FEN est dans les en-têtes ; les coups sont en UCI.
        fen = headers.get("FEN", chess.STARTING_FEN)
        board = chess.Board(fen)
        white_is_ab = headers.get("White", "") == "ab"
        uci_moves = []
        for line in move_lines:
            for token in line.split():
                if token in ("1-0", "0-1", "1/2-1/2", "*"):
                    continue
                if token.endswith(".") and token[:-1].isdigit():
                    continue  # le numero de coup
                uci_moves.append(token)
        result = headers.get("Result", "*")
        loser = None
        if result == "1-0":
            winner = "ab" if white_is_ab else "mcts"
        elif result == "0-1":
            winner = "mcts" if white_is_ab else "ab"
        else:
            winner = None
        if winner:
            results[winner][0] += 1
            results[loser][1] += 1
        else:
            results["ab"][2] += 1
            results["mcts"][2] += 1

        for ply, uci in enumerate(uci_moves):
            mover = "ab" if board.turn == chess.WHITE == white_is_ab else None
            if board.turn == chess.WHITE:
                mover = "ab" if white_is_ab else "mcts"
            else:
                mover = "mcts" if white_is_ab else "ab"
            plies[mover] += 1

            before = material(board)
            # L'occasion de mat en un AVANT le coup ?
            mate = has_mate_in_one(board)
            try:
                move = chess.Move.from_uci(uci)
            except ValueError:
                break
            if move not in board.legal_moves:
                print(f"partie {index} : coup illégal {uci}")
                break
            board.push(move)
            after = material(board)
            delta = after - before if board.turn == chess.WHITE else before - after
            if delta <= -3:
                blunders[mover].append(
                    (index, ply, uci, delta, board.fen())
                )
            if mate is not None and uci != mate.uci():
                missed_mate[mover].append((index, ply, uci, mate.uci()))

    print(f"coups joues : ab={plies['ab']} mcts={plies['mcts']}")
    print(
        "resultats : ab {0} victoire(s) / {1} defaite(s) / {2} nulle(s) ; "
        "mcts {3} / {4} / {5}".format(
            results["ab"][0],
            results["ab"][1],
            results["ab"][2],
            results["mcts"][0],
            results["mcts"][1],
            results["mcts"][2],
        )
    )
    for camp in ("ab", "mcts"):
        print()
        print(f"== {camp} ==")
        print(f"  coups qui perdent du materiel : {len(blunders[camp])}")
        for item in blunders[camp][:12]:
            print(f"    partie {item[0]} ply {item[1]} : {item[2]} ({item[3]} pions)")
        print(f"  mates en un manques : {len(missed_mate[camp])}")
        for item in missed_mate[camp][:12]:
            print(
                f"    partie {item[0]} ply {item[1]} : a joue {item[2]}, "
                f"mat disponible en {item[3]}"
            )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
