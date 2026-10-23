//! Phase AB8 — **les heuristiques de l'alpha-beta, mesurées sur les
//! deux axes** (débit et bruit).
//!
//! La phase AB1 (`docs/ab/phase1-2026-10-22.md`) a établi une
//! *corrélation* : à évaluation identique, l'alpha-beta est ~80 Elo plus
//! faible que le MCTS, et l'évaluation statique se trompe de ~490 cp.
//! Le diagnostic proposé — « l'alpha-beta **amplifie** le bruit de la
//! valeur, le MCTS l'**amortit** » — n'est mesuré directement que par
//! l'instrument d'AB7 (`examples/bruit_amplification`).
//!
//! Cet instrument pose la question **suivante** : *une heuristique
//! d'élagage classique est-elle un gain pour ce moteur ?* Le plan AB4
//! demandait « plus de débit et plus de score » ; le plan AB7 §6.3
//! demande en plus : « une heuristique qui augmente la profondeur *et*
//! le bruit n'est pas un gain ».
//!
//! # Les quatre mesures
//!
//! Par configuration, sur le **même** corpus de positions :
//!
//! | métrique | ce qu'elle dit | comment |
//! |---|---|---|
//! | **débit** | des nœuds en moins ? | nombre de nœuds moyen à profondeur 5 |
//! | **profondeur** | à temps égal, une profondeur de plus ? | profondeur **complète** atteinte à `movetime 200 ms` |
//! | **score** | la profondeur gagnée se traduit-elle en score ? | score moyen à profondeur 5, bruit nul |
//! | **amplification** | la profondeur gagnée se paye-t-elle en bruit ? | facteur d'amplification d'AB7 |
//!
//! Le facteur d'amplification est mesuré à **deux** budgets :
//!
//! * à **budget de nœuds égal** (100 k nœuds) : les deux recherches
//!   explorent le même nombre de nœuds — l'écart de score est donc
//!   l'effet du bruit à budget constant ;
//! * à **temps égal** (200 ms) : c'est le régime réel, et la profondeur
//!   atteinte est reportée à côté, parce que le bruit ne fait pas
//!   qu'écarter la valeur : il **casse l'élagage** (le rapport de la
//!   phase AB, §3.4), donc il coûte de la profondeur.
//!
//! # Le plafond de nœuds
//!
//! Toutes les recherches sont plafonnées à `NODE_CAP` nœuds. Ce
//! plafond n'est pas une précaution de façade : avec un bruit de 490 cp,
//! une recherche à profondeur 3 sur une position du livre peut coûter
//! **1,16 milliard de nœuds** (contre ≈ 10 000 sans bruit). Le
//! plafond borne l'explosion ; il ne change pas la lecture, parce que
//! la profondeur atteinte est reportée à côté.
//!
//! # Usage
//!
//! ```text
//! cargo run --release --example heuristiques_ab -- [livre.epd] [positions] [amplitudes]
//! ```
//!
//! `amplitudes` est une liste de centipions séparés par des virgules
//! (`250,490`). Le livre par défaut est le livre ECO du projet.

use mychess::ab::{self, Limits as AbLimits, Options};
use mychess::eval;
use mychess::position::Position;
use std::sync::atomic::AtomicBool;

/// La graine du champ de bruit : **fixe** (celle d'AB7), pour que le
/// champ soit le même d'un point de la courbe à l'autre.
const NOISE_SEED: u64 = eval::DEFAULT_NOISE_SEED;

/// Le budget temps de la mesure « à temps égal ».
const TIME_MS: u128 = 200;

/// Le plafond de nœuds (sécurité : le bruit peut faire exploser l'arbre).
const NODE_CAP: u64 = 2_000_000;

/// La profondeur de la mesure « débit / score » (bruit nul).
const FIXED_DEPTH: u32 = 5;

/// Le budget de nœuds de la mesure « amplification à budget égal ».
const AMP_NODES: u64 = 100_000;

