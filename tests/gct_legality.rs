//! Part 7 : les trois bitboards de légalité, vérifiés contre des oracles
//! **indépendants** : tout est dérivé des champs du plateau, avec deux sources
//! d'attaque seulement — la marche de coordonnées (`walk`) pour les glissantes
//! et les deltas explicites (`leaper`) pour les sauteurs, les pions et le roi.
//! L'oracle ne lit donc ni `tables::RAYS`, ni `tables::BETWEEN`, ni les magics
//! du moteur. Les attaques comparées, elles, viennent des magics sous test
//! (`bishop_attacks`, `rook_attacks`) : si l'oracle les réutilisait, il ne
//! prouverait rien.

use mychess::gct::legality::{compute_checkers, compute_pins, king_danger};
use mychess::position::Position;
use mychess::types::*;

const DIRS8: [(i8, i8); 8] = [
    (0, 1),
    (1, 1),
    (1, 0),
    (1, -1),
    (0, -1),
    (-1, -1),
    (-1, 0),
    (-1, 1),
];

/// Marche le long d'une direction, en s'arrêtant **sur** le premier bloqueur.
fn walk(from: u8, d: (i8, i8), occ: u64) -> u64 {
    let mut bb = 0u64;
    let mut f = (from % 8) as i8;
    let mut r = (from / 8) as i8;
    loop {
        f += d.0;
        r += d.1;
        if !(0..8).contains(&f) || !(0..8).contains(&r) {
            break;
        }
        let s = (r * 8 + f) as u8;
        bb |= 1u64 << s;
        if occ & (1u64 << s) != 0 {
            break;
        }
    }
    bb
}

fn leaper(from: u8, deltas: &[(i8, i8)]) -> u64 {
    let mut bb = 0u64;
    for d in deltas {
        let f = (from % 8) as i8 + d.0;
        let r = (from / 8) as i8 + d.1;
        if (0..8).contains(&f) && (0..8).contains(&r) {
            bb |= 1u64 << ((r * 8 + f) as u32);
        }
    }
    bb
}

const KNIGHT_D: [(i8, i8); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];

fn king_deltas() -> Vec<(i8, i8)> {
    DIRS8.to_vec()
}

/// Attaques d'une pièce de type `kind` (0..5) de couleur `c` posée sur `sq`,
/// calculées uniquement par marche/deltas.
fn walk_piece(kind: usize, sq: u8, occ: u64) -> u64 {
    match kind {
        PAWN => {
            // Les pions ne se marchent pas : leur attaque dépend de la couleur
            // et `pawn_attacks_of` la fournit. Cet arm n'existe que pour rendre
            // le `match` exhaustif ; il n'est jamais atteint par l'oracle.
            0
        }
        KNIGHT => leaper(sq, &KNIGHT_D),
        BISHOP => DIRS8
            .iter()
            .filter(|(df, dr)| df != &0 && dr != &0)
            .fold(0u64, |acc, d| acc | walk(sq, *d, occ)),
        ROOK => DIRS8
            .iter()
            .filter(|(df, dr)| df == &0 || dr == &0)
            .fold(0u64, |acc, d| acc | walk(sq, *d, occ)),
        QUEEN => DIRS8.iter().fold(0u64, |acc, d| acc | walk(sq, *d, occ)),
        KING => leaper(sq, &king_deltas()),
        _ => 0,
    }
}

fn pawn_attacks_of(c: Color, sq: u8) -> u64 {
    let dr: i8 = if c == Color::White { 1 } else { -1 };
    leaper(sq, &[(-1, dr), (1, dr)])
}

/// Cases strictement entre `king` et `pinner`, par pas de coordonnées, plus la
/// case du pinner. Aucune table.
///
/// Précondition : `pinner` est aligné avec `king` (c'est le cas par
/// construction : c'est le premier bloqueur d'un rayon depuis le roi).
fn pin_line_walk(king: u8, pinner: u8) -> u64 {
    let (mut f, mut r) = ((king % 8) as i8, (king / 8) as i8);
    let (tf, tr) = ((pinner % 8) as i8, (pinner / 8) as i8);
    let df = (tf - f).signum();
    let dr = (tr - r).signum();
    let mut line = 0u64;
    loop {
        f += df;
        r += dr;
        let s = (r * 8 + f) as u8;
        line |= 1u64 << s;
        if s == pinner {
            return line;
        }
    }
}

