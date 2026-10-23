//! Phase AB7 — **l'amplification du bruit de la valeur**.
//!
//! La phase AB1 (`docs/ab/phase1-2026-10-22.md`) a établi une
//! *corrélation* : à évaluation identique, l'alpha-beta est ~80 Elo plus
//! faible que le MCTS, et l'évaluation statique se trompe de ~490 cp. Le
//! diagnostic proposé — « l'alpha-beta **amplifie** le bruit de la valeur
//! (il prend le maximum sur un arbre), le MCTS l'**amortit** (il moyenne
//! ses feuilles) » — n'était qu'une explication plausible.
//!
//! Cet instrument la teste **directement**, sans passer par des parties :
//! il mesure de combien la **valeur racine** de chaque recherche bouge
//! quand on bruite la fonction de valeur.
//!
//! # Le protocole
//!
//! Pour chaque position d'un corpus, et pour chaque amplitude `A` :
//!
//! 1. la valeur racine est calculée avec le bruit **activé** (`A` cp) ;
//! 2. la valeur racine est calculée avec le bruit **nul** (`A = 0`) ;
//! 3. l'écart est normalisé par `A` : c'est le **facteur
//!    d'amplification**, sans dimension.
//!
//! Un facteur **nul** signifie « le bruit n'a aucune influence » ; un
//! facteur **positif** signifie « la recherche *suit* le bruit » ; un
//! facteur **négatif** signifie « la recherche le *repousse* ».
//!
//! # Ce que l'hypothèse prédit
//!
//! * **Alpha-beta** : la valeur racine est un **maximum** sur un arbre de
//!   feuilles bruitées. Le maximum *selectionne* les feuilles au bruit
//!   favorable ⇒ l'espérance du score est **biaisée vers le haut** et
//!   l'écart croît **avec la profondeur** (plus de feuilles ⇒ plus de
//!   chances de trouver une crête de bruit). Le facteur d'amplification
//!   doit donc **croître avec la profondeur**.
//!
//! * **MCTS** : la valeur racine est une **moyenne** sur les feuilles
//!   visitées. Lesoffsets de bruit de positions distinctes sont
//!   indépendants ⇒ la moyenne amortit le bruit, d'autant mieux que le
//!   budget de nœuds est grand. Le facteur d'amplification doit donc
//!   **décroître avec le budget** et rester **proche de zéro**.
//!
//! C'est le test décisif : si le facteur de l'alpha-beta croît avec la
//! profondeur tandis que celui du MCTS décroît avec le budget,
//! l'hypothèse est **confirmée** — et la phase NN (valeur apprise) est
//! justifiée, pour les deux algorithmes.
//!
//! # Usage
//!
//! ```text
//! cargo run --release --example bruit_amplification -- [livre.epd] [positions] [amplitudes]
//! ```
//!
//! `amplitudes` est une liste de centipions séparés par des virgules
//! (`0,100,250,490`). Le livre par défaut est le livre ECO du projet
//! (`/tmp/openings_src/eco_lichess.epd`), sinon `openings/builtin_min.epd`.

use mychess::ab::{self, Limits as AbLimits};
use mychess::eval;
use mychess::mcts::{Limits as MctsLimits, Search as MctsSearch};
use mychess::position::Position;
use std::sync::atomic::AtomicBool;

/// La graine du champ de bruit : **fixe**, pour que le champ soit le
/// même d'un point de la courbe à l'autre (c'est l'amplitude qui varie,
/// pas le bruit).
const NOISE_SEED: u64 = eval::DEFAULT_NOISE_SEED;

/// La graine de recherche : fixe, pour que les deux algorithmes
/// comparés fassent exactement le même travail à bruit nul.
const SEARCH_SEED: u64 = 42;

/// Les configurations de recherche comparées. Chacune est mesurée dans
/// les mêmes conditions aux amplitudes différentes.
struct Config {
    /// Le libellé (affiché dans le rapport).
    label: &'static str,
    /// `Some(depth)` : un alpha-beta à profondeur fixée.
    ab_depth: Option<u32>,
    /// `Some(nodes)` : un MCTS à budget de nœuds fixé.
    mcts_nodes: Option<u64>,
}

const CONFIGS: &[Config] = &[
    Config {
        label: "ab d3",
        ab_depth: Some(3),
        mcts_nodes: None,
    },
    Config {
        label: "ab d4",
        ab_depth: Some(4),
        mcts_nodes: None,
    },
    Config {
        label: "mcts 2k",
        ab_depth: None,
        mcts_nodes: Some(2_000),
    },
    Config {
        label: "mcts 5k",
        ab_depth: None,
        mcts_nodes: Some(5_000),
    },
];

