#!/usr/bin/env bash
# tools/elo_estimate.sh — estimer l'Elo *approximative* d'un moteur.
#
# Principe : on ne mesure pas un Elo absolu, on mesure un Elo **relatif à
# une ancre**. L'ancre utilisée ici est **Stockfish en force limitée**
# (`UCI_LimitStrength` + `UCI_Elo`), dont l'échelle de force est calibrée
# par les développeurs de Stockfish. On cherche le niveau de Stockfish où
# le moteur candidat fait 50 % : ce niveau est son Elo, dans le protocole
# de mesure utilisé (même budget de nœuds ou même TC pour les deux).
#
# Usage :
#   tools/elo_estimate.sh <candidat> <stockfish> [options]
#
# Options :
#   --levels 1320,1450,1600     niveaux d'UCI_Elo à tester (défaut : balayage)
#   --games 50                  parties par niveau (défaut 50)
#   --nodes 40000               budget de nœuds par coup, identique pour les
#                               deux moteurs (défaut 40000) ; utiliser
#                               --tc à la place
#   --tc 10+0.1                contrôle de temps identique pour les deux
#   --book <fichier.epd>       livre d'ouvertures (défaut : le book local
#                               s'il existe)
#   --rounds N                 parties par ouverture et par couleur
#                               (défaut : games/2, arrondi)
#   --out <répertoire>         où écrire les logs (défaut : prt/elo)
#   --analyse-only             ne rejoue aucun match : ré-analyse les logs
#                               déjà présents dans --out (utile pour
#                               corriger le parseur sans perdre les parties)
#
# Sortie : un tableau des scores par niveau, puis l'interpolation de la
# cruzado à 50 % avec son intervalle de confiance (l'écart-type du score
# est propagé dans l'échelle Elo locale).
#
# Avertissement de méthode : l'échelle `UCI_Elo` de Stockfish est
# calibrée sur son propre contrôle de temps ; l'utiliser sous un budget
# de nœuds est une **approximation**. Le résultat est donc un ordre de
# grandeur, à lire avec l'intervalle affiché.
#
# Note d'invocation : Stockfish ne doit **pas** recevoir `args="uci"` —
# il interprète un argument de ligne de commande comme un fichier à
# charger, sort avant le handshake et fastchess le déclare « not
# responsive ».

set -euo pipefail

CAND=""
SF=""
LEVELS=""
GAMES=50
NODES=40000
TC=""
BOOK=""
ROUNDS=""
OUT="prt/elo"
ANALYSE_ONLY=0

usage() {
    echo "usage: $0 <candidat> <stockfish> [--levels a,b,c] [--games N]" \
         "[--nodes N | --tc T+inc] [--book f] [--rounds N] [--out dir]" >&2
    exit 2
}

while [ $# -gt 0 ]; do
    case "$1" in
        --levels) LEVELS="${2:?--levels attend une liste}"; shift 2 ;;
        --games)  GAMES="${2:?}"; shift 2 ;;
        --nodes)  NODES="${2:?}"; shift 2 ;;
        --tc)     TC="${2:?}"; shift 2 ;;
        --book)   BOOK="${2:?}"; shift 2 ;;
        --rounds) ROUNDS="${2:?}"; shift 2 ;;
        --out)    OUT="${2:?}"; shift 2 ;;
        --analyse-only) ANALYSE_ONLY=1; shift ;;
        -h|--help) usage ;;
        *)        if [ -z "$CAND" ]; then CAND="$1"; elif [ -z "$SF" ]; then
                      SF="$1"; else echo "argument en trop : $1" >&2; usage
                  fi; shift ;;
    esac
done

[ -n "$CAND" ] && [ -n "$SF" ] || usage
[ -x "$CAND" ] || { echo "candidat introuvable ou non executable : $CAND" >&2; exit 2; }
[ -x "$SF" ] || { echo "stockfish introuvable ou non executable : $SF" >&2; exit 2; }
command -v fastchess >/dev/null 2>&1 || { echo "fastchess absent du PATH" >&2; exit 2; }

