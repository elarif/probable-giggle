//! MCTS v1 (STEP 0 baseline): plain UCT, one-node expansion, random playouts.
//!
//! This is the **point ZÉRO** of the ladder: measurably weak (~1200 or less),
//! the reference for the SPRT table (Rustic-style). Known-weak by design —
//! random playouts fall into shallow traps (Ramanujan et al. 2010); A1
//! replaces the playout by the static eval (`eval::eval_for_mover`).
//!
//! Value convention (`Node::sum`): value for the player **to move AT the
//! node**, in {-1, 0, 1} for v1 playouts. A parent selecting children uses
//! `-(child.avg)` (zero-sum flip). At the leaf boundary, `eval_for_mover`
//! (mover-positive cp) is the exact drop-in for the playout result — that is
//! hook A1, untouched in v1.
//!
//! Terminality (v1, all four + insufficient material): checkmate, stalemate,
//! 50-move (halfmove ≥ 100), **triple repetition via the search hash-history**
//! (game history + descent path + playout path; the repetition test runs at
//! expansion, inside playouts; the walkback pattern costs are the
//! `mcts_rep_walkback16` bench numbers, ~1-9 ns/node), insufficient material.
//! Repetition semantics: a position appearing **≥ 2 times in the history
//! before it** (3rd occurrence) draws. Draw checks that could mask a mate on
//! the same ply run mate check FIRST with the legal-move generate (the
//! 75/50-move auto rules yield to checkmate).
//!
//! Determinism: xorshift64* seeded per `Search` (UCI option `Seed`), no
//! wall-clock in the selection, ties broken by generation order → same seed +
//! same limits ⇒ same bestmove.
//!
//! Hooks designed for later ladder steps (NOT implemented here):
//! * A2: `Child::prior` + PUCT selection (hook `prior(pos, move)`);
//! * A1: leaf value = `eval_for_mover(pos)` instead of the random playout;
//! * B1: tree-parallel (arena + per-node stats made atomic/virtual-loss);
//! * B2: tree reuse between `go`s (arena persists, root shifts);
//! * B3: transpositions (hash→node map);
//! * D1: NN policy/value replace prior hook + eval hook (same call shapes).

use crate::eval;
use crate::movegen::{MoveList, generate};
use crate::position::Position;
use crate::types::{GenKind, Move};

/// Arena node: visits + mover-perspective accumulated value.
pub(crate) struct Node {
    pub visits: u64,
    /// Accumulated value for the side TO MOVE at this node (v1: playout
    /// results in {-1,0,1}).
    pub sum: f32,
    pub expanded: bool,
    /// Fixed value for the mover if the position is terminal (mate/-1,
    /// stalemate/0, 50-move or triple repetition or insufficient material/0).
    pub term: Option<f32>,
    pub children: Vec<Child>,
}

pub(crate) struct Child {
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

#[inline(always)]
fn q_from_parent(child: &Node) -> f32 {
    if child.visits == 0 {
        0.0
    } else {
        -(child.sum / child.visits as f32)
    }
}

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
    /// Playout ply cap (draw at limit in v1 playouts).
    pub max_playout_ply: u32,
    /// UCT exploration constant.
    pub c_uct: f32,
}

pub struct Outcome {
    pub best: Move,
    pub score_cp: i32,
    pub nodes: u64,
    pub elapsed_ms: u128,
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

    /// Run to the node/time budget; single-threaded in v1.
    pub fn run(&self, stop: &std::sync::atomic::AtomicBool) -> Outcome {
        use std::sync::atomic::Ordering;
        let start = std::time::Instant::now();
        let mut rng = Rng::new(self.seed);
        let mut arena = vec![Node::new()];
        // Root terminality (position delivered over by the UCI history).
        let root_term = terminal_value(&self.root_pos, &self.game_hist);
        arena[0].term = root_term;
        if arena[0].term.is_none() {
            arena[0].expanded = false;
        }

        let mut nodes: u64 = 0;
        let mut deadline_reached = false;
        while nodes < self.limits.nodes && !deadline_reached && !stop.load(Ordering::Relaxed) {
            search_once(self, &mut arena, &mut rng);
            nodes += 1;
            if nodes.is_multiple_of(BUDGET_POLL_EVERY)
                && let Some(ms) = self.limits.movetime_ms
                && start.elapsed().as_millis() >= ms
            {
                deadline_reached = true;
            }
        }

        // ROOT node: children exist if expanded; pick most-visited (ties: first).
        // If the root is terminal or unexpanded-with-no-children, best = 0000.
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
            elapsed_ms: start.elapsed().as_millis(),
        }
    }
}

