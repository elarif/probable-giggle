//! Le **corpus d'entraînement** (phase NN, tranche N1) : le format sur
//! disque, et l'écrivain / le lecteur de FEN.
//!
//! # Le format
//!
//! Un fichier texte, **une position par ligne**, les champs séparés par
//! `|` :
//!
//! ```text
//! # mychess corpus v1
//! # zobrist | fen | resultat | ply | partie
//! 3f2a... | rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1 | 1 | 0 | 0
//! ```
//!
//! * `zobrist` — la clé de la position (16 hexadécimaux) : elle permet
//!   de vérifier qu'une position relue est **exactement** celle qui a
//!   été jouée ;
//! * `fen` — la position complète, dans le format FEN standard (c'est
//!   ce qui rend le corpus **lisible par un tiers**, et rejouable par
//!   n'importe quel moteur) ;
//! * `resultat` — le résultat de la partie, dans le **cadre Blanc** :
//!   `+1` (victoire des blancs), `0` (nulle), `-1` (victoire des noirs).
//!   C'est la seule annotation du corpus : le **résultat de la partie**,
//!   pas la valeur d'une recherche (bruyante) ;
//! * `ply` — le demi-coup de la position dans la partie (0 = position de
//!   départ) ;
//! * `partie` — l'index de la partie (pour la reproductibilité et pour
//!   une découpe future train/test par partie).
//!
//! # Pourquoi un format texte
//!
//! Le corpus est un **artefact de mesure** autant qu'une donnée
//! d'entraînement : il doit être relu, comparé, et son contenu doit être
//! visible. Un format binaire serait plus rapide mais opaque ; le coût
//! d'analyse est de quelques secondes, une fois.

use crate::position::Position;
use crate::types::{BISHOP, KNIGHT, PAWN, QUEEN, ROOK, WHITE};

/// La version du format de corpus.
pub const CORPUS_VERSION: usize = 1;

/// Un enregistrement du corpus.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    /// La clé Zobrist de la position (elle inclut le trait).
    pub zobrist: u64,
    /// La position, en FEN.
    pub fen: String,
    /// Le résultat de la partie, dans le **cadre Blanc** : `+1`, `0`,
    /// `-1`.
    pub result: i8,
    /// Le demi-coup de la position dans la partie (0 = le départ).
    pub ply: u32,
    /// L'index de la partie.
    pub game: u32,
    /// L'étiquette **continue** de la position, côté trait, dans
    /// `[-1, 1]` : la **valeur de la recherche** (la position est
    /// valorisée par une recherche, pas par le résultat final de la
    /// partie). Absente pour un corpus annoté par le seul résultat de
    /// partie.
    ///
    /// Pourquoi une étiquette continue : le résultat de partie est une
    /// annotation **très** bruyante — sa variance autour de la vraie
    /// valeur est de l'ordre de 0,5 pour une position équilibrée, ce
    /// qui met l'évaluation statique (matériel + PST) **au plancher du
    /// bruit**. La valeur de la recherche, elle, est une moyenne sur
    /// l'arbre : elle est bien plus proche de la vraie valeur, et c'est
    /// elle qu'un réseau peut apprendre.
    pub label: Option<f32>,
}

impl Record {
    /// La **cible** d'entraînement, dans l'échelle de la valeur du
    /// projet : **côté trait**, dans `[-1, 1]`.
    ///
    /// Le résultat est exprimé dans le cadre Blanc ; il faut donc le
    /// multiplier par le signe du trait pour obtenir la valeur du
    /// position *vue par le joueur au trait* — qui est la convention de
    /// `eval::eval_for_mover` et de la feuille du MCTS.
    pub fn target(&self, white_to_move: bool) -> f32 {
        // L'étiquette continue (la valeur de la recherche) est déjà
        // côté trait ; sinon, c'est le résultat de partie, exprimé dans
        // le cadre Blanc, qu'il faut remettre dans le cadre du trait.
        if let Some(l) = self.label {
            return l;
        }
        let r = self.result as f32;
        if white_to_move { r } else { -r }
    }
}

/// Le caractère FEN d'une pièce (le code `mailbox` est
/// `1 + couleur * 6 + genre`).
fn piece_char(code: u8) -> char {
    let c = (code - 1) as usize;
    let kind = c % 6;
    let color = c / 6;
    let ch = match kind {
        PAWN => 'p',
        KNIGHT => 'n',
        BISHOP => 'b',
        ROOK => 'r',
        QUEEN => 'q',
        _ => 'k',
    };
    if color == WHITE {
        ch.to_ascii_uppercase()
    } else {
        ch
    }
}

/// La position en **FEN** (le format standard, lisible par n'importe quel
/// moteur).
///
/// `fullmove` est le numéro de la dame, que la position ne porte
/// pas : c'est l'appelant qui le sait (le compteur de la partie).
pub fn to_fen(pos: &Position, fullmove: u32) -> String {
    let mut board = String::new();
    for rank in (0..8usize).rev() {
        let mut empty = 0u8;
        for file in 0..8usize {
            let code = pos.mailbox[rank * 8 + file];
            if code == 0 {
                empty += 1;
                continue;
            }
            if empty > 0 {
                board.push((b'0' + empty) as char);
                empty = 0;
            }
            board.push(piece_char(code));
        }
        if empty > 0 {
            board.push((b'0' + empty) as char);
        }
        if rank > 0 {
            board.push('/');
        }
    }
    let side = if pos.side == crate::types::Color::White {
        "w"
    } else {
        "b"
    };
    let mut castle = String::new();
    if pos.castle & crate::types::CASTLE_WK != 0 {
        castle.push('K');
    }
    if pos.castle & crate::types::CASTLE_WQ != 0 {
        castle.push('Q');
    }
    if pos.castle & crate::types::CASTLE_BK != 0 {
        castle.push('k');
    }
    if pos.castle & crate::types::CASTLE_BQ != 0 {
        castle.push('q');
    }
    if castle.is_empty() {
        castle.push('-');
    }
    let ep = if pos.ep == crate::types::NO_EP {
        "-".to_string()
    } else {
        let file = (b'a' + (pos.ep & 7)) as char;
        let rank = (b'1' + (pos.ep >> 3)) as char;
        format!("{file}{rank}")
    };
    format!(
        "{board} {side} {castle} {ep} {} {fullmove}",
        pos.halfmove
    )
}