/// La valeur racine d'une recherche, dans l'échelle de son moteur.
fn root_value(cfg: &Config, pos: &Position) -> i32 {
    let hist = vec![pos.zobrist];
    if let Some(d) = cfg.ab_depth {
        let limits = AbLimits {
            max_depth: d,
            movetime_ms: None,
            nodes: u64::MAX,
        };
        ab::Search::new(*pos, hist, limits).run(&AtomicBool::new(false)).score_cp
    } else {
        let n = cfg.mcts_nodes.expect("une configuration a un moteur");
        let limits = MctsLimits {
            nodes: n,
            movetime_ms: None,
            max_playout_ply: 100,
            c_uct: std::f32::consts::SQRT_2,
            c_puct: mychess::mcts::DEFAULT_C_PUCT,
            root_noise: mychess::mcts::DEFAULT_ROOT_NOISE,
            leaf_q: 0,
        };
        MctsSearch::new(*pos, hist, limits, SEARCH_SEED)
            .run(&AtomicBool::new(false))
            .score_cp
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

/// Le livre de positions : une FEN par ligne (EPD ou FEN).
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
        // Une ligne EPD : les quatre premiers champs sont la FEN
        // (placement, trait, roque, prise en passant).
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| default_book().to_string());
    let max_positions: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(40);
    let amplitudes: Vec<u32> = args
        .get(3)
        .map(|s| {
            s.split(',')
                .filter_map(|d| d.trim().parse::<u32>().ok())
                .filter(|&d| d > 0)
                .collect()
        })
        .unwrap_or_else(|| vec![100, 250, 490]);

    let positions = load_positions(&path, max_positions);
    if positions.is_empty() {
        eprintln!("aucune position lisible dans {path}");
        std::process::exit(2);
    }
    println!("# AB7 — amplification du bruit de la valeur");
    println!("# livre : {path} — {} positions", positions.len());
    println!("# graine du bruit : {NOISE_SEED:#x} (fixe) — graine de recherche : {SEARCH_SEED}");
    println!("# le facteur est sans dimension : (valeur bruitee - valeur nette) / amplitude");

    // La valeur **nette** (bruit nul) de chaque position et de chaque
    // configuration : la référence de toutes les amplitudes.
    let mut base: Vec<Vec<i32>> = Vec::with_capacity(CONFIGS.len());
    eval::clear_noise();
    for cfg in CONFIGS {
        base.push(positions.iter().map(|p| root_value(cfg, p)).collect());
    }

    // Le champ de bruit, une fois, pour toutes les amplitudes : c'est la
    // même *forme*, mise à l'échelle. On le mesure pour le rapporter.
    println!("# |champ de bruit| : écart type et étendue des offsets (cp)");
    for &a in &amplitudes {
        eval::set_noise(a, NOISE_SEED);
        let mut sum = 0i64;
        let mut sum_sq = 0i64;
        let mut min = i32::MAX;
        let mut max = i32::MIN;
        for p in &positions {
            let o = eval::noise_offset(p);
            sum += o as i64;
            sum_sq += (o as i64) * (o as i64);
            min = min.min(o);
            max = max.max(o);
        }
        let n = positions.len() as f64;
        let mean = sum as f64 / n;
        let var = (sum_sq as f64 / n) - mean * mean;
        println!(
            "#   A={a:4} cp : moyenne {mean:+.1} cp, ecart-type {:.1} cp, etendue [{min}, {max}]",
            var.max(0.0).sqrt()
        );
    }

    println!();
    println!("config | amplitude | ecart moyen / A | ecart-type / A | |ecart| moyen / A");
    println!("---|---|---|---|---");
    for (ci, cfg) in CONFIGS.iter().enumerate() {
        for &a in &amplitudes {
            eval::set_noise(a, NOISE_SEED);
            let mut sum = 0i64;
            let mut sum_sq = 0i64;
            let mut abs_sum = 0i64;
            for (pi, p) in positions.iter().enumerate() {
                let d = (root_value(cfg, p) - base[ci][pi]) as i64;
                sum += d;
                sum_sq += d * d;
                abs_sum += d.abs();
            }
            let n = positions.len() as f64;
            let mean = sum as f64 / n;
            let var = (sum_sq as f64 / n) - mean * mean;
            let sd = var.max(0.0).sqrt();
            let abs_mean = abs_sum as f64 / n;
            println!(
                "{} | {a:4} cp | {:+.3} | {:.3} | {:.3}",
                cfg.label, mean / a as f64, sd / a as f64, abs_mean / a as f64
            );
        }
    }
    eval::clear_noise();
}
