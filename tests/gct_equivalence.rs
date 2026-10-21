//! Équivalence `gct::generate` ≡ `movegen::generate` sur toute la surface.
//!
//! La comparaison porte sur l'**ensemble** des coups (mêmes triplés
//! `(from, to, flags)`), pas sur l'ordre : le post et `mychess` n'émettent pas
//! dans le même ordre, et ce n'est pas un critère de correction.

use mychess::gct::generate::generate as gct_generate;
use mychess::movegen::{MoveList, generate};
use mychess::position::Position;
use mychess::types::*;

/// Clé de comparaison stable : un coup est identifié par ses trois champs
/// publics (`from`, `to`, `flags`), jamais par la valeur 16 bits brute.
type MoveKey = (u8, u8, u8);

fn key(m: Move) -> MoveKey {
    (m.from(), m.to(), m.flags())
}

fn sorted_gct(pos: &Position) -> Vec<MoveKey> {
    let mut l = MoveList::new();
    let n = gct_generate(pos, &mut l);
    assert_eq!(
        n,
        l.len(),
        "generate doit renvoyer le nouvel indice du buffer"
    );
    let mut v: Vec<MoveKey> = l.iter().map(key).collect();
    v.sort_unstable();
    v
}

fn sorted_movegen(pos: &Position) -> Vec<MoveKey> {
    let mut l = MoveList::new();
    generate(pos, &mut l, GenKind::All);
    let mut v: Vec<MoveKey> = l.iter().map(key).collect();
    v.sort_unstable();
    v
}

#[test]
fn generate_renvoie_l_indice_d_append() {
    // Contrat dont depend la tache 6 : `generate` ajoute a la suite du buffer
    // et renvoie le NOUVEL indice, pas le nombre de coups ajoutes.
    let pos = Position::startpos();
    let mut l = MoveList::new();
    let n1 = gct_generate(&pos, &mut l);
    assert_eq!(n1, l.len());
    assert!(n1 > 0);
    let n2 = gct_generate(&pos, &mut l);
    assert_eq!(n2, 2 * n1, "generate doit renvoyer l'indice d'append");
    assert_eq!(l.len(), 2 * n1);
}

/// `Position` n'implémente pas `Debug` : rendu du plateau depuis ses champs
/// publics (`mailbox`, `side`, `castle`, `ep`).
fn describe(pos: &Position) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    for r in (0..8).rev() {
        for f in 0..8 {
            let m = pos.mailbox[r * 8 + f];
            s.push(if m == 0 {
                '.'
            } else {
                b"PNBRQKpnbrqk"[(m - 1) as usize] as char
            });
        }
        s.push('\n');
    }
    let _ = write!(s, "{:?} castle={} ep={}", pos.side, pos.castle, pos.ep);
    s
}

/// Positions **racines** de la marche : aucune ne porte de droit de roque ni
/// de case en-passant. Les **enfants** de la marche, eux, portent l'en-passant
/// que `Position::make` pose après une double poussée et les droits de roque
/// restants : ils sont comparés comme les autres (tâche 5).
fn positions_racines() -> Vec<Position> {
    let fens = [
        "4k3/pppppppp/8/8/8/8/PPPPPPPP/4K3 w - - 0 1",
        "4k3/8/8/8/8/8/8/4K3 w - - 0 1",
        "r5k1/5ppp/8/8/8/8/5PPP/R5K1 w - - 0 1",
        "4k3/pp6/8/2b5/8/6P1/PP6/4K3 w - - 0 1",
        "4k3/8/8/8/8/8/8/R3K2R w - - 0 1",
        // Armee complete sans droits de roque : cavaliers, dames, fous, tours.
        // C'est le corpus le plus large possible dans le perimetre de la tache 4.
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w - - 0 1",
        // Ouverture des quatre cavaliers, variante italienne : huit demi-coups
        // (1.e4 e5 2.Nf3 Nc6 3.Nc3 Nf6 4.Bc4 Bc5), sans droit de roque. Les
        // dames restent passives au noeud racine (Dd1 n'a que d1-e2) et leur
        // mobilite s'ouvre aux niveaux suivants.
        "r1bqk2r/pppp1ppp/2n2n2/2b1p3/2B1P3/2N2N2/PPPP1PPP/R1BQK2R w - - 0 1",
        // Promotions blanches : poussee c7-c8, capture NW c7xb8 (cavalier) et
        // capture NE c7xd8 (tour) — les deux bras de capture sont exerces.
        "1n1rk3/2P5/8/8/8/8/8/4K3 w - - 0 1",
        // Promotions noires : poussee b2-b1 et captures b2xa1 / b2xc1.
        "4k3/8/8/8/8/8/1p6/R1B1K3 b - - 0 1",
        // Echec par tour sur la colonne e, bloquable UNIQUEMENT par Fe3 :
        // isole le terme `squares_between` de `check_mask` (aucun autre coup
        // legal que les coups de roi et d2-e3).
        "4r1k1/8/8/8/8/8/3B4/4K3 w - - 0 1",
        // Double echec (fou b4 + cavalier f3 sur le roi e1) : seuls les coups
        // de roi sont legaux, le retour anticipe doit s'appliquer.
        "4k3/8/8/8/1b6/5n2/8/4K1N1 w - - 0 1",
    ];
    fens.iter()
        .map(|f| Position::from_fen(f).unwrap())
        .collect()
}