# --- le livre d'ouvertures -------------------------------------------------
if [ -z "$BOOK" ]; then
    for cand_book in /tmp/openings_src/eco_lichess.epd \
                     openings/eco_lichess.epd \
                     openings/builtin_min.epd; do
        if [ -f "$cand_book" ]; then BOOK="$cand_book"; break; fi
    done
fi
[ -n "$BOOK" ] || { echo "aucun livre d'ouvertures trouve (--book)" >&2; exit 2; }

# --- les niveaux -----------------------------------------------------------
if [ -z "$LEVELS" ]; then
    LEVELS="1320,1450,1600,1800,2000,2250"
fi
[ -n "$ROUNDS" ] || ROUNDS=$(( (GAMES + 1) / 2 ))

mkdir -p "$OUT"
# `-each` attend l'option et sa valeur comme **deux arguments** distincts
# (`-each proto=uci nodes=40000`) : les regrouper fait echouer fastchess
# (« Unsupported protocol ») sans qu'il sorte du lot.
LIMIT_ARG=()
if [ -n "$TC" ]; then
    LIMIT_ARG=(-each proto=uci "tc=$TC")
else
    LIMIT_ARG=(-each proto=uci "nodes=$NODES")
fi

echo "# calibration Elo de $CAND"
echo "# ancre : Stockfish en force limitee (UCI_Elo), budget : ${TC:-nodes=$NODES}"
echo "# book : $BOOK ; $GAMES parties par niveau (rounds=$ROUNDS)"
echo

