//! Validation du générateur de production contre la table de perft détaillée
//! Grand Chess Tree — l'ancre externe du dépôt.
//!
//! Les goldens de `tests/perft/gct/` stockent les colonnes GCT **verbatim**.
//! La conversion vers la convention de `mychess` est faite ici, jamais dans
//! les fichiers — c'est ce qui rend l'oracle indépendant.
//!
//! Ce test ne dépend d'aucun autre générateur : il compare `movegen` à des
//! nombres publiés. `src/gct/` a été supprimé (phase 2) ; `motors` et
//! `src/refgen.rs` couvrent l'axe inter-implémentations.
//!
//! Conventions (voir `docs/gct/perft-results-gct.md`) :
//!   - `mychess.checks == gct.checks + gct.mates` (GCT compte les échecs
//!     hors mat ; `mychess` les compte mat inclus) ;
//!   - `captures`, `ep`, `castles`, `promotions`, `mates` : identité ;
//!   - `discovery` / `double` : non comparables, stockés `-` ;
//!   - `unique` : compte de positions distinctes, jamais importé.

use mychess::perft::perft_detailed;
use mychess::position::Position;

const SHALLOW_MAX_DEPTH: u32 = 4;
/// Trois positions × profondeurs 0 à 4 incluses.
const MIN_SHALLOW_ROWS: usize = 15;
const MID_MIN_DEPTH: u32 = 5;
const MID_MAX_DEPTH: u32 = 6;
/// Trois positions × profondeurs 5 et 6.
const MIN_MID_ROWS: usize = 6;
const DEEP_DEPTH: u32 = 7;
/// Trois positions × profondeur 7.
const MIN_DEEP_ROWS: usize = 3;

struct Row {
    depth: u32,
    nodes: u64,
    /// Colonnes assertées, dans l'ordre du fichier :
    /// captures, ep, castles, promotions, checks, mates.
    cols: [u64; 6],
}

struct Golden {
    fen: String,
    rows: Vec<Row>,
}

fn parse_number(path: &str, lineno: usize, column: &str, raw: &str) -> u64 {
    raw.replace('_', "")
        .parse()
        .unwrap_or_else(|e| panic!("{path}:{lineno}: colonne {column} invalide ({raw:?}) : {e}"))
}

fn required_column(path: &str, lineno: usize, toks: &[&str], idx: usize, column: &str) -> u64 {
    let raw = toks[idx];
    if raw == "-" {
        panic!("{path}:{lineno}: colonne {column} obligatoire mais '-' : {toks:?}");
    }
    parse_number(path, lineno, column, raw)
}