/// `(danger, checkers, pinned, pin_lines_oracle)`.
fn oracles(pos: &Position) -> (u64, u64, u64, [u64; 64]) {
    let us = pos.side;
    let them = us.opp();
    let ksq = pos.king_sq(us);
    assert!(ksq < 64, "oracle : position legale exigee (roi present)");
    let occ = pos.occ();
    let occ_no_king = occ ^ (1u64 << ksq);
    let ks_bit = 1u64 << ksq;

    // --- danger + checkers : une passe par pièce ennemie, attaques par marche
    let mut danger = 0u64;
    let mut checkers = 0u64;
    for sq in 0..64u8 {
        let m = pos.piece_on(sq);
        if m == 0 {
            continue;
        }
        let code = m - 1;
        if code / 6 != them.idx() as u8 {
            continue;
        }
        let kind = (code % 6) as usize;
        let att = if kind == PAWN {
            pawn_attacks_of(them, sq)
        } else {
            walk_piece(kind, sq, occ_no_king)
        };
        danger |= att;
        // Le roi ennemi ne donne pas échec dans `compute_checkers` : ses
        // attaques comptent pour `danger` mais pas pour `checkers`. Inatteignable
        // sur un corpus légal (rois adjacents interdits), mais l'oracle ne doit
        // pas encoder une sémantique que l'implémentation rejette.
        if kind != KING && att & ks_bit != 0 {
            checkers |= 1u64 << sq;
        }
    }

    // --- pins : « retirer la pièce expose-t-il le roi à un slider ennemi ? »
    // Premier bloqueur par direction sous une occupation donnée.
    let first_blockers = |occ: u64| -> [Option<u8>; 8] {
        let mut out = [None; 8];
        for (i, d) in DIRS8.iter().enumerate() {
            let mut f = (ksq % 8) as i8;
            let mut r = (ksq / 8) as i8;
            loop {
                f += d.0;
                r += d.1;
                if !(0..8).contains(&f) || !(0..8).contains(&r) {
                    break;
                }
                let s = (r * 8 + f) as u8;
                if occ & (1u64 << s) != 0 {
                    out[i] = Some(s);
                    break;
                }
            }
        }
        out
    };
    // Sliders ennemis qui attaquent `ksq` sous l'occupation `occ`.
    let slider_attackers = |occ: u64| -> u64 {
        let mut acc = 0u64;
        for (i, b) in first_blockers(occ).iter().enumerate() {
            let Some(s) = *b else { continue };
            let m = pos.piece_on(s);
            if m == 0 {
                continue;
            }
            let code = m - 1;
            if code / 6 != them.idx() as u8 {
                continue;
            }
            let kind = (code % 6) as usize;
            let diagonal = i % 2 == 1;
            let matches = match kind {
                QUEEN => true,
                BISHOP => diagonal,
                ROOK => !diagonal,
                _ => false,
            };
            if matches {
                acc |= 1u64 << s;
            }
        }
        acc
    };

    let baseline = slider_attackers(occ);
    let mut pinned = 0u64;
    let mut lines = [0u64; 64];
    for from in 0..64u8 {
        if from == ksq {
            continue;
        }
        let m = pos.piece_on(from);
        if m == 0 {
            continue;
        }
        let code = m - 1;
        if code / 6 != us.idx() as u8 {
            continue;
        }
        let occ2 = occ ^ (1u64 << from);
        let new_attackers = slider_attackers(occ2) & !baseline;
        if new_attackers == 0 {
            continue;
        }
        pinned |= 1u64 << from;
        let pinner = new_attackers.trailing_zeros() as u8;
        lines[from as usize] = pin_line_walk(ksq, pinner);
    }

    (danger, checkers, pinned, lines)
}

/// FEN hors corpus : roi blanc absent (entrée hors contrat). Les trois
/// fonctions doivent y rester totales ; `positions()` ne la contient pas car
/// les oracles exigent un roi.
const FEN_SANS_ROI: &str = "4k3/8/8/8/8/8/8/8 w - - 0 1";

/// Positions de travail : les trois goldens GCT + Kiwipete et la position
/// initiale, des positions fixes qui exercent les chemins rares (clouage par
/// dame sur rangée et diagonale, direction d'index négatif, échec double,
/// échec par pion), puis les positions issues d'un auto-jeu déterministe.
fn positions() -> Vec<Position> {
    let fens = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        // Clouage par une DAME sur une rangée (famille tour) : le fou c1 est
        // cloué par la dame a1 sur la ligne a1-g1.
        "4k3/8/8/8/8/8/8/q1B3K1 w - - 0 1",
        // Clouage par une DAME sur une diagonale (famille fou) : le pion f2
        // est cloué par la dame h4 sur la diagonale h4-e1.
        "4k3/8/8/8/7q/8/5P2/4K3 w - - 0 1",
        // Clouage dans une direction d'index NÉGATIF (ouest, DIR_POS = false) :
        // la tour b4 est clouée par la dame a4 sur la rangée 4.
        "4k3/8/8/8/qR1K4/8/8/8 w - - 0 1",
        // Échec double fixe (tour e8 sur la colonne, fou b4 sur la diagonale).
        "4r1k1/8/8/8/1b6/8/8/4K3 w - - 0 1",
        // Échec par un pion noir fixe : le pion d5 attaque e4.
        "4k3/8/8/3p4/4K3/8/8/8 w - - 0 1",
    ];
    let mut v: Vec<Position> = fens
        .iter()
        .map(|f| Position::from_fen(f).unwrap())
        .collect();
    let mut pos = Position::startpos();
    let mut rng = 0x2545_F491_4F6C_DD1Du64;
    for _ply in 0..160 {
        v.push(pos);
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
    v
}

