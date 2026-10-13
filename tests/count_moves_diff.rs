//! L1 differential: `count_moves` (popcount leaf path) must agree exactly
//! with `generate(.., All).len()` on every position: golden positions, random
//! self-play positions (including promotions and the pinned/EP corner cases),
//! and shallow perft over the golden suite.

use mychess::movegen::count_moves;
use mychess::position::Position;

fn golden_pos() -> Vec<&'static str> {
    vec![
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    ]
}

#[test]
fn count_matches_generate_golden_and_shallow_perft() {
    for fen in golden_pos() {
        for depth in 1u32..=3 {
            let mut pos = Position::from_fen(fen).unwrap();
            let want = mychess::perft::perft(&mut pos, depth);
            let mut pos = Position::from_fen(fen).unwrap();
            // Walk the tree with MoveList-based counting at every node.
            assert_eq!(walk_len(&mut pos, depth), want, "{fen} d{depth}");
        }
    }
}

fn walk_len(pos: &mut Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let list = mychess::movegen::generate_all(pos);
    if depth == 1 {
        return list.len() as u64;
    }
    let mut n = 0;
    for mv in list.iter() {
        let u = pos.make(mv);
        n += walk_len(pos, depth - 1);
        pos.unmake(mv, u);
    }
    n
}

#[test]
fn count_matches_generate_selfplay_every_ply() {
    let mut state = 0xDEAD_BEEF_CAFE_F00Du64;
    for game in 0..40 {
        let mut pos = Position::startpos();
        for ply in 0..100 {
            let list = mychess::movegen::generate_all(&pos);
            assert_eq!(
                count_moves(&pos),
                list.len() as u64,
                "game {game} ply {ply} at {pos:?}"
            );
            if list.is_empty() {
                break;
            }
            let pick = (rand(&mut state) as usize) % list.len();
            let mv = list.get(pick);
            // Also verify exactly after make (opponent side, possibly in check).
            let u = pos.make(mv);
            let l2 = mychess::movegen::generate_all(&pos);
            assert_eq!(
                count_moves(&pos),
                l2.len() as u64,
                "game {game} ply {ply} (after make) at {pos:?}"
            );
            pos.unmake(mv, u);
            let _ = pos.make(mv);
        }
    }
}

// Promotion flood (all FENs below verified LEGAL: neither king is attacked
// with the side NOT to move in check). pos4 is promotion/castle heavy; the
// last two add quiet promotion, promotion-capture and a bare endgame walk.
#[test]
fn count_matches_generate_promo_rich() {
    let fens = [
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        "r2qk3/2Pb4/8/8/8/8/4p3/R3K2R w Q - 0 1",
        "2n5/3P4/8/3K4/8/8/8/7k w - - 0 1",
        "4k3/8/8/8/8/8/P6P/K5Q1 w - - 0 1",
    ];
    for fen in fens {
        for d in 1..=3u32 {
            let mut a = Position::from_fen(fen).unwrap();
            let mut b = Position::from_fen(fen).unwrap();
            let want = mychess::perft::perft(&mut a, d);
            assert_eq!(walk_len(&mut b, d), want, "{fen} d{d}");
        }
    }
}

fn rand(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}
