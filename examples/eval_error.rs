//! AB5 (partiel) — la qualité de la **fonction de valeur**.
//!
//! L'alpha-beta et la feuille du MCTS utilisent **la même** évaluation
//! (`eval::eval_for_mover` : matériel + PST PeSTO-lite). Cette mesure
//! quantifie ce que cette évaluation vaut réellement, et ce que la
//! recherche en fait :
//!
//! * **l'erreur de l'évaluation** : `|eval(pos) − score(recherche)|`.
//!   L'évaluation statique est la *vérité terrain* de la recherche —
//!   si elle se trompe de 300 cp, la recherche optimise le mauvais
//!   critère, quelle que soit la profondeur atteinte ;
//! * **l'instabilité de la recherche** : `|score(d) − score(d+1)|`.
//!   C'est le bruit de l'évaluation **amplifié par la profondeur** : un
//!   alpha-beta qui creuse dans un arbre bruité suit le bruit, et joue
//!   moins bien qu'un alpha-beta moins profond.
//!
//! Le MCTS, lui, **moyenne** ses feuilles : il amortit ce bruit. Les
//! deux algorithmes utilisent donc la même fonction de valeur, mais ne
//! l'exploitent pas de la même façon — c'est le cœur du diagnostic.
//!
//! Usage :
//!   cargo run --release --example eval_error -- <fichier.fen> [profondeurs]
//!
//! Un fichier de positions : une FEN par ligne (`#` = commentaire). À
//! défaut, le livre ECO du dépôt est utilisé.

use mychess::ab::{self, Limits};
use mychess::eval;
use mychess::position::Position;
use std::sync::atomic::AtomicBool;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "/tmp/openings_src/eco_lichess.epd".to_string());
    let depths: Vec<u32> = args
        .get(2)
        .map(|s| {
            s.split(',')
                .filter_map(|d| d.trim().parse::<u32>().ok())
                .collect()
        })
        .unwrap_or_else(|| vec![4, 6]);

    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("lecture impossible de {path} : {e}");
            std::process::exit(2);
        }
    };

    let max_positions = args
        .get(3)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(usize::MAX);
    let mut n = 0usize;
    let mut err_sum = vec![0i64; depths.len()];
    let mut inst_sum = 0i64;
    let mut pairs = 0usize;
    let mut scores: Vec<Vec<i32>> = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Une ligne EPD : les quatre premiers champs sont la FEN
        // (placement, trait, roque, prise en passant).
        let fen = format!(
            "{} 0 1",
            line.split_whitespace()
                .take(4)
                .collect::<Vec<_>>()
                .join(" ")
        );
        let Ok(pos) = Position::from_fen(&fen) else {
            continue;
        };
        n += 1;
        if n >= max_positions {
            break;
        }
        let stat = eval::eval_for_mover(&pos);
        let mut row = Vec::new();
        for (i, d) in depths.iter().enumerate() {
            let hist = vec![pos.zobrist];
            let limits = Limits {
                max_depth: *d,
                movetime_ms: None,
                nodes: u64::MAX,
            };
            let o = ab::Search::new(pos, hist, limits).run(&AtomicBool::new(false));
            err_sum[i] += (o.score_cp - stat).abs() as i64;
            row.push(o.score_cp);
        }
        if row.len() >= 2 {
            inst_sum += (row[1] - row[0]).abs() as i64;
            pairs += 1;
        }
        scores.push(row);
        if n % 25 == 0 {
            eprintln!("  {n} positions…");
        }
    }

    println!("positions mesurees : {n}");
    for (i, d) in depths.iter().enumerate() {
        println!(
            "profondeur {d} : erreur moyenne |eval - score| = {:.1} cp",
            err_sum[i] as f64 / n as f64
        );
    }
    if pairs > 0 {
        println!(
            "instabilite moyenne |score({}) - score({})| = {:.1} cp",
            depths[0],
            depths[1],
            inst_sum as f64 / pairs as f64
        );
    }
    // Les scores d'un seul exemple, pour voir la dispersion.
    println!("premiers scores (profondeur {}) :", depths[0]);
    let head: Vec<String> = scores.iter().take(12).map(|r| r[0].to_string()).collect();
    println!("  {}", head.join(" "));
}
