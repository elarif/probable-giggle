//! MCTS v1 (STEP 0 baseline), **phase 3**: flat arena + cheap terminality +
//! root parallelism, **phase 4**: PUCT selection + heuristic policy priors.
//!
//! The **algorithm** is the v1 one: plain UCT, one-node expansion,
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
//! * the search can run on several threads, splitting the root moves into
//!   disjoint packets: each thread owns its tree, the root statistics are
//!   merged at the end. No shared mutable state, no lock, no race.
//!
//! **The mono-thread path is bit-for-bit identical to the v1 engine**: same
//! RNG draw order, same selection, same values. `tests/mcts_phase3.rs`
//! checks it against a copy of the v1 algorithm.
//!
//! What phase 4 changes is the **selection rule**:
//!
//! * the selection is **PUCT** (`Policy::Puct`, the default):
//!   `s(a) = Q(a) + c_puct·P(a)·sqrt(N)/(1+n(a))`, where `P(a)` is the
//!   **policy prior** of the move — an MVV-LVA heuristic computed once per
//!   developed node (`mcts::policy`) and stored in `Edge::prior`;
//! * the legacy **UCT** rule is kept intact (`Policy::Uct`): it is the
//!   frozen oracle of the phase-3 differential test and the A/B
//!   counter-measure of the phase 4;
//! * the PUCT rule keeps the **breadth** of the v1 rule (a uniform draw
//!   among the unvisited children) and changes the **depth** rule (the
//!   exploration bonus decays as `1/(1+n)` instead of `1/sqrt(n)`), which
//!   is what makes the search converge much faster in value — measured,
//!   see `docs/mcts/phase4-2026-10-22.md`;
//! * the root-parallel mode **perturbs the root priors** (Gumbel noise,
//!   `mcts::policy::perturb`): the amorce of C1 (the full Dirichlet
//!   noise). It is *neutral* in the current measurements — the workers
//!   already diverge through their independent RNG streams.
//!
//! The mono PUCT path consumes the RNG (the breadth draw), so it is
//! **reproducible for a given seed**, like the v1 path.
//!
//! Setting the environment variable `MYCHESS_DEBUG_MERGE=1` dumps the merged
//! root statistics (move, visits, value) of a parallel search on stderr —
//! the instrument that showed the biased merge of a partitioned root
//! (`docs/mcts/phase3-2026-10-22.md`).
//!
//! Hooks designed for later ladder steps (NOT implemented here):
//! * A3: FPU (first-play urgency) — the PUCT uses `Q = 0` for an unvisited
//!   child (AlphaZero convention);
//! * B1: **tree**-parallel (shared arena + atomic stats + virtual loss) — the
//!   phase 3 parallelism is root-parallel, which is race-free but does not
//!   share the depth between threads;
//! * B2: tree reuse between `go`s (arena persists, root shifts);
//! * B3: transpositions (hash→node map);
//! * C1: the full Dirichlet noise (the phase 4 Gumbel perturbation of the
//!   root is only the diversity the root-parallel mode needs);
//! * D1: NN policy/value replace the prior hook + eval hook (same call
//!   shapes). The prior module is already a "logits → softmax → prior"
//!   pipeline: the NN policy will feed the same softmax.

use crate::eval;
use crate::movegen::{MoveList, generate, has_legal_move};
use crate::position::Position;
use crate::types::{GenKind, Move};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

mod arena;
pub mod policy;
pub mod qsearch;

