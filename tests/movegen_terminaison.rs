//! `has_legal_move` contre `generate` : deux chemins, même verdict.
//!
//! L'oracle est `generate`, qui matérialise la liste. Le gros du corpus est
//! dérivé de parcours réels de perft, pas écrit à la main : les FEN inventées
//! de la phase 1 se sont révélées fausses deux fois. Les FEN du corpus
//! explicite sont donc construites à la main mais vérifiées une à une avec
//! `divide` sur le binaire avant d'être figées ici, et leurs verdicts
//! attendus viennent toujours de `generate`.

use mychess::movegen::{MoveList, attackers_to, generate, has_legal_move};
use mychess::position::Position;
use mychess::types::{Color, GenKind};

fn fen_legaux(fen: &str) -> bool {
    let pos = Position::from_fen(fen).unwrap();
    let mut l = MoveList::new();
    generate(&pos, &mut l, GenKind::All);
    !l.is_empty()
}

fn coups(fen: &str) -> MoveList {
    let pos = Position::from_fen(fen).unwrap();
    let mut l = MoveList::new();
    generate(&pos, &mut l, GenKind::All);
    l
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
    // La FEN du brief (`4k3/8/8/8/8/2n5/1q6/K7 w`) a été vérifiée avant
    // réutilisation : le cavalier y est en c3, pas en c2, donc le roi a1
    // n'est qu'en échec simple de la dame b2 et Kxb2 est le seul coup.
    // Remplacée par ce vrai échec double, confirmé par `divide` : la tour e1
    // et le fou b5 attaquent tous deux le roi e8, et il reste exactement
    // trois fuites de roi — d8, f7, f8. L'assertion « tous les coups partent
    // du roi » a donc des dents : trois coups, tous de roi, deux attaquants.
    let pos = Position::from_fen("4k3/8/8/1B6/8/8/8/4R2K b - - 0 1").unwrap();
    let mut l = MoveList::new();
    generate(&pos, &mut l, GenKind::All);
    assert_eq!(l.len(), 3, "le corpus du test suppose trois fuites de roi");
    assert!(
        l.iter().all(|m| m.from() == 60),
        "seuls des coups de roi e8"
    );
    // Deux attaquants distincts : c'est bien un échec double.
    assert_eq!(attackers_to(&pos, 60, Color::White).count_ones(), 2);
    assert_eq!(has_legal_move(&pos), !l.is_empty());
}

