//! Localisation de l'écart d9 sur les colonnes `discovery_checks` / `double_checks`.
//!
//! Le rapport `slaml/d9_checkcols_report_20261014.md` laisse deux conventions
//! candidates pour l'écart (+2 037 discovery, +768 715 double) :
//!
//! (a) un **double échec par prise en-passant** : la tour révélée sur la
//!     rangée + le pion qui capture donnent deux attaquants. `mychess`
//!     compte `att.count_ones() >= 2` → `double_checks`. Si CPW ne compte
//!     pas le pion ep comme attaquant du roi, il compte moins de doubles.
//!
//! (b) un **échec de roi-roque** : le roi dégage, la tour arrive et donne
//!     check → deux attaquants, donc `double_checks` chez mychess.
//!
//! Ce probe compte, sur un FEN donné, les quatre classes séparément, pour
//! un seul coup à la fois (profondeur 1). Il sert à voir, sur une position
//! miniature, comment `mychess` classe un échec.
//!
//! Usage : `mychess-ep-double-check "<fen>" [move-uci]`

use mychess::movegen::{MoveList, attackers_to, generate};
use mychess::position::Position;
use mychess::types::{GenKind, Move};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let fen = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".to_string());

    let mut pos = Position::from_fen(&fen).expect("valid FEN");
    let us = pos.side;
    let opp = us.opp();

    let mut list = MoveList::new();
    generate(&pos, &mut list, GenKind::All);

    let mut plain = 0u64;
    let mut discovery = 0u64;
    let mut double = 0u64;
    let mut mates = 0u64;
    let mut ep_double = 0u64;
    let mut castle_double = 0u64;
    let mut rows: Vec<String> = Vec::new();

    for mv in list.iter() {
        let m: Move = mv;
        let is_ep = m.is_ep();
        let is_castle = m.is_castle();
        let u = pos.make(m);
        if pos.in_check(opp) {
            plain += 1;
            let att = attackers_to(&pos, pos.king_sq(opp), us);
            let n = att.count_ones();
            let moved_attacker = 1u64 << m.to();
            let kind = if n >= 2 {
                "DOUBLE"
            } else if att != moved_attacker {
                "DISCOVERY"
            } else {
                "DIRECT"
            };
            if kind == "DOUBLE" {
                double += 1;
                if is_ep {
                    ep_double += 1;
                }
                if is_castle {
                    castle_double += 1;
                }
            } else if kind == "DISCOVERY" {
                discovery += 1;
            }
            let mut replies = MoveList::new();
            generate(&pos, &mut replies, GenKind::All);
            if replies.is_empty() {
                mates += 1;
            }
            rows.push(format!(
                "  {}  kind={kind:9} attackers={n} ep={is_ep} castle={is_castle} att={att:#018x}",
                m.uci()
            ));
        }
        pos.unmake(m, u);
    }

    println!("FEN: {fen}");
    println!("side to move: {us:?}");
    println!("checks          : {plain}");
    println!("  direct        : {}", plain - discovery - double);
    println!("  discovery     : {discovery}");
    println!("  double        : {double}");
    println!("    of which ep : {ep_double}");
    println!("    of which cast: {castle_double}");
    println!("mates           : {mates}");
    if !rows.is_empty() {
        println!("detail:");
        for r in rows.iter().take(40) {
            println!("{r}");
        }
    }
}
