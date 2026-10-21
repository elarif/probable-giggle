//! Part 4 du post : les décalages de bitboard, la table `SquaresBetween` et
//! `IsAttackedBy`. Trois oracles indépendants :
//!   1. les décalages contre des décalages sur une grille naïve ;
//!   2. `squares_between` contre `slider::between` (implémentations
//!      différentes : marche de rayon ici, table précalculée là) ;
//!   3. `is_attacked_by` contre `Position::is_square_attacked` sur toutes les
//!      positions visitées par un auto-jeu déterministe.

use mychess::gct::attacks::{is_attacked_by, shift_n, shift_s, shift_e, shift_w,
                            shift_ne, shift_nw, shift_se, shift_sw, squares_between};
use mychess::position::Position;
use mychess::types::*;

/// Référence naïve : applique un delta (df, dr) à chaque bit et ne garde que
/// les cases sur l'échiquier. Aucune table, aucun décalage de bitboard.
fn naive_shift(b: u64, df: i32, dr: i32) -> u64 {
    let mut out = 0u64;
    for sq in 0..64u8 {
        if b & (1u64 << sq) == 0 {
            continue;
        }
        let f = (sq % 8) as i32 + df;
        let r = (sq / 8) as i32 + dr;
        if (0..8).contains(&f) && (0..8).contains(&r) {
            out |= 1u64 << ((r * 8 + f) as u32);
        }
    }
    out
}

#[test]
fn les_huit_decalages_correspondent_a_la_grille_naive() {
    // Un motif dense et un motif clairsemé : les deux attrapent les débordements
    // de bord (rangée 1/8, colonne a/h).
    let motifs = [0xFFFF_FFFF_FFFF_FFFFu64, 0x8040_2010_0804_0201];
    for m in motifs {
        assert_eq!(shift_n(m), naive_shift(m, 0, 1), "shift_n");
        assert_eq!(shift_s(m), naive_shift(m, 0, -1), "shift_s");
        assert_eq!(shift_e(m), naive_shift(m, 1, 0), "shift_e");
        assert_eq!(shift_w(m), naive_shift(m, -1, 0), "shift_w");
        assert_eq!(shift_ne(m), naive_shift(m, 1, 1), "shift_ne");
        assert_eq!(shift_nw(m), naive_shift(m, -1, 1), "shift_nw");
        assert_eq!(shift_se(m), naive_shift(m, 1, -1), "shift_se");
        assert_eq!(shift_sw(m), naive_shift(m, -1, -1), "shift_sw");
    }
    // Un bit isole sur chaque case : 64 cas de bord.
    for sq in 0..64u8 {
        let b = 1u64 << sq;
        assert_eq!(shift_n(b), naive_shift(b, 0, 1));
        assert_eq!(shift_s(b), naive_shift(b, 0, -1));
        assert_eq!(shift_e(b), naive_shift(b, 1, 0));
        assert_eq!(shift_w(b), naive_shift(b, -1, 0));
        assert_eq!(shift_ne(b), naive_shift(b, 1, 1));
        assert_eq!(shift_nw(b), naive_shift(b, -1, 1));
        assert_eq!(shift_se(b), naive_shift(b, 1, -1));
        assert_eq!(shift_sw(b), naive_shift(b, -1, -1));
    }
}

/// `Position` n'implémente pas `Debug`, et ajouter le derive toucherait au
/// core gelé : on rend le plateau depuis ses champs publics (`mailbox`,
/// `side`, `castle`, `ep`).
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

#[test]
fn squares_between_est_identique_a_la_table_precalculee() {
    for a in 0..64u8 {
        for b in 0..64u8 {
            assert_eq!(
                squares_between(a, b),
                mychess::slider::between(a, b),
                "between({a}, {b})"
            );
        }
    }
}

#[test]
fn is_attacked_by_est_identique_a_position_is_square_attacked() {
    // Auto-jeu déterministe : deux couleurs jouent des coups pseudo-aléatoires
    // déterministes, et à chaque nœud on compare les 64 cases pour les deux
    // couleurs.
    let mut pos = Position::startpos();
    let mut rng = 0x9E37_79B9_7F4A_7C15u64;
    for _ply in 0..400 {
        for sq in 0..64u8 {
            for by in [Color::White, Color::Black] {
                assert_eq!(
                    is_attacked_by(&pos, sq, by),
                    pos.is_square_attacked(sq, by),
                    "case {sq} attaquee par {by:?} dans {}", describe(&pos)
                );
            }
        }
        let mut list = mychess::movegen::MoveList::new();
        mychess::movegen::generate(&pos, &mut list, GenKind::All);
        if list.is_empty() {
            break;
        }
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let mv = list.get((rng as usize) % list.len());
        pos.make(mv);
    }
}