/// Les configurations comparées. Chacune est mesurée dans les mêmes
/// conditions que les autres (mêmes positions, même graine).
fn configs() -> Vec<(&'static str, Options)> {
    let base = Options::default();
    let mut v: Vec<(&'static str, Options)> = vec![("base (AB gelee)", base)];
    v.push((
        "sans PVS",
        Options {
            pvs: false,
            ..base
        },
    ));
    v.push((
        "sans killers",
        Options {
            killers: false,
            ..base
        },
    ));
    v.push((
        "sans histoire",
        Options {
            history: false,
            ..base
        },
    ));
    v.push((
        "+ null-move",
        Options {
            null_move: true,
            ..base
        },
    ));
    v.push((
        "+ razoring",
        Options {
            razoring: true,
            ..base
        },
    ));
    v.push((
        "+ futilite",
        Options {
            futility: true,
            ..base
        },
    ));
    v.push((
        "+ LMR",
        Options {
            lmr: true,
            ..base
        },
    ));
    v.push((
        "tout",
        Options {
            null_move: true,
            razoring: true,
            futility: true,
            lmr: true,
            ..base
        },
    ));
    v
}

/// Le résultat d'une recherche : score (côté au trait), profondeur
/// atteinte, nœuds.
#[derive(Clone, Copy)]
struct R {
    score: i32,
    depth: u32,
    nodes: u64,
}

/// Une recherche de l'alpha-beta avec une configuration donnée.
///
/// `depth` et `time_ms` ne sont pas exclusifs : le **plafond de nœuds**
/// s'applique toujours (cf. le module : le bruit fait exploser
/// l'arbre).
fn search(opts: Options, pos: &Position, depth: u32, time_ms: Option<u128>, nodes: u64) -> R {
    let hist = vec![pos.zobrist];
    let limits = AbLimits {
        max_depth: depth,
        movetime_ms: time_ms,
        nodes,
    };
    let o = ab::Search::with_options(*pos, hist, limits, opts).run(&AtomicBool::new(false));
    R {
        score: o.score_cp,
        depth: o.depth,
        nodes: o.nodes,
    }
}

/// Le livre de positions : une FEN par ligne (EPD ou FEN).
fn default_book() -> &'static str {
    for cand in [
        "/tmp/openings_src/eco_lichess.epd",
        "openings/eco_lichess.epd",
        "openings/builtin_min.epd",
    ] {
        if std::path::Path::new(cand).exists() {
            return cand;
        }
    }
    "openings/builtin_min.epd"
}

fn load_positions(path: &str, max: usize) -> Vec<Position> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("lecture impossible de {path} : {e}");
            std::process::exit(2);
        }
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fen = format!(
            "{} 0 1",
            line.split_whitespace().take(4).collect::<Vec<_>>().join(" ")
        );
        if let Ok(p) = Position::from_fen(&fen) {
            out.push(p);
            if out.len() >= max {
                break;
            }
        }
    }
    out
}

