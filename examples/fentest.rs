use mychess::position::Position;
use mychess::types::*;
use mychess::movegen::{MoveList, generate};

fn fen_of(pos: &Position, side: Color) -> String {
    let mut rows = Vec::new();
    for r in (0..8).rev() {
        let mut row = String::new();
        let mut empty = 0u8;
        for f in 0..8 {
            let sq = r * 8 + f;
            let bit = 1u64 << sq;
            if pos.occ() & bit == 0 {
                empty += 1;
                continue;
            }
            if empty > 0 {
                row.push_str(&empty.to_string());
                empty = 0;
            }
            let k = pos.piece_on(sq) as usize % 6;
            let ch = ["p", "n", "b", "r", "q", "k"][k];
            if pos.color_bb(Color::White) & bit != 0 {
                row.push_str(ch.to_uppercase().as_str());
            } else {
                row.push(ch.chars().next().unwrap());
            }
        }
        if empty > 0 {
            row.push_str(&empty.to_string());
        }
        rows.push(row);
    }
    format!(
        "{} {} -",
        rows.join("/"),
        if side == Color::White { "w" } else { "b" }
    )
}

fn main() {
    let mut pos =
        Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
    println!("fen_of(startpos) = {}", fen_of(&pos, pos.side));
    // find move e2->e4 (12->28), then e7->e5 (52->36), then g1->f3 (6->38)
    for (from, to, label) in [(12u8, 28u8, "e2e4"), (52, 36, "e7e5"), (6, 38, "g1f3")] {
        let mut list = MoveList::new();
        generate(&pos, &mut list, GenKind::All);
        let mut found = None;
        for i in 0..list.len() {
            let mv = list.get(i);
            if mv.from() == from && mv.to() == to {
                found = Some(mv);
            }
        }
        match found {
            Some(mv) => {
                let u = pos.make(mv);
                println!("after {label}: {}", fen_of(&pos, pos.side));
                pos.unmake(mv, u);
            }
            None => println!("after {label}: MOVE NOT FOUND (illegal?)"),
        }
        // re-apply persistently
        let mut list2 = MoveList::new();
        generate(&pos, &mut list2, GenKind::All);
        for i in 0..list2.len() {
            let mv = list2.get(i);
            if mv.from() == from && mv.to() == to {
                pos.make(mv);
            }
        }
    }
    println!("final: {}", fen_of(&pos, pos.side));
}