use arena::{Arena, NO_CHILD, ROOT};
use policy::priors;

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
///
/// # Le triple de décalages (corrigé le 2026-10-23)
///
/// Le xorshift64* canonique est
/// `x ^= x >> 12; x ^= x << 25; x ^= x >> 27;` — le décalage du milieu
/// est un **décalage à gauche**. La version qui était dans ce fichier
/// (`x ^= x >> 25`) n'était **pas** un xorshift64* : la transformation
/// n'était pas un état réversible, et le générateur avait une **période
/// de 4** (quatre valeurs, répétées indéfiniment). C'est un **bug prouvé**
/// (un test de non-répétition le met en évidence), pas une opinion :
///
/// ```text
/// 3154547314117558073, 2506398873287524326,
/// 468725656381219356, 5192220531023863043,   <- et on recommence
/// ```
///
/// La conséquence n'était pas cosmétique : le tirage « au hasard » du
/// MCTS (le choix parmi les enfants non visités) et la perturbation
/// Gumbel de la racine ne tiraient que **quatre** valeurs. C'est ce qui
/// a été trouvé en cherchant à entraîner le réseau de valeur : un
/// corpus d'auto-play collecté avec ce générateur n'avait **aucune**
/// diversité, et le réseau n'apprenait rien (il n'y avait pas de signal
/// à apprendre, seulement quatre positions à caser).
///
/// Le core gelé (`vcore-1.0`) n'est **pas** touché : `position`,
/// `movegen`, `types` et `zobrist` ne contiennent aucun générateur.
/// Le changement de comportement du MCTS est en revanche **mesuré** par
/// un duel (voir `docs/nn/`).
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
        x ^= x << 25;
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
    /// PUCT exploration constant (phase 4). Only used by `Policy::Puct`.
    ///
    /// The PUCT score is `Q + c_puct · P · sqrt(N) / (1 + n)`: the bonus
    /// decays with the number of visits of the child, so a large `c_puct`
    /// keeps exploring the moves the prior likes, a small one trusts the
    /// values faster. AlphaZero uses ~1.5 on chess.
    pub c_puct: f32,
    /// Scale of the Gumbel perturbation applied to the **root** priors in
    /// the root-parallel mode (phase 4). `0` disables it.
    ///
    /// The mono path never perturbs: it stays clean and deterministic. The
    /// parallel mode needs it — without diversity between the workers, the
    /// parallel search would collapse to the mono search at `N/T` nodes.
    pub root_noise: f32,
    /// **Depth of the tactical resolution of the leaf** (phase 5). `0`
    /// disables it: the leaf is then valued by the static evaluation
    /// alone, exactly as in phase 4 (bit-for-bit).
    ///
    /// With `leaf_q = d > 0`, the leaf value is computed after resolving
    /// the exchanges on `d` plies (`mcts::qsearch::qvalue`) — the
    /// quiescence-lite. This is what removes the *horizon effect*: a leaf
    /// that stands in the middle of an exchange is no longer scored as if
    /// the exchange did not exist.
    ///
    /// **Measured verdict (2026-10-22): the default stays `0`.** The
    /// quiescence-lite is *negative* against the frozen phase 4
    /// (−177 Elo at 2 000 nodes, −241 at 10 000 for `d = 1`): the `max`
    /// over captures is optimistic and double-counts the exchanges the
    /// tree explores by itself. The knob is kept (default `0`, i.e. the
    /// phase-4 behaviour) as a documented experiment — see
    /// `docs/mcts/phase5-2026-10-22.md` §4.
    pub leaf_q: u32,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            nodes: 10_000,
            movetime_ms: None,
            max_playout_ply: 100,
            c_uct: std::f32::consts::SQRT_2,
            c_puct: DEFAULT_C_PUCT,
            root_noise: DEFAULT_ROOT_NOISE,
            leaf_q: 0,
        }
    }
}

/// The PUCT exploration constant used by default (phase 4).
pub const DEFAULT_C_PUCT: f32 = 1.5;

/// The default scale of the root perturbation in the parallel mode.
pub const DEFAULT_ROOT_NOISE: f32 = 1.0;

/// The selection rule of the search (phase 4).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Policy {
    /// The legacy v1 rule: a uniform draw among the unvisited children,
    /// UCT among the visited ones. **Frozen** — it is the oracle of the
    /// phase-3 differential test and the A/B counter-measure of phase 4.
    Uct,
    /// PUCT with a heuristic policy prior (phase 4, the default).
    #[default]
    Puct,
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
    /// Number of parallel root workers. `1` = mono-thread (the default).
    pub threads: usize,
    /// The selection rule (phase 4). `Policy::Puct` by default.
    pub policy: Policy,
}

impl Search {
    pub fn new(root_pos: Position, game_hist: Vec<u64>, limits: Limits, seed: u64) -> Search {
        Search {
            root_pos,
            game_hist,
            limits,
            seed,
            threads: 1,
            policy: Policy::default(),
        }
    }

