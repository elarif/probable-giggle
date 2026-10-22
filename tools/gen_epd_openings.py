#!/usr/bin/env python3
"""Genere un book d'ouvertures EPD pour le SPRT fastchess.

Le book officiel du protocole est `uho_lichess_4852_v1.epd` (UHO), qui
n'etait pas telechargeable depuis cette machine le 2026-10-22. Ce script
fabrique un book de **remplacement** a partir du jeu de donnees ECO de
lichess (`lichess-org/chess-openings`, volumes a..e) : chaque ligne du TSV
donne un code ECO et une ligne de coups PGN en SAN ; la position est
rejouee depuis la position initiale et sortie en EPD (FEN sans les
compteurs de coups, champ "en passant" uniquement s'il est legalement
jouable — la convention EPD).

Le book obtenu est **diversifie** (≈3 900 positions d'ouvertures
standard, une par variation ECO), mais ce n'est **pas** l'UHO : le
protocole gele utilise des positions volontairement desequilibrees. Toute
mesure produite avec ce book doit le dire (cf. ELO_LOG.md).

Usage :

    python3 tools/gen_epd_openings.py <sortie.epd> [dossier_tsv]

Le dossier TSV doit contenir a.tsv .. e.tsv (telecharges depuis
https://github.com/lichess-org/chess-openings). Par defaut, le script les
telecharge dans un dossier temporaire.

Prerequis : `python3 -m pip install chess` (python-chess).
"""

import csv
import os
import re
import sys
import tempfile
import urllib.request

BASE = "https://raw.githubusercontent.com/lichess-org/chess-openings/master"
VOLUMES = "abcde"
SAN_TOKEN = re.compile(r"[^\s]+")


def fetch(src_dir: str) -> None:
    """Recupere a.tsv .. e.tsv dans `src_dir` si besoin."""
    for vol in VOLUMES:
        path = os.path.join(src_dir, f"{vol}.tsv")
        if os.path.exists(path):
            continue
        url = f"{BASE}/{vol}.tsv"
        print(f"telechargement {url}")
        with urllib.request.urlopen(url, timeout=60) as resp, open(path, "wb") as fh:
            fh.write(resp.read())


def san_moves(pgn_field: str):
    """Extrait les coups SAN d'un champ PGN (numeros de coups retires)."""
    out = []
    for tok in SAN_TOKEN.findall(pgn_field):
        tok = re.sub(r"^\d+\.+", "", tok.strip())
        tok = tok.replace("!", "").replace("?", "")
        if not tok or tok in ("*", "1-0", "0-1", "1/2-1/2"):
            continue
        out.append(tok)
    return out


def build(src_dir: str, out_path: str) -> int:
    import chess

    seen = set()
    for vol in VOLUMES:
        path = os.path.join(src_dir, f"{vol}.tsv")
        with open(path, newline="", encoding="utf-8") as fh:
            for row in csv.DictReader(fh, delimiter="\t"):
                board = chess.Board()
                try:
                    for san in san_moves(row.get("pgn") or ""):
                        board.push_san(san)
                except Exception:
                    continue
                if board.is_game_over():
                    continue
                # EPD = 4 champs ; "en passant" rempli seulement s'il est jouable.
                seen.add(" ".join(board.epd().split(" ")[:4]))
    with open(out_path, "w", encoding="utf-8") as fh:
        for epd in sorted(seen):
            fh.write(epd + "\n")
    return len(seen)


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    out_path = argv[1]
    src_dir = argv[2] if len(argv) > 2 else tempfile.mkdtemp(prefix="eco-opts-")
    fetch(src_dir)
    n = build(src_dir, out_path)
    print(f"{n} positions EPD ecrites dans {out_path}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