#[test]
fn gct_et_movegen_donnent_le_meme_ensemble_sur_deux_demi_coups() {
    for pos in positions_racines() {
        // On marche tout l'arbre jusqu'à la profondeur 2 et on compare les
        // niveaux 0, 1 et 2, pour multiplier les configurations de clouages,
        // d'échecs et de promotions.
        let mut stack: Vec<Position> = vec![pos];
        for depth in 0..3 {
            let mut next = Vec::new();
            for p in stack {
                assert_eq!(
                    sorted_gct(&p),
                    sorted_movegen(&p),
                    "ensembles de coups differents dans {}",
                    describe(&p)
                );
                // La derniere iteration ne developpe pas : comparer un niveau
                // qu'on jette ne prouve rien et coute du temps.
                if depth + 1 == 3 {
                    continue;
                }
                let mut l = MoveList::new();
                generate(&p, &mut l, GenKind::All);
                for mv in l.iter() {
                    let mut q = p;
                    q.make(mv);
                    // Tâche 5/7 : plus de filtre sur `ep`. `Position::make` ne
                    // conserve la case ep que si une capture est réellement
                    // disponible (canonicalisation) ; les deux générateurs
                    // lisent le même `pos.ep` et le même `ep_capture_square`,
                    // donc l'équivalence tient et l'en-passant est comparé.
                    next.push(q);
                }
            }
            stack = next;
            if stack.is_empty() {
                break;
            }
        }
    }
}

/// Toutes les positions des goldens perft du dépôt : elles couvrent roque,
/// en-passant, promotions et clouages en densité.
fn positions_goldens() -> Vec<Position> {
    let fens = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        // En-passant avec capture possible :
        "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1",
        // Roque des deux cotes encore disponible :
        "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
        // Roque bloque par une piece interposee :
        "r3k2r/8/8/8/8/8/8/R2QK2R w KQkq - 0 1",
    ];
    fens.iter()
        .map(|f| Position::from_fen(f).unwrap())
        .collect()
}

#[test]
fn gct_et_movegen_donnent_le_meme_ensemble_sur_les_goldens() {
    for pos in positions_goldens() {
        assert_eq!(
            sorted_gct(&pos),
            sorted_movegen(&pos),
            "ensembles de coups differents dans {}",
            describe(&pos)
        );
    }
}

#[test]
fn gct_et_movegen_donnent_le_meme_ensemble_sur_un_auto_jeu() {
    // 40 parties de 60 demi-coups, graine fixe : reproductible.
    let mut rng = 0xDEAD_BEEF_CAFE_F00Du64;
    for _game in 0..40 {
        let mut pos = Position::startpos();
        for _ply in 0..60 {
            assert_eq!(
                sorted_gct(&pos),
                sorted_movegen(&pos),
                "ensembles de coups differents dans {}",
                describe(&pos)
            );
            let mut l = MoveList::new();
            generate(&pos, &mut l, GenKind::All);
            if l.is_empty() {
                break;
            }
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let mv = l.get((rng as usize) % l.len());
            pos.make(mv);
        }
    }
}

