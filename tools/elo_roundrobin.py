#!/usr/bin/env python3
"""Analyse d'un tournoi round-robin fastchess (estimation d'Elo + signaux d'alarme).

Usage :
    tools/elo_roundrobin.py <journal fastchess> [--nominal sf1425=1425 ...]

Le journal est celui produit par `fastchess -tournament roundrobin` (les
lignes « Finished game N (A vs B): resultat » sont la source de vérité).

Sortie :
  1. la **matrice des scores** entre moteurs (score du premier vu du
     premier) ;
  2. les **contrôles** : toute paire de moteurs *de force identique*
     (le même binaire, ou le même réglage) doit être à 50 %. C'est le
     détecteur de signaux d'alarme : un écart à 50 % signale un
     environnement de mesure fautif (adjudication qui se déclenche, options
     appliquées au mauvais moteur, corruption entre instances) ;
  3. l'**ajustement de Bradley-Terry** (Elo par moteur) et la **cohérence**
     du tournoi : les résidus par paire doivent être petits — c'est le
     test de transitivité (une matrice de scores incohérente ne donne pas
     un classement, elle donne du bruit) ;
  4. le **ramenage vers l'échelle nominale** : les moteurs d'ancrage
     (Stockfish à `UCI_Elo` connu) permettent de convertir l'Elo relatif
     du champ en Elo absolu.
"""

import argparse
import math
import re
import sys
from collections import defaultdict

RESULT = re.compile(
    r"^Finished game \d+ \((?P<a>[^)]+?) vs (?P<b>[^)]+?)\): "
    r"(?P<res>1-0|0-1|1/2-1/2|0-1 \{.*)"
)
RES_SCORE = {"1-0": 1.0, "0-1": 0.0, "1/2-1/2": 0.5}


def parse(path):
    games = defaultdict(float)
    counts = defaultdict(int)
    names = []
    seen = set()
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            m = RESULT.match(line.strip())
            if not m:
                continue
            a, b, res = m.group("a"), m.group("b"), m.group("res")
            key = res[:7] if res[:7] in RES_SCORE else res[:3]
            if key not in RES_SCORE:
                # « 1-0 {White mates} » et « 0-1 {…} » : on prend le prefixe
                key = res[:3] if res[:3] in ("1-0", "0-1") else res[:7]
            if key not in RES_SCORE:
                continue
            s = RES_SCORE[key]
            for n in (a, b):
                if n not in seen:
                    seen.add(n)
                    names.append(n)
            games[(a, b)] += s
            games[(b, a)] += 1.0 - s
            counts[(a, b)] += 1
            counts[(b, a)] += 1
    return games, counts, names


