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
    let n = gct_generate(pos, &mut l);
    assert_eq!(
        n,
        l.len(),
        "generate doit renvoyer le nouvel indice du buffer"
    );
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

#[test]
fn generate_renvoie_l_indice_d_append() {
    // Contrat dont depend la tache 6 : `generate` ajoute a la suite du buffer
    // et renvoie le NOUVEL indice, pas le nombre de coups ajoutes.
    let pos = Position::startpos();
    let mut l = MoveList::new();
    let n1 = gct_generate(&pos, &mut l);
    assert_eq!(n1, l.len());
    assert!(n1 > 0);
    let n2 = gct_generate(&pos, &mut l);
    assert_eq!(n2, 2 * n1, "generate doit renvoyer l'indice d'append");
    assert_eq!(l.len(), 2 * n1);
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
        // Armee complete sans droits de roque : cavaliers, dames, fous, tours.
        // C'est le corpus le plus large possible dans le perimetre de la tache 4.
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1",
        // Ouverture des quatre cavaliers, variante italienne : huit demi-coups
        // (1.e4 e5 2.Nf3 Nc6 3.Nc3 Nf6 4.Bc4 Bc5), sans droit de roque. Les
        // dames restent passives au noeud racine (Dd1 n'a que d1-e2) et leur
        // mobilite s'ouvre aux niveaux suivants.
        "r1bqk2r/pppp1ppp/2n2n2/2b1p3/2B1P3/2N2N2/PPPP1PPP/R1BQK2R w - - 0 1",
        // Promotions blanches : poussee c7-c8, capture NW c7xb8 (cavalier) et
        // capture NE c7xd8 (tour) — les deux bras de capture sont exerces.
        "1n1rk3/2P5/8/8/8/8/8/4K3 w - - 0 1",
        // Promotions noires : poussee b2-b1 et captures b2xa1 / b2xc1.
        "4k3/8/8/8/8/8/1p6/R1B1K3 b - - 0 1",
        // Echec par tour sur la colonne e, bloquable UNIQUEMENT par Fe3 :
        // isole le terme `squares_between` de `check_mask` (aucun autre coup
        // legal que les coups de roi et d2-e3).
        "4r1k1/8/8/8/8/8/3B4/4K3 w - - 0 1",
        // Double echec (fou b4 + cavalier f3 sur le roi e1) : seuls les coups
        // de roi sont legaux, le retour anticipe doit s'appliquer.
        "4k3/8/8/8/1b6/5n2/8/4K1N1 w - - 0 1",
    ];
    fens.iter()
        .map(|f| Position::from_fen(f).unwrap())
        .collect()
}

#[test]
fn gct_et_movegen_donnent_le_meme_ensemble_sans_roque_ni_ep() {
    for pos in positions_sans_roque_ni_ep() {
        // On marche tout l'arbre jusqu'à la profondeur 2 et on compare les
        // niveaux 0, 1 et 2, pour multiplier les configurations de clouages,
        // d'échecs et de promotions.
        let mut stack: Vec<Position> = vec![pos];
        for depth in 0..3 {
            let mut next = Vec::new();
            for p in stack {
                assert_eq!(
                    sorted_gct(&p),
                    sorted_movegen(&p),
                    "ensembles de coups differents dans {}",
                    describe(&p)
                );
                // La derniere iteration ne developpe pas : comparer un niveau
                // qu'on jette ne prouve rien et coute du temps.
                if depth + 1 == 3 {
                    continue;
                }
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
