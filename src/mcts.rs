//! MCTS v1 (STEP 0 baseline), **phase 3**: flat arena + cheap terminality.
//!
//! The **algorithm** is the v1 one, unchanged: plain UCT, one-node expansion,
//! value for the player to move at the node, terminality in four causes
//! (checkmate, stalemate, 50-move, triple repetition) plus insufficient
//! material, deterministic xorshift64* seeded per search.
//!
//! What phase 3 changes is the **data structure** and the **terminality
//! probe**:
//!
//! * the tree lives in a flat arena (`mcts::arena`): `Vec<Node>` +
//!   `Vec<Edge>`, the edges of a node are contiguous, and a child node is
//!   allocated only when it is *developed* — the v1 tree allocated one full
//!   node per legal move, including the children that were never played;
//! * the statistics of a child live on the **edge** that leads to it, so the
//!   UCT scan reads one contiguous array instead of chasing a pointer per
//!   child (`mcts::arena` documents the mirror invariant);
//! * terminality is decided by `movegen::has_legal_move` — no `MoveList` is
//!   materialised to answer "is there a legal move?" on a leaf;
//!
//! **The mono-thread path is bit-for-bit identical to the v1 engine**: same
//! RNG draw order, same selection, same values. `tests/mcts_phase3.rs`
//! checks it against a copy of the v1 algorithm.
//!
//! Hooks designed for later ladder steps (NOT implemented here):
//! * A2: `Edge::prior` + PUCT selection (hook `prior(pos, move)`);
//! * A1: leaf value = `eval_for_mover(pos)` instead of the random playout;
//! * B1: **tree**-parallel (shared arena + atomic stats + virtual loss) — the
//!   phase 3 parallelism is root-parallel, which is race-free but does not
//!   share the depth between threads;
//! * B2: tree reuse between `go`s (arena persists, root shifts);
//! * B3: transpositions (hash→node map);
//! * D1: NN policy/value replace prior hook + eval hook (same call shapes).

use crate::eval;
use crate::movegen::{MoveList, generate, has_legal_move};
use crate::position::Position;
use crate::types::{GenKind, Move};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

mod arena;

use arena::{Arena, NO_CHILD, ROOT};

// ---- A1 value bridge (TUNABLE AU LOT C3) ------------------------------------
/// Fixed cp→Q calibration chosen for A1: linear `/1000` clamped to
/// `±EVAL_Q_CLAMP` (the sigmoid(cp, τ) alternative is rejected for now — see
/// ELO_LOG A1 notes). These are the ONLY numbers of the bridge: every call
/// site must go through [`eval_normalized`], never a buried literal.
///
/// **Tunable au lot C3**: the tapered-eval lot recalibrates this mapping
/// (scale and/or clamp and/or sigmoid shape) — flip these constants, nothing
/// else.
const EVAL_Q_CP_SCALE: f32 = 1.0 / 1000.0;
const EVAL_Q_CLAMP: f32 = 1.0;

/// Deterministic RNG (xorshift64*), seeded per search.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }
    #[inline(always)]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x >> 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// Search knobs (UCI-settable).
pub struct Limits {
    /// Total playouts (node budget for fastchess fixed-nodes matches).
    pub nodes: u64,
    /// Wall-clock cap in milliseconds (`None` = node budget only).
    pub movetime_ms: Option<u128>,
    /// Maximum descent depth, in plies.
    ///
    /// In v1 this capped the *random playout* walk (a playout that hit the
    /// cap was scored as a draw). Since A1 replaced the random playout by
    /// the static evaluation at the leaf, the descent itself is the
    /// playout: a node at this depth is **not expanded** and is scored like
    /// any other leaf (terminality first, then the static eval). The knob
    /// is therefore a search-depth limit, and it is live again.
    pub max_playout_ply: u32,
    /// UCT exploration constant.
    pub c_uct: f32,
}

pub struct Outcome {
    pub best: Move,
    pub score_cp: i32,
    pub nodes: u64,
    pub elapsed_ms: u128,
    /// Nodes allocated in the tree arena (the root included). In parallel
    /// mode: the sum over the workers.
    pub tree_nodes: u64,
    /// Edges allocated in the tree arena. In parallel mode: the sum over the
    /// workers.
    pub tree_edges: u64,
}

