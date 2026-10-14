#!/bin/sh
# Setup de la chaîne lichess-bot pour portable-giggle — reconstruit tout ce
# qui est volatile (venv, clone lichess-bot, config rendue) à partir des
# artefacts persistants de CE dossier. À relancer après chaque redémarrage
# du codespace (le /tmp est essuyé, le workspace survit).
set -e
cd /workspaces/probable-giggle/lichessbot

# 0. venv (full deps: sert aux deux — scripts légers ET bridge)
if [ ! -x venv/bin/python ]; then
    python3 -m venv venv
    if [ -f lichess-bot/requirements.txt ]; then
        venv/bin/pip install -q -r lichess-bot/requirements.txt
    else
        venv/bin/pip install -q requests chess
    fi
    echo "venv OK"
fi

# 1. clone lichess-bot si absent
if [ ! -f lichess-bot/lichess-bot.py ]; then
    git clone -q --depth 1 https://github.com/lichess-bot-devs/lichess-bot.git
    cp engines/mychess-uci.sh lichess-bot/engines/ 
    chmod +x lichess-bot/engines/mychess-uci.sh
    echo "clone lichess-bot OK"
fi

# 2. config rendue à partir de config.yml.default + token du config.yml local
venv/bin/python - <<'PYEOF'
import re, os
cfg_dir = "/workspaces/probable-giggle/lichessbot"
head = open(f"{cfg_dir}/config.yml").read().splitlines()[0]
tok = re.search(r'token: "([^"]+)"', head)
if not tok or "PUT_YOUR" in tok.group(1):
    print("token manquant — génère un nouveau token bot:play:")
    print("  https://lichess.org/account/oauth/token/create?scopes=bot:play&description=probable-giggle")
    print(f"  puis ligne 1 de {cfg_dir}/config.yml -> token: \"lip_...\"")
    raise SystemExit(0)
base = open(f"{cfg_dir}/config.yml.default").read()
s = base.replace('token: "xxxxxxxxxxxxxxxxxxxxxx"', f'token: "{tok.group(1)}"')
s = s.replace('name: "engine_name"'
              '              # Binary name of the engine to use.',
              'name: "mychess-uci.sh"           # wrapper -> workspace release mychess uci')
s = s.replace("  ponder: true                     # Think on opponent's time.",
              "  ponder: false                    # mychess v1 has no ponder support")
s = re.sub(r'  uci_options:.*?\n\n', '  uci_options: {}\n\n', s, flags=re.S)
s = s.replace('    offer_draw_enabled: true       # Whether or not the bot should offer/accept draw.',
              '    offer_draw_enabled: false      # engine terminality claims its own draws')
s = s.replace('# pgn_directory: "game_records"', 'pgn_directory: "game_records"')
out = f"{cfg_dir}/lichess-bot/config.yml"
open(out, 'w').write(s)
print("bridge config OK ->", out)
PYEOF

# 3. bridge en fond (s'il n'y est pas déjà)
if ! pgrep -f "lichess-bot[.]py" >/dev/null; then
    (cd lichess-bot && nohup ../venv/bin/python lichess-bot.py --config config.yml >> /workspaces/probable-giggle/lichessbot/bridge.log 2>&1 &)
    echo "bridge lancé (fond) -> lichessbot/bridge.log"
else
    echo "bridge déjà en route"
fi
echo "defis: venv/bin/python defi_bots.py <tc> <inc> [bots...] | defi_serie.py <bot> <n> <tc> <inc> [casual]"