    /// The selection rule (phase 4).
    ///
    /// * `Policy::Puct` (the default) — PUCT with the MVV-LVA priors of
    ///   `mcts::policy`; the mono path is then **deterministic**;
    /// * `Policy::Uct` — the legacy v1 rule, kept frozen as the oracle of
    ///   the phase-3 differential test and as the A/B counter-measure of
    ///   the phase 4.
    pub fn with_policy(mut self, policy: Policy) -> Search {
        self.policy = policy;
        self
    }

    /// The depth of the **tactical resolution of the leaf** (phase 5).
    ///
    /// * `0` (the default) — the leaf is valued by the static evaluation
    ///   alone: the behaviour of phase 4, **bit-for-bit** ;
    /// * `d > 0` — the leaf value is computed after resolving the
    ///   exchanges on `d` plies (`mcts::qsearch`), which removes the
    ///   *horizon effect*.
    pub fn with_leaf_q(mut self, leaf_q: u32) -> Search {
        self.limits.leaf_q = leaf_q;
        self
    }

    /// Number of parallel root workers. `1` (the default) is the mono-thread
    /// engine, bit-for-bit identical to v1.
    ///
    /// The root moves are split into `threads` disjoint packets, one per
    /// worker; each worker owns its tree and the root statistics are merged
    /// at the end. Two consequences, both deliberate:
    ///
    /// * a **node** budget is split between the workers, so the total stays
    ///   the requested one (an SPRT ladder at fixed nodes stays comparable);
    /// * a **time** budget is a shared deadline, so every worker runs until
    ///   it and the total number of nodes is `≈threads ×` the mono one — the
    ///   throughput mode, where more nodes at the same depth is the point.
    pub fn with_threads(mut self, threads: usize) -> Search {
        self.threads = threads.max(1);
        self
    }