#[test]
fn gct_generent_le_roque_des_deux_cotes() {
    let pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    let moves = sorted_gct(&pos);
    assert!(
        moves.contains(&(4, 6, FLAG_KING_CASTLE)),
        "petit roque blanc"
    );
    assert!(
        moves.contains(&(4, 2, FLAG_QUEEN_CASTLE)),
        "grand roque blanc"
    );
}

#[test]
fn gct_ne_roquepas_a_travers_une_case_attaquee() {
    // Tour noire en f8 : la case f1 est attaquee, le petit roque blanc est
    // illegal, le grand roque reste legal.
    let pos = Position::from_fen("4kr2/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
    let moves = sorted_gct(&pos);
    assert!(
        !moves.iter().any(|&(_, _, f)| f == FLAG_KING_CASTLE),
        "le petit roque doit etre refuse (f1 attaquee)"
    );
    assert!(
        moves.iter().any(|&(_, _, f)| f == FLAG_QUEEN_CASTLE),
        "le grand roque doit rester legal"
    );
}

#[test]
fn gct_ne_roquepas_si_c1_est_attaquee() {
    // Grand roque : b1/c1/d1 vides, mais la tour c8 attaque c1 — case de
    // transit du roi. Refus attendu.
    let pos = Position::from_fen("2r3k1/8/8/8/8/8/8/R3K3 w Q - 0 1").unwrap();
    let moves = sorted_gct(&pos);
    assert!(
        !moves.iter().any(|&(_, _, f)| f == FLAG_QUEEN_CASTLE),
        "le grand roque ne doit pas traverser une case attaquee"
    );
    // Controle : le roi peut toujours bouger, la position n'est pas un mat.
    assert!(!moves.is_empty());
}

#[test]
fn gct_roquepart_quand_seule_b1_est_attaquee() {
    // La tour b8 attaque b1. Le roi ne traverse que d1 et c1 : b1 n'est qu'une
    // case de passage de la TOUR (elle finit en d1), donc le grand roque reste
    // legal.
    let pos = Position::from_fen("1r4k1/8/8/8/8/8/8/R3K3 w Q - 0 1").unwrap();
    let moves = sorted_gct(&pos);
    assert!(
        moves.iter().any(|&(_, _, f)| f == FLAG_QUEEN_CASTLE),
        "b1 attaquee n'empeche pas le grand roque"
    );
}

#[test]
fn gct_ne_roquepas_si_seule_g1_est_attaquee() {
    // Tour g8 : g1 attaquee, f1 non. Le petit roque est refuse, le grand
    // roque reste legal — les deux cotes sont decides independamment.
    let pos = Position::from_fen("6rk/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
    let moves = sorted_gct(&pos);
    assert!(!moves.iter().any(|&(_, _, f)| f == FLAG_KING_CASTLE));
    assert!(moves.iter().any(|&(_, _, f)| f == FLAG_QUEEN_CASTLE));
}

#[test]
fn gct_genere_l_en_passant_et_refuse_le_clouage_horizontal() {
    // En-passant legal :
    let pos = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
    assert!(
        sorted_gct(&pos).contains(&(36, 43, FLAG_EP_CAPTURE)),
        "e5xd6 en passant doit etre genere"
    );

    // Clouage horizontal : roi blanc a5, pion blanc e5, pion noir d5, ep d6.
    // La capture e5xd6 retirerait les deux pions de la 5e rangee et laisserait
    // la tour noire h5 attaquer a5.
    let pos = Position::from_fen("4k3/8/8/K2pP2r/8/8/8/8 w - d6 0 1").unwrap();
    let moves = sorted_gct(&pos);
    assert!(
        !moves.iter().any(|&(_, _, f)| f == FLAG_EP_CAPTURE),
        "un en-passant qui decouvre une tour sur la rangee doit etre refuse"
    );
}

#[test]
fn generate_est_total_sans_roi() {
    // Entree hors contrat (roi absent) : contrat du projet = defini, silencieux,
    // sans panique — y compris en profil debug, ou `king_sq` porte un
    // `debug_assert!`. C'est pour cela que `generate` teste `has_king` avant.
    let pos = Position::from_fen("4k3/8/8/8/8/8/8/8 w - - 0 1").unwrap();
    assert!(!pos.has_king(Color::White));
    let mut l = MoveList::new();
    assert_eq!(gct_generate(&pos, &mut l), 0);
    assert_eq!(l.len(), 0);
}

#[test]
fn gct_genere_le_roque_noir_des_deux_cotes() {
    // Le roque noir emprunte le meme chemin avec `back = 56` : une constante
    // blanche codee en dur passerait inapercue sur les positions blanches.
    let pos = Position::from_fen("r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1").unwrap();
    let moves = sorted_gct(&pos);
    assert!(
        moves.contains(&(60, 62, FLAG_KING_CASTLE)),
        "petit roque noir"
    );
    assert!(
        moves.contains(&(60, 58, FLAG_QUEEN_CASTLE)),
        "grand roque noir"
    );
}

#[test]
fn gct_grand_roque_exige_b1_vide() {
    // Le roi ne passe que par d1/c1, mais la tour doit traverser b1 : case
    // occupee = grand roque illegal, meme si c1/d1 sont vides et sures.
    let pos = Position::from_fen("4k3/8/8/8/8/8/8/RN2K3 w Q - 0 1").unwrap();
    let moves = sorted_gct(&pos);
    assert!(
        !moves.iter().any(|&(_, _, f)| f == FLAG_QUEEN_CASTLE),
        "b1 occupee : le grand roque doit etre refuse"
    );

    // Meme position, b1 vide : le grand roque doit etre offert (sinon le test
    // ci-dessus passerait pour une raison quelconque).
    let pos = Position::from_fen("4k3/8/8/8/8/8/8/R3K3 w Q - 0 1").unwrap();
    assert!(
        sorted_gct(&pos).contains(&(4, 2, FLAG_QUEEN_CASTLE)),
        "b1 vide : le grand roque doit etre genere"
    );
}

#[test]
fn gct_ne_roquepas_en_echec() {
    // Echec par la tour e7 sur la colonne e : les cases de passage f1/g1 et
    // d1/c1 ne sont pas attaquees, seul le statut d'echec interdit le roque.
    let pos = Position::from_fen("4k3/4r3/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
    assert!(pos.in_check(Color::White), "la position doit etre un echec");
    let moves = sorted_gct(&pos);
    assert!(
        !moves
            .iter()
            .any(|&(_, _, f)| f == FLAG_KING_CASTLE || f == FLAG_QUEEN_CASTLE),
        "aucun roque n'est legal en echec"
    );
}

#[test]
fn gct_refuse_le_roque_que_le_plateau_ne_supporte_pas() {
    // Les droits d'un FEN sont normalises par `from_fen`, mais `castle` reste
    // un champ public : les deux gardes de presence doivent tenir sur des
    // droits incoherents construits a la main (entree hors contrat : pas de
    // roque, pas de panique).
    //
    // (a) droits `KQ` sans aucune tour.
    let mut pos = Position::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
    // `pos.zobrist` reste volontairement perime : `generate` ne le lit jamais.
    pos.castle = CASTLE_WK | CASTLE_WQ;
    assert!(
        !sorted_gct(&pos)
            .iter()
            .any(|&(_, _, f)| f == FLAG_KING_CASTLE || f == FLAG_QUEEN_CASTLE),
        "pas de tour en h1/a1 : aucun roque"
    );

    // (b) droit `KQ` avec le roi en d1 et non en e1.
    let mut pos = Position::from_fen("4k3/8/8/8/8/8/8/R2K3R w - - 0 1").unwrap();
    assert!(pos.has_king(Color::White));
    // Idem : `pos.zobrist` perime, sans effet ici (`generate` ne le lit pas).
    pos.castle = CASTLE_WK | CASTLE_WQ;
    assert!(
        !sorted_gct(&pos)
            .iter()
            .any(|&(_, _, f)| f == FLAG_KING_CASTLE || f == FLAG_QUEEN_CASTLE),
        "roi hors de e1 : aucun roque"
    );
}

#[test]
fn gct_refuse_une_case_ep_sans_pion_a_capturer() {
    // FEN hors contrat : la case ep d6 est nommee mais aucun pion noir ne
    // stationne en d5. `ep_capture_square` renvoie alors `None` et aucune
    // capture ne doit sortir — sinon `make` retirerait une piece absente.
    let pos = Position::from_fen("4k3/8/8/4P3/8/8/8/4K3 w - d6 0 1").unwrap();
    assert!(
        !sorted_gct(&pos)
            .iter()
            .any(|&(_, _, f)| f == FLAG_EP_CAPTURE),
        "case ep sans pion a capturer : pas de capture en passant"
    );
}

#[test]
fn gct_refuse_l_en_passant_qui_decouvre_un_fou() {
    // Fou noir c6, pion noir d5 (il masque le fou), pion blanc e5, roi blanc
    // f3 : apres e5xd6 la diagonale c6-d5-e4-f3 s'ouvre. Le test glissant de
    // `gen_en_passant` doit voir le fou comme il voit la tour du clouage
    // horizontal.
    let pos = Position::from_fen("4k3/8/2b5/3pP3/8/5K2/8/8 w - d6 0 1").unwrap();
    assert!(
        !sorted_gct(&pos)
            .iter()
            .any(|&(_, _, f)| f == FLAG_EP_CAPTURE),
        "un en-passant qui decouvre un fou sur le roi doit etre refuse"
    );
}

#[test]
fn gct_genere_l_en_passant_qui_pare_un_echec() {
    // Tour noire h6 : le roi blanc a6 est en echec sur la 6e rangee. La
    // capture e5xd6 pose le pion en d6, entre la tour et le roi : elle pare
    // l'echec et doit etre generee (l'occupation d'apres capture contient le
    // pion d'arrivee, pas seulement les deux cases videes).
    let pos = Position::from_fen("7k/8/K6r/3pP3/8/8/8/8 w - d6 0 1").unwrap();
    assert!(pos.in_check(Color::White), "la position doit etre un echec");
    assert!(
        sorted_gct(&pos).contains(&(36, 43, FLAG_EP_CAPTURE)),
        "e5xd6 pare l'echec et doit etre genere"
    );
}

#[test]
fn gct_refuse_l_en_passant_qui_ne_pare_pas_un_echec_de_pion() {
    // Echec par le pion noir d2 sur e1 : la capture e5xd6 ne retire pas ce
    // pion. Le terme « pions » du test des pieces sautantes doit le voir.
    let pos = Position::from_fen("4k3/8/8/3pP3/8/8/3p4/4K3 w - d6 0 1").unwrap();
    assert!(pos.in_check(Color::White), "la position doit etre un echec");
    assert!(
        !sorted_gct(&pos)
            .iter()
            .any(|&(_, _, f)| f == FLAG_EP_CAPTURE),
        "un en-passant qui laisse l'echec de pion doit etre refuse"
    );
}

#[test]
fn gct_refuse_l_en_passant_qui_ne_pare_pas_un_echec_de_cavalier() {
    // Echec par le cavalier f3 : la capture e5xd6 ne retire ni ne bloque le
    // cavalier, donc elle reste illegale. Seul le test « pieces sautantes »
    // de `gen_en_passant` le voit — le test glissant ne regarde que les
    // fous/tours/dames.
    let pos = Position::from_fen("4k3/8/8/3pP3/8/5n2/8/4K3 w - d6 0 1").unwrap();
    assert!(pos.in_check(Color::White), "la position doit etre un echec");
    assert!(
        !sorted_gct(&pos)
            .iter()
            .any(|&(_, _, f)| f == FLAG_EP_CAPTURE),
        "un en-passant qui laisse l'echec de cavalier doit etre refuse"
    );
}
