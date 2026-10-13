use mychess::movegen::{MoveList, generate};
use mychess::position::Position;
use mychess::types::GenKind;
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let fens = [
        (
            "startpos",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        ),
        (
            "kiwipete",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        ),
    ];
    for (name, fen) in fens {
        let pos = Position::from_fen(fen).unwrap();
        let n = 2_000_000u64;
        let t = Instant::now();
        let mut acc = 0u64;
        for _ in 0..n {
            let mut l = MoveList::new();
            generate(&pos, &mut l, GenKind::All);
            acc += l.len() as u64;
        }
        let s = t.elapsed().as_secs_f64();
        println!(
            "generate {name}: {:.1} ns/call (acc {acc})",
            s / n as f64 * 1e9
        );

        let t = Instant::now();
        let mut acc = 0u64;
        let p2 = pos;
        for _ in 0..n {
            black_box(
                p2.is_square_attacked(black_box(20), black_box(mychess::types::Color::Black)),
            );
            acc += 1;
        }
        let s = t.elapsed().as_secs_f64();
        println!(
            "is_square_attacked: {:.2} ns/call (acc {acc})",
            s / n as f64 * 1e9
        );

        let t = Instant::now();
        let mut acc = 0u64;
        for _ in 0..n {
            acc += mychess::slider::rook_attacks(black_box(0), black_box(pos.occ()));
        }
        let s = t.elapsed().as_secs_f64();
        println!(
            "rook_attacks: {:.2} ns/call (acc {acc})",
            s / n as f64 * 1e9
        );
    }
}
