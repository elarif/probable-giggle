#!/usr/bin/env python3
"""Compte les parties adjugées sur dépassement de temps, par moteur.

Usage : count_time_losses.py <log fastchess> [cand_name] [base_name]

Le log fastchess annonce les fins de partie comme
``Finished game 12 (cand vs base): 0-1 {White loses on time (40ms overrun)}``.
Le camp qui « perd sur le temps » est celui qui a dépassé ; selon la
parenthèse (cand vs base) ou (base vs cand), on sait de quel moteur il
s'agit.
"""
import re
import sys

LINE = re.compile(
    r"Finished game \d+ \((?P<order>[^)]*)\): (?P<result>[^ ]+) \{(?P<reason>[^}]*)\}"
)
OVERRUN = re.compile(r"loses on time")


def main() -> int:
    path = sys.argv[1]
    cand = sys.argv[2] if len(sys.argv) > 2 else "cand"
    base = sys.argv[3] if len(sys.argv) > 3 else "base"
    on_time = {cand: 0, base: 0}
    total = 0
    reasons = {}
    with open(path, encoding="utf-8", errors="replace") as fh:
        for line in fh:
            m = LINE.search(line)
            if not m:
                continue
            order = m.group("order")
            result = m.group("result")
            reason = m.group("reason").strip()
            total += 1
            # le camp qui perd : "0-1" -> les blancs ; "1-0" -> les noirs
            white_lost = result.startswith("0-1")
            if "cand vs" in order:
                white_is, black_is = cand, base
            else:
                white_is, black_is = base, cand
            loser = white_is if white_lost else black_is
            if OVERRUN.search(reason):
                on_time[loser] += 1
                key = (loser, reason)
                reasons[key] = reasons.get(key, 0) + 1
    print(f"log        : {path}")
    print(f"parties    : {total}")
    for who in (cand, base):
        print(f"  {who:<5} : {on_time[who]:>4} partie(s) adjugée(s) sur dépassement de temps")
    for (who, reason), n in sorted(reasons.items(), key=lambda kv: -kv[1])[:10]:
        print(f"    {who:<5} {n:>4} x {reason}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
