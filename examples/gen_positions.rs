//! Le **collecteur de corpus** (phase NN, tranche N1) : des positions
//! jouées par le moteur lui-même, annotées par le résultat de la partie.
//!
//! # Ce qu'il fait
//!
//! Il fait **jouer** une série de parties par le MCTS du dépôt (auto-play)
//! et écrit, pour chaque position visitée, une ligne de corpus :
//!
//! ```text
//! zobrist | fen | résultat (cadre Blanc) | ply | partie
//! ```
//!
//! Le résultat est celui de la **partie** — la seule annotation honnête
//! dont on dispose : la valeur d'une recherche serait bruitée (c'est
//! mesuré, phase AB7), et une étiquette inventée apprendrait au réseau
//! à imiter l'évaluation statique, ce qu'on ne veut pas mesurer.
//!
//! # Les games qui n'ont pas de résultat sont **jetées**
//!
//! Une partie qui atteint la limite de demi-coups n'a pas de résultat :
//! on **jette la partie entière** plutôt que de l'arbitrer avec
//! l'évaluation statique. Arbitrer avec l'éval statique reviendrait à
//! écrire la réponse dans la question : le réseau apprendrait
//! l'évaluation statique, et la mesure « le réseau vaut mieux que
//! l'éval statique » n'aurait plus de sens.
//!
//! # Le déterminisme
//!
//! Tout est tiré d'une graine fixe : les ouvertures (dans l'ordre du
//! livre), les choix ε-greedy, et les graines de recherche. Deux
//! exécutions avec la même graine produisent **exactement** le même
//! fichier. C'est le gate de la tranche N1.
//!
//! # Usage
//!
//! ```text
//! cargo run --release --example gen_positions -- <positions> <sortie> \
//!     [--nodes N] [--plies N] [--seed S] [--book F] [--epsilon E]
//! ```

use mychess::corpus::{to_fen, write_corpus, Record};
use mychess::eval;
use mychess::mcts::{Limits, Rng, Search};
use mychess::movegen::{generate, has_legal_move, MoveList};
use mychess::position::Position;
use mychess::types::{Color, GenKind};
use std::sync::atomic::AtomicBool;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut target_positions: usize = 2000;
    let mut out_path = String::from("/tmp/corpus.txt");
    let mut nodes: u64 = 4_000;
    let mut max_plies: usize = 250;
    let mut seed: u64 = 20261023;
    let mut book = String::from("/tmp/openings_src/eco_lichess.epd");
    let mut epsilon: f32 = 0.10;
    // L'étiquette : `game` = le résultat de la partie ; `search` = la
    // **valeur de la recherche** (côté trait, dans l'échelle de la
    // valeur du MCTS).
    let mut label_kind = String::from("game");

    let mut positional: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--nodes" => {
                nodes = args[i + 1].parse().expect("--nodes");
                i += 2;
            }
            "--plies" => {
                max_plies = args[i + 1].parse().expect("--plies");
                i += 2;
            }
            "--seed" => {
                seed = args[i + 1].parse().expect("--seed");
                i += 2;
            }
            "--book" => {
                book = args[i + 1].clone();
                i += 2;
            }
            "--epsilon" => {
                epsilon = args[i + 1].parse().expect("--epsilon");
                i += 2;
            }
            "--label" => {
                label_kind = args[i + 1].clone();
                i += 2;
            }
            a if a.starts_with("--") => {
                eprintln!("option inconnue : {a}");
                std::process::exit(2);
            }
            a => {
                positional.push(a.to_string());
                i += 1;
            }
        }
    }
    if !positional.is_empty() {
        target_positions = positional[0].parse().expect("positions");
    }
    if positional.len() > 1 {
        out_path = positional[1].clone();
    }
    if positional.len() > 2 {
        eprintln!("argument en trop : {}", positional[2]);
        std::process::exit(2);
    }

    let openings = load_openings(&book);
    if openings.is_empty() {
        eprintln!("aucune ouverture lisible dans {book} — le corpus ne couvrira que la position initiale");
    }
    eprintln!(
        "collecteur : {target_positions} positions visées, {nodes} nœuds/move, {max_plies} demi-coups max, \
         graine {seed}, epsilon {epsilon}, {} ouvertures",
        openings.len()
    );

    let mut records: Vec<Record> = Vec::new();
    let mut games = 0usize;
    let mut discarded = 0usize;
    let mut wins_white = 0usize;
    let mut wins_black = 0usize;
    let mut draws = 0usize;
    let mut game_index = 0u32;

    while records.len() < target_positions {
        let opening = &openings[(game_index as usize) % openings.len().max(1)];
        game_index += 1;
        games += 1;
        let game_seed = seed
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(game_index as u64);
        match play_game(opening, nodes, max_plies, game_seed, epsilon, &label_kind) {
            Some((positions, result)) => {
                match result {
                    1 => wins_white += 1,
                    -1 => wins_black += 1,
                    _ => draws += 1,
                }
                let total_plies = positions.len();
                for (zobrist, fen, ply, label) in positions {
                    records.push(Record {
                        zobrist,
                        fen,
                        result,
                        ply,
                        game: game_index - 1,
                        label,
                    });
                }
                let _ = total_plies;
            }
            None => discarded += 1,
        }
        if records.len() >= target_positions {
            break;
        }
        if games % 20 == 0 {
            eprintln!(
                "  {games} parties, {} positions, {discarded} jetées",
                records.len()
            );
        }
    }

    // On **tronque** au nombre de positions demandé : le corpus est
    // ainsi reproductible *et* de taille exactement contrôlée.
    records.truncate(target_positions);

    let meta = vec![
        ("graine", seed.to_string()),
        ("parties", games.to_string()),
        ("parties jetees (limite de demi-coups)", discarded.to_string()),
        ("victoires blancs", wins_white.to_string()),
        ("victoires noirs", wins_black.to_string()),
        ("nulles", draws.to_string()),
        ("noeuds par move", nodes.to_string()),
        ("demi-coups max", max_plies.to_string()),
        ("epsilon", epsilon.to_string()),
        ("etiquette", label_kind.clone()),
        ("ouvertures", openings.len().to_string()),
    ];
    write_corpus(std::path::Path::new(&out_path), &records, &meta).expect("écriture du corpus");
    eprintln!(
        "corpus écrit : {} ({} positions, {} parties jouées, {} jetées)",
        out_path,
        records.len(),
        games,
        discarded
    );
}