/// Catégories que le parcours perft n'atteint pas. Mesure sur le parcours du
/// test suivant (444 826 positions avant l'ajout de l'arbre d'ep cloué, qui
/// en apporte 65 de plus et couvre enfin ep-cloué et ep-seul-coup) : zéro
/// échec double, zéro promotion-seule-échappatoire, zéro coup-de-pièce-
/// clouée-seul. Un faux négatif dans ces classes y survivait donc sans
/// bruit ; il y en avait un (ep d'un pion cloué), trouvé en revue, pas par
/// le différentiel.
///
/// Chaque FEN a d'abord été vérifiée avec `divide` sur le binaire, puis les
/// verdicts attendus viennent de `generate`, qui reste l'oracle.
#[test]
fn corpus_explicite_des_categories_rares() {
    // Attendu de `generate`, vérifié par `divide`, pour chaque position.
    let attendus: [(&str, bool); 7] = [
        // 1. Ep joué par un pion CLOUÉ : le pion f5 est sur la ligne
        // d'épingle Bd3-h7 (d3-e4-f5-g6-h7) ; capturer vers g6 le laisse sur
        // cette ligne. Unique coup légal.
        ("k7/4nn1K/8/5Pp1/8/3b4/1b6/8 w - g6 0 1", true),
        // 2. Contrast : le même type de capture par un pion non cloué.
        ("8/8/8/3pP3/8/8/8/K6k w - d6 0 1", true),
        // 3. L'ep est le SEUL coup : roi a1 étouffé par Kc2 (b1, b2) et Bb3
        // (a2), poussée e5-e6 barrée par le cavalier e6, ep e5xd6 libre.
        ("8/8/4n3/3pP3/8/1b6/2k5/K7 w - d6 0 1", true),
        // 4. Échec double sans issue : les deux attaquants (Th1, Fb2) et le
        // roi blanc f7 couvrent les trois fuites de h8. Aucun coup légal, et
        // aucune pièce ne peut s'interposer : c'est le seul cas qui atteint
        // `ncheck >= 2` sans sortie par le roi.
        ("7k/5K2/8/8/8/8/1B6/7R b - - 0 1", false),
        // 5. Promotion seule échappatoire : les quatre coups légaux sont
        // b7-b8=N/B/R/Q (a2 tenu par Bb3, b1/b2 par le roi c2).
        ("8/1P6/8/8/8/1b6/2k5/K7 w - - 0 1", true),
        // 6. Coup de pièce clouée seul : le fou g2 est cloué sur la diagonale
        // b7-h1, et ses cinq coups sur la ligne sont les seuls légaux.
        ("k7/1b6/8/8/8/8/4n1B1/5n1K w - - 0 1", true),
        // 7. Roque : impossible d'en faire le SEUL coup légal (voir plus bas).
        ("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1", true),
    ];
    for (fen, attendu) in attendus {
        let pos = Position::from_fen(fen).unwrap();
        assert_eq!(fen_legaux(fen), attendu, "oracle generate sur {fen}");
        assert_eq!(has_legal_move(&pos), attendu, "has_legal_move sur {fen}");
    }

    // 1. Ep d'un pion cloué : exactement f5-g6 (37 -> 46).
    let l = coups("k7/4nn1K/8/5Pp1/8/3b4/1b6/8 w - g6 0 1");
    assert_eq!(l.len(), 1);
    assert_eq!((l.get(0).from(), l.get(0).to()), (37, 46), "f5-g6 attendu");

    // 2. Contrast non cloué : l'ep est bien présent parmi d'autres coups.
    let l = coups("8/8/8/3pP3/8/8/8/K6k w - d6 0 1");
    assert!(l.len() > 1);
    assert!(l.iter().any(|m| m.from() == 36 && m.to() == 43)); // e5-d6

    // 3. Ep seul coup : exactement e5-d6 (36 -> 43).
    let l = coups("8/8/4n3/3pP3/8/1b6/2k5/K7 w - d6 0 1");
    assert_eq!(l.len(), 1);
    assert_eq!((l.get(0).from(), l.get(0).to()), (36, 43), "e5-d6 attendu");

    // 4. Échec double sans issue : deux attaquants, aucune liste.
    let pos = Position::from_fen("7k/5K2/8/8/8/8/1B6/7R b - - 0 1").unwrap();
    assert_eq!(attackers_to(&pos, 63, Color::White).count_ones(), 2);

    // 5. Promotion seule : quatre coups, tous depuis b7 (49), tous promus.
    let l = coups("8/1P6/8/8/8/1b6/2k5/K7 w - - 0 1");
    assert_eq!(l.len(), 4);
    assert!(l.iter().all(|m| m.from() == 49 && m.is_promotion()));

    // 6. Pièce clouée seule : cinq coups, tous depuis g2 (14).
    let l = coups("k7/1b6/8/8/8/8/4n1B1/5n1K w - - 0 1");
    assert_eq!(l.len(), 5);
    assert!(l.iter().all(|m| m.from() == 14));

    // 7. Roque : aucun cas « roque seul coup légal » n'existe. Le roque
    // exige f1 et g1 vides, donc la tour h1 peut toujours jouer h1-f1 ou
    // h1-g1 ; et elle ne peut pas être clouée sur la rangée 1, puisque le
    // roi e1 est entre elle et toute tour ennemie de la rangée. La branche
    // roque de `has_legal_move` ne décide donc jamais du verdict : elle est
    // défensive, et ce test vérifie seulement qu'elle n'invente pas de coup
    // et que les coups de tour qui rendent la catégorie vide sont produits.
    let l = coups("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
    assert!(
        l.iter().any(|m| m.from() == 4 && m.to() == 6),
        "O-O attendu"
    );
    assert!(
        l.iter().any(|m| m.from() == 4 && m.to() == 2),
        "O-O-O attendu"
    );
    assert!(
        l.iter()
            .any(|m| m.from() == 7 && (m.to() == 5 || m.to() == 6)),
        "la tour h1 doit avoir f1 ou g1, ce qui vide la catégorie"
    );
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
        // Arbre ajouté pour que l'ep d'un pion cloué soit exercé par le
        // parcours lui-même, et pas seulement par le corpus explicite.
        ("k7/4nn1K/8/5Pp1/8/3b4/1b6/8 w - g6 0 1", 3),
    ];
    let mut checked = 0u32;
    for (fen, depth) in fens {
        let mut pos = Position::from_fen(fen).unwrap();
        walk(&mut pos, depth, &mut checked);
    }
    assert!(
        checked >= 444_891,
        "corpus rétréci : {checked} positions, plancher 444891"
    );
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
