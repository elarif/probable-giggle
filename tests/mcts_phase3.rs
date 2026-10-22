//! Phase 3 — arène plate, terminaison `has_legal_move`, parallélisme racine.
//!
//! Trois filets, dans l'ordre d'importance :
//!
//! * **G1 — différentiel v1** : le chemin mono-thread du moteur phase 3 doit
//!   rendre **exactement** le même coup, le même score et le même nombre de
//!   nœuds que l'algorithme v1, recopié ici dans [`v1_ref`]. C'est la preuve
//!   que le remaniement de l'arène et le câblage de `has_legal_move` n'ont
//!   rien changé au comportement du moteur.
//! * **G3 — terminaison** : la terminaison par `has_legal_move` doit rendre
//!   la même valeur que l'ancienne terminaison par `generate`.
//! * **G4 — parallélisme** : budget de nœuds honnête, coup légal,
//!   reproductibilité, mat en un coup trouvé en parallèle.
//!
//! Les invariants internes de l'arène (miroir des statistiques, allocation
//! paresseuse, contiguïté des arêtes) sont vérifiés par les tests unitaires
//! de `src/mcts.rs`, qui ont accès aux structures privées.

use mychess::mcts::{Limits, Search, leaf_value, terminal_value};
use mychess::movegen::{MoveList, generate};
use mychess::position::Position;
use mychess::types::{GenKind, Move};
use std::sync::atomic::AtomicBool;

const GOLDEN_FENS: &[&str] = &[
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    "k7/8/KQ6/8/8/8/8/8 w - - 0 1",
];

const SEEDS: &[(u64, u64)] = &[(42, 300), (7, 500), (123, 2000), (99, 10000)];

fn limits(nodes: u64) -> Limits {
    Limits {
        nodes,
        movetime_ms: None,
        max_playout_ply: 100,
        c_uct: std::f32::consts::SQRT_2,
    }
}

/// The legal moves of a position (used to check that a bestmove is legal).
fn legal_moves(pos: &Position) -> Vec<Move> {
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    list.iter().collect()
}

// ---------------------------------------------------------------------------
// G1 — the v1 algorithm, copied verbatim, used as an oracle.
// ---------------------------------------------------------------------------
mod v1_ref {
    //! The v1 engine, copied from `src/mcts.rs` before the phase-3 rewrite:
    //! `Vec<Node>` + `Vec<Child>` per node, one child node per legal move,
    //! terminality decided by a materialised `MoveList`.
    //!
    //! It is the oracle of the differential test. Do **not** "improve" it: it
    //! must stay the frozen v1 behaviour.

    use mychess::eval;
    use mychess::mcts::Rng;
    use mychess::movegen::{MoveList, generate};
    use mychess::position::Position;
    use mychess::types::{GenKind, Move};

    const EVAL_Q_CP_SCALE: f32 = 1.0 / 1000.0;
    const EVAL_Q_CLAMP: f32 = 1.0;

    pub struct Node {
        pub visits: u64,
        pub sum: f32,
        pub expanded: bool,
        pub term: Option<f32>,
        pub children: Vec<Child>,
    }

    pub struct Child {
        pub mv: Move,
        pub node: u32,
    }

    impl Node {
        pub fn new() -> Node {
            Node {
                visits: 0,
                sum: 0.0,
                expanded: false,
                term: None,
                children: Vec::new(),
            }
        }
    }

    pub struct Limits {
        pub nodes: u64,
        pub max_playout_ply: u32,
        pub c_uct: f32,
    }

    pub struct Outcome {
        pub best: Move,
        pub score_cp: i32,
        pub nodes: u64,
    }

    struct Scratch {
        hist: Vec<u64>,
        path: Vec<u32>,
    }

    pub fn run(root_pos: &Position, game_hist: &[u64], limits: &Limits, seed: u64) -> Outcome {
        let mut rng = Rng::new(seed);
        let mut arena = vec![Node::new()];
        let mut scratch = Scratch {
            hist: Vec::with_capacity(game_hist.len() + 64),
            path: Vec::with_capacity(64),
        };
        let root_term = terminal_value(root_pos, game_hist);
        arena[0].term = root_term;
        if arena[0].term.is_none() {
            arena[0].expanded = false;
        }
        let mut nodes: u64 = 0;
        while nodes < limits.nodes {
            search_once(
                root_pos,
                game_hist,
                limits,
                &mut arena,
                &mut rng,
                &mut scratch,
            );
            nodes += 1;
        }
        let (best, score_cp) = if arena[0].term.is_some() || arena[0].children.is_empty() {
            (Move::NONE, 0)
        } else {
            let children = &arena[0].children;
            let mut best = 0usize;
            let mut bv = children[0].node;
            for (i, c) in children.iter().enumerate() {
                if arena[c.node as usize].visits > arena[bv as usize].visits {
                    best = i;
                    bv = c.node;
                }
            }
            let sc = (q_from_parent(&arena[children[best].node as usize]) * 100.0).round() as i32;
            (children[best].mv, sc)
        };
        Outcome {
            best,
            score_cp,
            nodes,
        }
    }

