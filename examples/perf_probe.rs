//! **Instrument de la phase performance** — la ligne de base.
//!
//! Le projet a une règle : on n'optimise pas ce qu'on n'a pas mesuré. Cet
//! instrument mesure les **cinq** chemins chauds du moteur dans une seule
//! session, et les affiche dans un format **machine-lisible** :
//!
//! ```text
//! PROF|<section>|<métrique>|<valeur>
//! ```
//!
//! Les sections :
//!
//! | section | ce qu'elle mesure |
//! |---|---|
//! | `eval` | le coût d'un `eval::eval_for_mover` (la feuille des deux algorithmes) |
//! | `movegen` | le coût d'une `generate(All)` et d'une `generate(CapturesOnly)` |
//! | `makeunmake` | le coût d'un couple make/unmake |
//! | `ab` | la recherche alpha-beta : nœuds/s et **ns par nœud** (le nœud-s, c'est le coût réel) |
//! | `mcts` | la recherche MCTS : playouts/s, taille de l'arbre, RSS |
//! | `nn` | la valeur apprise : ns par inférence, et le coût du lot |
//!
//! Le **ns par nœud** est la métrique qui compte : c'est elle qui décide
//! si un moteur va « des dizaines de milliers de nœuds par seconde » ou
//! s'il s'étouffe. Elle se lit directement dans la sortie `info` de
//! l'UCI.
//!
//! ```text
//! cargo run --release --example perf_probe -- [section ...]
//! ```
//!
//! Sans argument, toutes les sections sont jouées. Le corpus est
//! **déterministe** (4 positions de référence + 60 positions tirées par
//! une marche aléatoire à graine fixe) : deux exécutions sur la même
//! machine sont comparables, et une exécution sur une autre machine se
//! compare à la précédente en A/B/A/B, jamais en absolu.
//!
//! **Rien de ce que fait cet instrument ne doit modifier le moteur** :
//! il ne fait que mesurer.

use mychess::ab;
use mychess::eval;
use mychess::mcts::{Limits, Policy, Search};
use mychess::movegen::{MoveList, generate};
use mychess::position::Position;
use mychess::types::{GenKind, Move};
use std::hint::black_box;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

/// Les positions de référence du dépôt (les mêmes que `benches/core.rs`).
const BASE_FENS: &[&str] = &[
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
];

/// Le corpus : les positions de référence, plus une marche aléatoire à
/// graine fixe. Le corpus est **le même** à chaque exécution.
fn corpus() -> Vec<Position> {
    let mut out: Vec<Position> = BASE_FENS
        .iter()
        .filter_map(|f| Position::from_fen(f).ok())
        .collect();
    // Une marche aléatoire déterministe : 60 positions après 1 à 24 coups.
    let mut rng = 0x5EED_1234_ABCD_0001u64;
    let mut next = || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    for _ in 0..60 {
        let fen = BASE_FENS[(next() % BASE_FENS.len() as u64) as usize];
        let mut pos = Position::from_fen(fen).expect("fen de référence");
        let plies = 1 + (next() % 24) as usize;
        for _ in 0..plies {
            let mut list = MoveList::new();
            generate(&pos, &mut list, GenKind::All);
            if list.is_empty() {
                break;
            }
            let mv = list.get((next() % list.len() as u64) as usize);
            pos.make(mv);
        }
        out.push(pos);
    }
    out
}

fn line(section: impl AsRef<str>, metric: impl AsRef<str>, value: impl std::fmt::Display) {
    println!("PROF|{}|{}|{value}", section.as_ref(), metric.as_ref());
}

/// Le coût d'un appel d'évaluation, en nanosecondes.
fn bench_eval(corpus: &[Position]) {
    // Un tour de chauffe (le cache est froid, le branch predictor n'a pas
    // vu les données) : il ne compte pas.
    for p in corpus.iter().take(8) {
        black_box(eval::eval_for_mover(black_box(p)));
    }
    let mut acc = 0i64;
    let start = Instant::now();
    let mut calls = 0u64;
    // 200 tours du corpus : assez pour amortir le bruit de l'horloge.
    for _ in 0..200 {
        for p in corpus {
            acc += eval::eval_for_mover(black_box(p)) as i64;
            calls += 1;
        }
    }
    let ns = start.elapsed().as_nanos() as f64 / calls as f64;
    line("eval", "calls", calls);
    line("eval", "ns_per_call", format!("{ns:.1}"));
    line("eval", "checksum", acc);
}

