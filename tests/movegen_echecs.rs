//! Task 4 : corpus de positions EN ÉCHEC issu d'auto-jeu déterministe, et
//! équivalence exacte du chemin d'évasion.
//!
//! Le corpus est produit par un parcours réel : une partie est avancée coup
//! après coup, le coup étant tiré d'un PRNG à graine fixe dans la liste des
//! coups légaux. Toute position où le camp au trait est en échec **après** le
//! coup est empilée. Deux précautions :
//!
//! * l'avancement de la partie utilise `refgen` (générateur naïf indépendant),
//!   pas `generate` : l'ordre des coups de `generate` change avec le chemin
//!   d'évasion, et un corpus construit dessus ne serait pas comparable avant
//!   et après la modification ;
//! * le corpus ne contient aucune position ajoutée à la main. Les catégories
//!   présentes sont donc celles qu'un jeu réel atteint — c'est justement ce
//!   qu'il faut vérifier au lieu de le supposer (la tâche 3 a livré un corpus
//!   de 444 826 positions qui ne contenait aucune position de la catégorie
//!   visée).
//!
//! `tests/movegen_echecs.rs` est le seul endroit où la composition est
//! affirmée ; le banc `echec` de `benches/movegen.rs` reconstruit le même
//! parcours avec un plafond plus bas.

use mychess::movegen::{MoveList, attackers_to, count_moves, generate, has_legal_move};
use mychess::position::Position;
use mychess::refgen;
use mychess::types::{GenKind, KNIGHT, PAWN, QUEEN, ROOK};

/// Graines d'auto-jeu : FEN de perft publiées (positions légales, vérifiées
/// ailleurs dans la suite), plies et état initial du PRNG.
const GRAINES: [(&str, u32, u64); 10] = [
    (
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        900,
        0x9E37_79B9_7F4A_7C15,
    ),
    (
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        900,
        0x1234_5678_9ABC_DEF0,
    ),
    (
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
        900,
        0xFEDC_BA98_7654_3210,
    ),
    (
        "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
        900,
        0x0F1E_2D3C_4B5A_6978,
    ),
    (
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
        900,
        0xA5A5_5A5A_1234_4321,
    ),
    (
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
        900,
        0xDEAD_BEEF_CAFE_F00D,
    ),
    (
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R b KQkq - 0 1",
        900,
        0x1111_2222_3333_4444,
    ),
    (
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 b - - 0 1",
        900,
        0x5555_AAAA_0F0F_F0F0,
    ),
    (
        "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R b KQ - 1 8",
        900,
        0x0123_4567_89AB_CDEF,
    ),
    (
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 b - - 0 10",
        900,
        0xC001_D00D_F00D_1234,
    ),
];

/// PRNG déterministe (xorshift64), identique à celui du test différentiel.
fn prochain_etat(etat: &mut u64) -> u64 {
    let mut x = *etat;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *etat = x;
    x
}

/// Une partie par état de PRNG et par graine : le même FEN rejoué avec des
/// tirages différents donne des parcours différents.
const PARTIES_PAR_GRAINE: u32 = 8;

/// Positions en échec rencontrées par auto-jeu, dans l'ordre du parcours.
///
/// `plafond` borne la taille pour les usages qui n'ont pas besoin de tout le
/// corpus (banc, mesure d'instructions). Le parcours s'arrête aussi quand la
/// partie n'a plus de coup légal (mat ou pat).
fn corpus_echec(plafond: usize) -> Vec<Position> {
    let mut out: Vec<Position> = Vec::new();
    for (fen, plies, graine) in GRAINES {
        for partie in 0..PARTIES_PAR_GRAINE {
            let mut pos = Position::from_fen(fen).unwrap();
            let mut etat = graine ^ (0x9E37_79B9_7F4A_7C15u64.wrapping_mul(partie as u64 + 1));
            for _ in 0..plies {
                let l = refgen::legal_moves(&pos);
                if l.is_empty() {
                    break;
                }
                let pick = (prochain_etat(&mut etat) % l.len() as u64) as usize;
                let mv = l.get(pick);
                pos.make(mv);
                if pos.in_check(pos.side) {
                    out.push(pos);
                    if out.len() >= plafond {
                        return out;
                    }
                }
            }
        }
    }
    out
}

/// Sous-échantillon déterministe réparti sur tout le corpus (pas un préfixe :
/// les premières graines ne doivent pas décider seules du coût mesuré).
pub fn echantillon(corpus: &[Position], n: usize) -> Vec<Position> {
    let n = n.min(corpus.len());
    (0..n).map(|i| corpus[i * corpus.len() / n]).collect()
}

fn cles(list: &MoveList) -> Vec<u16> {
    let mut v: Vec<u16> = list.iter().map(|m| m.0).collect();
    v.sort_unstable();
    v
}

/// Nombre d'attaquants et répartition par type de pièce, via `attackers_to`
/// (aucune méthode ajoutée à `Position`, qui est gelé).
fn attaquants(pos: &Position) -> ([u32; 6], u32) {
    let mut m = attackers_to(pos, pos.king_sq(pos.side), pos.side.opp());
    let mut par_type = [0u32; 6];
    let mut total = 0u32;
    while m != 0 {
        let sq = m.trailing_zeros() as u8;
        m &= m - 1;
        let code = pos.piece_on(sq) - 1;
        par_type[(code % 6) as usize] += 1;
        total += 1;
    }
    (par_type, total)
}