/// Écrit un corpus.
pub fn write_corpus(path: &std::path::Path, records: &[Record], meta: &[(&str, String)]) -> Result<(), String> {
    let mut out = String::new();
    out.push_str("# mychess corpus v1\n");
    for (k, v) in meta {
        out.push_str(&format!("# {k} : {v}\n"));
    }
    out.push_str("# zobrist | fen | resultat | ply | partie\n");
    for r in records {
        let label = match r.label {
            Some(l) => format!("{l:.6}"),
            None => "-".to_string(),
        };
        out.push_str(&format!(
            "{:016x} | {} | {} | {} | {} | {}\n",
            r.zobrist, r.fen, r.result, r.ply, r.game, label
        ));
    }
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).map_err(|e| format!("création de {} : {e}", dir.display()))?;
    }
    std::fs::write(path, out).map_err(|e| format!("écriture de {} : {e}", path.display()))
}

/// Lit un corpus. Les lignes de commentaire (`#`) sont ignorées ; une
/// ligne illisible est une **erreur** (un corpus tronqué ou mal formé
/// ne doit pas passer silencieusement).
pub fn read_corpus(path: &std::path::Path) -> Result<Vec<Record>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("lecture de {} : {e}", path.display()))?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('|').map(|s| s.trim()).collect();
        if fields.len() != 5 && fields.len() != 6 {
            return Err(format!(
                "{}:{} : {} champs (attendus 5 ou 6) : {line}",
                path.display(),
                i + 1,
                fields.len()
            ));
        }
        let zobrist = u64::from_str_radix(fields[0], 16)
            .map_err(|e| format!("{}:{} : zobrist illisible : {e}", path.display(), i + 1))?;
        let result: i8 = fields[2]
            .parse()
            .map_err(|e| format!("{}:{} : résultat illisible : {e}", path.display(), i + 1))?;
        let ply: u32 = fields[3]
            .parse()
            .map_err(|e| format!("{}:{} : ply illisible : {e}", path.display(), i + 1))?;
        let game: u32 = fields[4]
            .parse()
            .map_err(|e| format!("{}:{} : partie illisible : {e}", path.display(), i + 1))?;
        let label = if fields.len() == 6 && fields[5] != "-" {
            Some(
                fields[5]
                    .parse::<f32>()
                    .map_err(|e| format!("{}:{} : étiquette illisible : {e}", path.display(), i + 1))?,
            )
        } else {
            None
        };
        out.push(Record {
            zobrist,
            fen: fields[1].to_string(),
            result,
            ply,
            game,
            label,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Color;

    const FENS: &[&str] = &[
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 b - - 0 1",
    ];

    /// L'aller-retour FEN → position → FEN doit restituer **exactement**
    /// la même position : c'est la garantie que le corpus est
    /// rejouable par un tiers.
    #[test]
    fn fen_roundtrip_is_exact() {
        for fen in FENS {
            let pos = Position::from_fen(fen).unwrap();
            let back = to_fen(&pos, 1);
            let reparsed = Position::from_fen(&back).unwrap();
            assert_eq!(pos.zobrist, reparsed.zobrist, "le FEN relu diffère : {fen}");
            assert_eq!(pos.side, reparsed.side);
            assert_eq!(pos.castle, reparsed.castle);
            assert_eq!(pos.ep, reparsed.ep);
            assert_eq!(pos.halfmove, reparsed.halfmove);
        }
    }

    /// Le trait doit survivre à l'aller-retour.
    #[test]
    fn the_side_survives_the_roundtrip() {
        let mut pos = Position::startpos();
        pos.side = Color::Black;
        let back = to_fen(&pos, 1);
        assert!(back.contains(" b "), "le trait a été perdu : {back}");
    }

    /// Le corpus s'écrit et se relit **identique**.
    #[test]
    fn corpus_roundtrip() {
        let records: Vec<Record> = FENS
            .iter()
            .enumerate()
            .map(|(i, fen)| {
                let pos = Position::from_fen(fen).unwrap();
                Record {
                    zobrist: pos.zobrist,
                    fen: to_fen(&pos, 1),
                    result: if i % 3 == 0 { 1 } else if i % 3 == 1 { 0 } else { -1 },
                    ply: i as u32 * 2,
                    game: i as u32,
                    label: None,
                }
            })
            .collect();
        let dir = std::env::temp_dir().join("mychess-corpus-test");
        let path = dir.join("corpus.txt");
        write_corpus(&path, &records, &[("test", "1".into())]).unwrap();
        let back = read_corpus(&path).unwrap();
        assert_eq!(back, records);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// La cible est **côté trait** : une position du même plateau, jouée
    /// par les blancs puis par les noirs, a des cibles **opposées**.
    #[test]
    fn the_target_is_side_to_move() {
        let r = Record {
            zobrist: 0,
            fen: String::new(),
            result: 1,
            ply: 0,
            game: 0,
            label: None,
        };
        assert_eq!(r.target(true), 1.0);
        assert_eq!(r.target(false), -1.0);
    }
}