/// La moyenne d'une grandeur.
fn mean(v: &[f64]) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f64>() / v.len() as f64
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| default_book().to_string());
    let max_positions: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20);
    let amplitudes: Vec<u32> = args
        .get(3)
        .map(|s| {
            s.split(',')
                .filter_map(|d| d.trim().parse::<u32>().ok())
                .filter(|&d| d > 0)
                .collect()
        })
        .unwrap_or_else(|| vec![250, 490]);

    let positions = load_positions(&path, max_positions);
    if positions.is_empty() {
        eprintln!("aucune position lisible dans {path}");
        std::process::exit(2);
    }
    let configs = configs();

    println!("# AB8 — les heuristiques de l'alpha-beta, mesurées sur les deux axes");
    println!("# livre : {path} — {} positions", positions.len());
    println!("# graine du bruit : {NOISE_SEED:#x} (fixe)");
    println!(
        "# profondeur « debit/score » : {FIXED_DEPTH} — budget « amplification a noeuds egaux » : {AMP_NODES} — budget temps : {TIME_MS} ms — plafond de noeuds : {NODE_CAP}"
    );
    println!("# le facteur d'amplification est sans dimension : (valeur bruitee - valeur nette) / A");

    // Le **champ de bruit** est le même pour toutes les amplitudes
    // (c'est la propriété testée par `tests/eval_noise.rs`).
    print!("# |champ de bruit| (cp) :");
    for &a in &amplitudes {
        eval::set_noise(a, NOISE_SEED);
        let offs: Vec<i64> = positions.iter().map(|p| eval::noise_offset(p) as i64).collect();
        let mean_off = mean(&offs.iter().map(|v| *v as f64).collect::<Vec<_>>());
        let var = mean(
            &offs
                .iter()
                .map(|v| (*v as f64 - mean_off).powi(2))
                .collect::<Vec<_>>(),
        );
        print!(
            "  A={a}: moyenne {mean_off:+.1}, ecart-type {:.1}",
            var.max(0.0).sqrt()
        );
    }
    println!();

    for (label, opts) in &configs {
        eprintln!("# mesure : {label}");

        // --- le débit et le score, à profondeur fixée, bruit nul ---
        let fixed: Vec<R> = positions
            .iter()
            .map(|p| {
                eval::clear_noise();
                search(*opts, p, FIXED_DEPTH, None, NODE_CAP)
            })
            .collect();

        // --- la référence « à temps égal », bruit nul ---
        let timed: Vec<R> = positions
            .iter()
            .map(|p| {
                eval::clear_noise();
                search(*opts, p, 0, Some(TIME_MS), NODE_CAP)
            })
            .collect();

        // --- la référence « à budget de nœuds égal », bruit nul ---
        let budgeted: Vec<R> = positions
            .iter()
            .map(|p| {
                eval::clear_noise();
                search(*opts, p, 0, None, AMP_NODES)
            })
            .collect();

        // --- l'amplification, à budget de nœuds égal et à temps égal ---
        let mut amp_budget: Vec<Vec<f64>> = vec![Vec::new(); amplitudes.len()];
        let mut amp_timed: Vec<Vec<f64>> = vec![Vec::new(); amplitudes.len()];
        let mut depth_noisy: Vec<Vec<f64>> = vec![Vec::new(); amplitudes.len()];
        for (ai, &a) in amplitudes.iter().enumerate() {
            for (pi, p) in positions.iter().enumerate() {
                eval::set_noise(a, NOISE_SEED);
                // Budget de nœuds égal : les deux recherches explorent le
                // même nombre de nœuds.
                let noisy_budget = search(*opts, p, 0, None, AMP_NODES);
                amp_budget[ai].push((noisy_budget.score - budgeted[pi].score) as f64 / a as f64);

                // Temps égal : le régime réel. Le bruit y coûte de la
                // profondeur (l'élagage ne coupe plus) — c'est ce que la
                // profondeur reportée montre.
                let noisy_timed = search(*opts, p, 0, Some(TIME_MS), NODE_CAP);
                amp_timed[ai].push((noisy_timed.score - timed[pi].score) as f64 / a as f64);
                depth_noisy[ai].push(noisy_timed.depth as f64);
            }
        }
        eval::clear_noise();

        let nodes = mean(&fixed.iter().map(|r| r.nodes as f64).collect::<Vec<_>>());
        let score_fixed = mean(&fixed.iter().map(|r| r.score as f64).collect::<Vec<_>>());
        let score_timed = mean(&timed.iter().map(|r| r.score as f64).collect::<Vec<_>>());
        let depth_timed = mean(&timed.iter().map(|r| r.depth as f64).collect::<Vec<_>>());

        print!("{label} | noeuds@{FIXED_DEPTH} {nodes:.0}");
        print!(" | score@{FIXED_DEPTH} {score_fixed:+.1}");
        print!(" | prof@{TIME_MS}ms {depth_timed:.2}");
        print!(" | score@{TIME_MS}ms {score_timed:+.1}");
        for (ai, &a) in amplitudes.iter().enumerate() {
            print!(
                " | A={a} amp@noeuds {:.3} amp@temps {:.3} prof_bruitee {:.2}",
                mean(&amp_budget[ai]),
                mean(&amp_timed[ai]),
                mean(&depth_noisy[ai])
            );
        }
        println!();
        use std::io::Write;
        std::io::stdout().flush().ok();
    }
    eval::clear_noise();
}
