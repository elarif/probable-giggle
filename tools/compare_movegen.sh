#!/usr/bin/env bash
# Comparaison de debit perft : mychess (movegen) / motors.
#
# Les deux mesures sont mono-thread et scalaires du cote mychess : motors
# recoit `go perft <depth> threads 1`, car son perft par defaut est parallele
# (rayon, tous les coeurs) et donnerait un ratio qui ne mesure pas le
# generateur.
#
# motors est clone dans un cache stable (pas /tmp : trois redemarrages l'ont
# deja efface) et epingle a un commit precis, pour que la comparaison reste
# reproductible.
#
# Le build motors se fait depuis son propre repertoire : son
# .cargo/config.toml fixe `-Ctarget-cpu=native`, et cargo ne lit un
# config.toml qu'en remontant depuis le repertoire courant, pas depuis
# --manifest-path. Lancer `cargo build --manifest-path ...` depuis mychess
# produirait donc un binaire different (sans AVX2), non comparable.
#
# Usage:
#   tools/compare_movegen.sh
#   MYCHESS_CACHE=/tmp/cache-froid tools/compare_movegen.sh   # cache alternatif
#
# Variables d'environnement:
#   MOTORS_REPO    URL du depot motors
#   MOTORS_COMMIT  commit epingle
#   MYCHESS_CACHE  racine du cache (defaut : ${XDG_CACHE_HOME:-$HOME/.cache}/mychess)
set -euo pipefail

MOTORS_REPO="${MOTORS_REPO:-https://github.com/toanth/motors}"
MOTORS_COMMIT="${MOTORS_COMMIT:-fdb697373e03d21e26a254dea0f8613a596ca630}"
CACHE="${MYCHESS_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/mychess}"
MOTORS_DIR="${CACHE}/motors"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ ! -f "${ROOT}/Cargo.toml" ]]; then
    echo "compare_movegen.sh: ${ROOT} n'est pas la racine du depot mychess" >&2
    exit 1
fi
cd "$ROOT"

die() {
    echo "compare_movegen.sh: $*" >&2
    exit 1
}

for tool in git cargo sed; do
    command -v "$tool" >/dev/null 2>&1 || die "'$tool' introuvable dans PATH"
done

echo "== build mychess =="
cargo build --release --quiet || die "build mychess echoue"
MYCHESS="${ROOT}/target/release/mychess"
[[ -x "$MYCHESS" ]] || die "binaire absent : $MYCHESS"

# Vrai si le cache est deja sur le commit epingle (SHA court ou long).
motors_at_pin() {
    [[ -d "${MOTORS_DIR}/.git" ]] || return 1
    local head pin
    head=$(git -C "$MOTORS_DIR" rev-parse HEAD 2>/dev/null || true)
    pin=$(git -C "$MOTORS_DIR" rev-parse --verify --quiet \
        "${MOTORS_COMMIT}^{commit}" 2>/dev/null || true)
    [[ -n "$head" && "$head" == "$pin" ]]
}

MOTORS="${MOTORS_DIR}/target/release/motors"
if [[ -x "$MOTORS" ]] && motors_at_pin; then
    echo "== motors deja construit @ ${MOTORS_COMMIT:0:7} (cache : ${MOTORS_DIR}) =="
else
    if [[ ! -d "${MOTORS_DIR}/.git" ]]; then
        mkdir -p "$CACHE"
        echo "== clone motors dans ${MOTORS_DIR} =="
        git clone --quiet "$MOTORS_REPO" "$MOTORS_DIR" || die "clone de ${MOTORS_REPO} echoue"
    fi
    if ! motors_at_pin; then
        echo "== checkout motors @ ${MOTORS_COMMIT:0:7} =="
        # Le commit peut ne pas etre dans les refs locales d'un clone recent :
        # on tente un fetch cible, sans echouer si le serveur refuse les SHA.
        git -C "$MOTORS_DIR" fetch --quiet origin "$MOTORS_COMMIT" 2>/dev/null || true
        git -C "$MOTORS_DIR" checkout --quiet --detach "$MOTORS_COMMIT" ||
            die "commit ${MOTORS_COMMIT} introuvable dans ${MOTORS_DIR}"
    fi
    echo "== build motors @ ${MOTORS_COMMIT:0:7} =="
    (
        cd "$MOTORS_DIR" || die "repertoire inutilisable : ${MOTORS_DIR}"
        cargo build --release --quiet -p motors
    ) || die "build motors echoue"
    [[ -x "$MOTORS" ]] || die "binaire motors absent : $MOTORS"
