//! Trouve la **première** position (FEN) produisant un motif de check
//! donné, en descendant l'arbre d9.
//!
//! C'est l'étape 3 du plan `docs/plan_ecart_d9.md` : un décompte dit
//! *combien*, un contre-exemple dit *quoi*. Ce runner s'arrête à la
//! première occurrence et affiche la position, le coup joué, et les
//! attaquants du roi adverse après le coup — de quoi vérifier la
//! convention à la main et avec python-chess.
//!
//! Le motif est désigné par le couple (classe, drapeaux) :
//!   classe   : "double" | "discovery"
//!   drapeaux : "promo" | "ep" | "castle" | "any"
//!
//! Usage :
//!   mychess-first-case <depth> <classe> <motif> [fen]
//!
//! Exemple :
//!   mychess-first-case 9 double promo
//!
//! La descente est en ordre déterministe (coups dans l'ordre de
//! génération), donc le contre-exemple trouvé est reproductible.

use mychess::movegen::{MoveList, attackers_to, generate};
use mychess::position::Position;
use mychess::types::{Color, GenKind};

/// Reconstruit la FEN d'une position. Le core n'a pas de serialiseur FEN
/// (il est gele depuis GATE 2.5), mais `mailbox` et les champs de
/// `Position` sont publics : la reconstruction est locale a ce probe.
fn to_fen(pos: &Position) -> String {
    const FILES: &[u8] = b"abcdefgh";
    let mut board = [b' '; 64];
    for sq in 0..64usize {
        let m = pos.mailbox[sq];
        if m == 0 {
            continue;
        }
        // mailbox : 1..=6 = pieces blanches (pion..roi), 7..=12 = noires.
        let kind = (m - 1) % 6;
        let white = m <= 6;
        let ch = match kind {
            0 => b'p',
            1 => b'n',
            2 => b'b',
            3 => b'r',
            4 => b'q',
            _ => b'k',
        };
        board[sq] = if white { ch.to_ascii_uppercase() } else { ch };
    }
    let mut out = String::new();
    // Le core indexe a1 = 0 : la rangée 1 occupe les indices 0..7, la
    // rangée 8 les indices 56..63. La FEN s'ecrit donc rangée 8 en
    // premier, soit l'ordre inverse de l'indexation.
    for rank in (0..8usize).rev() {
        let mut empty = 0u8;
        for file in 0..8usize {
            let sq = rank * 8 + file;
            if board[sq] == b' ' {
                empty += 1;
            } else {
                if empty > 0 {
                    out.push((b'0' + empty) as char);
                    empty = 0;
                }
                out.push(board[sq] as char);
            }
        }
        if empty > 0 {
            out.push((b'0' + empty) as char);
        }
        if rank > 0 {
            out.push('/');
        }
    }
    out.push(' ');
    out.push(match pos.side {
        Color::White => 'w',
        Color::Black => 'b',
    });
    out.push(' ');
    // Droits de roque : le core les normalise a la frontiere FEN, on
    // reconstruit donc ce qui est *actuellement* possible.
    let mut rights = String::new();
    let white_king = pos.king_sq(Color::White);
    let black_king = pos.king_sq(Color::Black);
    if white_king == 4 {
        if pos.castle & 1 != 0 {
            rights.push('K');
        }
        if pos.castle & 2 != 0 {
            rights.push('Q');
        }
    }
    if black_king == 60 {
        if pos.castle & 4 != 0 {
            rights.push('k');
        }
        if pos.castle & 8 != 0 {
            rights.push('q');
        }
    }
    if rights.is_empty() {
        rights.push('-');
    }
    out.push_str(&rights);
    out.push(' ');
    out.push_str(&match pos.ep_square() {
        Some(sq) => format!(
            "{}{}",
            FILES[(sq & 7) as usize] as char,
            (b'1' + (sq >> 3)) as char
        ),
        None => "-".to_string(),
    });
    out.push(' ');
    out.push_str(&pos.halfmove.to_string());
    out.push(' ');
    out.push_str("1");
    out
}