/// Les positions d'ouverture : le livre EPD (une position par ligne), ou
/// la position initiale si le livre est absent.
fn load_openings(path: &str) -> Vec<Position> {
    let mut out = Vec::new();
    if let Ok(text) = std::fs::read_to_string(path) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Une ligne EPD : plateau, trait, castling, ep. On ajoute les
            // compteurs pour en faire un FEN.
            let fen = format!("{line} 0 1");
            if let Ok(pos) = Position::from_fen(&fen) {
                out.push(pos);
            }
        }
    }
    if out.is_empty() {
        out.push(Position::startpos());
    }
    out
}

/// Joue **une** partie en auto-play. Renvoie les positions visitées et
/// le résultat (cadre Blanc), ou `None` si la partie a atteint la limite
/// de demi-coups (et est donc **jetée**).
fn play_game(
    opening: &Position,
    nodes: u64,
    max_plies: usize,
    seed: u64,
    epsilon: f32,
    label_kind: &str,
) -> Option<(Vec<(u64, String, u32, Option<f32>)>, i8)> {
    let mut pos = opening.clone();
    let mut hist: Vec<u64> = vec![pos.zobrist];
    let mut seen: Vec<(u64, String, u32, Option<f32>)> = Vec::new();
    let mut rng = Rng::new(seed ^ 0xA5A5_A5A5_A5A5_A5A5);
    let stop = AtomicBool::new(false);

    for ply in 0..max_plies {
        // --- le résultat, s'il est déjà connu -----------------------------
        if !has_legal_move(&pos) {
            // Pas de coup légal : mat si le roi est en échec, sinon pat.
            let result: i8 = if pos.in_check(pos.side) {
                if pos.side == Color::White { -1 } else { 1 }
            } else {
                0
            };
            return Some((seen, result));
        }
        if eval::insufficient_material(&pos) {
            return Some((seen, 0));
        }
        if pos.halfmove >= 100 {
            return Some((seen, 0));
        }
        // Triple répétition : le zobrist courant apparaît trois fois
        // dans l'historique (l'historique contient la position courante).
        if hist.iter().filter(|&&z| z == pos.zobrist).count() >= 3 {
            return Some((seen, 0));
        }

        // --- le coup ------------------------------------------------------
        let limits = Limits {
            nodes,
            movetime_ms: None,
            ..Limits::default()
        };
        let search = Search::new(pos.clone(), hist.clone(), limits, seed ^ (ply as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let outcome = search.run(&stop);

        // L'étiquette : soit le résultat de la partie (ajouté à la fin),
        // soit la **valeur de la recherche** (le score de la racine,
        // ramené à l'échelle [-1, 1] de la valeur du MCTS : le score
        // fastchess utilise ×100, l'interne ×1000).
        let label = if label_kind == "search" {
            Some(((outcome.score_cp as f32) / 100.0).clamp(-1.0, 1.0))
        } else {
            None
        };
        seen.push((pos.zobrist, to_fen(&pos, (ply / 2) as u32 + 1), ply as u32, label));

        let mut list = MoveList::new();
        generate(&pos, &mut list, GenKind::All);
        if list.is_empty() {
            // Ne devrait pas arriver (le test ci-dessus l'a ruled out).
            return Some((seen, 0));
        }
        // Choix ε-greedy : avec une probabilité `epsilon`, un coup légal
        // est tiré au hasard — c'est ce qui **diversifie** le corpus
        // (sans lui, toutes les parties se ressemblent).
        let mv = if (rng.next_u64() >> 40) as f32 / ((1u64 << 24) as f32) < epsilon {
            list.get((rng.next_u64() % list.len() as u64) as usize)
        } else {
            outcome.best
        };
        pos.make(mv);
        hist.push(pos.zobrist);
    }
    // La partie n'a pas de résultat : elle est **jetée**.
    None
}