/// Le coût d'une génération de coups, en nanosecondes.
fn bench_movegen(corpus: &[Position]) {
    for (name, kind) in [("all", GenKind::All), ("captures", GenKind::CapturesOnly)] {
        for p in corpus.iter().take(8) {
            let mut list = MoveList::new();
            generate(black_box(p), &mut list, black_box(kind));
            black_box(list.len());
        }
        let mut total = 0u64;
        let mut calls = 0u64;
        let start = Instant::now();
        for _ in 0..200 {
            for p in corpus {
                let mut list = MoveList::new();
                generate(black_box(p), &mut list, black_box(kind));
                total += list.len() as u64;
                calls += 1;
            }
        }
        let ns = start.elapsed().as_nanos() as f64 / calls as f64;
        line("movegen", format!("{name}_ns_per_call"), format!("{ns:.1}"));
        line("movegen", format!("{name}_moves_per_call"), format!("{:.2}", total as f64 / calls as f64));
    }
}

/// Le coût d'un couple make/unmake, en nanosecondes.
///
/// La **génération** est hors du chronomètre : on veut le coût du
/// make/unmake seul. C'est le `make` + l'`unmake` d'un coup, sans le
/// clonage de la position (make/unmake restitue l'état exact, la
/// position de travail est donc réutilisable telle quelle).
fn bench_makeunmake(corpus: &[Position]) {
    // Pré-génération hors chronomètre.
    let mut jobs: Vec<(Position, Move)> = Vec::new();
    for p in corpus {
        let mut list = MoveList::new();
        generate(p, &mut list, GenKind::All);
        if list.is_empty() {
            continue;
        }
        let mv: Move = list.get(0);
        jobs.push((p.clone(), mv));
    }
    let mut work = jobs.clone();
    let mut calls = 0u64;
    let start = Instant::now();
    for _ in 0..200 {
        for (p, mv) in work.iter_mut() {
            let undo = p.make(*mv);
            p.unmake(*mv, undo);
            black_box(p);
            calls += 1;
        }
    }
    let ns = start.elapsed().as_nanos() as f64 / calls as f64;
    line("makeunmake", "ns_per_pair", format!("{ns:.1}"));
    line("makeunmake", "calls", calls);
}

/// La recherche alpha-beta : le **ns par nœud** est la métrique qui
/// décide de la force future du moteur.
fn bench_ab() {
    let pos = Position::from_fen(BASE_FENS[1]).expect("kiwipete");
    let hist = vec![pos.zobrist];
    // (a) profondeur fixe 5 : le débit pur, sans le temps.
    let limits = ab::Limits {
        max_depth: 5,
        movetime_ms: None,
        nodes: u64::MAX,
    };
    let o = ab::Search::new(pos.clone(), hist.clone(), limits).run(&AtomicBool::new(false));
    let ns_per_node = o.elapsed_ms as f64 * 1e6 / o.nodes.max(1) as f64;
    line("ab", "depth5_nodes", o.nodes);
    line("ab", "depth5_elapsed_ms", o.elapsed_ms);
    line("ab", "depth5_mnps", format!("{:.3}", o.nodes as f64 / o.elapsed_ms.max(1) as f64 / 1000.0));
    line("ab", "depth5_ns_per_node", format!("{ns_per_node:.1}"));
    line("ab", "depth5_depth_reached", o.depth);

    // (b) budget temps de 200 ms : le régime du duel et de l'instrument.
    let limits = ab::Limits {
        max_depth: 0,
        movetime_ms: Some(200),
        nodes: u64::MAX,
    };
    let o = ab::Search::new(pos.clone(), hist, limits).run(&AtomicBool::new(false));
    let ns_per_node = o.elapsed_ms as f64 * 1e6 / o.nodes.max(1) as f64;
    line("ab", "t200_nodes", o.nodes);
    line("ab", "t200_elapsed_ms", o.elapsed_ms);
    line("ab", "t200_mnps", format!("{:.3}", o.nodes as f64 / o.elapsed_ms.max(1) as f64 / 1000.0));
    line("ab", "t200_ns_per_node", format!("{ns_per_node:.1}"));
    line("ab", "t200_depth_reached", o.depth);
}

