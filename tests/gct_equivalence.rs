//! Équivalence `gct::generate` ≡ `movegen::generate` sur toute la surface.
//!
//! La comparaison porte sur l'**ensemble** des coups (mêmes triplés
//! `(from, to, flags)`), pas sur l'ordre : le post et `mychess` n'émettent pas
//! dans le même ordre, et ce n'est pas un critère de correction.

use mychess::gct::generate::generate as gct_generate;
use mychess::movegen::{MoveList, generate};
use mychess::position::Position;
use mychess::types::*;

/// Clé de comparaison stable : un coup est identifié par ses trois champs
/// publics (`from`, `to`, `flags`), jamais par la valeur 16 bits brute.
type MoveKey = (u8, u8, u8);

fn key(m: Move) -> MoveKey {
    (m.from(), m.to(), m.flags())
}

fn sorted_gct(pos: &Position) -> Vec<MoveKey> {
    let mut l = MoveList::new();
    gct_generate(pos, &mut l);
    let mut v: Vec<MoveKey> = l.iter().map(key).collect();
    v.sort_unstable();
    v
}

fn sorted_movegen(pos: &Position) -> Vec<MoveKey> {
    let mut l = MoveList::new();
    generate(pos, &mut l, GenKind::All);
    let mut v: Vec<MoveKey> = l.iter().map(key).collect();
    v.sort_unstable();
    v
}

/// `Position` n'implémente pas `Debug` : rendu du plateau depuis ses champs
/// publics (`mailbox`, `side`, `castle`, `ep`).
fn describe(pos: &Position) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    for r in (0..8).rev() {
        for f in 0..8 {
            let m = pos.mailbox[r * 8 + f];
            s.push(if m == 0 {
                '.'
            } else {
                b"PNBRQKpnbrqk"[(m - 1) as usize] as char
            });
        }
        s.push('\n');
    }
    let _ = write!(s, "{:?} castle={} ep={}", pos.side, pos.castle, pos.ep);
    s
}

/// Positions sans droit de roque et sans case en-passant : à cette étape le
/// générateur `gct` ne produit encore ni roque ni en-passant (tâche 5), donc
/// seules ces positions sont comparables.
fn positions_sans_roque_ni_ep() -> Vec<Position> {
    let fens = [
        "4k3/pppppppp/8/8/8/8/PPPPPPPP/4K3 w - - 0 1",
        "4k3/8/8/8/8/8/8/4K3 w - - 0 1",
        "r5k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1",
        "4k3/pp6/8/2b5/8/6P1/PP6/4K3 w - - 0 1",
        "4k3/8/8/8/8/8/8/R3K2R w - - 0 1",
    ];
    fens.iter()
        .map(|f| Position::from_fen(f).unwrap())
        .collect()
}

#[test]
fn gct_et_movegen_donnent_le_meme_ensemble_sans_roque_ni_ep() {
    for pos in positions_sans_roque_ni_ep() {
        // On marche tout l'arbre jusqu'à la profondeur 3 pour multiplier les
        // configurations de clouages et d'échecs.
        let mut stack: Vec<Position> = vec![pos];
        for _depth in 0..3 {
            let mut next = Vec::new();
            for p in stack {
                assert_eq!(
                    sorted_gct(&p),
                    sorted_movegen(&p),
                    "ensembles de coups differents dans {}",
                    describe(&p)
                );
                let mut l = MoveList::new();
                generate(&p, &mut l, GenKind::All);
                for mv in l.iter() {
                    let mut q = p;
                    q.make(mv);
                    if q.ep == NO_EP {
                        next.push(q);
                    }
                }
            }
            stack = next;
            if stack.is_empty() {
                break;
            }
        }
    }
}
