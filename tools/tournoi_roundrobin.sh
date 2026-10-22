#!/usr/bin/env bash
# tools/tournoi_roundrobin.sh — tournoi round-robin (toutes les paires).
#
# Usage :
#   tools/tournoi_roundrobin.sh [--st 0.2] [--rounds 6] [--concurrency 5]
#                               [--book fichier.epd] [--seed N]
#                               [--out prt/rr/tournoi.log]
#
# Champ de moteurs (11) :
#   mychess        le moteur du dépôt          (target/release/mychess)
#   mychess_ctrl   le **même** binaire         -> contrôle d'environnement
#   reckless       Reckless v0.1.0             (UCI, bits 64)
#   reckless_ctrl  le **même** binaire         -> contrôle d'environnement
#   sf1320 … sf3190  Stockfish 19 en force limitée (UCI_LimitStrength)
#   sf1900_ctrl    le **même** réglage que sf1900 -> contrôle d'environnement
#
# Budget : `st` (movetime), un budget **par coup identique pour tous les
# moteurs**. C'est l'analogue direct du budget `nodes=40000` du tournoi
# précédent, et c'est le seul budget que *tous* les moteurs honorent :
# **Reckless v0.1.0 n'implémente pas `go nodes`** (son parseur UCI
# n'accepte que `depth`, `movetime` et `wtime/btime/winc/binc`) — l'envoyer
# en `go nodes` le ferait tomber en `TimeControl::Infinite`, c'est-à-dire
# en recherche infinie. Le budget en temps est donc le seul dénominateur
# commun, et il évite en prime le biais de gestion d'horloge que le
# contrôle de temps « horloge » introduisait (`mychess` ne reçoit que
# ~4 % de son temps par coup, cf. ELO_LOG.md).
#
# `timemargin` : tolérance au dépassement, pour qu'une itération un peu
# trop longue (Reckless ne teste le temps qu'entre deux itérations)
# ne fasse pas adjuger la partie par « flag ». Les parties adjugées
# sont comptées et signalées dans le rapport.
#
# Adjudication : **aucune** (ni `-resign`, ni `-draw`). C'est le protocole
# du tournoi précédent : ces règles se déclenchent sur le score *rapporté*
# par le moteur, que le handicap de Stockfish brouille — les parties
# étaient alors décidées « by adjudication » et non aux échecs. Les
# parties se terminent donc par mat, nulle par la règle des 50 coups ou
# par la règle de répétition.
#
# Livre : `eco_lichess.epd` (3 863 positions), ouvertures en ordre
# aléatoire avec graine fixe (reproductibilité).

set -euo pipefail

ST=0.2
ROUNDS=6
GAMES=2
CONCURRENCY=5
BOOK=""
SEED=20261022
OUT="prt/rr/tournoi.log"
SF="${SF:-/home/elarif/.cache/mychess/sf/stockfish/stockfish-linux-x86-64-universal}"
RECKLESS="${RECKLESS:-/home/elarif/.cache/mychess/engines/reckless-0.1.0}"
MYCHESS="${MYCHESS:-target/release/mychess}"
TIMEMARGIN=200

usage() {
    sed -n '2,40p' "$0" >&2
    exit 2
}

while [ $# -gt 0 ]; do
    case "$1" in
        --st)         ST="${2:?}"; shift 2 ;;
        --rounds)     ROUNDS="${2:?}"; shift 2 ;;
        --games)      GAMES="${2:?}"; shift 2 ;;
        --concurrency) CONCURRENCY="${2:?}"; shift 2 ;;
        --book)       BOOK="${2:?}"; shift 2 ;;
        --seed)       SEED="${2:?}"; shift 2 ;;
        --out)        OUT="${2:?}"; shift 2 ;;
        --timemargin) TIMEMARGIN="${2:?}"; shift 2 ;;
        -h|--help)    usage ;;
        *) echo "option inconnue : $1" >&2; usage ;;
    esac
done