    /// Run to the node/time budget.
    pub fn run(&self, stop: &AtomicBool) -> Outcome {
        if self.threads > 1 {
            self.run_parallel(stop)
        } else {
            self.run_mono(stop)
        }
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
            c_puct: self.limits.c_puct,
            // The mono path is clean: no root perturbation, so the PUCT
            // search is fully deterministic.
            root_noise: 0.0,
            policy: self.policy,
            leaf_q: self.limits.leaf_q,
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

    /// Root-parallel search: one tree per worker, root statistics summed.
    ///
    /// Every worker searches the **whole** root with its own tree and its own
    /// RNG stream; the root statistics (visits, values) are then summed, move
    /// by move. This is the classic root-parallel MCTS: no shared mutable
    /// state, no lock, no race, and the root statistics stay comparable
    /// between the moves — which a *partitioned* root cannot promise (the
    /// visit counts of two disjoint packets are not comparable: the packet
    /// whose best move faces fewer competitors accumulates more visits on
    /// it, and the "most visited" rule then picks the wrong move — measured,
    /// see `docs/mcts/phase3-2026-10-22.md`).
    ///
    /// The price is **diversity**: the workers explore the same subtrees, so
    /// the union of the trees covers less than a partitioned split would. The
    /// total node count and the total memory are unchanged (each worker holds
    /// `nodes / threads` nodes).
    fn run_parallel(&self, stop: &AtomicBool) -> Outcome {
        let start = Instant::now();
        let t = self.threads.max(1);
        let empty = Outcome {
            best: Move::NONE,
            score_cp: 0,
            nodes: 0,
            elapsed_ms: start.elapsed().as_millis(),
            tree_nodes: 0,
            tree_edges: 0,
        };
        // A terminal root has nothing to search.
        if terminal_value(&self.root_pos, &self.game_hist).is_some() {
            return empty;
        }
        let mut list = MoveList::new();
        generate(&self.root_pos, &mut list, GenKind::All);
        let root_moves: Vec<Move> = list.iter().collect();
        if root_moves.is_empty() {
            return empty;
        }
        let deadline = self
            .limits
            .movetime_ms
            .map(|ms| start + Duration::from_millis(ms as u64));
        // Node budget: split, so the total stays the requested one. With a
        // time budget: every worker runs until the shared deadline.
        let (per, extra) = if deadline.is_some() {
            (u64::MAX / 4, 0)
        } else {
            (self.limits.nodes / t as u64, self.limits.nodes % t as u64)
        };
        let results: Vec<WorkerResult> = std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(t - 1);
            for (i, _) in (1..t).enumerate() {
                let seed = derive_seed(self.seed, i as u64 + 1);
                let pos = &self.root_pos;
                let hist = &self.game_hist;
                let c_uct = self.limits.c_uct;
                let c_puct = self.limits.c_puct;
                let root_noise = self.limits.root_noise;
                let policy = self.policy;
                let leaf_q = self.limits.leaf_q;
                let max_playout_ply = self.limits.max_playout_ply;
                handles.push(scope.spawn(move || {
                    let worker = Worker {
                        pos,
                        hist,
                        c_uct,
                        c_puct,
                        root_noise,
                        policy,
                        leaf_q,
                        max_playout_ply,
                        seed,
                    };
                    run_worker(&worker, per, deadline, stop)
                }));
            }
            // The calling thread takes the first share (and the extra nodes).
            let worker = Worker {
                pos: &self.root_pos,
                hist: &self.game_hist,
                c_uct: self.limits.c_uct,
                c_puct: self.limits.c_puct,
                root_noise: self.limits.root_noise,
                policy: self.policy,
                leaf_q: self.limits.leaf_q,
                max_playout_ply: self.limits.max_playout_ply,
                seed: self.seed,
            };
            let mine = run_worker(&worker, per + extra, deadline, stop);
            // The results must stay in the worker order: every worker expands
            // the root in the same generation order, so the merge below maps
            // the k-th root edge of worker i to the k-th root move.
            let mut out = Vec::with_capacity(t);
            out.push(mine);
            for h in handles {
                out.push(h.join().expect("MCTS worker thread panicked"));
            }
            out
        });
        // Merge the root statistics, move by move.
        let mut visits = vec![0u64; root_moves.len()];
        let mut sums = vec![0f32; root_moves.len()];
        let mut nodes = 0u64;
        let mut tree_nodes = 0u64;
        let mut tree_edges = 0u64;
        for res in results.iter() {
            nodes += res.nodes;
            tree_nodes += res.tree_nodes;
            tree_edges += res.tree_edges;
            for (g, (mv, v, s)) in res.root.iter().enumerate() {
                if g >= root_moves.len() {
                    continue;
                }
                debug_assert_eq!(root_moves[g], *mv, "root edge order");
                visits[g] += *v;
                sums[g] += *s;
            }
        }
        let merged: Vec<(Move, u64, f32)> = root_moves
            .iter()
            .enumerate()
            .map(|(g, &mv)| (mv, visits[g], sums[g]))
            .collect();
        if std::env::var("MYCHESS_DEBUG_MERGE").is_ok() {
            for (g, (mv, v, s)) in merged.iter().enumerate() {
                eprintln!("MERGE|{g}|{}|{v}|{s}", mv.uci());
            }
        }
        let (best, score_cp) = pick_best(&merged);
        Outcome {
            best,
            score_cp,
            nodes,
            elapsed_ms: start.elapsed().as_millis(),
            tree_nodes,
            tree_edges,
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
    /// PUCT exploration constant (phase 4).
    c_puct: f32,
    /// Scale of the Gumbel perturbation of the root priors (phase 4).
    root_noise: f32,
    /// The selection rule.
    policy: Policy,
    /// The depth of the tactical resolution of the leaf (phase 5). `0` =
    /// static evaluation only (the phase-4 behaviour, bit-for-bit).
    leaf_q: u32,
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
                let r = leaf_value_q(&p, &scratch.hist, w.leaf_q);
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
            // EXPAND: the edges are created with their **policy prior**
            // (phase 4). The prior is computed ONCE, here: the MVV-LVA
            // heuristic of each legal move, turned into a probability
            // distribution. The legacy UCT path does not compute it (it
            // would not use it) — its RNG draw order stays untouched.
            match w.policy {
                Policy::Uct => {
                    arena.add_edges(idx, list.iter().map(|m| (m, 0.0)));
                }
                Policy::Puct => {
                    let mut pr = priors(&p, list.as_slice());
                    // The root of a *parallel* search is perturbed: the
                    // workers must not explore the same tree, otherwise
                    // the parallel search collapses to the mono search at
                    // N/T nodes. The mono path keeps a clean root.
                    if idx == ROOT && w.root_noise > 0.0 {
                        policy::perturb(&mut pr, rng, w.root_noise);
                    }
                    arena.add_edges(idx, list.iter().zip(pr));
                }
            }
            node_idx = Some(idx);
        }

