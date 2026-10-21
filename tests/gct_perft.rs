//! Perft du générateur `gct`, validé contre la table détaillée Grand Chess
//! Tree (goldens de `tests/perft/gct/`, conversion de convention appliquée
//! ici comme dans `tests/gct_oracle.rs`).

use mychess::gct::perft::{perft as gct_perft, perft_detailed as gct_detailed};
use mychess::position::Position;

const SHALLOW_MAX_DEPTH: u32 = 4;
/// Trois positions × profondeurs 0 à 4 incluses.
const MIN_SHALLOW_ROWS: usize = 15;
const MID_MIN_DEPTH: u32 = 5;
const MID_MAX_DEPTH: u32 = 6;
/// Trois positions × profondeurs 5 et 6.
const MIN_MID_ROWS: usize = 6;

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
                "{path}:{lineno}: ligne de données invalide : 10 colonnes attendues, {} trouvées : {line}",
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
    let got = [c.captures, c.ep, c.castles, c.promotions, c.checks, c.mates];
    let want = [
        row.cols[0],
        row.cols[1],
        row.cols[2],
        row.cols[3],
        // `mychess.checks == gct.checks + gct.mates`.
        row.cols[4] + row.cols[5],
        row.cols[5],
    ];
    let names = ["captures", "ep", "castles", "promotions", "checks", "mates"];
    for i in 0..6 {
        assert_eq!(got[i], want[i], "{} d{} {}", g.fen, row.depth, names[i]);
    }
}

#[test]
fn t1_gct_perft_shallow() {
    let mut checked = 0usize;
    for g in all() {
        for row in &g.rows {
            if row.depth > SHALLOW_MAX_DEPTH {
                continue;
            }
            check(&g, row);
            checked += 1;
        }
    }
    assert!(
        checked >= MIN_SHALLOW_ROWS,
        "t1_gct_perft_shallow : seulement {checked} ligne(s) vérifiée(s), minimum attendu {MIN_SHALLOW_ROWS}"
    );
}

#[test]
#[ignore = "T2: perft gct aux profondeurs 5 et 6"]
fn t2_gct_perft_mid() {
    let mut checked = 0usize;
    for g in all() {
        for row in &g.rows {
            if !(MID_MIN_DEPTH..=MID_MAX_DEPTH).contains(&row.depth) {
                continue;
            }
            let started = std::time::Instant::now();
            check(&g, row);
            checked += 1;
            println!(
                "T2 {} d{} nodes={} elapsed={:.3}s",
                g.fen,
                row.depth,
                row.nodes,
                started.elapsed().as_secs_f64()
            );
        }
    }
    assert!(
        checked >= MIN_MID_ROWS,
        "t2_gct_perft_mid : seulement {checked} ligne(s) vérifiée(s), minimum attendu {MIN_MID_ROWS}"
    );
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
