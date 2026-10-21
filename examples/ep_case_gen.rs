//! Compare les compteurs détaillés de `mychess` aux valeurs publiées CPW,
//! et isole les sous-arbres racine qui portent l'écart.
//!
//! Référence : `slaml/cpw_d9_reference.md` (table CPW relue le 2026-10-13)
//! et `slaml/d9_checkcols_report_20261014.md` (analyse de l'écart d9).
//!
//! Usage : `mychess-ep-case-gen <depth> [fen]`
//!
//! Le split racine est le même que celui du runner d9 shardé (une tâche par
//! coup racine), ce qui rend les totaux directement comparables.

use mychess::movegen::generate;
use mychess::perft::perft_detailed;
use mychess::position::Position;
use mychess::types::GenKind;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let depth: u32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(8);
    let fen = args.get(2).cloned().unwrap_or_else(|| {
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1".to_string()
    });

    let mut pos = Position::from_fen(&fen).expect("valid FEN");
    let mut list = mychess::movegen::MoveList::new();
    generate(&pos, &mut list, GenKind::All);

    let total = perft_detailed(&mut pos, depth);
    println!(
        "depth {depth}: nodes={} captures={} ep={} castles={} promo={} checks={} disc={} dbl={} mates={}",
        total.nodes,
        total.captures,
        total.ep,
        total.castles,
        total.promotions,
        total.checks,
        total.discovery_checks,
        total.double_checks,
        total.mates
    );

    let moves: Vec<_> = list.iter().collect();
    let mut rows: Vec<(String, u64, u64, u64, u64)> = Vec::new();
    for mv in moves {
        let m = mv;
        let u = pos.make(m);
        let sub = perft_detailed(&mut pos, depth - 1);
        rows.push((
            m.uci(),
            sub.discovery_checks,
            sub.double_checks,
            sub.checks,
            sub.ep,
        ));
        pos.unmake(m, u);
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.1 + r.2));
    println!("\ntop subtrees by (disc+dbl):");
    for (uci, disc, dbl, chk, ep) in rows.iter().take(25) {
        println!(
            "  {uci:6} disc={disc:>10} dbl={dbl:>10} checks={chk:>12} ep={ep:>8}  (disc+dbl={})",
            disc + dbl
        );
    }
}
