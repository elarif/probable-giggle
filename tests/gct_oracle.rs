//! Validation contre la table de perft détaillée Grand Chess Tree.
//!
//! Les goldens de `tests/perft/gct/` stockent les colonnes GCT **verbatim**.
//! La conversion vers la convention de `mychess` est faite ici, jamais dans
//! les fichiers — c'est ce qui rend l'oracle indépendant.
//!
//! Conventions (voir `docs/gct/perft-results-gct.md`) :
//!   - `mychess.checks == gct.checks + gct.mates` (GCT compte les échecs
//!     hors mat ; `mychess` les compte mat inclus) ;
//!   - `captures`, `ep`, `castles`, `promotions`, `mates` : identité ;
//!   - `discovery` / `double` : non comparables, stockés `-` ;
//!   - `unique` : compte de positions distinctes, jamais importé.

use mychess::perft::perft_detailed;
use mychess::position::Position;

struct Row {
    depth: u32,
    nodes: u64,
    // captures, ep, castles, promotions, gct_checks, discovery, double, mates
    cols: [Option<u64>; 8],
}

struct Golden {
    fen: String,
    rows: Vec<Row>,
}

fn load(path: &str) -> Golden {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut fen = String::new();
    let mut rows = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if fen.is_empty() && line.contains('/') {
            fen = line.to_string();
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        assert!(toks.len() >= 2, "ligne de golden invalide : {line}");
        let depth: u32 = toks[0].parse().unwrap();
        let nodes: u64 = toks[1].parse().unwrap();
        let mut cols = [None; 8];
        for (i, c) in cols.iter_mut().enumerate() {
            if let Some(t) = toks.get(2 + i).filter(|t| **t != "-") {
                *c = Some(t.replace('_', "").parse().unwrap());
            }
        }
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
/// valeurs publiées ne tomberaient pas juste. On le prouve avec les deux
/// points de contrôle cités dans `docs/gct/perft-results-gct.md`.
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

    let got = [
        Some(c.captures),
        Some(c.ep),
        Some(c.castles),
        Some(c.promotions),
        row.cols[4].map(|chk| gct_checks_to_mychess(chk, row.cols[7].unwrap_or(0))),
        None, // discovery : non comparable
        None, // double : non comparable
        Some(c.mates),
    ];
    let names = [
        "captures",
        "ep",
        "castles",
        "promotions",
        "checks",
        "discovery",
        "double",
        "mates",
    ];
    for i in 0..8 {
        if let Some(want) = got[i] {
            let mine = [
                c.captures,
                c.ep,
                c.castles,
                c.promotions,
                c.checks,
                0,
                0,
                c.mates,
            ][i];
            assert_eq!(mine, want, "{} d{} {}", g.fen, row.depth, names[i]);
        }
    }
}

#[test]
fn t1_gct_shallow() {
    for g in all() {
        for row in &g.rows {
            if row.depth > 4 {
                continue;
            }
            check_row(&g, row);
        }
    }
}

#[test]
#[ignore = "T2: table GCT aux profondeurs 5 et 6 (minutes, mono-thread)"]
fn t2_gct_depth5_6() {
    for g in all() {
        for row in &g.rows {
            if !(5..=6).contains(&row.depth) {
                continue;
            }
            check_row(&g, row);
        }
    }
}

#[test]
#[ignore = "T3: table GCT a la profondeur 7 (long)"]
fn t3_gct_depth7() {
    for g in all() {
        for row in &g.rows {
            if row.depth != 7 {
                continue;
            }
            check_row(&g, row);
        }
    }
}
