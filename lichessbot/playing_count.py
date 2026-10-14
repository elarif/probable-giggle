#!/usr/bin/env python3
"""Nombre de parties en cours de portable-giggle (poll léger)."""
import re, requests
S = requests.Session()
S.headers["Authorization"] = "Bearer " + re.search(
    r'token: "([^"]+)"',
    open("/workspaces/probable-giggle/lichessbot/config.yml").read()).group(1)
try:
    np = S.get("https://lichess.org/api/account/playing", timeout=15).json().get("nowPlaying", [])
    print(len(np))
except Exception:
    print(-1)