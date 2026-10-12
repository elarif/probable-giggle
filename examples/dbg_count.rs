use mychess::movegen::{count_moves, generate_all};
use mychess::position::Position;

fn main() {
    let pos = Position::from_fen(
        "r3k2r/pPpp1Ppp/5n2/5N2/1BP1P3/q4N2/Pp1P2PP/R2Q1PK1 w kq - 0 1",
    )
    .unwrap();
    let list = generate_all(&pos);
    for (i, m) in list.iter().enumerate() {
        let mut p = pos;
        let u = p.make(m);
        let _ = count_moves(&p);
        let ok = p.zobrist == p.recompute_zobrist();
        p.unmake(m, u);
        let restored = p.mailbox == pos.mailbox
            && p.bb_piece == pos.bb_piece
            && p.bb_color == pos.bb_color
            && p.zobrist == pos.zobrist
            && p.castle == pos.castle
            && p.ep == pos.ep
            && p.halfmove == pos.halfmove;
        if !restored {
            println!("{i}: {}", m.uci());
            println!("CORRUPT after unmake of {}", m.uci());
            panic!("state diverged");
        }
    }
    println!("done");
}