        // --- selection --------------------------------------------------------
        //
        // Two rules live here (phase 4):
        //
        // * `Policy::Uct` — the frozen v1 rule: a uniform draw among the
        //   unvisited children, UCT among the visited ones. Same RNG draw
        //   order as v1, so this path is bit-for-bit the v1 engine.
        // * `Policy::Puct` — `Q + c_puct·P·sqrt(N)/(1+n)`: the prior `P`
        //   decides which subtree is explored first, and how long it is
        //   kept. No RNG draw: the path is deterministic.
        let idx = node_idx.expect("the node has just been developed");
        let (edge_idx, mv) = match w.policy {
            Policy::Uct => select_uct(arena, idx, rng, w.c_uct),
            Policy::Puct => select_puct(arena, idx, w.c_puct, rng),
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

/// The **frozen v1 selection**: a uniform draw among the unvisited
/// children, UCT argmax among the visited ones.
///
/// The code is the one of the v1 engine, moved verbatim: the RNG draw
/// order (`next_u64()` exactly once per selection, and only when an
/// unvisited child exists) is what makes the `Policy::Uct` path
/// bit-for-bit identical to v1 — the oracle of the phase-3 differential
/// test.
fn select_uct(arena: &Arena, idx: u32, rng: &mut Rng, c_uct: f32) -> (u32, Move) {
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
            let u = -(e.sum / e.visits as f32) + c_uct * (log_pv / e.visits as f32).sqrt();
            if u > bs {
                bs = u;
                bi = i;
            }
        }
        (arena.node(idx).first_edge + bi as u32, edges[bi].mv)
    }
}