const BUDGET_POLL_EVERY: u64 = 64;

pub struct Search {
    pub root_pos: Position,
    /// Zobrist of every position BEFORE the root (UCI replay), for repetition.
    pub game_hist: Vec<u64>,
    pub limits: Limits,
    pub seed: u64,
}

impl Search {
    pub fn new(root_pos: Position, game_hist: Vec<u64>, limits: Limits, seed: u64) -> Search {
        Search {
            root_pos,
            game_hist,
            limits,
            seed,
        }
    }

    /// Run to the node/time budget.
    pub fn run(&self, stop: &AtomicBool) -> Outcome {
        self.run_mono(stop)
    }

    /// Mono-thread search (the reference path).
    fn run_mono(&self, stop: &AtomicBool) -> Outcome {
        let start = Instant::now();
        let deadline = self
            .limits
            .movetime_ms
            .map(|ms| start + Duration::from_millis(ms as u64));
        let worker = Worker {
            pos: &self.root_pos,
            hist: &self.game_hist,
            c_uct: self.limits.c_uct,
            max_playout_ply: self.limits.max_playout_ply,
            seed: self.seed,
        };
        let res = run_worker(&worker, self.limits.nodes, deadline, stop);
        let (best, score_cp) = pick_best(&res.root);
        Outcome {
            best,
            score_cp,
            nodes: res.nodes,
            elapsed_ms: start.elapsed().as_millis(),
            tree_nodes: res.tree_nodes,
            tree_edges: res.tree_edges,
        }
    }
}

/// The most-visited root edge wins; ties keep the earliest (generation
/// order). The score is the UCT value of that edge, in centipawns, as v1.
fn pick_best(root: &[(Move, u64, f32)]) -> (Move, i32) {
    if root.is_empty() {
        return (Move::NONE, 0);
    }
    let mut best = 0usize;
    let mut bv = root[0].1;
    for (i, e) in root.iter().enumerate() {
        if e.1 > bv {
            best = i;
            bv = e.1;
        }
    }
    if bv == 0 {
        // The root was never developed (budget too small): v1 answered
        // `0000` in that case (no child at all).
        return (Move::NONE, 0);
    }
    let sc = (-(root[best].2 / bv as f32) * 100.0).round() as i32;
    (root[best].0, sc)
}

/// One search worker: its own tree, its own RNG, its own buffers.
struct Worker<'a> {
    pos: &'a Position,
    hist: &'a [u64],
    c_uct: f32,
    max_playout_ply: u32,
    seed: u64,
}

/// What a worker hands back to the parent.
struct WorkerResult {
    /// Playouts executed.
    nodes: u64,
    /// Root edges, in generation order: (move, visits, sum).
    root: Vec<(Move, u64, f32)>,
    /// Nodes allocated in the tree arena.
    tree_nodes: u64,
    /// Edges allocated in the tree arena.
    tree_edges: u64,
}

/// Run one worker until its budget (nodes), its deadline (time) or `stop`.
fn run_worker(
    w: &Worker,
    budget: u64,
    deadline: Option<Instant>,
    stop: &AtomicBool,
) -> WorkerResult {
    let mut rng = Rng::new(w.seed);
    let mut arena = Arena::new();
    // The descent buffers are allocated once and reused by every
    // playout (the hash history and the path both grow per ply).
    let mut scratch = Scratch {
        hist: Vec::with_capacity(w.hist.len() + 64),
        path: Vec::with_capacity(64),
    };
    // Root terminality (position delivered over by the UCI history).
    if let Some(tv) = terminal_value(w.pos, w.hist) {
        arena.node_mut(ROOT).set_terminal(tv);
    }
    let mut nodes: u64 = 0;
    let mut deadline_reached = false;
    while nodes < budget && !deadline_reached && !stop.load(Ordering::Relaxed) {
        search_once(w, &mut arena, &mut rng, &mut scratch);
        nodes += 1;
        if nodes.is_multiple_of(BUDGET_POLL_EVERY)
            && let Some(d) = deadline
            && Instant::now() >= d
        {
            deadline_reached = true;
        }
    }
    let root = arena
        .edges_of(ROOT)
        .iter()
        .map(|e| (e.mv, e.visits as u64, e.sum))
        .collect();
    WorkerResult {
        nodes,
        root,
        tree_nodes: arena.len_nodes() as u64,
        tree_edges: arena.len_edges() as u64,
    }
}