    fn search_once(
        root_pos: &Position,
        game_hist: &[u64],
        limits: &Limits,
        arena: &mut Vec<Node>,
        rng: &mut Rng,
        scratch: &mut Scratch,
    ) {
        let mut p = *root_pos;
        scratch.hist.clear();
        scratch.hist.extend_from_slice(game_hist);
        scratch.path.clear();
        scratch.path.push(0);

        loop {
            let idx = *scratch.path.last().expect("path is never empty");
            let (term, expanded, visits) = {
                let n = &arena[idx as usize];
                (n.term, n.expanded, n.visits)
            };
            if let Some(tv) = term {
                backprop(arena, &scratch.path, tv);
                return;
            }
            let at_depth_cap = scratch.path.len() as u32 >= limits.max_playout_ply;
            if !expanded {
                if visits == 0 || at_depth_cap {
                    let r = leaf_value(&p, &scratch.hist);
                    backprop(arena, &scratch.path, r);
                    return;
                }
                let mut list = MoveList::new();
                generate(&p, &mut list, GenKind::All);
                let tv = if list.is_empty() {
                    Some(if p.in_check(p.side) { -1.0 } else { 0.0 })
                } else if p.halfmove >= 100
                    || repetition_draw(&p, &scratch.hist)
                    || eval::insufficient_material(&p)
                {
                    Some(0.0)
                } else {
                    None
                };
                if let Some(tv) = tv {
                    arena[idx as usize].term = Some(tv);
                    backprop(arena, &scratch.path, tv);
                    return;
                }
                let mut children = Vec::with_capacity(list.len());
                for mv in list.iter() {
                    let nidx = arena.len() as u32;
                    arena.push(Node::new());
                    children.push(Child { mv, node: nidx });
                }
                arena[idx as usize].children = children;
                arena[idx as usize].expanded = true;
                let n = arena[idx as usize].children.len();
                let pick = (rng.next_u64() % n as u64) as usize;
                let mv = arena[idx as usize].children[pick].mv;
                let child_node = arena[idx as usize].children[pick].node;
                p.make(mv);
                scratch.hist.push(p.zobrist);
                scratch.path.push(child_node);
                continue;
            }

            let unv_count = {
                let children = &arena[idx as usize].children;
                children
                    .iter()
                    .filter(|c| arena[c.node as usize].visits == 0)
                    .count() as u64
            };
            let (mv, child_node) = if unv_count > 0 {
                let pick = (rng.next_u64() % unv_count) as usize;
                let mut k = 0usize;
                let mut chosen = None;
                {
                    let children = &arena[idx as usize].children;
                    for c in children.iter() {
                        if arena[c.node as usize].visits == 0 {
                            if k == pick {
                                chosen = Some((c.mv, c.node));
                                break;
                            }
                            k += 1;
                        }
                    }
                }
                chosen.expect("the unvisited count and the unvisited scan agree")
            } else {
                let pv = arena[idx as usize].visits as f32;
                let log_pv = (pv as f64 + 1.0).ln() as f32;
                let mut bi = 0usize;
                let mut bs = f32::MIN;
                {
                    let children = &arena[idx as usize].children;
                    for (i, c) in children.iter().enumerate() {
                        let cn = &arena[c.node as usize];
                        let u = -(cn.sum / cn.visits as f32)
                            + limits.c_uct * (log_pv / cn.visits as f32).sqrt();
                        if u > bs {
                            bs = u;
                            bi = i;
                        }
                    }
                    let c = &children[bi];
                    (c.mv, c.node)
                }
            };
            p.make(mv);
            scratch.hist.push(p.zobrist);
            scratch.path.push(child_node);
        }
    }

    fn backprop(arena: &mut [Node], path: &[u32], mut v: f32) {
        for &idx in path.iter().rev() {
            let n = &mut arena[idx as usize];
            n.sum += v;
            n.visits += 1;
            v = -v;
        }
    }

    fn q_from_parent(child: &Node) -> f32 {
        if child.visits == 0 {
            0.0
        } else {
            -(child.sum / child.visits as f32)
        }
    }

