//! Le code **partagé** par les tests d'intégration.
//!
//! Rust ne partage pas de code entre les binaires de test par
//! défaut : ce module est inclus par `mod common;` dans chaque test qui
//! en a besoin. Il n'est **pas** compilé comme un binaire de test
//! lui-même (c'est un sous-répertoire).

/// Le **retournement vertical** d'un FEN : les rangs sont inversés, les
/// fichiers sont **conservés**, et les **couleurs des pièces sont
/// échangées** (majuscule <-> minuscule).
///
/// # Pourquoi cette transformation
///
/// C'est la symétrie **exacte** de l'évaluation : l'évaluation lit la
/// même table pour les deux camps, dans le cadre de chacun (les blancs
/// lisent `PSTV[kind][sq ^ 56]`, les noirs `PSTV[kind][sq]`). Le
/// retournement vertical **plus** l'échange des couleurs fait donc
/// changer **exactement** de signe la contribution de chaque pièce.
///
/// La rotation de 180° (qui inverse aussi les fichiers) n'est **pas**
/// une symétrie : les tables PeSTO ne sont pas symétriques en fichiers
/// (a2 et h2 n'ont pas la même valeur) — l'écart mesuré est de 30 à
/// 45 cp. Le retournement **gauche-droite** non plus.
///
/// # Les droits de roque
///
/// Après le retournement, le roi blanc passe du 1er au 8e rang (il ne
/// peut plus roquer) et le roi noir du 8e au 1er rang (il le peut) :
/// les lettres de roque sont donc **échangées** d'un camp à l'autre.
pub fn flip_vertically(fen: &str) -> String {
    let p: Vec<&str> = fen.split_whitespace().collect();
    // 1. Les **rangs** sont inversés.
    // 2. Les **couleurs** des pièces sont échangées (majuscule <->
    //    minuscule) : sans cela, ce ne serait pas un miroir mais une
    //    position où les blancs joueraient depuis le 8e rang — et
    //    l'anti-symétrie de l'évaluation ne s'y applique pas.
    let board: String = p[0]
        .split('/')
        .rev()
        .collect::<Vec<_>>()
        .join("/")
        .chars()
        .map(|c| {
            if c.is_ascii_uppercase() {
                c.to_ascii_lowercase()
            } else if c.is_ascii_lowercase() {
                c.to_ascii_uppercase()
            } else {
                c
            }
        })
        .collect();
    let side = if p[1] == "w" { "b" } else { "w" };
    let mut white = String::new();
    let mut black = String::new();
    if let Some(c) = p.get(2) {
        for ch in c.chars() {
            match ch {
                'K' => black.push('k'),
                'Q' => black.push('q'),
                'k' => white.push('K'),
                'q' => white.push('Q'),
                _ => {}
            }
        }
    }
    let castle = if white.is_empty() && black.is_empty() {
        "-".to_string()
    } else {
        format!("{white}{black}")
    };
    let ep = match p.get(3) {
        Some(e) if *e != "-" && e.len() == 2 => {
            let f = e.as_bytes();
            let rank = if f[1] == b'3' { '6' } else { '3' };
            format!("{}{rank}", f[0] as char)
        }
        _ => "-".to_string(),
    };
    let halfmove = p.get(4).copied().unwrap_or("0");
    let fullmove = p.get(5).copied().unwrap_or("1");
    format!("{board} {side} {castle} {ep} {halfmove} {fullmove}")
}
