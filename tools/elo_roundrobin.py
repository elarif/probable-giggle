#!/usr/bin/env python3
"""Analyse d'un tournoi round-robin fastchess (estimation d'Elo + signaux d'alarme).

Usage :
    tools/elo_roundrobin.py <journal fastchess> [--nominal sf1425=1425 ...]

Le journal est celui produit par `fastchess -tournament roundrobin` (les
lignes « Finished game N (A vs B): resultat » sont la source de vérité).

Sortie :
  1. la **matrice des scores** entre moteurs (score du premier vu du
     premier) ;
  2. les **contrôles** : les paires *déclarées* identiques (`--control A=B`,
     même binaire ou même réglage) doivent être à 50 %. C'est le détecteur
     de signaux d'alarme : un écart à 50 % signale un environnement de
     mesure fautif (adjudication qui se déclenche, options appliquées au
     mauvais moteur, corruption entre instances). Seules les paires
     déclarées sont des contrôles : entre deux forces *différentes*, un
     écart à 50 % est le résultat attendu, pas une panne ;
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
    r"(?P<res>1-0|0-1|1/2-1/2)(?:\s*\{(?P<reason>[^}]*)\})?"
)
RES_SCORE = {"1-0": 1.0, "0-1": 0.0, "1/2-1/2": 0.5}


def parse(path, exclude_time_loss=False):
    games = defaultdict(float)
    counts = defaultdict(int)
    names = []
    seen = set()
    time_losses = []
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            m = RESULT.match(line.strip())
            if not m:
                continue
            a, b, res = m.group("a"), m.group("b"), m.group("res")
            reason = m.group("reason") or ""
            if res not in RES_SCORE:
                continue
            # Une partie perdue **sur le temps** n'est pas un résultat de
            # jeu : c'est un défaut de gestion du temps du moteur. Elle est
            # comptée à part (`--exclude-time-loss` permet de l'écarter de
            # la matrice).
            if "loses on time" in reason or "flags" in reason:
                time_losses.append((a, b, res + " {" + reason + "}"))
                if exclude_time_loss:
                    continue
            s = RES_SCORE[res]
            for n in (a, b):
                if n not in seen:
                    seen.add(n)
                    names.append(n)
            games[(a, b)] += s
            games[(b, a)] += 1.0 - s
            counts[(a, b)] += 1
            counts[(b, a)] += 1
    return games, counts, names, time_losses


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
    ap.add_argument(
        "--control",
        action="append",
        default=[],
        metavar="A=B",
        help="paire de moteurs de force identique (meme binaire ou meme "
             "reglage) : doit etre a 50 %% ; c'est le seul test valide "
             "de l'environnement de mesure (repetable)",
    )
    ap.add_argument(
        "--exclude-time-loss",
        action="store_true",
        help="ecarter de la matrice les parties decidees par depassement "
             "de temps (elles sont listees separement) : un depassement "
             "est un defaut de gestion du temps, pas un resultat de jeu",
    )
    args = ap.parse_args()

    games, counts, names, time_losses = parse(args.log, args.exclude_time_loss)
    if not games:
        sys.exit("aucune partie lisible dans le journal")

    # --- 0. parties perdues sur le temps --------------------------------
    if time_losses:
        print(f"## Parties décidées par dépassement de temps ({len(time_losses)})")
        print("Un dépassement est un **défaut de gestion du temps**, pas un "
              "résultat de jeu.")
        for a, b, res in time_losses:
            print(f"| {a} vs {b} | {res} |")
        if args.exclude_time_loss:
            print()
            print("# ces parties sont **écartées** de la matrice "
                  "(`--exclude-time-loss`).")
        else:
            print()
            print("# elles sont **comptées** comme des défaites "
                  "(`--exclude-time-loss` pour les écarter).")
        print()

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
    #
    # Un « contrôle » n'est **que** la paire explicitement déclarée comme
    # telle (`--control A=B`) : deux occurrences du **même** binaire, ou
    # le **même** réglage. Toutes les autres paires sont des duels
    # entre forces *différentes* : un écart à 50 % n'y est pas un défaut
    # de mesure, c'est le résultat attendu. Les confondre déclencherait un
    # signal d'alarme sur chaque paire exacerbate (sf1320 vs sf3190 à
    # 0 % n'est pas une panne), et le contrôle — qui est la seule chose
    # qui valide l'environnement — perdrait sa valeur.
    print()
    print("## Contrôles (paires déclarées identiques : doivent être à 50 %)")
    declared = []
    for item in args.control:
        a, b = item.split("=", 1)
        declared.append((a, b))
    if not declared:
        print("⚠️ **aucun contrôle déclaré** (`--control A=B`) : l'environnement "
              "de mesure n'est pas validé, les scores ne sont pas "
              "interprétables.")
    alarms = []
    for a, b in declared:
        n = counts.get((a, b), 0)
        if not n:
            print(f"| {a} vs {b} | 0 | — | paire absente du journal |")
            continue
        s = games[(a, b)] / n
        se = math.sqrt(0.25 / n)
        z = abs(s - 0.5) / se if se else 0.0
        print(f"| {a} vs {b} | {n} | {s*100:.1f} % | {z:.1f} σ |")
        if z > 3.0:
            alarms.append((z, a, b, s, n))
    for z, a, b, s, n in alarms:
        print()
        print(f"⚠️ **signal d'alarme** : la paire `{a}` / `{b}` "
              f"est à {s*100:.1f} % ({z:.1f} σ de 50 %) alors que "
              "les deux moteurs sont identiques. L'environnement de mesure "
              "n'est pas fiable : il faut corriger avant de lire un Elo.")
    if declared and not alarms:
        print()
        print("# tous les contrôles sont à 50 % : l'environnement de mesure "
              "est sain.")

    # Les écarts de **toutes** les paires, à titre d'information : ils
    # disent quels duels sont les plus déséquilibrés, pas si la mesure
    # est fautive.
    allpairs = []
    for a in names:
        for b in names:
            if a < b and counts[(a, b)]:
                s = games[(a, b)] / counts[(a, b)]
                n = counts[(a, b)]
                se = math.sqrt(0.25 / n) if n else 0.0
                allpairs.append((abs(s - 0.5) / se if se else 0.0, a, b, s, n))
    print()
    print("## Écarts sur toutes les paires (information : un écart à 50 % "
          "est *attendu* entre forces différentes)")
    for z, a, b, s, n in sorted(allpairs, reverse=True)[:15]:
        mark = " *(contrôle)*" if (a, b) in declared or (b, a) in declared else ""
        print(f"| {a} vs {b} | {n} | {s*100:.1f} % | {z:.1f} σ |{mark}")

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

            # La relation « Elo du champ » -> « Elo nominal » n'est pas
            # nécessairement **linéaire** : le handicap de Stockfish ne
            # réduit pas la force d'une façon linéaire en `UCI_Elo` (la
            # relation est convexe : 1320 -> 1900 rend plus de Elo par
            # point que 1900 -> 3190). Si les ancrages s'écartent
            # fortement de la droite ajustée, la droite n'est pas
            # exploitable et l'estimateur **local** (interpolation entre
            # les deux ancrages qui encadrent le moteur) est le seul
            # défendable.
            residuals = []
            for name, t in theta.items():
                if name in nominal:
                    residuals.append(abs((inter + slope * t) - nominal[name]))
            if residuals:
                worst = max(residuals)
                if worst > 100:
                    print()
                    print(f"⚠️ **l'ajustement global est inexploitable** : les "
                          f"ancrages s'écartent de la droite ajustée de "
                          f"{worst:.0f} Elo au plus (la relation n'est pas "
                          "linéaire). L'estimateur local ci-dessous est le "
                          "seul retenu.")

        # --- 5. l'échelle d'ancrage est-elle une échelle de force ? -----
        #
        # C'est le test qui a annulé le « 1417 Elo » : l'échelle
        # `UCI_Elo` de Stockfish n'est une **échelle de force** que si
        # elle est *monotone*, c'est-à-dire si le moteur nominally le
        # plus fort bat toujours le moteur nominally le plus faible.
        # Une seule inversion suffit à rendre le ramenage (§4) caduc :
        # la droite d'ajustement n'a alors plus d'interprétation.
        print()
        print("## Monotonie de l'échelle d'ancrage (le plus fort doit gagner)")
        anchor = [n for n in nominal if n in theta]
        inversions = []
        ties = []
        checked = 0
        for hi in anchor:
            for lo in anchor:
                if nominal[hi] <= nominal[lo]:
                    continue
                n = counts.get((hi, lo), 0)
                if not n:
                    continue
                checked += 1
                s = games[(hi, lo)] / n
                se = math.sqrt(0.25 / n)
                if s < 0.5 - 1.96 * se:
                    inversions.append((hi, lo, s, n))
                elif s <= 0.5:
                    # nulité observée : le nominally plus fort ne gagne pas
                    # clairement. C'est un signal d'alerte *moindre* qu'une
                    # inversion, mais ce n'est pas la preuve d'une échelle
                    # de force — on le signale à part.
                    ties.append((hi, lo, s, n))
        if checked == 0:
            print("⚠️ **aucune paire d'ancrage jouée** : l'échelle nominale "
                  "n'est pas testable sur ce tournoi.")
        elif inversions:
            print(f"⚠️ **échelle NON monotone** : {len(inversions)} inversion(s) "
                  f"significative(s) sur {checked} paires testées — le "
                  "ramenage (§4) n'a **pas** d'interprétation (l'ancre n'est "
                  "pas une échelle de force dans ce protocole).")
            for hi, lo, s, n in inversions:
                print(f"| {hi} ({nominal[hi]:.0f}) vs {lo} ({nominal[lo]:.0f}) | "
                      f"{n} | {s*100:.1f} % | **inversion** |")
        else:
            print(f"# **aucune inversion significative** sur les {checked} "
                  "paires testées : l'échelle se comporte comme une échelle "
                  "de force dans ce protocole.")
        if ties:
            print()
            print(f"### Nulités observées ({len(ties)}) — le nominally plus "
                  "fort ne gagne pas clairement")
            for hi, lo, s, n in ties:
                print(f"| {hi} ({nominal[hi]:.0f}) vs {lo} ({nominal[lo]:.0f}) | "
                      f"{n} | {s*100:.1f} % |")

    # --- 6. estimateur local : interpolation entre les ancrages encadrants
    #
    # Un moteur se place entre les **deux** ancrages qui l'encadrent dans
    # l'ordre du champ ; on interpole alors le nominal entre ces deux
    # points. C'est l'analogue de l'estimateur local de
    # `tools/elo_estimate.sh` : il n'utilise que les points qui
    # *portent* l'information (ceux qui encadrent la position du moteur),
    # là où un ajustement global dilue l'information dans les extrémités
    # de l'échelle.
    if nominal and len(pts) >= 2:
        ordered = sorted(((theta[n], nominal[n], n) for n in nominal
                          if n in theta))
        print()
        print("## Estimateur local (interpolation entre les deux ancrages "
              "encadrants)")
        for name, t in sorted(theta.items(), key=lambda kv: -kv[1]):
            if name in nominal:
                continue
            below = [p for p in ordered if p[0] <= t]
            above = [p for p in ordered if p[0] > t]
            if not below or not above:
                # hors de l'échelle : extrapolation depuis les deux
                # ancrages les plus proches, signalée comme telle.
                near = sorted(ordered, key=lambda p: abs(p[0] - t))[:2]
                if len(near) < 2:
                    continue
                (f1, n1, a1), (f2, n2, a2) = sorted(near)
                if f2 == f1:
                    continue
                est = n1 + (t - f1) * (n2 - n1) / (f2 - f1)
                print(f"| **{name}** | champ {t:+.0f} | ≈ **{est:.0f}** | "
                      f"extrapolation hors échelle ({a1}, {a2}) |")
                continue
            f1, n1, a1 = below[-1]
            f2, n2, a2 = above[0]
            if f2 == f1:
                continue
            est = n1 + (t - f1) * (n2 - n1) / (f2 - f1)
            print(f"| **{name}** | champ {t:+.0f} | ≈ **{est:.0f}** | "
                  f"entre {a1} ({n1:.0f}) et {a2} ({n2:.0f}) |")


if __name__ == "__main__":
    main()
