//! Coût par appel du générateur de production.
//!
//! Criterion, pas de seuil absolu : on lit des ratios dans une même session.

use criterion::{Criterion, criterion_group, criterion_main};
use mychess::movegen::{MoveList, count_moves, generate, has_legal_move};
use mychess::position::Position;
use mychess::refgen;
use mychess::types::GenKind;

const FENS: [(&str, &str); 4] = [
    (
        "startpos",
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    ),
    (
        "kiwipete",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    ),
    ("pos3", "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1"),
    (
        "sje",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    ),
];

// Pas de banc `movelist_new` : `black_box(MoveList::new())` déplace les 514
// octets de la structure à travers le retour de la closure, donc il mesure une
// copie de 512 octets (le désassemblage montre 256 `movzwl` par itération), pas
// le coût du constructeur. `generate/all/*` est le signal honnête ; ne pas
// rajouter ce banc.

fn bench_generate(c: &mut Criterion) {
    let mut g = c.benchmark_group("generate");
    for (name, fen) in FENS {
        let pos = Position::from_fen(fen).unwrap();
        g.bench_function(format!("all/{name}"), |b| {
            b.iter(|| {
                let mut l = MoveList::new();
                generate(&pos, &mut l, GenKind::All);
                // Black-boxing the slice (pointer + len, not a copy) keeps the
                // whole generated buffer observable, so `generate` cannot be
                // elided even if it ever inlines fully.
                let s = l.as_slice();
                std::hint::black_box((s.as_ptr(), s.len()))
            });
        });
        g.bench_function(format!("count/{name}"), |b| b.iter(|| count_moves(&pos)));
    }
    g.finish();
}

/// Terminaison : le verdict « au moins un coup légal » sans liste.
///
/// `mat` est le pire cas assumé : aucune sortie anticipée n'y est possible
/// (toutes les classes sont épuisées avant de répondre `false`), donc son
/// coût est du même ordre que `generate/all/*`. Ce n'est pas une régression,
/// c'est la borne haute de la fonction.
fn bench_terminaison(c: &mut Criterion) {
    let mut g = c.benchmark_group("terminaison");
    for (name, fen) in FENS {
        let pos = Position::from_fen(fen).unwrap();
        g.bench_function(format!("has_legal_move/{name}"), |b| {
            b.iter(|| std::hint::black_box(has_legal_move(&pos)))
        });
    }
    let mat = Position::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 1 3")
        .unwrap();
    g.bench_function("has_legal_move/mat", |b| {
        b.iter(|| std::hint::black_box(has_legal_move(&mat)))
    });
    g.finish();
}

/// Positions EN ÉCHEC issues d'auto-jeu déterministe (tâche 4).
///
/// La partie est avancée avec `refgen`, pas avec `generate` : l'ordre des
/// coups de `generate` change avec le chemin d'évasion, donc un corpus
/// construit dessus ne serait pas le même avant et après la modification.
/// Le parcours et les graines sont ceux de `tests/movegen_echecs.rs` (qui
/// affirme la composition du corpus) ; le plafond est plus bas ici pour que
/// le coût mesuré soit celui de `generate`, pas celui du corpus.
fn corpus_echec(plafond: usize) -> Vec<Position> {
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
    const PARTIES_PAR_GRAINE: u32 = 8;

    fn prochain_etat(etat: &mut u64) -> u64 {
        let mut x = *etat;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *etat = x;
        x
    }

    let mut out = Vec::new();
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

/// Sous-échantillon réparti sur tout le corpus (même règle que
/// `tests/movegen_echecs.rs::echantillon`).
fn echantillon(corpus: &[Position], n: usize) -> Vec<Position> {
    let n = n.min(corpus.len());
    (0..n).map(|i| corpus[i * corpus.len() / n]).collect()
}

/// Chemin d'échec : `generate` sur des positions où le camp au trait est en
/// échec, donc où `allowed` fait au plus deux cases.
///
/// Pas de `iter_batched` : le corpus (48 positions) est prêt avant la boucle,
/// et une copie par itération dominerait le coût mesuré.
fn bench_echec(c: &mut Criterion) {
    let corpus = echantillon(&corpus_echec(usize::MAX), 48);
    assert!(
        corpus.len() >= 20,
        "corpus en échec trop maigre: {}",
        corpus.len()
    );
    let mut g = c.benchmark_group("echec");
    g.bench_function("generate", |b| {
        b.iter(|| {
            let mut n = 0usize;
            for p in &corpus {
                let mut l = MoveList::new();
                generate(p, &mut l, GenKind::All);
                n += l.len();
            }
            std::hint::black_box(n)
        })
    });
    g.finish();
}

criterion_group!(benches, bench_generate, bench_terminaison, bench_echec);
criterion_main!(benches);