// Duplication VOLONTAIRE avec `tests/perft_golden.rs` :
// la contrainte globale du plan
// (`docs/superpowers/plans/2026-10-21-movegen-reecriture-posts-gct.md`,
// « Convention de la colonne `checks` de la table Grand Chess Tree ») impose
// des goldens GCT verbatim (`discovery`/`double` à `-`), convention
// incompatible avec celle du Chess Programming Wiki ; et ici `checks` reste en
// convention GCT (la conversion `+ mates` se fait dans `check_row`), alors que
// l'ancien `tests/gct_perft.rs` la faisait dans `check`. Un chargeur partagé
// mélangerait les deux conventions. Ne pas factoriser.
fn load(path: &str) -> Golden {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut fen = String::new();
    let mut rows = Vec::new();
    for (line_idx, raw_line) in text.lines().enumerate() {
        let lineno = line_idx + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if fen.is_empty() && line.contains('/') {
            fen = line.to_string();
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.len() != 10 {
            panic!(
                "{path}:{lineno}: ligne de donnees invalide : 10 colonnes attendues, {} trouvees : {line}",
                toks.len()
            );
        }
        let depth: u32 = toks[0].parse().unwrap_or_else(|e| {
            panic!("{path}:{lineno}: profondeur invalide ({:?}) : {e}", toks[0])
        });
        let nodes = parse_number(path, lineno, "nodes", toks[1]);
        let cols = [
            required_column(path, lineno, &toks, 2, "captures"),
            required_column(path, lineno, &toks, 3, "ep"),
            required_column(path, lineno, &toks, 4, "castles"),
            required_column(path, lineno, &toks, 5, "promotions"),
            required_column(path, lineno, &toks, 6, "checks"),
            required_column(path, lineno, &toks, 9, "mates"),
        ];
        rows.push(Row { depth, nodes, cols });
    }
    assert!(!fen.is_empty(), "{path}: aucune FEN");
    Golden { fen, rows }
}

fn all() -> Vec<Golden> {
    [
        "tests/perft/gct/startpos.txt",
        "tests/perft/gct/kiwipete.txt",
        "tests/perft/gct/sje.txt",
    ]
    .iter()
    .map(|p| load(p))
    .collect()
}

/// `mychess.checks = gct.checks + gct.mates`.
fn gct_checks_to_mychess(gct_checks: u64, gct_mates: u64) -> u64 {
    gct_checks + gct_mates
}

/// La conversion doit « mordre » : si l'oracle appliquait l'identité, les
/// valeurs publiées ne tomberaient pas juste.
///
/// La note `docs/gct/perft-results-gct.md` cite **deux** points de contrôle
/// (startpos d4, kiwipete d3). Les deux suivants sont lus dans les goldens
/// eux-mêmes (startpos d5, sje d5) : le test en vérifie donc quatre, la note
/// n'en publie que deux.
#[test]
fn la_conversion_des_checks_est_celle_du_document() {
    assert_eq!(gct_checks_to_mychess(461, 8), 469, "startpos d4");
    assert_eq!(gct_checks_to_mychess(992, 1), 993, "kiwipete d3");
    assert_eq!(gct_checks_to_mychess(27004, 347), 27351, "startpos d5");
    assert_eq!(gct_checks_to_mychess(2998380, 228), 2998608, "sje d5");
}

fn check_row(g: &Golden, row: &Row) {
    let mut pos = Position::from_fen(&g.fen).unwrap();
    let c = perft_detailed(&mut pos, row.depth);
    assert_eq!(c.nodes, row.nodes, "{} d{} nodes", g.fen, row.depth);

    // Colonnes du golden, dans l'ordre du fichier :
    // captures, ep, castles, promotions, checks, discovery, double, mates.
    // Les six colonnes comparables sont toutes obligatoires : le chargement
    // refuse `-` et toute ligne qui n'a pas exactement dix jetons.
    let want = [
        row.cols[0],
        row.cols[1],
        row.cols[2],
        row.cols[3],
        // `mychess.checks == gct.checks + gct.mates`.
        gct_checks_to_mychess(row.cols[4], row.cols[5]),
        row.cols[5],
    ];
    let got = [c.captures, c.ep, c.castles, c.promotions, c.checks, c.mates];
    let names = ["captures", "ep", "castles", "promotions", "checks", "mates"];
    for i in 0..6 {
        assert_eq!(got[i], want[i], "{} d{} {}", g.fen, row.depth, names[i]);
    }
    // `discovery` et `double` ne sont pas comparables entre les deux
    // conventions (`docs/ecart_d9_convention.md`) : jamais importées.
}

#[test]
fn t1_gct_shallow() {
    let mut checked = 0usize;
    for g in all() {
        for row in &g.rows {
            if row.depth > SHALLOW_MAX_DEPTH {
                continue;
            }
            check_row(&g, row);
            checked += 1;
        }
    }
    assert!(
        checked >= MIN_SHALLOW_ROWS,
        "t1_gct_shallow : seulement {checked} ligne(s) verifiee(s), minimum attendu {MIN_SHALLOW_ROWS}"
    );
}

#[test]
#[ignore = "T2: table GCT aux profondeurs 5 et 6 (minutes, mono-thread)"]
fn t2_gct_depth5_6() {
    let mut checked = 0usize;
    for g in all() {
        for row in &g.rows {
            if !(MID_MIN_DEPTH..=MID_MAX_DEPTH).contains(&row.depth) {
                continue;
            }
            check_row(&g, row);
            checked += 1;
        }
    }
    assert!(
        checked >= MIN_MID_ROWS,
        "t2_gct_depth5_6 : seulement {checked} ligne(s) verifiee(s), minimum attendu {MIN_MID_ROWS}"
    );
}

#[test]
#[ignore = "T3: table GCT a la profondeur 7 (long)"]
fn t3_gct_depth7() {
    let mut checked = 0usize;
    for g in all() {
        for row in &g.rows {
            if row.depth != DEEP_DEPTH {
                continue;
            }
            check_row(&g, row);
            checked += 1;
        }
    }
    assert!(
        checked >= MIN_DEEP_ROWS,
        "t3_gct_depth7 : seulement {checked} ligne(s) verifiee(s), minimum attendu {MIN_DEEP_ROWS}"
    );
}

#[test]
fn les_goldens_couvrent_d0_a_d7_sans_trou() {
    for g in all() {
        let depths: Vec<u32> = g.rows.iter().map(|r| r.depth).collect();
        assert_eq!(depths, (0..=7).collect::<Vec<u32>>(), "{}", g.fen);
    }
}