/// One playout = one descent (select/expand/leaf/backprop).
///
/// The descent walks the flat arena. `cur_edge` is the edge that led to the
/// current node (`None` at the root). The current node may **not exist yet**:
/// an edge whose child was never developed is a leaf (the v1 "first visit
/// scores the leaf" rule), and the child node is allocated only when the
/// edge is visited a second time.
fn search_once(w: &Worker, arena: &mut Arena, rng: &mut Rng, scratch: &mut Scratch) {
    let mut p = *w.pos;
    // The hash history is *reused* across playouts: it is truncated back to
    // the game history and re-grown during the descent. Cloning it per
    // playout was an allocation on the hottest path of the search.
    scratch.hist.clear();
    scratch.hist.extend_from_slice(w.hist);
    scratch.path.clear();

    let mut cur_edge: Option<u32> = None;
    // The current node, if it exists (the root always does).
    let mut node_idx: Option<u32> = Some(ROOT);

    loop {
        // --- state of the current node ---------------------------------------
        let cur_visits = match cur_edge {
            None => arena.node(ROOT).visits,
            Some(e) => arena.edge(e).visits,
        };
        if let Some(idx) = node_idx
            && arena.node(idx).is_terminal()
        {
            let tv = arena.node(idx).term;
            backprop(arena, &scratch.path, tv);
            return;
        }
        let expanded = node_idx.is_some_and(|i| arena.node(i).is_expanded());
        // Depth in nodes (the root is 1), the v1 convention: the path length
        // in nodes, root included.
        let depth = scratch.path.len() as u32 + 1;

        if !expanded {
            if cur_visits == 0 || depth >= w.max_playout_ply {
                // A1 leaf: exact terminality first, else the static eval.
                // Single source of truth (`mcts::leaf_value`) so the unit
                // tests exercise the EXACT path the search takes.
                let r = leaf_value(&p, &scratch.hist);
                backprop(arena, &scratch.path, r);
                return;
            }
            // DEVELOP the node. A child that was never developed is
            // allocated here, seeded with the statistics of its edge (the
            // mirror invariant).
            let idx = match node_idx {
                Some(i) => i,
                None => {
                    let e = cur_edge.expect("a non-root node always has an incoming edge");
                    let (visits, sum) = {
                        let ed = arena.edge(e);
                        (ed.visits, ed.sum)
                    };
                    let i = arena.alloc_node();
                    arena.node_mut(i).visits = visits;
                    arena.node_mut(i).sum = sum;
                    arena.edge_mut(e).child = i;
                    i
                }
            };
            // EXPAND: terminality first, then the edges.
            let mut list = MoveList::new();
            generate(&p, &mut list, GenKind::All);
            let tv = if list.is_empty() {
                Some(if p.in_check(p.side) { -1.0 } else { 0.0 }) // mate / stalemate
            } else if p.halfmove >= 100
                || repetition_draw(&p, &scratch.hist)
                || eval::insufficient_material(&p)
            {
                Some(0.0)
            } else {
                None
            };
            if let Some(tv) = tv {
                arena.node_mut(idx).set_terminal(tv);
                backprop(arena, &scratch.path, tv);
                return;
            }
            arena.add_edges(idx, list.iter());
            node_idx = Some(idx);
        }

        // --- selection --------------------------------------------------------
        //
        // The unvisited children are counted in a single pass; the chosen
        // one is then located with a second pass over the *unvisited*
        // children only (the counter `k` advances on unvisited entries
        // alone, so the two passes agree by construction).
        let idx = node_idx.expect("the node has just been developed");
        let (edge_idx, mv) = {
            let edges = arena.edges_of(idx);
            let unv_count = edges.iter().filter(|e| e.visits == 0).count() as u64;
            if unv_count > 0 {
                // v1: uniform over unvisited.
                let pick = (rng.next_u64() % unv_count) as usize;
                let mut k = 0usize;
                let mut chosen = None;
                for (i, e) in edges.iter().enumerate() {
                    if e.visits == 0 {
                        if k == pick {
                            chosen = Some(i);
                            break;
                        }
                        k += 1;
                    }
                }
                let i = chosen.expect("the unvisited count and the unvisited scan agree");
                (arena.node(idx).first_edge + i as u32, edges[i].mv)
            } else {
                // UCT argmax: q(child for parent's mover) + C*sqrt(ln N / n).
                let pv = arena.node(idx).visits as f32;
                let log_pv = (pv as f64 + 1.0).ln() as f32;
                let mut bi = 0usize;
                let mut bs = f32::MIN;
                for (i, e) in edges.iter().enumerate() {
                    let u =
                        -(e.sum / e.visits as f32) + w.c_uct * (log_pv / e.visits as f32).sqrt();
                    if u > bs {
                        bs = u;
                        bi = i;
                    }
                }
                (arena.node(idx).first_edge + bi as u32, edges[bi].mv)
            }
        };
        p.make(mv);
        scratch.hist.push(p.zobrist);
        scratch.path.push(edge_idx);
        cur_edge = Some(edge_idx);
        // The child node exists only if it was developed; until then the
        // edge's statistics are the child's ones (mirror invariant).
        node_idx = {
            let child = arena.edge(edge_idx).child;
            if child == NO_CHILD { None } else { Some(child) }
        };
    }
}