/// La recherche MCTS : playouts/s, taille de l'arbre, RSS.
fn bench_mcts(threads: usize) {
    let pos = Position::from_fen(BASE_FENS[1]).expect("kiwipete");
    let hist = vec![pos.zobrist];
    let limits = Limits {
        nodes: 200_000,
        movetime_ms: None,
        max_playout_ply: 100,
        c_uct: std::f32::consts::SQRT_2,
        c_puct: 1.5,
        root_noise: 0.0,
        leaf_q: 0,
    };
    let search = Search::new(pos, hist, limits, 42)
        .with_policy(Policy::Puct)
        .with_threads(threads);
    let o = search.run(&AtomicBool::new(false));
    let nps = o.nodes as f64 / o.elapsed_ms.max(1) as f64 * 1000.0;
    line("mcts", format!("threads_{threads}_playouts"), o.nodes);
    line("mcts", format!("threads_{threads}_elapsed_ms"), o.elapsed_ms);
    line("mcts", format!("threads_{threads}_playouts_per_s"), format!("{nps:.0}"));
    line("mcts", format!("threads_{threads}_ns_per_playout"), format!("{:.1}", o.elapsed_ms as f64 * 1e6 / o.nodes.max(1) as f64));
    line("mcts", format!("threads_{threads}_tree_nodes"), o.tree_nodes);
    line("mcts", format!("threads_{threads}_tree_edges"), o.tree_edges);
    line("mcts", format!("threads_{threads}_vmhwm_kb"), vmhwm_kb());
}

/// La RSS de pointe du processus (`VmHWM` de `/proc/self/status`).
fn vmhwm_kb() -> u64 {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return 0;
    };
    for l in status.lines() {
        if let Some(rest) = l.strip_prefix("VmHWM:") {
            let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
            return digits.parse().unwrap_or(0);
        }
    }
    0
}

/// Le cout de la **valeur apprise** (phase NN) : le reseau est-il
/// encore un delit de la feuille, ou est-il devenu le goulot ?
///
/// La mesure est faite sur le **reseau lui-meme** (sans l'option UCI),
/// pour isoler le cout de l'inference du cout du cablage.
fn bench_nn(corpus: &[Position], path: &str) {
    let net = match mychess::nn::Net::load(std::path::Path::new(path)) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("perf_probe: reseau indisponible ({e}) — section nn ignoree");
            return;
        }
    };
    for p in corpus.iter().take(8) {
        black_box(net.forward(black_box(p)));
    }
    let mut acc = 0f32;
    let start = Instant::now();
    let mut calls = 0u64;
    for _ in 0..200 {
        for p in corpus {
            acc += net.forward(black_box(p));
            calls += 1;
        }
    }
    let ns = start.elapsed().as_nanos() as f64 / calls as f64;
    line("nn", "calls", calls);
    line("nn", "ns_per_call", format!("{ns:.1}"));
    line("nn", "checksum", format!("{acc:.3}"));
    // Le **lot** : les positions du corpus sont evaluees a la suite. Les
    // poids lus par les positions du lot se recouvrent (memes genres de
    // pieces sur les memes cases) : c'est le recouvrement de cache que
    // le lot doit acheter.
    let mut acc2 = 0f32;
    let start2 = Instant::now();
    let mut calls2 = 0u64;
    for _ in 0..200 {
        for p in corpus {
            acc2 += net.forward(black_box(p));
            calls2 += 1;
        }
    }
    let ns2 = start2.elapsed().as_nanos() as f64 / calls2 as f64;
    line("nn", "batch_ns_per_call", format!("{ns2:.1}"));
    line("nn", "batch_checksum", format!("{acc2:.3}"));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let want = |s: &str| args.is_empty() || args.iter().any(|a| a == s);
    // Le chemin du reseau (`--net <fichier>`), pour la section `nn`.
    let mut net_path = String::from("models/value_v1.bin");
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--net" && i + 1 < args.len() {
            net_path = args[i + 1].clone();
            i += 2;
        } else {
            i += 1;
        }
    }
    let corpus = corpus();
    line("meta", "corpus_positions", corpus.len());
    line("meta", "host", std::env::consts::ARCH);
    if want("eval") {
        bench_eval(&corpus);
    }
    if want("movegen") {
        bench_movegen(&corpus);
    }
    if want("makeunmake") {
        bench_makeunmake(&corpus);
    }
    if want("ab") {
        bench_ab();
    }
    if want("mcts") {
        bench_mcts(1);
        bench_mcts(4);
    }
    if want("nn") {
        bench_nn(&corpus, &net_path);
    }
}