/// Un motif recherché.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Wanted {
    Double,
    Discovery,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Motif {
    Promo,
    Ep,
    Castle,
    Any,
}

/// Un contre-exemple trouvé.
struct Found {
    fen: String,
    uci: String,
    class: &'static str,
    attackers: u64,
    n_attackers: u32,
    motif: &'static str,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: first-case <depth> <double|discovery> <promo|ep|castle|any> [fen]");
        std::process::exit(2);
    }
    let depth: u32 = args[1].parse().expect("depth");
    let wanted = match args[2].as_str() {
        "double" => Wanted::Double,
        "discovery" => Wanted::Discovery,
        other => {
            eprintln!("unknown class: {other}");
            std::process::exit(2);
        }
    };
    let motif = match args[3].as_str() {
        "promo" => Motif::Promo,
        "ep" => Motif::Ep,
        "castle" => Motif::Castle,
        "any" => Motif::Any,
        other => {
            eprintln!("unknown motif: {other}");
            std::process::exit(2);
        }
    };
    let fen = args.get(4).cloned().unwrap_or_else(|| {
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".to_string()
    });

    let mut pos = Position::from_fen(&fen).expect("valid FEN");
    match find(&mut pos, depth, wanted, motif, 0) {
        Some(f) => {
            println!("FOUND");
            println!("fen       : {}", f.fen);
            println!("move      : {}", f.uci);
            println!("class     : {}", f.class);
            println!("motif     : {}", f.motif);
            println!("attackers : {} ({:#018x})", f.n_attackers, f.attackers);
        }
        None => {
            println!("not found: depth {depth}, class {:?}, motif {:?}", wanted, motif);
        }
    }
}

/// Descend en profondeur, et renvoie le premier cas rencontre.
fn find(
    pos: &mut Position,
    depth: u32,
    wanted: Wanted,
    motif: Motif,
    ply: u32,
) -> Option<Found> {
    if depth == 0 {
        return None;
    }
    let us = pos.side;
    let opp = us.opp();
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    for mv in list.iter() {
        let m = mv;
        let is_promo = m.is_promotion();
        let is_ep = m.is_ep();
        let is_castle = m.is_castle();
        // La FEN doit decrire la position *avant* le coup : c'est elle
        // qu'un tiers rejoue pour verifier la convention.
        let fen_before = if depth == 1 {
            Some(to_fen(pos))
        } else {
            None
        };
        let u = pos.make(m);

        if depth == 1 {
            if pos.in_check(opp) {
                let att = attackers_to(pos, pos.king_sq(opp), us);
                let n = att.count_ones();
                let class = if n >= 2 {
                    "double"
                } else if att != (1u64 << m.to()) {
                    "discovery"
                } else {
                    "direct"
                };
                let class_ok = match wanted {
                    Wanted::Double => class == "double",
                    Wanted::Discovery => class == "discovery",
                };
                let motif_ok = match motif {
                    Motif::Any => true,
                    Motif::Promo => is_promo,
                    Motif::Ep => is_ep,
                    Motif::Castle => is_castle,
                };
                if class_ok && motif_ok {
                    let fen = fen_before.expect("FEN prise avant le coup");
                    let motif_name = match motif {
                        Motif::Any => "any",
                        Motif::Promo => "promotion",
                        Motif::Ep => "en-passant",
                        Motif::Castle => "castling",
                    };
                    return Some(Found {
                        fen,
                        uci: m.uci(),
                        class,
                        attackers: att,
                        n_attackers: n,
                        motif: motif_name,
                    });
                }
            }
        } else if let Some(f) = find(pos, depth - 1, wanted, motif, ply + 1) {
            return Some(f);
        }
        pos.unmake(m, u);
    }
    None
}