def bradley_terry(pairs, iters=500):
    """Elo (centipions) par moteur, ajustement MLE de Bradley-Terry."""
    names = sorted({n for a, b in pairs for n in (a, b)})
    idx = {n: i for i, n in enumerate(names)}
    theta = {n: 0.0 for n in names}
    for _ in range(iters):
        grad = {n: 0.0 for n in names}
        for (a, b), s in pairs.items():
            p = 1.0 / (1.0 + math.exp(-(theta[a] - theta[b]) / 400.0))
            grad[a] += s - p
            grad[b] += p - s
        step = 1.0
        for n in names:
            theta[n] += step * 400.0 * grad[n] / max(len(pairs), 1)
        m = sum(theta.values()) / len(theta)
        for n in names:
            theta[n] -= m
    return theta


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("log")
    ap.add_argument(
        "--nominal",
        action="append",
        default=[],
        metavar="moteur=Elo",
        help="Elo nominal connu d'un moteur d'ancrage (repetable)",
    )
    args = ap.parse_args()

    games, counts, names = parse(args.log)
    if not games:
        sys.exit("aucune partie lisible dans le journal")

    # --- 1. matrice des scores ------------------------------------------
    print("## Matrice des scores (ligne = moteur, colonne = adversaire)")
    print("| | " + " | ".join(names) + " |")
    print("|---|" + "---|" * len(names))
    for a in names:
        cells = []
        for b in names:
            if a == b:
                cells.append("—")
            else:
                n = counts[(a, b)]
                cells.append(f"{games[(a, b)] / n:.2f} ({n})" if n else "—")
        print(f"| **{a}** | " + " | ".join(cells) + " |")

    # --- 2. contrôles : paires de force identique -----------------------
    print()
    print("## Contrôles (paires de force identique : doivent être à 50 %)")
    ctrl = []
    for a in names:
        for b in names:
            if a < b and counts[(a, b)]:
                s = games[(a, b)] / counts[(a, b)]
                n = counts[(a, b)]
                se = math.sqrt(0.25 / n) if n else 0.0
                ctrl.append((abs(s - 0.5) / se if se else 0.0, a, b, s, n))
    for z, a, b, s, n in sorted(ctrl, reverse=True):
        print(f"| {a} vs {b} | {n} | {s*100:.1f} % | {z:.1f} σ |")
    worst = max(ctrl) if ctrl else (0, "", "", 0.5, 0)
    if worst[0] > 3.0:
        print()
        print(f"⚠️ **signal d'alarme** : la paire `{worst[1]}` / `{worst[2]}` "
              f"est à {worst[3]*100:.1f} % ({worst[0]:.1f} σ de 50 %) alors que "
              "les deux moteurs sont identiques. L'environnement de mesure "
              "n'est pas fiable : il faut corriger avant de lire un Elo.")

    # --- 3. ajustement + cohérence --------------------------------------
    pairs = {k: v / counts[k] for k, v in games.items()}
    theta = bradley_terry(pairs)
    print()
    print("## Ajustement de Bradley-Terry (Elo relatif au champ)")
    for n, t in sorted(theta.items(), key=lambda kv: -kv[1]):
        print(f"| {n} | {t:+.0f} |")
    print()
    print("## Cohérence (résidus par paire : |attendu − observé| en Elo)")
    resid = []
    for (a, b), s in pairs.items():
        if a < b:
            p = 1.0 / (1.0 + math.exp(-(theta[a] - theta[b]) / 400.0))
            resid.append((abs(p - s) * 400.0, a, b, s, p))
    for r, a, b, s, p in sorted(resid, reverse=True):
        print(f"| {a} vs {b} | observé {s*100:.1f} % | modèle {p*100:.1f} % "
              f"| résidu {r:+.0f} Elo |")
    worst_r = max(resid) if resid else (0, "", "", 0, 0)
    print()
    print(f"# résidu maximal : {worst_r[0]:.0f} Elo (paire {worst_r[1]} / {worst_r[2]})")

    # --- 4. ramenage vers l'échelle nominale ----------------------------
    nominal = {}
    for item in args.nominal:
        name, val = item.rsplit("=", 1)
        nominal[name] = float(val)
    if nominal:
        pts = [(nominal[n], theta[n]) for n in nominal if n in theta]
        if len(pts) >= 2:
            n_ = len(pts)
            mx = sum(p[0] for p in pts) / n_
            my = sum(p[1] for p in pts) / n_
            sxx = sum((p[0] - mx) ** 2 for p in pts)
            sxy = sum((p[0] - mx) * (p[1] - my) for p in pts)
            slope = sxy / sxx if sxx else 0.0
            inter = my - slope * mx
            print()
            print(f"# rameneage : Elo nominal = {inter:+.0f} {slope:+.3f} x "
                  "Elo relatif au champ")
            for name, t in sorted(theta.items(), key=lambda kv: -kv[1]):
                if name in nominal:
                    print(f"| {name} | nominal {nominal[name]:.0f} | "
                          f"champ {t:+.0f} |")
                else:
                    est = (t - inter) / slope if slope else float("nan")
                    print(f"| **{name}** | **≈ {est:.0f}** | champ {t:+.0f} |")


if __name__ == "__main__":
    main()