/// The **PUCT selection** (phase 4):
///
/// ```text
/// s(a) = Q(a) + c_puct · P(a) · sqrt(N) / (1 + n(a))
/// ```
///
/// * `Q(a)` — the value of the child **for the mover of the parent**:
///   the sum of the edge is accumulated on the child's side (zero-sum
///   backpropagation), hence the minus sign (same convention as the UCT
///   path).
/// * `P(a)` — the policy prior of the move, computed once at the
///   expansion of the node (`mcts::policy`).
/// * `N` — the number of playouts that went through the node, `n(a)` the
///   number that went through the child.
///
/// The selection runs in **two phases**:
///
/// 1. **breadth** — as long as a child has never been visited, one of
///    them is drawn **uniformly at random** (the v1 rule, one RNG draw).
///    The tree therefore keeps the shape of the v1 tree: every child is
///    tried once before any child is deepened.
/// 2. **depth** — every child has been visited: the PUCT argmax decides.
///
/// The two rules together are what makes the phase 4 search converge
/// faster than the v1 one: the **breadth** is unchanged (the tree covers
/// the same moves), but the **exploration bonus decays as `1/(1+n)`**
/// instead of `1/sqrt(n)`, so the values are trusted sooner — measured,
/// see `docs/mcts/phase4-2026-10-22.md`.
///
/// The formal PUCT (which would compare the `Q = 0` of the unvisited
/// children against the values of the visited ones) is *not* used: with
/// a heuristic prior whose dynamic range is small, its exploration term
/// stays far below the noise of a leaf value, and the search degenerates
/// into a greedy descent — measured, the hit rate of the tactics battery
/// fell from 22/22 to 18/22 at 200 nodes.
fn select_puct(arena: &Arena, idx: u32, c_puct: f32, rng: &mut Rng) -> (u32, Move) {
    let edges = arena.edges_of(idx);
    // Phase 1 — breadth: a **uniform draw among the unvisited children**
    // (the v1 rule). The tree therefore keeps the shape of the UCT tree:
    // every child is tried once before any child is deepened.
    let unv_count = edges.iter().filter(|e| e.visits == 0).count() as u64;
    if unv_count > 0 {
        let pick = (rng.next_u64() % unv_count) as usize;
        let mut k = 0usize;
        for (i, e) in edges.iter().enumerate() {
            if e.visits == 0 {
                if k == pick {
                    return (arena.node(idx).first_edge + i as u32, edges[i].mv);
                }
                k += 1;
            }
        }
        unreachable!("the unvisited count and the unvisited scan agree");
    }
    // Phase 2 — depth: the PUCT argmax.
    let sqrt_pv = (arena.node(idx).visits as f32).sqrt();
    let mut bi = 0usize;
    let mut bs = f32::MIN;
    for (i, e) in edges.iter().enumerate() {
        let n = e.visits as f32;
        let q = -(e.sum / n);
        let s = q + c_puct * e.prior * sqrt_pv / (1.0 + n);
        if s > bs {
            bs = s;
            bi = i;
        }
    }
    (arena.node(idx).first_edge + bi as u32, edges[bi].mv)
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

/// Derive the seed of a parallel worker from the search seed: distinct,
/// deterministic, and never zero (the RNG maps zero to a fixed constant).
fn derive_seed(seed: u64, worker: u64) -> u64 {
    let mut z = seed
        .wrapping_add(worker.wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add(0x2545_F491_4F6C_DD1D);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
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
    // Phase NN : la feuille est la valeur **apprise** quand le réseau est
    // activé, l'évaluation statique sinon (le défaut : le comportement
    // d'avant la phase, bit-à-bit). Le pont centipions est le même
    // dans les deux cas : l'échelle des Q du MCTS ne change pas.
    (crate::nn::leaf_cp(pos) as f32 * EVAL_Q_CP_SCALE).clamp(-EVAL_Q_CLAMP, EVAL_Q_CLAMP)
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

/// The **phase-5** leaf: terminality first (never replaced), then the
/// value of the position *after the exchanges are resolved* when
/// `leaf_q > 0`.
///
/// * `leaf_q == 0` — the **phase-4** behaviour, bit-for-bit: the static
///   evaluation alone (`leaf_value`);
/// * `leaf_q == d > 0` — the quiescence-lite (`mcts::qsearch::qvalue`):
///   the exchanges standing on the leaf are resolved on `d` plies, which
///   removes the *horizon effect*.
///
/// The terminality is checked **first** in both cases: a mate, a
/// stalemate, a draw by the 50-move rule or by triple repetition keep
/// their exact value — the tactical resolution never overrides them.
#[inline(always)]
pub fn leaf_value_q(pos: &Position, hist: &[u64], leaf_q: u32) -> f32 {
    match terminal_value(pos, hist) {
        Some(v) => v,
        None => {
            if leaf_q == 0 {
                eval_normalized(pos)
            } else {
                let cp = qsearch::qvalue(pos, leaf_q) as f32;
                (cp * EVAL_Q_CP_SCALE).clamp(-EVAL_Q_CLAMP, EVAL_Q_CLAMP)
            }
        }
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

    /// **Le générateur doit avoir une période longue.**
    ///
    /// C'est le test de non-répétition qui a démontré le bug du
    /// 2026-10-23 : le triple de décalages avait un `>> 25` au lieu d'un
    /// `<< 25`, et le générateur ne produisait que **quatre** valeurs
    /// répétées. Toute la phase NN (le corpus d'auto-play, la
    /// reproductibilité) en dépendait.
    #[test]
    fn the_rng_has_a_long_period() {
        let mut rng = Rng::new(20261023);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..10_000 {
            seen.insert(rng.next_u64());
        }
        assert_eq!(seen.len(), 10_000, "le RNG a une période courte");
    }

    /// Le générateur est **déterministe** : même graine, même suite.
    #[test]
    fn the_rng_is_deterministic() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    /// Run a search and hand the arena back, for the invariant checks.
    fn build_arena(pos: &Position, hist: &[u64], seed: u64, nodes: u64) -> Arena {
        let worker = Worker {
            pos,
            hist,
            c_uct: std::f32::consts::SQRT_2,
            c_puct: DEFAULT_C_PUCT,
            root_noise: 0.0,
            policy: Policy::Uct,
            leaf_q: 0,
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
            c_puct: DEFAULT_C_PUCT,
            root_noise: 0.0,
            policy: Policy::Uct,
            leaf_q: 0,
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
