//! Malformed / inconsistent FEN entries must never crash the engine.
//!
//! Three crash classes were found by fuzzing the FEN space (5940
//! combinations of board / side / castling / en-passant):
//!
//! 1. **En-passant square without a pawn.** A FEN may name an ep square
//!    with no pawn behind it. The generator used to emit an ep capture
//!    anyway, and `make` then removed "the piece" from an empty square —
//!    `mailbox[sq] - 1` underflowed to 255 and indexed past the end of
//!    the bitboard array.
//! 2. **Kingless position.** `king_sq` returned `trailing_zeros()` of an
//!    empty bitboard (square 0 — a wrong answer), and the movegen then
//!    indexed its tables with it.
//! 3. **Castling rights the board does not support.** A FEN claiming
//!    `K` with no rook on h1 made the generator emit a castling move
//!    from an empty square, corrupting the mailbox.
//!
//! The contract stays: a *legal* position is the normal case and is
//! untouched; these entries are out of contract but **defined, silent and
//! crash-free**.

use mychess::movegen::{MoveList, count_moves, generate};
use mychess::position::Position;
use mychess::types::{GenKind, Move};

/// Drive the whole public surface over a position and report the moves.
fn probe(pos: &mut Position) -> Vec<Move> {
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    let moves: Vec<Move> = list.iter().collect();

    // The twin property must hold on any entry.
    assert_eq!(
        count_moves(pos),
        moves.len() as u64,
        "count_moves must agree with generate on any entry"
    );

    // Every emitted move must be makeable and unmakable without a panic,
    // and must restore the position exactly.
    let before = *pos;
    for &mv in &moves {
        let undo = pos.make(mv);
        pos.unmake(mv, undo);
        assert_eq!(*pos, before, "make/unmake must round-trip for {}", mv.uci());
    }
    moves
}

#[test]
fn ep_square_without_a_pawn_is_not_a_capture() {
    // White pawn on e5, ep square d6 — but no black pawn on d5 to capture.
    // The ep capture must not be offered, and nothing may crash.
    let mut pos = Position::from_fen("4k3/8/8/4P3/8/8/8/4K3 w - d6 0 1").unwrap();
    let moves = probe(&mut pos);
    assert!(
        !moves.iter().any(|m| m.is_ep()),
        "no ep capture may be offered when there is no pawn to capture"
    );
}

#[test]
fn ep_square_without_a_pawn_stays_consistent_over_a_perft() {
    let mut pos = Position::from_fen("4k3/8/8/4P3/8/8/8/4K3 w - d6 0 1").unwrap();
    // The whole subtree must be walkable without a panic.
    let n = mychess::perft::perft(&mut pos, 3);
    assert!(n > 0, "perft must produce a node count");
}

#[test]
fn ep_capture_still_works_when_the_pawn_is_there() {
    // The normal case: black pawn on d5, white pawn on e5, ep square d6
    // (the black pawn has just played d7-d5).
    let mut pos = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
    assert_eq!(
        pos.ep_capture_square(mychess::types::Color::White),
        Some(35)
    );
    let moves = probe(&mut pos);
    assert!(
        moves
            .iter()
            .any(|m| m.is_ep() && m.from() == 36 && m.to() == 43),
        "the genuine ep capture (e5xd6) must still be generated"
    );
}

#[test]
fn kingless_position_is_defined_and_silent() {
    // No king at all: the position is out of contract, but it must not
    // crash and must report "no legal moves".
    let mut pos = Position::from_fen("8/8/8/8/8/8/8/8 w - - 0 1").unwrap();
    assert!(!pos.has_king(mychess::types::Color::White));
    let moves = probe(&mut pos);
    assert!(moves.is_empty(), "a kingless position has no legal move");
    assert_eq!(count_moves(&pos), 0);
}

#[test]
fn missing_own_king_yields_no_moves() {
    // Black has a king, White does not.
    let mut pos = Position::from_fen("4k3/8/8/8/8/8/8/8 w - - 0 1").unwrap();
    let moves = probe(&mut pos);
    assert!(moves.is_empty(), "no king for the side to move => no move");
}

#[test]
fn castling_rights_without_a_rook_are_not_offered() {
    // The FEN claims `K` (kingside) but there is no rook on h1.
    let mut pos = Position::from_fen("4k3/8/8/8/8/8/8/4K3 w K - 0 1").unwrap();
    let moves = probe(&mut pos);
    assert!(
        !moves.iter().any(|m| m.is_castle()),
        "castling must not be offered without a rook on h1"
    );
}

#[test]
fn castling_rights_without_the_king_on_its_home_square_are_not_offered() {
    // The FEN claims `KQ` but the king is on d1, not e1.
    let mut pos = Position::from_fen("4k3/8/8/8/8/8/8/R2K3R w KQ - 0 1").unwrap();
    let moves = probe(&mut pos);
    assert!(
        !moves.iter().any(|m| m.is_castle()),
        "castling must not be offered when the king is off its home square"
    );
}

#[test]
fn genuine_castling_is_still_offered() {
    let mut pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    let moves = probe(&mut pos);
    let castles = moves.iter().filter(|m| m.is_castle()).count();
    assert_eq!(castles, 2, "both castlings must still be generated");
}

#[test]
fn malformed_fens_never_crash() {
    // A spread of inconsistent entries; every one of them must survive a
    // full generate / make / unmake / perft cycle.
    let boards = [
        "8/8/8/8/8/8/8/8",
        "4k3/8/8/8/8/8/8/4K3",
        "4k3/8/8/4P3/8/8/8/4K3",
        "4k3/4p3/8/8/8/8/8/4K3",
        "4k3/8/8/4K3/8/8/4K3/8",
        "4k3/8/8/8/8/8/8/4K2R",
        "4k3/8/8/8/8/8/8/4K1NR",
        "r3k2r/8/8/8/8/8/8/R3K2R",
    ];
    let eps = ["-", "a1", "a3", "a6", "a8", "d6", "e3", "e6", "h1", "h8"];
    let castles = ["-", "K", "Q", "k", "q", "KQkq"];
    let sides = ["w", "b"];

    for b in boards {
        for ep in eps {
            for c in castles {
                for s in sides {
                    let fen = format!("{b} {s} {c} {ep} 0 1");
                    let Ok(mut pos) = Position::from_fen(&fen) else {
                        continue;
                    };
                    probe(&mut pos);
                    let mut p = pos;
                    let _ = mychess::perft::perft(&mut p, 2);
                }
            }
        }
    }
}

#[test]
fn fen_parser_rejects_a_malformed_board() {
    // A board with only 7 ranks is not a FEN.
    assert!(
        Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP w KQkq - 0 1").is_err(),
        "a 7-rank board must be rejected"
    );
    // A rank that does not sum to 8 files.
    assert!(
        Position::from_fen("rnbqkbnr/pppppppp/9/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_err(),
        "a rank wider than 8 files must be rejected"
    );
    assert!(
        Position::from_fen("rnbqkbnr/ppppppp/7/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_err(),
        "a rank narrower than 8 files must be rejected"
    );
    // A well-formed board is still accepted.
    assert!(
        Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").is_ok(),
        "a valid board must still parse"
    );
}
