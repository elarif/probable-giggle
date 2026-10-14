#!/usr/bin/env python3
"""Série de défis vers UN bot lichess (un seul jeu à la fois — concurrency
du bridge = 1). Crée -> attend démarrage -> attend fin -> compteur W/L/D ->
recommence. Backoff sur 429, abort sur quota-d'épuisé/refus.
usage: defi_serie.py <bot> <nb_games> <tc_sec> <inc_sec> [rated|casual]
défaut: defi_serie.py lc0-latest 100 60 0 casual
"""
import sys, time, re, requests

CONFIG = "/workspaces/probable-giggle/lichessbot/config.yml"
token = re.search(r'token: "([^"]+)"', open(CONFIG).read()).group(1)
S = requests.Session()
S.headers["Authorization"] = f"Bearer {token}"

opp = sys.argv[1] if len(sys.argv) > 1 else "lc0-latest"
N = int(sys.argv[2]) if len(sys.argv) > 2 else 100
limit = int(sys.argv[3]) if len(sys.argv) > 3 else 60
inc = int(sys.argv[4]) if len(sys.argv) > 4 else 0
rated = len(sys.argv) <= 5 or sys.argv[5] != "casual"

W = L = D = 0

def api(fn, tries=5):
    for k in range(tries):
        try:
            return fn()
        except requests.RequestException as e:
            print(f"  (réseau {type(e).__name__}, retry {k+1})")
            time.sleep(8)
    return None

def playing():
    r = api(lambda: S.get("https://lichess.org/api/account/playing", timeout=15))
    if r is None:
        return -1
    return len(r.json().get("nowPlaying", []))

def create():
    try:
        r = S.post(
            f"https://lichess.org/api/challenge/{opp}",
            data={"rated": str(rated).lower(), "clock.limit": limit,
                  "clock.increment": inc, "variant": "standard"},
            timeout=20,
        )
    except requests.RequestException as e:
        return "net", None, type(e).__name__
    if r.status_code == 429:
        return "ratelimit", None, r.headers.get("X-RateLimit-Remaining", "?")
    try:
        body = r.json()
    except Exception:
        return "http", None, r.status_code
    err = body.get("error")
    if err:
        return "quota" if "played 100" in err else "refus", None, err
    ch = body.get("challenge") or body
    return ch.get("status"), ch.get("id"), err

def tally():
    """résultat de la dernière partie du bot (PGN)."""
    r = api(lambda: S.get(
        "https://lichess.org/api/games/user/portable-giggle?max=1", timeout=15))
    if r is None:
        return None
    m = re.search(r'\[White "([^"]+)".*\[Result "([^"]+)"', r.text, re.S)
    if not m:
        return None
    white, res = m.group(1), m.group(2)
    if res == "1/2-1/2":
        return "D"
    we_white = white == "portable-giggle"
    win = (res == "1-0") == we_white
    return "W" if win else "L"

print(f"== série vs {opp} : {N} parties, {'rated' if rated else 'casual'} {limit//60}+{inc}, 1 à la fois", flush=True)

aborted = 0
for g in range(1, N + 1):
    st, cid, err = create()
    if st == "quota" or st == "refus":
        print(f"   ABORT partie {g}: {err}", flush=True)
        break
    if st == "ratelimit":
        print(f"   429 ({err}) — pause 60s, retry", flush=True)
        time.sleep(60)
        continue
    if st not in ("created", "accepted"):
        print(f"   partie {g}: statut={st} err={err} — pause 30s", flush=True)
        aborted += 1
        if aborted >= 3:
            print("   trop d'échecs de création — stop", flush=True)
            break
        time.sleep(30)
        continue
    aborted = 0
    t0, started = time.time(), False
    while time.time() - t0 < 120:
        n = playing()
        if n > 0:
            started = True
            break
        if n < 0:
            time.sleep(10)
            continue
        time.sleep(8)
    if not started:
        print(f"   partie {g} ({cid}): jamais acceptée — continue", flush=True)
        continue
    print(f"   PARTIE {g}/{N}! {opp} ({cid})", flush=True)
    deadline = time.time() + 90 * 60
    while time.time() < deadline:
        n = playing()
        if n == 0:
            break  # plus aucune partie en cours
        time.sleep(12)
    res = tally()
    if res == "W":
        W += 1
    elif res == "L":
        L += 1
    elif res == "D":
        D += 1
    print(f"   résultat {g}/{N} {cid}: {res}  (W-L-D {W}-{L}-{D})", flush=True)
    time.sleep(12)  # pacing anti rate-limit création

print(f"== série vs {opp} terminée : W-L-D {W}-{L}-{D}", flush=True)