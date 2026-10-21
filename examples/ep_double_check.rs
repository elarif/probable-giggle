//! Mesure la contribution de chaque pattern suspect à l'écart d9 sur les
//! colonnes `discovery_checks` / `double_checks`.
//!
//! L'écart d9 (reproduit deux fois, sur deux hôtes) est :
//!
//!   discovery_checks  37 103 750  vs 37 101 713  (+2 037)
//!   double_checks      6 315 946  vs  5 547 231  (+768 715)
//!
//! La règle de classification est dans `src/perft.rs` :
//!
//! ```text
//! if att.count_ones() >= 2   -> double_checks
//! else if att != moved_bit    -> discovery_checks
//! else                        -> direct
//! ```
//!
//! Ce probe recompte les memes classes, mais **découpe par motif** :
//! prise en passant, roque, et le reste. Si la somme `double_ep` vaut
//! 768 715 (ou `double_castle`), le pattern est le coupable ; si aucune
//! des deux ne l'atteint, la divergence est ailleurs.
//!
//! L'exécution est coûteuse : c'est un d9 complet. Utiliser le runner
//! shardé (`examples/d9detailed_shard.rs`) pour un d9 complet, et ce
//! probe pour une profondeur moindre ou une position dédiée.

use mychess::movegen::{MoveList, attackers_to, generate};
use mychess::position::Position;
use mychess::types::GenKind;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let depth: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(6);
    let fen = args.get(2).cloned().unwrap_or_else(|| {
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".to_string()
    });

    let mut pos = Position::from_fen(&fen).expect("valid FEN");
    let mut total_direct = 0u64;
    let mut total_discovery = 0u64;
    let mut total_double = 0u64;
    let mut dbl_ep = 0u64;
    let mut dbl_castle = 0u64;
    let mut disc_ep = 0u64;
    let mut disc_castle = 0u64;
    let mut mates = 0u64;

    walk(&mut pos, depth, &mut |_m, kind, is_ep, is_castle, is_mate| {
        match kind {
            0 => total_direct += 1,
            1 => {
                total_discovery += 1;
                if is_ep {
                    disc_ep += 1;
                }
                if is_castle {
                    disc_castle += 1;
                }
            }
            _ => {
                total_double += 1;
                if is_ep {
                    dbl_ep += 1;
                }
                if is_castle {
                    dbl_castle += 1;
                }
            }
        }
        if is_mate {
            mates += 1;
        }
    });

    println!("FEN: {fen}  depth {depth}");
    println!("  checks total   : {}", total_direct + total_discovery + total_double);
    println!("  direct         : {total_direct}");
    println!("  discovery      : {total_discovery}");
    println!("    dont ep       : {disc_ep}");
    println!("    dont roque    : {disc_castle}");
    println!("  double         : {total_double}");
    println!("    dont ep       : {dbl_ep}");
    println!("    dont roque    : {dbl_castle}");
    println!("  mates          : {mates}");
    println!();
    println!("Écart CPW à expliquer : discovery +2 037, double +768 715");
    println!("  -> si dbl_ep      == 768 715, le double-échec par prise ep en est la source");
    println!("  -> si dbl_castle  == 768 715, le double-échec par roque en est la source");
}

type Visit<'a> = dyn FnMut(String, u8, bool, bool, bool) + 'a;

/// Descend l'arbre jusqu'à `depth`, en comptant les classes de check.
fn walk(pos: &mut Position, depth: u32, visit: &mut Visit<'_>) {
    if depth == 0 {
        return;
    }
    let us = pos.side;
    let opp = us.opp();
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    for mv in list.iter() {
        let m = mv;
        let is_ep = m.is_ep();
        let is_castle = m.is_castle();
        let u = pos.make(m);
        if depth == 1 {
            if pos.in_check(opp) {
                let att = attackers_to(pos, pos.king_sq(opp), us);
                let n = att.count_ones();
                let kind = if n >= 2 {
                    2u8
                } else if att != (1u64 << m.to()) {
                    1u8
                } else {
                    0u8
                };
                let mut replies = MoveList::new();
                generate(pos, &mut replies, GenKind::All);
                visit(m.uci(), kind, is_ep, is_castle, replies.is_empty());
            }
        } else {
            walk(pos, depth - 1, visit);
        }
        pos.unmake(m, u);
    }
}