/// Composition réelle du corpus. Les seuils sont des affirmations sur ce que
/// l'auto-jeu atteint : si une catégorie tombe à zéro, le test échoue et le
/// corpus doit être élargi (c'est le point de la tâche).
#[test]
fn corpus_echec_composition() {
    let corpus = corpus_echec(usize::MAX);
    assert!(
        corpus.len() >= 20,
        "corpus en échec trop maigre : {} positions",
        corpus.len()
    );

    let mut par_type = [0u32; 6];
    let mut double_echec = 0u32;
    let mut un_seul_coup = 0u32;
    let mut aucun_coup = 0u32;
    let mut total_coups = 0usize;
    for pos in &corpus {
        let (types, n) = attaquants(pos);
        // `in_check` a dit vrai : `attackers_to` doit trouver au moins un
        // attaquant, sinon les deux réponses se contredisent.
        assert!(n >= 1, "en échec mais aucun attaquant trouvé");
        if n >= 2 {
            double_echec += 1;
            continue;
        }
        assert_eq!(n, 1, "position du corpus sans attaquant alors qu'en échec");
        for (k, c) in types.iter().enumerate() {
            par_type[k] += c;
        }
        let mut l = MoveList::new();
        generate(pos, &mut l, GenKind::All);
        let n_coups = l.len();
        total_coups += n_coups;
        if n_coups == 1 {
            un_seul_coup += 1;
        }
        if n_coups == 0 {
            aucun_coup += 1;
        }
    }

    println!("=== corpus en échec ===");
    println!("positions            : {}", corpus.len());
    println!(
        "échec simple         : {}",
        corpus.len() as u32 - double_echec
    );
    println!("  pion               : {}", par_type[PAWN]);
    println!("  cavalier           : {}", par_type[KNIGHT]);
    println!("  fou                : {}", par_type[2]);
    println!("  tour               : {}", par_type[ROOK]);
    println!("  dame               : {}", par_type[QUEEN]);
    println!("échec double         : {double_echec}");
    println!("exactement 1 coup    : {un_seul_coup}");
    println!("aucun coup (mat)     : {aucun_coup}");
    println!("coups légaux au total: {total_coups}");

    assert_eq!(
        corpus.len() as u32,
        par_type.iter().sum::<u32>() + double_echec,
        "chaque position est simple (un attaquant) ou double"
    );
    for (nom, k) in [
        ("pion", PAWN),
        ("cavalier", KNIGHT),
        ("fou", 2usize),
        ("tour", ROOK),
        ("dame", QUEEN),
    ] {
        assert!(
            par_type[k] > 0,
            "aucune position en échec simple par un {nom} : catégorie non couverte"
        );
    }
    assert!(
        double_echec > 0,
        "aucun échec double dans le corpus : catégorie non couverte"
    );
    assert!(
        un_seul_coup > 0,
        "aucune position à évasion unique : catégorie non couverte"
    );
}

/// Équivalence exacte : mêmes coups, mêmes drapeaux, en multiset trié (le
/// contrat d'ordre de `generate` n'existe pas ; `tests/differential.rs`
/// compare déjà des multisets triés, on suit la même règle).
#[test]
fn generate_egale_refgen_sur_le_corpus_echec() {
    let corpus = corpus_echec(usize::MAX);
    assert!(!corpus.is_empty());
    for (i, pos) in corpus.iter().enumerate() {
        assert!(pos.in_check(pos.side), "position {i} n'est pas en échec");

        let mut rapide = MoveList::new();
        generate(pos, &mut rapide, GenKind::All);
        let oracle = refgen::legal_moves(pos);
        assert_eq!(
            cles(&rapide),
            cles(&oracle),
            "position {i} ({pos:?}) : coups différents"
        );

        // `has_legal_move` et `count_moves` doivent rester les jumeaux exacts
        // de `generate` (tâches 2 et 3).
        assert_eq!(
            has_legal_move(pos),
            !rapide.is_empty(),
            "position {i} : has_legal_move divergent"
        );
        // `count_moves` doit rester le jumeau exact de `generate`.
        assert_eq!(
            count_moves(pos) as usize,
            rapide.len(),
            "position {i} : count_moves divergent"
        );

        // CapturesOnly : sous-ensemble, encodage identique.
        let mut caps = MoveList::new();
        generate(pos, &mut caps, GenKind::CapturesOnly);
        let toutes = cles(&rapide);
        for m in cles(&caps) {
            assert!(
                toutes.contains(&m),
                "position {i} : capture-only hors liste"
            );
        }
        // `CapturesOnly` garde les coups de roi (fuites) et les promotions
        // calmes : c'est le contrat existant, hors périmètre. On vérifie
        // seulement l'emboîtement et l'encodage identique, comme
        // `tests/differential.rs`.
    }
    println!("{} positions en échec comparées à refgen", corpus.len());
}

/// Harnais de comptage d'instructions (ignoré par défaut) : boucle fixe sur un
/// préfixe du corpus. Lancé sous `perf stat -e instructions:u`, il donne le
/// coût de `generate` sur des positions en échec, sans horloge.
#[test]
#[ignore = "harnais de mesure : lancer le binaire de test sous perf stat"]
fn compte_instructions_echec() {
    let corpus = echantillon(&corpus_echec(usize::MAX), 256);
    assert!(corpus.len() >= 20, "corpus trop maigre");
    let iterations = (8_000_000 / corpus.len()) as u64;
    let mut acc = 0usize;
    let debut = std::time::Instant::now();
    for _ in 0..iterations {
        for p in &corpus {
            let mut l = MoveList::new();
            generate(p, &mut l, GenKind::All);
            acc += l.len();
        }
    }
    let duree = debut.elapsed();
    std::hint::black_box(acc);
    let appels = iterations * corpus.len() as u64;
    println!(
        "positions={} appels={} checksum={} duree={:?} par_appel={:?}",
        corpus.len(),
        appels,
        acc,
        duree,
        duree / appels as u32
    );
}