/// Reusable buffers for the descent (no allocation per playout).
struct Scratch {
    hist: Vec<u64>,
    /// The edges walked by the current descent (the root has no incoming
    /// edge, so the path is one shorter than the node path).
    path: Vec<u32>,
}

/// Retropropagate `v` (the value for the side to move at the leaf) along the
/// path: the edge carries the child's statistics, the child node mirrors
/// them, and the value flips at every level (zero-sum).
fn backprop(arena: &mut Arena, path: &[u32], mut v: f32) {
    for &e in path.iter().rev() {
        {
            let edge = arena.edge_mut(e);
            edge.sum += v;
            edge.visits += 1;
        }
        let child = arena.edge(e).child;
        if child != NO_CHILD {
            let node = arena.node_mut(child);
            node.sum += v;
            node.visits += 1;
        }
        v = -v;
    }
    let root = arena.node_mut(ROOT);
    root.sum += v;
    root.visits += 1;
}

/// Triple-repetition draw check (v1 semantics, exact): `pos` draws iff it
/// appeared **≥2 times strictly before it** within the reversible span. The
/// window is `min(halfmove, len-1)` — an irreversible move (pawn move /
/// capture) resets the halfmove clock, so nothing older can repeat the
/// current position; the last history entry is excluded (it is `pos` itself,
/// pushed when the UCI replay or the descent reached it).
#[inline]
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

/// A1 leaf value: static eval normalised to the Q space [-1, 1] used by
/// MCTS/PUCT/NN (D1 tanh convention): cp is mapped by the named bridge
/// constants above. Terminal positions keep their exact {-1, 0, 1} values
/// (checked first — see [`leaf_value`]).
#[inline]
pub fn eval_normalized(pos: &Position) -> f32 {
    (eval::eval_for_mover(pos) as f32 * EVAL_Q_CP_SCALE).clamp(-EVAL_Q_CLAMP, EVAL_Q_CLAMP)
}

/// The A1 leaf contract, one function so the unit tests exercise the exact
/// path the search takes: **terminality first** (exact {-1, 0, 1} — mate,
/// stalemate, 50-move, triple repetition, insufficient material — NEVER the
/// eval), static eval only on a non-terminal leaf.
#[inline(always)]
pub fn leaf_value(pos: &Position, hist: &[u64]) -> f32 {
    match terminal_value(pos, hist) {
        Some(v) => v,
        None => eval_normalized(pos),
    }
}