# Le livre par défaut est celui de l'échelle du dépôt.
if [ -z "$BOOK" ]; then
    for cand in /tmp/openings_src/eco_lichess.epd \
                openings/eco_lichess.epd \
                openings/builtin_min.epd; do
        [ -f "$cand" ] && { BOOK="$cand"; break; }
    done
fi
[ -n "$BOOK" ] || { echo "aucun livre d'ouvertures" >&2; exit 2; }
[ -x "$MYCHESS" ] || { echo "mychess introuvable : $MYCHESS" >&2; exit 2; }
[ -x "$RECKLESS" ] || { echo "reckless introuvable : $RECKLESS" >&2; exit 2; }
[ -x "$SF" ] || { echo "stockfish introuvable : $SF" >&2; exit 2; }
command -v fastchess >/dev/null || { echo "fastchess absent du PATH" >&2; exit 2; }

mkdir -p "$(dirname "$OUT")"

# `fastchess` **réécrit** un `config.json` dans le répertoire courant : ce
# doit être le répertoire du journal, jamais la racine du dépôt (ce
# fichier y est versionné, et il n'est pas un livrable de la mesure).
OUT_ABS="$(cd "$(dirname "$OUT")" && pwd)/$(basename "$OUT")"
OUT_DIR="$(dirname "$OUT_ABS")"
MYCHESS_ABS="$(cd "$(dirname "$MYCHESS")" && pwd)/$(basename "$MYCHESS")"
if [ -n "${BOOK:-}" ] && [ "${BOOK:0:1}" != "/" ]; then
    BOOK_ABS="$(cd "$(dirname "$BOOK")" && pwd)/$(basename "$BOOK")"
else
    BOOK_ABS="$BOOK"
fi

# La graine du book et le budget sont inscrits dans l'en-tête du journal :
# un journal sans son en-tête n'est pas une preuve.
{
    echo "# tournoi round-robin (mychess + Reckless v0.1.0 + echelle Stockfish)"
    echo "# budget : movetime ${ST}s par coup, identique pour tous les moteurs"
    echo "# timemargin : ${TIMEMARGIN} ms ; adjudication : aucune"
    echo "# book : ${BOOK} (ordre aleatoire, graine ${SEED})"
    echo "# rondes : ${ROUNDS} x ${GAMES} couleurs par paire"
    echo "# date : $(date -Is)"
} > "$OUT"

(
cd "$OUT_DIR"
fastchess \
    -engine name=mychess cmd="$MYCHESS_ABS" args="uci" \
    -engine name=mychess_ctrl cmd="$MYCHESS_ABS" args="uci" \
    -engine name=reckless cmd="$RECKLESS" \
    -engine name=reckless_ctrl cmd="$RECKLESS" \
    -engine name=sf1320 cmd="$SF" option.UCI_LimitStrength=true option.UCI_Elo=1320 \
    -engine name=sf1425 cmd="$SF" option.UCI_LimitStrength=true option.UCI_Elo=1425 \
    -engine name=sf1500 cmd="$SF" option.UCI_LimitStrength=true option.UCI_Elo=1500 \
    -engine name=sf1600 cmd="$SF" option.UCI_LimitStrength=true option.UCI_Elo=1600 \
    -engine name=sf1900 cmd="$SF" option.UCI_LimitStrength=true option.UCI_Elo=1900 \
    -engine name=sf1900_ctrl cmd="$SF" option.UCI_LimitStrength=true option.UCI_Elo=1900 \
    -engine name=sf3190 cmd="$SF" option.UCI_LimitStrength=true option.UCI_Elo=3190 \
    -each proto=uci "st=$ST" "timemargin=$TIMEMARGIN" \
    -openings file="$BOOK_ABS" format=epd order=random \
    -rounds "$ROUNDS" -games "$GAMES" -concurrency "$CONCURRENCY" -srand "$SEED" \
    >> "$OUT_ABS" 2>&1 < /dev/null
)

echo "journal : $OUT"
grep -c "^Finished game" "$OUT" || true
