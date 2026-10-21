//! Perft du générateur `gct`, validé contre la table détaillée Grand Chess
//! Tree (goldens de `tests/perft/gct/`, conversion de convention appliquée
//! ici comme dans `tests/gct_oracle.rs`).

use mychess::gct::perft::{perft as gct_perft, perft_detailed as gct_detailed};
use mychess::position::Position;

struct Row {
    depth: u32,
    nodes: u64,
    cols: [Option<u64>; 8],
}
struct Golden {
    fen: String,
    rows: Vec<Row>,
}

fn load(path: &str) -> Golden {
    let text = std::fs::read_to_string(path).unwrap();
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

fn check(g: &Golden, row: &Row) {
    let mut pos = Position::from_fen(&g.fen).unwrap();
    assert_eq!(
        gct_perft(&mut pos, row.depth),
        row.nodes,
        "{} d{} nodes (gct::perft)",
        g.fen,
        row.depth
    );

    let mut pos = Position::from_fen(&g.fen).unwrap();
    let c = gct_detailed(&mut pos, row.depth);
    assert_eq!(
        c.nodes, row.nodes,
        "{} d{} nodes (detail)",
        g.fen, row.depth
    );
    let gct_mates = row.cols[7].unwrap_or(0);
    let got = [c.captures, c.ep, c.castles, c.promotions, c.checks, c.mates];
    let mut want = [None; 6];
    want[0] = row.cols[0];
    want[1] = row.cols[1];
    want[2] = row.cols[2];
    want[3] = row.cols[3];
    want[4] = row.cols[4].map(|chk| chk + gct_mates);
    want[5] = row.cols[7];
    let names = ["captures", "ep", "castles", "promotions", "checks", "mates"];
    for i in 0..6 {
        if let Some(w) = want[i] {
            assert_eq!(got[i], w, "{} d{} {}", g.fen, row.depth, names[i]);
        }
    }
}

#[test]
fn t1_gct_perft_shallow() {
    for g in all() {
        for row in &g.rows {
            if row.depth > 4 {
                continue;
            }
            check(&g, row);
        }
    }
}

#[test]
#[ignore = "T2: perft gct aux profondeurs 5 et 6"]
fn t2_gct_perft_mid() {
    for g in all() {
        for row in &g.rows {
            if !(5..=6).contains(&row.depth) {
                continue;
            }
            check(&g, row);
        }
    }
}

#[test]
fn gct_perft_egale_le_movegen_existant_aux_profondeurs_courtes() {
    // Contrôle croisé indépendant de la table GCT : les deux générateurs
    // doivent produire exactement les mêmes comptes.
    for g in all() {
        for row in &g.rows {
            if row.depth > 3 {
                continue;
            }
            let mut a = Position::from_fen(&g.fen).unwrap();
            let mut b = Position::from_fen(&g.fen).unwrap();
            assert_eq!(
                gct_perft(&mut a, row.depth),
                mychess::perft::perft(&mut b, row.depth),
                "{} d{}",
                g.fen,
                row.depth
            );
        }
    }
}