for level in ${LEVELS//,/ }; do
    log="$OUT/elo_${level}.log"
    if [ "$ANALYSE_ONLY" = "1" ]; then
        echo "== UCI_Elo $level -> $log (analyse seule)"
        continue
    fi
    echo "== UCI_Elo $level -> $log"
    # Le journal de parties de fastchess est sur **stdout** (l'option
    # `-log` est le log de debogage, pas le journal) : on capture donc la
    # sortie standarde, comme le fait la session de mesure du dépôt.
    fastchess \
        -engine name=cand cmd="$CAND" args="uci" \
        -engine name=sf cmd="$SF" \
            option.UCI_LimitStrength=true option.UCI_Elo="$level" \
        "${LIMIT_ARG[@]}" \
        -openings file="$BOOK" format=epd order=random \
        -rounds "$ROUNDS" -games 2 -concurrency 5 -repeat \
        -resign movecount=3 score=600 -draw movenumber=40 movecount=8 score=10 \
        >"$log" 2>&1 < /dev/null
    if grep -q "Error while reading option" "$log"; then
        echo "fastchess a refuse la ligne de commande (voir $log) :" >&2
        sed -n '1,3p' "$log" >&2
        exit 1
    fi
done

# --- interpolation et intervalle ------------------------------------------
python3 - "$OUT" $GAMES <<'PY'
import math, re, sys, glob, os

out_dir, games = sys.argv[1], int(sys.argv[2])
rows = []
for log in sorted(glob.glob(os.path.join(out_dir, "elo_*.log"))):
    level = int(re.search(r"elo_(\d+)\.log$", log).group(1))
    text = open(log, encoding="utf-8", errors="replace").read()
    # fastchess imprime un resume **intermediaire** apres chaque partie :
    # c'est le **dernier** bloc « Results of » qui fait foi.
    ms = re.findall(r"Points:\s+([\d.]+)\s+\(([\d.]+) %\)", text)
    gs = re.findall(
        r"Games:\s+(\d+),\s+Wins:\s+(\d+),\s+Losses:\s+(\d+),\s+Draws:\s+(\d+)", text
    )
    if not ms or not gs:
        print(f"# {log}: resultat illisible", file=sys.stderr)
        continue
    score = float(ms[-1][1]) / 100.0
    games_played = int(gs[-1][0])
    rows.append((level, score, games_played, int(gs[-1][1]), int(gs[-1][3])))

if not rows:
    sys.exit("aucun resultat exploitable")

rows.sort()
K = 400.0 / math.log(10)          # constante d'echelle Elo
print()
print("| UCI_Elo SF | parties | score | Elo du candidat | +/- |")
print("|---|---|---|---|---|")
for level, score, n, w, d in rows:
    if 0.0 < score < 1.0:
        elo = -K * math.log((1 - score) / score)
        # propagation de l'ecart-type binomial (borne de Wilson simplifiee)
        p = score
        se_p = math.sqrt(max(p * (1 - p), 1e-9) / n)
        d_elo = K * se_p / (p * (1 - p))
    else:
        elo, d_elo = float("nan"), float("nan")
    print(f"| {level} | {n} | {score*100:.1f} % | {elo:+.0f} | {d_elo:.0f} |")

# interpolation : l'Elo du candidat est lineaire en `logit` du score, donc
# une **droite** Elo_cand = a + b * niveau_SF. L'ajustement est pondere par
# 1/sigma^2 et ne retient que les points **non satures** (5 % < score < 95 %) :
# un point a 2 % n'apporte aucune information sur la croisee a 50 %, et son
# erreur enorme polluerait la propagation.
def elo_of(score):
    return -K * math.log((1 - score) / score)

def sigma_elo(score, n):
    p = score
    return K * math.sqrt(max(p * (1 - p), 1e-9) / n) / (p * (1 - p))

usable = [(l, s, n) for l, s, n, w, d in rows if 0.05 < s < 0.95]
if len(usable) >= 2:
    wts = [1.0 / sigma_elo(s, n) ** 2 for l, s, n in usable]
    sw = sum(wts)
    mx = sum(w * l for w, (l, s, n) in zip(wts, usable)) / sw
    my = sum(w * elo_of(s) for w, (l, s, n) in zip(wts, usable)) / sw
    sxx = sum(w * (l - mx) ** 2 for w, (l, s, n) in zip(wts, usable))
    sxy = sum(w * (l - mx) * (elo_of(s) - my)
              for w, (l, s, n) in zip(wts, usable))
    slope = sxy / sxx if sxx else 0.0
    inter = my - slope * mx
    res = sum(w * (elo_of(s) - (inter + slope * l)) ** 2
              for w, (l, s, n) in zip(wts, usable))
    dof = max(len(usable) - 2, 1)
    s2 = res / dof
    var_a = s2 * (1.0 / sw + mx * mx / sxx)
    var_b = s2 / sxx
    crossing = (0 - inter) / slope
    var_cross = (var_a + inter * inter * var_b) / (slope * slope)
    print()
    print(f"# ajustement pondere (Elo du candidat = {inter:+.0f} "
          f"{slope:+.3f} x UCI_Elo_SF) sur {len(usable)} points non satures")
    print(f"# le niveau de Stockfish ou le candidat fait 50 % : "
          f"{crossing:.0f} d'UCI_Elo_SF")
    print(f"# +/- {math.sqrt(var_cross):.0f} Elo (delta-method sur l'ajustement)")

# Estimateur **local** : la croisee a 50 % est determinee par les deux
# niveaux qui l'encadrent (le plus haut au-dessus de 50 %, le plus bas en
# dessous). On interpole en espace `logit` (donc en Elo, lineairement) et
# on propage les deux erreurs de score. C'est l'estimateur retenue : il
# n'utilise que les points qui **portent** l'information, la ou la
# fonction de force sature.
above = [(l, s, n) for l, s, n, w, d in rows if s >= 0.5]
below = [(l, s, n) for l, s, n, w, d in rows if s < 0.5]
if above and below:
    x1, s1, n1 = max(above)          # niveau le plus haut ou l'on gagne
    x2, s2, n2 = min(below)          # niveau le plus bas ou l'on perd
    e1, e2 = elo_of(s1), elo_of(s2)
    wgt = e1 / (e1 - e2)             # poids du point haut
    crossing = x1 + wgt * (x2 - x1)
    v = (wgt * sigma_elo(s1, n1)) ** 2 + ((1 - wgt) * sigma_elo(s2, n2)) ** 2
    print()
    print(f"# estimateur local : encadrement par {x1} ({s1*100:.1f} %) et "
          f"{x2} ({s2*100:.1f} %)")
    print(f"# Elo du moteur = {crossing:.0f} +/- {math.sqrt(v):.0f} "
          f"(echelle UCI_Elo de Stockfish)")
PY