/// One playout = one descent (select/expand/playout/backprop). `arena`
/// local to `run` so no self-borrow conflicts.
fn search_once(search: &Search, arena: &mut Vec<Node>, rng: &mut Rng) {
    let mut p = search.root_pos;
    let mut hist: Vec<u64> = search.game_hist.clone();
    let mut path: Vec<u32> = vec![0];

    loop {
        let idx = *path.last().unwrap();
        let (term, expanded, visits) = {
            let n = &arena[idx as usize];
            (n.term, n.expanded, n.visits)
        };
        if let Some(tv) = term {
            backprop(arena, &path, tv);
            return;
        }
        if !expanded {
            if visits == 0 {
                // First visit: random playout from this position (v1 leaf).
                let r = playout(&p, &mut hist, rng, search.limits.max_playout_ply);
                backprop(arena, &path, r);
                return;
            }
            // EXPAND (one node at a time): terminality first, then children.
            let mut list = MoveList::new();
            generate(&p, &mut list, GenKind::All);
            let tv = if list.is_empty() {
                Some(if p.in_check(p.side) { -1.0 } else { 0.0 }) // mate / stalemate
            } else if p.halfmove >= 100
                || repetition_draw(&p, &hist)
                || eval::insufficient_material(&p)
            {
                Some(0.0)
            } else {
                None
            };
            if let Some(tv) = tv {
                arena[idx as usize].term = Some(tv);
                backprop(arena, &path, tv);
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
            // v1: choose ONE unvisited child uniformly at random.
            let n = arena[idx as usize].children.len();
            let pick = (rng.next_u64() % n as u64) as usize;
            let mv = arena[idx as usize].children[pick].mv;
            let child_node = arena[idx as usize].children[pick].node;
            p.make(mv);
            hist.push(p.zobrist);
            path.push(child_node);
            continue;
        }

        // Selection among expanded children.
        let mut first_unvisited: Option<(Move, u32)> = None;
        let mut unv_count = 0u64;
        {
            let children = &arena[idx as usize].children;
            for c in children.iter() {
                if arena[c.node as usize].visits == 0 {
                    unv_count += 1;
                    if first_unvisited.is_none() {
                        first_unvisited = Some((c.mv, c.node));
                    }
                }
            }
        }
        let (mv, child_node) = if unv_count > 0 {
            // v1: uniform over unvisited.
            let pick = (rng.next_u64() % unv_count) as usize;
            let mut k = 0usize;
            {
                let children = &arena[idx as usize].children;
                for c in children.iter() {
                    if arena[c.node as usize].visits == 0 {
                        if k == pick {
                            first_unvisited = Some((c.mv, c.node));
                            break;
                        }
                        k += 1;
                    }
                }
            }
            first_unvisited.unwrap()
        } else {
            // UCT argmax: q(child for parent's mover) + C*sqrt(ln N / n).
            let pv = arena[idx as usize].visits as f32;
            let log_pv = (pv as f64 + 1.0).ln() as f32;
            let mut bi = 0usize;
            let mut bs = f32::MIN;
            {
                let children = &arena[idx as usize].children;
                for (i, c) in children.iter().enumerate() {
                    let cn = &arena[c.node as usize];
                    let u = -(cn.sum / cn.visits as f32)
                        + search.limits.c_uct * (log_pv / cn.visits as f32).sqrt();
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
        hist.push(p.zobrist);
        path.push(child_node);
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

/// Random playout until terminal or ply limit (v1). `hist` carries the game +
/// descent path hashes and is freely extended (the search ends the iteration
/// right after backprop, the buffer is rebuilt next playout).
fn playout(pos: &Position, hist: &mut Vec<u64>, rng: &mut Rng, max_ply: u32) -> f32 {
    let mut p = *pos;
    for _ in 0..max_ply {
        let mut list = MoveList::new();
        generate(&p, &mut list, GenKind::All);
        if list.is_empty() {
            return if p.in_check(p.side) { -1.0 } else { 0.0 };
        }
        if p.halfmove >= 100 || repetition_draw(&p, hist) || eval::insufficient_material(&p) {
            return 0.0;
        }
        let mv = list.get((rng.next_u64() % list.len() as u64) as usize);
        p.make(mv);
        hist.push(p.zobrist);
    }
    0.0 // playout cap: v1 treats it as a draw
}

/// Terminality of `pos` given the history of positions BEFORE it — the
/// exported face of v1 terminality (tests); expansion uses the same logic.
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