#[test]
fn king_danger_correspond_a_l_oracle() {
    for pos in positions() {
        let us = pos.side;
        let them = us.opp();
        let ksq = pos.king_sq(us);
        let got = king_danger(&pos, ksq, them);
        let (want, _, _, _) = oracles(&pos);
        assert_eq!(got, want, "danger dans {pos:?}");
    }
}

#[test]
fn compute_checkers_correspond_a_l_oracle() {
    for pos in positions() {
        let us = pos.side;
        let them = us.opp();
        let ksq = pos.king_sq(us);
        let got = compute_checkers(&pos, ksq, them);
        let (_, want, _, _) = oracles(&pos);
        assert_eq!(got, want, "checkers dans {pos:?}");
    }
}

#[test]
fn compute_pins_correspond_a_l_oracle() {
    for pos in positions() {
        let us = pos.side;
        let them = us.opp();
        let ksq = pos.king_sq(us);
        let (pinned, lines) = compute_pins(&pos, ksq, them, us);
        let (_, _, want_pinned, want_lines) = oracles(&pos);
        assert_eq!(pinned, want_pinned, "pinned dans {pos:?}");
        for sq in 0..64usize {
            if want_pinned & (1u64 << sq) != 0 {
                assert_eq!(lines[sq], want_lines[sq], "pin_line[{sq}] dans {pos:?}");
            } else {
                assert_eq!(
                    lines[sq],
                    u64::MAX,
                    "une piece non clouee doit avoir pin_lines = u64::MAX ({sq})"
                );
            }
        }
    }
}

#[test]
fn la_position_initiale_n_a_ni_echec_ni_clouage() {
    let pos = Position::startpos();
    let ksq = pos.king_sq(Color::White);
    assert_eq!(compute_checkers(&pos, ksq, Color::Black), 0);
    let (pinned, lines) = compute_pins(&pos, ksq, Color::Black, Color::White);
    assert_eq!(pinned, 0);
    assert!(lines.iter().all(|&l| l == u64::MAX));
}

#[test]
fn les_trois_passes_restent_totales_sans_roi() {
    // Entrée hors contrat (roi absent) : contrat du projet = défini, silencieux,
    // sans panique (GATE 2.5 M3). `king_sq` porte lui-même un `debug_assert!`
    // (roi exigé), donc on lui préfère `has_king` + la sentinelle `NO_SQUARE` :
    // le test reste valide en profil debug comme en release.
    let pos = Position::from_fen(FEN_SANS_ROI).unwrap();
    assert!(!pos.has_king(Color::White));
    let ksq = NO_SQUARE;
    assert_eq!(king_danger(&pos, ksq, Color::Black), 0);
    assert_eq!(compute_checkers(&pos, ksq, Color::Black), 0);
    let (pinned, lines) = compute_pins(&pos, ksq, Color::Black, Color::White);
    assert_eq!(pinned, 0);
    assert!(lines.iter().all(|&l| l == u64::MAX));
}

#[test]
fn l_oracle_ne_compte_pas_le_roi_ennemi_comme_donneur_d_echec() {
    // Position illégale (rois adjacents), hors corpus : elle fige la sémantique
    // de l'oracle. Le roi ennemi contribue à `danger`, jamais à `checkers`.
    let pos = Position::from_fen("8/8/8/4k3/4K3/8/8/8 w - - 0 1").unwrap();
    let ksq = pos.king_sq(Color::White);
    let (want_danger, want_checkers, _, _) = oracles(&pos);
    assert_eq!(compute_checkers(&pos, ksq, Color::Black), 0);
    assert_eq!(
        want_checkers, 0,
        "le roi ennemi n'est pas un donneur d'échec"
    );
    assert_ne!(
        want_danger & (1u64 << ksq),
        0,
        "le roi ennemi doit rester une source de danger"
    );
    assert_eq!(king_danger(&pos, ksq, Color::Black), want_danger);
}