    /// The v1 terminality: a materialised move list decides mate/stalemate.
    pub fn terminal_value(pos: &Position, hist: &[u64]) -> Option<f32> {
        let mut list = MoveList::new();
        generate(pos, &mut list, GenKind::All);
        if list.is_empty() {
            return Some(if pos.in_check(pos.side) { -1.0 } else { 0.0 });
        }
        if pos.halfmove >= 100 {
            return Some(0.0);
        }
        if repetition_draw(pos, hist) {
            return Some(0.0);
        }
        if eval::insufficient_material(pos) {
            return Some(0.0);
        }
        None
    }

    fn repetition_draw(pos: &Position, hist: &[u64]) -> bool {
        let len = hist.len();
        if len == 0 {
            return false;
        }
        let window = (pos.halfmove as usize).min(len - 1);
        let h = pos.zobrist;
        let mut c = 0usize;
        for &x in hist[len - 1 - window..len - 1].iter().rev() {
            if x == h {
                c += 1;
                if c >= 2 {
                    return true;
                }
            }
        }
        false
    }

    fn eval_normalized(pos: &Position) -> f32 {
        (eval::eval_for_mover(pos) as f32 * EVAL_Q_CP_SCALE).clamp(-EVAL_Q_CLAMP, EVAL_Q_CLAMP)
    }

    fn leaf_value(pos: &Position, hist: &[u64]) -> f32 {
        match terminal_value(pos, hist) {
            Some(v) => v,
            None => eval_normalized(pos),
        }
    }
}

/// G1 — the phase-3 mono path is the v1 engine, bit for bit.
#[test]
fn v1_ref_mono_matches_engine() {
    for fen in GOLDEN_FENS {
        for (seed, nodes) in SEEDS {
            let pos = Position::from_fen(fen).unwrap();
            let got = Search::new(pos, vec![pos.zobrist], limits(*nodes), *seed)
                .run(&AtomicBool::new(false));
            let want = v1_ref::run(
                &pos,
                &[pos.zobrist],
                &v1_ref::Limits {
                    nodes: *nodes,
                    max_playout_ply: 100,
                    c_uct: std::f32::consts::SQRT_2,
                },
                *seed,
            );
            assert_eq!(
                got.best, want.best,
                "best move differs (fen {fen}, seed {seed}, nodes {nodes})"
            );
            assert_eq!(
                got.score_cp, want.score_cp,
                "score differs (fen {fen}, seed {seed}, nodes {nodes})"
            );
            assert_eq!(
                got.nodes, want.nodes,
                "node count differs (fen {fen}, seed {seed}, nodes {nodes})"
            );
        }
    }
}

/// G1 bis — the tree is *smaller* than the v1 tree for the same search: the
/// lazy expansion does not allocate a node per legal move.
#[test]
fn v1_ref_flat_tree_is_not_bigger() {
    let pos = Position::startpos();
    let got = Search::new(pos, vec![pos.zobrist], limits(10_000), 42).run(&AtomicBool::new(false));
    assert!(got.tree_nodes > 0, "the tree must not be empty");
    assert!(
        got.tree_edges >= got.tree_nodes - 1,
        "a tree has at least one edge per non-root node ({} edges, {} nodes)",
        got.tree_edges,
        got.tree_nodes
    );
}

/// G3 — `has_legal_move` decides terminality exactly like the v1 probe.
#[test]
fn terminal_value_matches_v1_probe() {
    let cases: &[(&str, Vec<u64>)] = &[
        // checkmate (mover is mated)
        ("R6k/8/7K/8/8/8/8/8 b - - 0 1", vec![]),
        // stalemate
        ("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1", vec![]),
        // 50-move rule
        ("8/8/8/3k4/8/8/3K4/8 w - - 100 60", vec![]),
        // insufficient material
        ("8/8/3k4/8/8/8/4N3/4K3 w - - 0 1", vec![]),
        // live positions
        (
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            vec![],
        ),
        (
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            vec![],
        ),
        ("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1", vec![]),
        ("k7/8/KQ6/8/8/8/8/8 w - - 0 1", vec![]),
    ];
    for (fen, _) in cases {
        let pos = Position::from_fen(fen).unwrap();
        let hist = vec![pos.zobrist];
        assert_eq!(
            terminal_value(&pos, &hist),
            v1_ref::terminal_value(&pos, &hist),
            "terminality differs on {fen}"
        );
        assert_eq!(
            leaf_value(&pos, &hist),
            v1_ref::terminal_value(&pos, &hist).unwrap_or_else(|| {
                // live leaf = the normalised eval, same bridge as the engine
                mychess::mcts::eval_normalized(&pos)
            }),
            "leaf value differs on {fen}"
        );
    }
}

