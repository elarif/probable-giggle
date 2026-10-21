//! Le carré en-passant doit être **canonique** : conservé seulement si une
//! capture en-passant est réellement disponible.
//!
//! Bug prouvé : `make` pose la case ep après tout double pas, même sans pion
//! adverse adjacent, et le hash Zobrist inclut le fichier ep. La même position
//! atteinte autrement a donc un hash différent.

use mychess::position::Position;
use mychess::types::*;

fn sq(name: &str) -> u8 {
    square_from_uci(name.as_bytes())
        .expect("case valide")
        .index()
}

#[test]
fn double_pas_sans_pion_adjacent_ne_laisse_pas_de_case_ep() {
    let mut pos = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
    let u = pos.make(Move::new(sq("e2"), sq("e4"), FLAG_DOUBLE_PUSH));
    assert_eq!(
        pos.ep, NO_EP,
        "aucun pion noir en d4/f4 : la case ep doit etre effacee"
    );
    pos.unmake(Move::new(sq("e2"), sq("e4"), FLAG_DOUBLE_PUSH), u);
    assert_eq!(pos.ep, NO_EP, "unmake doit restaurer la case ep d'origine");
}

#[test]
fn double_pas_avec_pion_adjacent_conserve_la_case_ep() {
    let mut pos = Position::from_fen("4k3/8/8/8/3p4/8/4P3/4K3 w - - 0 1").unwrap();
    let u = pos.make(Move::new(sq("e2"), sq("e4"), FLAG_DOUBLE_PUSH));
    assert_eq!(pos.ep, sq("e3"), "un pion noir en d4 peut capturer e3");
    assert_eq!(pos.ep_capture_square(Color::Black), Some(sq("e4")));
    pos.unmake(Move::new(sq("e2"), sq("e4"), FLAG_DOUBLE_PUSH), u);
}

#[test]
fn deux_chemins_vers_la_meme_position_donnent_le_meme_hash() {
    // Position A : le pion blanc arrive en e4 par un double pas (aucun pion
    // noir en d4/f4 -> la case ep est un fantome).
    let mut a = Position::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
    a.make(Move::new(sq("e2"), sq("e4"), FLAG_DOUBLE_PUSH));

    // Position B : la meme position, construite par FEN (aucune case ep).
    let b = Position::from_fen("4k3/8/8/8/4P3/8/8/4K3 b - - 0 1").unwrap();

    assert_eq!(
        a.zobrist, b.zobrist,
        "deux positions identiques doivent hasher identiquement"
    );
    assert_eq!(a.recompute_zobrist(), a.zobrist);
    assert_eq!(b.recompute_zobrist(), b.zobrist);
}

#[test]
fn une_fen_avec_case_ep_fantome_est_canonisee() {
    // Case ep e3 annoncee sans pion blanc en d4/f4 : rien ne peut capturer.
    let p = Position::from_fen("4k3/8/8/8/4P3/8/8/4K3 b - e3 0 1").unwrap();
    assert_eq!(p.ep, NO_EP, "case ep sans captureur : canonisee a l'entree");
    assert_eq!(p.recompute_zobrist(), p.zobrist);

    // Et quand un captureur existe, la case est conservee.
    let q = Position::from_fen("4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1").unwrap();
    assert_eq!(q.ep, sq("e3"));
    assert_eq!(q.recompute_zobrist(), q.zobrist);
}

#[test]
fn une_fen_avec_attaquant_sans_pion_capturable_est_canonisee() {
    // Pion noir en d4 qui attaque e3, mais aucun pion blanc en e4 : cette
    // position est impossible par le jeu. Le generateur n'offre aucune
    // capture ep (`ep_capture_square` est None), donc la case doit disparaitre
    // et la cle doit egaler celle de la meme position sans case ep.
    let with_ghost = Position::from_fen("4k3/8/8/8/3p4/8/8/4K3 b - e3 0 1").unwrap();
    let without = Position::from_fen("4k3/8/8/8/3p4/8/8/4K3 b - - 0 1").unwrap();
    assert_eq!(
        with_ghost.ep, NO_EP,
        "pion capturable absent : la case ep doit etre effacee"
    );
    assert_eq!(
        with_ghost.zobrist, without.zobrist,
        "meme plateau, meme trait, aucune capture ep : meme cle"
    );
    assert_eq!(with_ghost.recompute_zobrist(), with_ghost.zobrist);

    // Controle positif : les deux conditions tiennent (pion noir d4 qui
    // attaque e3 ET pion blanc capturable en e4) -> la case est conservee.
    let genuine = Position::from_fen("4k3/8/8/8/3pP3/8/8/4K3 b - e3 0 1").unwrap();
    assert_eq!(genuine.ep, sq("e3"));
    assert_eq!(genuine.ep_capture_square(Color::Black), Some(sq("e4")));
    assert_eq!(genuine.recompute_zobrist(), genuine.zobrist);
}

#[test]
fn la_canonisation_ne_change_aucun_compte_de_perft() {
    // Constantes publiees dans `tests/perft/startpos.txt` et
    // `tests/perft/kiwipete.txt` (source chessprogramming.org/Perft_Results).
    // La canonicalisation ne doit pas fuiter dans la legalite.
    let mut startpos =
        Position::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
    assert_eq!(
        mychess::perft::perft(&mut startpos, 4),
        197_281,
        "startpos d4"
    );

    let mut kiwipete =
        Position::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1")
            .unwrap();
    assert_eq!(
        mychess::perft::perft(&mut kiwipete, 3),
        97_862,
        "kiwipete d3"
    );
}
