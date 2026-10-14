#!/usr/bin/env python3
"""Défis séquentiels vers des bots lichess (un seul jeu à la fois — la
concurrency du bridge est 1). Le bridge lichess-bot DÉJÀ EN COURS joue les
parties (même token) ; ce script ne fait que créer les défis et attendre.
usage: defi_bots.py <tc_sec> <inc_sec> [bot1 bot2 ...]
"""
import sys, time, re, requests

CONFIG = "/workspaces/probable-giggle/lichessbot/config.yml"
token = re.search(r'token: "([^"]+)"', open(CONFIG).read()).group(1)
S = requests.Session()
S.headers["Authorization"] = f"Bearer {token}"

limit = int(sys.argv[1]) if len(sys.argv) > 1 else 300
inc = int(sys.argv[2]) if len(sys.argv) > 2 else 3
bots = sys.argv[3:] or ["maia1", "maia5", "maia9"]

def playing():
    for _ in range(4):
        try:
            r = S.get("https://lichess.org/api/account/playing", timeout=15)
            return len(r.json().get("nowPlaying", []))
        except requests.RequestException as e:
            print(f"   (réseau: {type(e).__name__}, retry)", flush=True)
            time.sleep(6)
    return -1  # inconnu; on considère "pas de partie" plutôt que crasher

def start_game(opp):
    """POST /api/challenge/{opp} : gère les DEUX shapes de réponse API."""
    last = None
    for _ in range(3):
        try:
            r = S.post(
                f"https://lichess.org/api/challenge/{opp}",
                data={"rated": "true", "clock.limit": limit, "clock.increment": inc,
                      "variant": "standard"},
                timeout=20,
            )
            body = r.json()
            ch = body.get("challenge") or body  # {"challenge":{...}} ou {...} direct
            return ch.get("status"), ch.get("id"), body.get("error")
        except requests.RequestException as e:
            last = e
            print(f"   (réseau POST: {type(e).__name__}, retry)", flush=True)
            time.sleep(6)
    return None, None, f"connexion échouée après retries: {type(last).__name__}"

for opp in bots:
    print(f"== défi -> {opp} (rated {limit//60}{limit%60:+02d}+{inc}, standard) ==", flush=True)
    try:
        st, cid, err = start_game(opp)
        print(f"   statut={st} id={cid or '-'} err={err or '-'}", flush=True)
        if st not in ("created", "accepted"):
            continue
    except Exception as e:
        print(f"   ERREUR POST: {e}", flush=True)
        continue
    # attends le début (acceptation / refus / quota ; ~3 min max)
    t0, started = time.time(), False
    while time.time() - t0 < 180:
        n = playing()
        if n > 0:
            started = True
            break
        if n < 0:
            print("   API injoignable — cible suivante", flush=True)
            break
        time.sleep(10)
    if not started:
        print("   pas de partie démarrée en 3 min — cible suivante", flush=True)
        continue
    print(f"   PARTIE! {opp} joue ({cid})", flush=True)
    deadline = time.time() + 60 * 60
    while time.time() < deadline:
        n = playing()
        if n == 0:
            break
        time.sleep(15)  # -1 = API injoignable : on continue d'attendre
    print(f"   partie {opp} terminée", flush=True)
    time.sleep(5)
print("== séquence terminée ==", flush=True)