fi

POSITIONS=(
    "startpos|rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1|6|119060324"
    "kiwipete|r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1|5|193690690"
    "sje|r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10|5|164075551"
)

printf '\n%-10s %5s %14s %12s %12s\n' \
    "position" "depth" "nodes" "mychess" "motors"
printf '%.0s-' {1..57}
printf '\n'

fail_count=0
for entry in "${POSITIONS[@]}"; do
    IFS='|' read -r name fen depth expected <<<"$entry"

    # stderr est capture avec stdout : les diagnostics (`[slider path: ...]`,
    # message d'erreur de FEN) sont ainsi disponibles si le parsing echoue,
    # et la ligne `perft(N) = ...` reste la seule a correspondre au motif.
    my_out=$("$MYCHESS" perft "$fen" "$depth" 2>&1) ||
        die "mychess perft a echoue ($name d$depth) : $my_out"
    my_nodes=$(sed -nE 's/.*= ([0-9]+) +\[.*/\1/p' <<<"$my_out")
    my_mnps=$(sed -nE 's/.*, ([0-9.]+) Mnodes\/s.*/\1/p' <<<"$my_out")
    [[ "$my_nodes" =~ ^[0-9]+$ ]] || die "sortie mychess illisible pour $name d$depth : $my_out"
    [[ "$my_mnps" =~ ^[0-9.]+$ ]] || die "debit mychess illisible pour $name d$depth : $my_out"

    # `go perft` sans `threads` lance un perft *parallele* (rayon, tous les
    # coeurs) : notre moteur est scalaire et mono-thread ici, donc la
    # seule cellule comparable est `threads 1`. Le debit parallele de motors
    # est releve separement dans le rapport, jamais dans ce tableau.
    motors_out=$(printf 'ugi\nposition fen %s\ngo perft %s threads 1\nquit\n' "$fen" "$depth" |
        "$MOTORS" 2>&1) || die "motors a echoue ($name d$depth)"
    motors_nodes=$(sed -nE 's/^Nodes searched: ([0-9]+).*/\1/p' <<<"$motors_out")
    motors_mnps=$(sed -nE 's/.*\(([0-9.]+)Mnps\).*/\1/p' <<<"$motors_out")
    # motors ecrit ses erreurs sur stdout, au milieu de la liste d'options UGI :
    # on les extrait pour que l'echec soit lisible.
    motors_err=$(sed -nE '/error|invalid|requires|can only/p' <<<"$motors_out" | head -3 | tr '\n' ' ')
    [[ "$motors_nodes" =~ ^[0-9]+$ ]] ||
        die "sortie motors illisible pour $name d$depth (${motors_err:-pas de message d'erreur})"
    [[ "$motors_mnps" =~ ^[0-9.]+$ ]] ||
        die "debit motors illisible pour $name d$depth (${motors_err:-pas de message d'erreur})"

    if [[ "$my_nodes" != "$expected" ]]; then
        echo "ECHEC: $name d$depth = $my_nodes, table publiee = $expected" >&2
        fail_count=$((fail_count + 1))
    fi
    if [[ "$my_nodes" != "$motors_nodes" ]]; then
        echo "ECHEC: comptes de noeuds divergents pour $name d$depth" >&2
        echo "  attendu=$expected mychess=$my_nodes motors=$motors_nodes" >&2
        fail_count=$((fail_count + 1))
    fi

    printf '%-10s %5s %14s %12s %12s\n' \
        "$name" "$depth" "$my_nodes" "$my_mnps" "$motors_mnps"
done

if [[ "$fail_count" -ne 0 ]]; then
    die "${fail_count} divergence(s) de comptes : harnais ou generateur en defaut, pas un probleme de mesure"
fi

echo
echo "Mnodes/s mono-thread, une seule execution par cellule : pour un verdict,"
echo "utiliser l'A/B/A/B de docs/movegen/gct-benchmark-2026-10-21.md."
