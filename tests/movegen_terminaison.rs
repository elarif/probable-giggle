//! `has_legal_move` contre `generate` : deux chemins, même verdict.
//!
//! L'oracle est `generate`, qui matérialise la liste. Les positions sont
//! dérivées de parcours réels (perft et auto-jeu), pas écrites à la main :
//! les FEN inventées de la phase 1 se sont révélées fausses deux fois.

use mychess::movegen::{MoveList, generate, has_legal_move};
use mychess::position::Position;
use mychess::types::GenKind;

fn fen_legaux(fen: &str) -> bool {
    let pos = Position::from_fen(fen).unwrap();
    let mut l = MoveList::new();
    generate(&pos, &mut l, GenKind::All);
    !l.is_empty()
}

#[test]
fn mat_et_pat() {
    // Mat de l'imbecile : les blancs n'ont aucun coup.
    assert!(!has_legal_move(
        &Position::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3")
            .unwrap()
    ));
    // Pat : roi a8, dame c7, roi blanc c6 — les blancs sont étouffés.
    let pat = "k7/2Q5/2K5/8/8/8/8/8 b - - 0 1";
    assert!(!has_legal_move(&Position::from_fen(pat).unwrap()));
    assert!(!fen_legaux(pat));
}

#[test]
fn double_echec_ne_laisse_que_le_roi() {
    // Position du brief, vérifiée par `divide` avant d'écrire ce test : ce
    // n'est **pas** un mat. Blancs Ka1, noirs Ke8/Qb2/Nc2, blancs au trait :
    // la dame b2 et le cavalier c2 donnent bien un échec double, mais la
    // dame est prenable — `perft 1` et `divide` donnent exactement un coup,
    // `a1b2` (Kxb2). L'assertion `all(from == 0)` n'est donc pas vide : elle
    // exige un coup, et exige que ce soit un coup de roi (a1 = 0), seul type
    // de coup qu'un échec double autorise.
    let pos = Position::from_fen("4k3/8/8/8/8/2n5/1q6/K7 w - - 0 1").unwrap();
    let mut l = MoveList::new();
    generate(&pos, &mut l, GenKind::All);
    assert_eq!(l.len(), 1, "le corpus du test suppose un seul coup légal");
    assert!(l.iter().all(|m| m.from() == 0));
    assert_eq!(has_legal_move(&pos), !l.is_empty());
}

#[test]
fn les_deux_chemins_concordent_sur_tout_l_arbre_de_kiwipete() {
    let fens = [
        (
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            3u32,
        ),
        (
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            4,
        ),
        ("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1", 4),
        (
            "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
            3,
        ),
    ];
    let mut checked = 0u32;
    for (fen, depth) in fens {
        let mut pos = Position::from_fen(fen).unwrap();
        walk(&mut pos, depth, &mut checked);
    }
    assert!(checked > 10_000, "corpus trop maigre : {checked} positions");
}

/// Parcourt tous les coups légaux jusqu'à `depth` et compare les deux chemins
/// à chaque nœud. `Position` n'a pas de `to_fen` (type gelé) : le message
/// d'échec imprime le `Debug` de la position, qui porte le plateau, le trait,
/// les droits de roque, l'ep et le Zobrist — assez pour rejouer le cas.
fn walk(pos: &mut Position, depth: u32, checked: &mut u32) {
    let mut l = MoveList::new();
    generate(pos, &mut l, GenKind::All);
    let a = !l.is_empty();
    let b = has_legal_move(pos);
    assert_eq!(
        a,
        b,
        "desaccord has_legal_move={b} generate={a} n={} position={pos:?}",
        l.len()
    );
    *checked += 1;
    if depth == 0 {
        return;
    }
    for mv in l.iter() {
        let u = pos.make(mv);
        walk(pos, depth - 1, checked);
        pos.unmake(mv, u);
    }
}
