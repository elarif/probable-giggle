//! Diagnostic — **`0000` sur une position non terminale**.
//!
//! Un moteur UCI qui n'a pas de coup à jouer doit répondre `bestmove 0000`
//! **seulement** si la position est terminale (mat ou pat). Répondre `0000`
//! dans une position où il existe des coups légaux est un **défaut** :
//! l'adversaire (ou l'arbitre) le signale comme « coup illégal » et la
//! partie est perdue sur une position qui était jouable.
//!
//! Ce probe rejoue une partie (`<fen> moves ...`) et **diagnostique** la
//! terminaison que la recherche MCTS en déduit, en testant une par une
//! les quatre conditions de `mcts::terminal_value` :
//!
//! 1. `has_legal_move` — le prédicat optimisé ;
//! 2. le compteur de la règle des 50 coups (`halfmove >= 100`) ;
//! 3. la **troisième répétition** (clé de Zobrist de la position
//!    courante retrouvée deux fois dans l'historique) ;
//! 4. le matériel insuffisant.
//!
//! Il affiche aussi le coup rendu par la recherche : c'est le chemin qui
//! a produit le `0000` du tournoi round-robin du 2026-10-22.
//!
//! ```text
//! cargo run --release --example legal_move_probe -- "<fen>" [moves...]
//! ```

use mychess::eval;
use mychess::mcts::{Limits, Policy, Search};
use mychess::movegen::{generate_all, has_legal_move};
use mychess::position::Position;
use mychess::types::Move;

fn main() {
    let mut args = std::env::args().skip(1);
    let fen = args
        .next()
        .unwrap_or_else(|| "2r2k2/2B4Q/p2PpbpP/1p6/5R2/5p2/P4P2/3K4 b - - 0 31".to_string());
    let moves: Vec<String> = args.collect();

    let mut pos = Position::from_fen(&fen).expect("FEN invalide");
    // L'historique Zobrist, comme le construit la couche UCI : la position
    // initiale, puis la position après chaque coup.
    let mut hist: Vec<u64> = vec![pos.zobrist];
    let mut unknown = 0usize;
    for tok in &moves {
        let found = generate_all(&pos).iter().find(|mv| mv.uci() == *tok);
        match found {
            Some(mv) => {
                pos.make(mv);
                hist.push(pos.zobrist);
            }
            None => {
                unknown += 1;
                eprintln!("coup inconnu (ignore) : {tok}");
            }
        }
    }

    println!(
        "coups rejoues            : {} (inconnus : {unknown})",
        moves.len()
    );
    println!("coups legaux generes     : {}", generate_all(&pos).len());
    println!("has_legal_move           : {}", has_legal_move(&pos));
    println!("en echec                 : {}", pos.in_check(pos.side));
    println!("compteur 50 coups        : {}", pos.halfmove);
    println!(
        "materiel insuffisant     : {}",
        eval::insufficient_material(&pos)
    );

    // La règle de répétition, telle que la recherche l'applique.
    let len = hist.len();
    let window = (pos.halfmove as usize).min(len - 1);
    let h = pos.zobrist;
    let mut count = 0usize;
    let mut positions: Vec<usize> = Vec::new();
    for (i, &x) in hist[len - 1 - window..len - 1].iter().rev().enumerate() {
        if x == h {
            count += 1;
            positions.push(len - 1 - window + i);
        }
    }
    println!("occurrences de la cle    : {count} (regle : 3 => repetition)");
    println!("indices dans l'historique : {positions:?}");
    println!("terminal selon la regle  : {}", count >= 2);

    // Le chemin de la recherche : c'est lui qui produit le `bestmove`.
    let search = Search::new(pos, hist, Limits::default(), 0x5eed).with_policy(Policy::Puct);
    let out = search.run(&std::sync::atomic::AtomicBool::new(false));
    let best = if out.best == Move::NONE {
        mychess::uci::fallback_move(&pos)
    } else {
        out.best
    };
    println!(
        "meilleur coup de la recherche  : {:?} ({})",
        best,
        best.uci()
    );
    println!("coups joues                    : {}", out.nodes);
}