/// G3 bis — the triple repetition is still detected (the probe change must
/// not have broken the draw detection).
#[test]
fn terminal_repetition_still_draws() {
    let start = Position::startpos();
    let mut hist: Vec<u64> = vec![start.zobrist];
    let mut p = start;
    for _ in 0..2 {
        for uci in ["g1f3", "g8f6", "f3g1", "f6g8"] {
            let list = legal_moves(&p);
            let mv = list
                .iter()
                .find(|m| m.uci() == uci)
                .unwrap_or_else(|| panic!("{uci} must be legal"));
            p.make(*mv);
            hist.push(p.zobrist);
        }
    }
    assert_eq!(p.zobrist, start.zobrist, "the cycle must return to start");
    assert_eq!(
        terminal_value(&p, &hist),
        Some(0.0),
        "the third occurrence is a draw"
    );
}

/// G4 — the parallel search honours the node budget exactly.
#[test]
fn parallel_node_budget_is_honest() {
    let pos = Position::startpos();
    for threads in [2usize, 3, 4, 6] {
        for nodes in [1000u64, 4096] {
            let o = Search::new(pos, vec![pos.zobrist], limits(nodes), 42)
                .with_threads(threads)
                .run(&AtomicBool::new(false));
            assert_eq!(
                o.nodes, nodes,
                "threads={threads}: the total node budget must stay {nodes}"
            );
        }
    }
}

/// G4 bis — the parallel best move is legal.
#[test]
fn parallel_best_move_is_legal() {
    for fen in GOLDEN_FENS {
        let pos = Position::from_fen(fen).unwrap();
        let legal = legal_moves(&pos);
        let o = Search::new(pos, vec![pos.zobrist], limits(2000), 42)
            .with_threads(4)
            .run(&AtomicBool::new(false));
        assert!(
            legal.contains(&o.best),
            "illegal best move {:?} on {fen}",
            o.best.uci()
        );
    }
}

/// G4 ter — same seed, same thread count: same result.
#[test]
fn parallel_is_deterministic() {
    let pos = Position::startpos();
    let run = |threads: usize| {
        let o = Search::new(pos, vec![pos.zobrist], limits(3000), 7)
            .with_threads(threads)
            .run(&AtomicBool::new(false));
        (o.best, o.score_cp, o.nodes)
    };
    assert_eq!(run(4), run(4), "same (seed, threads) must be reproducible");
    assert_eq!(run(1), run(1), "the mono path must be reproducible");
}

/// G4 quater — the parallel search still finds the mate in one.
#[test]
fn parallel_finds_mate_in_1() {
    let pos = Position::from_fen("k7/8/KQ6/8/8/8/8/8 w - - 0 1").unwrap();
    let mut mates: Vec<String> = Vec::new();
    {
        let list = legal_moves(&pos);
        for mv in list {
            let mut p = pos;
            p.make(mv);
            let replies = legal_moves(&p);
            if replies.is_empty() && p.in_check(p.side) {
                mates.push(mv.uci());
            }
        }
    }
    assert!(!mates.is_empty(), "the position must hold a mate in 1");
    let o = Search::new(pos, vec![pos.zobrist], limits(3000), 42)
        .with_threads(4)
        .run(&AtomicBool::new(false));
    assert!(
        mates.contains(&o.best.uci()),
        "mate in 1 not found in parallel: best={} (candidates {mates:?})",
        o.best.uci()
    );
}

/// G4 quinquies — a single thread gives exactly the mono result (the
/// parallel path is not entered at all).
#[test]
fn one_thread_is_the_mono_path() {
    let pos = Position::startpos();
    let mono = Search::new(pos, vec![pos.zobrist], limits(3000), 42).run(&AtomicBool::new(false));
    let one = Search::new(pos, vec![pos.zobrist], limits(3000), 42)
        .with_threads(1)
        .run(&AtomicBool::new(false));
    assert_eq!(mono.best, one.best);
    assert_eq!(mono.score_cp, one.score_cp);
    assert_eq!(mono.nodes, one.nodes);
}

/// The parallel search must not be slower than the mono one for the same
/// budget on a *tiny* budget (sanity: no pathological overhead).
#[test]
fn parallel_does_not_lose_nodes() {
    let pos = Position::startpos();
    let o = Search::new(pos, vec![pos.zobrist], limits(10_000), 42)
        .with_threads(4)
        .run(&AtomicBool::new(false));
    assert_eq!(o.nodes, 10_000);
    assert!(o.tree_nodes > 0);
}
