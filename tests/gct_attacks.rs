//! Part 4 du post : les décalages de bitboard, la table `SquaresBetween` et
//! `IsAttackedBy`. Trois oracles indépendants :
//!   1. les décalages contre des décalages sur une grille naïve ;
//!   2. `squares_between` contre une identité de bitboards dérivée des
//!      attaques de tours et de fous (aucune marche de rayon écrite à la
//!      main) ;
//!   3. `is_attacked_by` contre un oracle de géométrie qui lit le mailbox et
//!      décide l'attaque par arithmétique de coordonnées, sans table
//!      d'attaque par pièce et sans passer par `Position`.
//!
//! S'y ajoute un contrôle de dérive : `is_attacked_by` face à
//! `Position::is_square_attacked`. Les deux algorithmes sont structurellement
//! identiques, donc ce test n'est **pas** un oracle indépendant ; il attrape
//! une erreur de portage (table de pions inversée, couleur mal indexée,
//! `& them` oublié), pas un bug partagé par les deux implémentations.

use mychess::gct::attacks::{
    is_attacked_by, shift_e, shift_n, shift_ne, shift_nw, shift_s, shift_se, shift_sw, shift_w,
    squares_between,
};
use mychess::position::Position;
use mychess::slider::{bishop_attacks, rook_attacks};
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

/// Identité classique, indépendante de toute marche écrite à la main : avec
/// seulement les deux extrémités sur l'échiquier, les attaques de `a` et de
/// `b` se recoupent exactement sur les cases strictement entre elles.
///
/// Le type de glisseur doit suivre l'alignement : prendre l'union des
/// intersections tour et fou est faux. Contre-exemple mesuré : `a1` et `c1`
/// sont alignés horizontalement, mais `b2` est attaqué diagonalement par les
/// deux, donc l'intersection des fous ajoute une case hors du segment.
fn between_via_sliders(a: u8, b: u8) -> u64 {
    if a == b {
        return 0;
    }
    let only_b = 1u64 << b;
    let only_a = 1u64 << a;
    let df = (b % 8) as i32 - (a % 8) as i32;
    let dr = (b / 8) as i32 - (a / 8) as i32;
    if (df == 0) != (dr == 0) {
        rook_attacks(a, only_b) & rook_attacks(b, only_a)
    } else if df != 0 && df.abs() == dr.abs() {
        bishop_attacks(a, only_b) & bishop_attacks(b, only_a)
    } else {
        0
    }
}

#[test]
fn squares_between_respecte_l_identite_des_bitboards() {
    for a in 0..64u8 {
        for b in 0..64u8 {
            assert_eq!(
                squares_between(a, b),
                between_via_sliders(a, b),
                "between({a}, {b})"
            );
            // Une case n'est jamais entre elle-même, et deux cases adjacentes
            // n'ont rien entre elles.
            if a == b {
                assert_eq!(squares_between(a, b), 0);
            }
        }
    }
}

/// Oracle géométrique : décide l'attaque depuis les coordonnées et le
/// mailbox, sans réutiliser `Position::is_square_attacked` ni les tables
/// d'attaque par pièce.
fn attacked_by_geometry(pos: &Position, sq: u8, by: Color) -> bool {
    let occ = pos.occ();
    let (tf, tr) = ((sq % 8) as i32, (sq / 8) as i32);
    for from in 0..64u8 {
        let m = pos.piece_on(from);
        if m == 0 {
            continue;
        }
        let code = m - 1;
        if code / 6 != by.idx() as u8 {
            continue;
        }
        let kind = (code % 6) as usize;
        let (ff, fr) = ((from % 8) as i32, (from / 8) as i32);
        let df = tf - ff;
        let dr = tr - fr;
        let aligned = df == 0 || dr == 0 || df.abs() == dr.abs();
        let clear = squares_between(from, sq) & occ == 0;
        let hits = match kind {
            PAWN => dr == (if by == Color::White { 1 } else { -1 }) && df.abs() == 1,
            KNIGHT => matches!((df.abs(), dr.abs()), (1, 2) | (2, 1)),
            KING => df.abs() <= 1 && dr.abs() <= 1 && (df != 0 || dr != 0),
            BISHOP => df != 0 && df.abs() == dr.abs() && clear,
            ROOK => ((df == 0) != (dr == 0)) && clear,
            QUEEN => (df != 0 || dr != 0) && aligned && clear,
            _ => false,
        };
        if hits {
            return true;
        }
    }
    false
}

/// Rejoue l'auto-jeu déterministe et applique `check` à chaque position
/// visitée, position de départ incluse. Les deux tests d'attaque passent donc
/// exactement par les mêmes positions.
fn sur_chaque_position_de_l_auto_jeu(mut check: impl FnMut(&Position)) {
    let mut pos = Position::startpos();
    let mut rng = 0x9E37_79B9_7F4A_7C15u64;
    for _ply in 0..400 {
        check(&pos);
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

#[test]
fn is_attacked_by_correspond_a_position_is_square_attacked() {
    // Contrôle de dérive, pas un oracle indépendant : `Position` suit le même
    // algorithme que le portage, donc un bug partagé passerait ce test.
    sur_chaque_position_de_l_auto_jeu(|pos| {
        for sq in 0..64u8 {
            for by in [Color::White, Color::Black] {
                assert_eq!(
                    is_attacked_by(pos, sq, by),
                    pos.is_square_attacked(sq, by),
                    "case {sq} attaquee par {by:?} dans {pos:?}"
                );
            }
        }
    });
}

#[test]
fn is_attacked_by_correspond_a_l_oracle_geometrique() {
    // Mêmes positions que le contrôle de dérive : auto-jeu déterministe.
    sur_chaque_position_de_l_auto_jeu(|pos| {
        for sq in 0..64u8 {
            for by in [Color::White, Color::Black] {
                assert_eq!(
                    is_attacked_by(pos, sq, by),
                    attacked_by_geometry(pos, sq, by),
                    "case {sq} attaquee par {by:?} dans {pos:?}"
                );
            }
        }
    });
}