/// Terminality of `pos` given the history of positions BEFORE it — the
/// exported face of v1 terminality (tests); expansion uses the same logic.
///
/// **Phase 3:** the "is there a legal move?" probe is
/// [`movegen::has_legal_move`], which answers without materialising a
/// `MoveList` (the v1 code built a 512-byte list on every leaf). The order of
/// the tests is normative: a mate beats the draw rules (a position mated on
/// the 100th halfmove is a mate, not a draw).
pub fn terminal_value(pos: &Position, hist: &[u64]) -> Option<f32> {
    if !has_legal_move(pos) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcts::arena::F_EXPANDED;

    /// Run a search and hand the arena back, for the invariant checks.
    fn build_arena(pos: &Position, hist: &[u64], seed: u64, nodes: u64) -> Arena {
        let worker = Worker {
            pos,
            hist,
            c_uct: std::f32::consts::SQRT_2,
            max_playout_ply: 100,
            seed,
        };
        let mut arena = Arena::new();
        let mut rng = Rng::new(seed);
        let mut scratch = Scratch {
            hist: Vec::with_capacity(hist.len() + 64),
            path: Vec::with_capacity(64),
        };
        for _ in 0..nodes {
            search_once(&worker, &mut arena, &mut rng, &mut scratch);
        }
        arena
    }

    /// The mirror invariant: a developed child's statistics are the copy of
    /// the statistics of the edge that leads to it.
    #[test]
    fn arena_mirror_invariant() {
        let pos = Position::startpos();
        let arena = build_arena(&pos, &[pos.zobrist], 42, 5000);
        let mut developed = 0usize;
        for i in 0..arena.len_nodes() as u32 {
            let n = arena.node(i);
            assert_eq!(
                arena.edges_of(i).len(),
                n.n_edges as usize,
                "the edges of a node are its contiguous range"
            );
            for e in arena.edges_of(i) {
                if e.child != NO_CHILD {
                    developed += 1;
                    let c = arena.node(e.child);
                    assert_eq!(c.visits, e.visits, "visits mirror");
                    assert_eq!(c.sum, e.sum, "sum mirror");
                }
            }
        }
        assert!(developed > 0, "the search must develop children");
    }

    /// Lazy expansion: an edge that was never revisited has no child node,
    /// so an undeveloped child costs nothing but its edge.
    #[test]
    fn arena_lazy_expansion() {
        let pos = Position::startpos();
        let arena = build_arena(&pos, &[pos.zobrist], 42, 2000);
        let mut undeveloped = 0usize;
        for i in 0..arena.len_nodes() as u32 {
            for e in arena.edges_of(i) {
                if e.child == NO_CHILD {
                    undeveloped += 1;
                }
            }
        }
        assert!(
            undeveloped > 0,
            "some children must stay undeveloped (no node allocated)"
        );
        assert!(
            arena.len_nodes() < arena.len_edges(),
            "nodes ({}) must be fewer than edges ({})",
            arena.len_nodes(),
            arena.len_edges()
        );
    }

    /// The root is visited by every playout, exactly once.
    #[test]
    fn arena_root_visits_match_playouts() {
        let pos = Position::startpos();
        let arena = build_arena(&pos, &[pos.zobrist], 7, 1234);
        assert_eq!(arena.node(ROOT).visits, 1234);
    }

    /// The expanded flag is set exactly once per node, and the edge range of
    /// a node is never rewritten.
    #[test]
    fn arena_edges_are_stable() {
        let pos = Position::startpos();
        let mut arena = Arena::new();
        let mut rng = Rng::new(3);
        let mut scratch = Scratch {
            hist: Vec::new(),
            path: Vec::with_capacity(64),
        };
        let worker = Worker {
            pos: &pos,
            hist: &[pos.zobrist],
            c_uct: std::f32::consts::SQRT_2,
            max_playout_ply: 100,
            seed: 3,
        };
        let mut first_ranges: Vec<(u32, u8)> = Vec::new();
        for _ in 0..500 {
            search_once(&worker, &mut arena, &mut rng, &mut scratch);
            if arena.node(ROOT).is_expanded() && first_ranges.is_empty() {
                let n = arena.node(ROOT);
                first_ranges.push((n.first_edge, n.n_edges));
            }
        }
        let n = arena.node(ROOT);
        assert_eq!(n.flags & F_EXPANDED, F_EXPANDED, "root is expanded");
        assert_eq!(
            first_ranges[0],
            (n.first_edge, n.n_edges),
            "the edge range of a node never changes"
        );
    }
}
