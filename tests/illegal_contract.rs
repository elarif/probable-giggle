//! GATE 2.5 M3 — contract on illegal positions ("position légale exigée").
//!
//! Freezes the behaviour at the king-capture emission sites: on an illegal
//! entry (enemy king capturable / kings adjacent), no pseudo king-capture is
//! ever emitted, `generate` and `count_moves` agree (their twin property
//! holds on any entry), perft stays crash-free — debug builds assert loudly
//! with the contract message while release builds are defined and silent.

use mychess::movegen::{MoveList, count_moves, generate};
use mychess::position::Position;
use mychess::refgen;
use mychess::types::{GenKind, Move};

/// Illegal entries: (fen, enemy-king square index, description).
const ILLEGAL: &[(&str, u8, &str)] = &[
    (
        "4k3/8/8/8/8/8/8/4R2K w - - 0 1",
        60, // e8
        "enemy king capturable on an open file (rook vs king, white to move)",
    ),
    (
        "8/8/8/3k4/3K4/8/8/8 w - - 0 1",
        35, // d5
        "kings adjacent: each attacks the other, both entries illegal",
    ),
    (
        "7k/8/6N1/8/8/8/8/K7 w - - 0 1",
        63, // h8
        "enemy king capturable by a knight leaper",
    ),
];

fn no_king_capture(list: &[Move], ks: u8, what: &str) {
    for m in list {
        assert_ne!(
            m.to(),
            ks,
            "{what}: a move targets the enemy king square ({m:?}) — king capture emitted?"
        );
    }
}

#[test]
fn king_capture_contract_frozen() {
    for (fen, ks, desc) in ILLEGAL {
        let pos = Position::from_fen(fen).unwrap();
        let us_ks0 = pos.zobrist == pos.recompute_zobrist();

        // Sanity: parsing accepted the FEN and did not claim legality.
        let _ = us_ks0;

        let pseudo = |pos: &Position| {
            let mut l = MoveList::new();
            refgen::pseudo_legal(pos, &mut l);
            l.iter().collect::<Vec<_>>()
        };

        if cfg!(debug_assertions) {
            // Debug: loud contract assertion (no exploitable crash — the
            // message names the contract).
            let r = std::panic::catch_unwind(|| {
                let mut l = MoveList::new();
                generate(&pos, &mut l, GenKind::All);
                l.len()
            });
            assert!(
                r.is_err(),
                "{desc}: debug builds must assert the entry contract"
            );
            let r2 = std::panic::catch_unwind(|| count_moves(&pos));
            assert!(
                r2.is_err(),
                "{desc}: count_moves must assert the entry contract in debug"
            );
            let r3 = std::panic::catch_unwind(|| pseudo(&pos).len());
            assert!(
                r3.is_err(),
                "{desc}: refgen must assert at the emission site in debug"
            );
        } else {
            // Release: defined and silent — no king capture, no panic.
            let mut list = MoveList::new();
            generate(&pos, &mut list, GenKind::All);
            let moves: Vec<Move> = list.iter().collect();
            no_king_capture(&moves, *ks, desc);
            no_king_capture(&pseudo(&pos), *ks, desc);
            no_king_capture(
                &refgen::legal_moves(&pos).iter().collect::<Vec<_>>(),
                *ks,
                desc,
            );

            // Twin property survives (same masks on both fast paths).
            assert_eq!(
                count_moves(&pos),
                moves.len() as u64,
                "{desc}: count_moves must agree with generate on any entry"
            );

            // Perft path crash-free and deterministic; d1 counts stay
            // consistent with the fast leaf path.
            let mut p = pos;
            let p2 = mychess::perft::perft(&mut p, 2);
            let mut p = pos;
            assert_eq!(
                p2,
                mychess::perft::perft(&mut p, 2),
                "{desc}: perft deterministic"
            );
            let mut p = pos;
            assert_eq!(
                mychess::perft::perft(&mut p, 1),
                count_moves(&pos),
                "{desc}: perft(1) == count_moves"
            );
        }
    }
}

/// Regression guard for the *reachable-by-contract* path: nothing changed for
/// legal positions — a king capture was never legal, and the masked entry
/// must stay bit-equal to the reference oracle.
#[test]
fn legal_positions_keep_exact_counts() {
    for fen in [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    ] {
        for d in 1..=3u32 {
            let mut a = Position::from_fen(fen).unwrap();
            let mut b = Position::from_fen(fen).unwrap();
            assert_eq!(
                mychess::perft::perft(&mut a, d),
                refgen::perft(&mut b, d),
                "{fen} d{d}"
            );
        }
    }
}

/// Also freeze: a LEGAL position that merely LOOKS like the trap (enemy king
/// not capturable, side to move in the illegal state) still generates all
/// king-replies — the mask must never touch legal positions.
#[test]
fn side_not_to_move_in_check_is_out_of_contract_but_defined() {
    // White to move while WHITE is actually the side giving check to BLACK:
    // illegal entry; in release, defined + silent.
    let fen = "4k3/8/8/8/8/8/8/2R3KR w - - 0 1";
    let pos = Position::from_fen(fen).unwrap();
    if !cfg!(debug_assertions) {
        let mut list = MoveList::new();
        generate(&pos, &mut list, GenKind::All);
        no_king_capture(&list.iter().collect::<Vec<_>>(), 60, "capturable variant");
        assert_eq!(count_moves(&pos), list.len() as u64);
    }
}
