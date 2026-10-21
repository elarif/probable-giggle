//! Part 7 du post : perft du générateur de coups légaux.
//!
//! [`perft`] reprend le « bulk counting » du post : à `depth == 1`, le nombre
//! de nœuds est la longueur de la liste générée, sans aucune paire
//! make/unmake au bord.
//!
//! [`perft_detailed`] ne peut pas compter en bulk : il doit attribuer les
//! catégories (captures, roques, échecs, mats…) au coup qui mène à la feuille.
//! Il applique donc la convention figée de [`crate::perft::perft_detailed`] :
//! à `depth == 1`, chaque coup est joué puis le nœud et ses attributs sont
//! comptés. C'est cette convention qui rend les compteurs comparables à la
//! table Grand Chess Tree après la conversion `checks = checks + mates`.

use crate::gct::generate::generate;
use crate::movegen::{MoveList, attackers_to};
use crate::perft::PerftCounts;
use crate::position::Position;
use crate::types::*;

/// Perft bulk : nœuds = longueur de la liste à `depth == 1`.
pub fn perft(pos: &mut Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let mut list = MoveList::new();
    let n = generate(pos, &mut list);
    if depth == 1 {
        return n as u64;
    }
    let moves: Vec<Move> = list.iter().collect();
    let mut nodes = 0;
    for mv in moves {
        let u = pos.make(mv);
        nodes += perft(pos, depth - 1);
        pos.unmake(mv, u);
    }
    nodes
}

/// Perft avec compteurs, comptés aux feuilles (même convention que
/// [`crate::perft::perft_detailed`], donc directement comparable à la table
/// Grand Chess Tree après conversion `checks = checks + mates`).
pub fn perft_detailed(pos: &mut Position, depth: u32) -> PerftCounts {
    let mut c = PerftCounts::default();
    if depth == 0 {
        c.nodes = 1;
        return c;
    }
    let mut list = MoveList::new();
    generate(pos, &mut list);
    let us = pos.side;
    for mv in list.iter() {
        let is_cap = mv.is_capture();
        let is_ep = mv.is_ep();
        let is_castle = mv.is_castle();
        let is_promo = mv.is_promotion();
        let u = pos.make(mv);
        if depth == 1 {
            c.nodes += 1;
            if is_cap {
                c.captures += 1;
            }
            if is_ep {
                c.ep += 1;
            }
            if is_castle {
                c.castles += 1;
            }
            if is_promo {
                c.promotions += 1;
            }
            let opp = us.opp();
            if pos.in_check(opp) {
                c.checks += 1;
                let att = attackers_to(pos, pos.king_sq(opp), us);
                if att.count_ones() >= 2 {
                    c.double_checks += 1;
                } else if att != (1u64 << mv.to()) {
                    c.discovery_checks += 1;
                }
                let mut replies = MoveList::new();
                if generate(pos, &mut replies) == 0 {
                    c.mates += 1;
                }
            }
        } else {
            let sub = perft_detailed(pos, depth - 1);
            c.nodes += sub.nodes;
            c.captures += sub.captures;
            c.ep += sub.ep;
            c.castles += sub.castles;
            c.promotions += sub.promotions;
            c.checks += sub.checks;
            c.discovery_checks += sub.discovery_checks;
            c.double_checks += sub.double_checks;
            c.mates += sub.mates;
        }
        pos.unmake(mv, u);
    }
    c
}
