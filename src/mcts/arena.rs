//! Flat arena for the MCTS tree (phase 3).
//!
//! The v1 tree stored, for every expanded node, a `Vec<Child>` and, for every
//! legal move of that node, a **complete child node** — even the children that
//! were never developed. The analysis (`docs/analyse_movegen_mcts.md` §7) put
//! that waste at ~280 bytes per simulation.
//!
//! This module replaces that with two POD arenas:
//!
//! * [`Node`] — the statistics of a position **plus** the range of edges it
//!   owns. A node owns a *contiguous* range of the edge arena, so the
//!   selection scan walks memory linearly.
//! * [`Edge`] — one move of a node, the statistics of the **child** position
//!   (`visits`, `sum`), and the index of the child node (`NO_CHILD` while the
//!   child has not been developed).
//!
//! Two invariants govern the whole structure, and both are checked by the
//! unit tests of `src/mcts.rs`:
//!
//! 1. **Contiguity** — the edges of a node are `first_edge..first_edge +
//!    n_edges`, and a node owns no edge before its expansion.
//! 2. **Mirror** — the statistics of a child node are the copy of the
//!    statistics of the edge that leads to it (`node.visits == edge.visits`,
//!    `node.sum == edge.sum`). The edge is the single source of truth; the
//!    node is its mirror, and it exists only once the child is *developed*.
//!
//! The child node is allocated lazily, at the second visit of its edge: the
//! first visit is a leaf (the v1 "first visit scores the leaf" rule), so a
//! node that is never developed costs nothing but its edge.
//!
//! No `unsafe`: both arenas are `Vec`s of `#[repr(C)]` plain-old-data.

use crate::types::Move;

/// The edge leads to a child that has not been developed yet.
pub(crate) const NO_CHILD: u32 = u32::MAX;

/// The node is expanded: it owns a contiguous range of edges.
pub(crate) const F_EXPANDED: u8 = 0b01;
/// The node is terminal: `term` holds its fixed value.
pub(crate) const F_TERMINAL: u8 = 0b10;

/// Index of the root node (the arena always starts with it).
pub(crate) const ROOT: u32 = 0;

/// A node of the search tree: statistics + owned edges + terminality.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct Node {
    /// Playouts that went through this node.
    pub visits: u32,
    /// Accumulated value **for the side to move at this node**.
    pub sum: f32,
    /// Fixed value of a terminal node (mate −1, stalemate/draw 0).
    pub term: f32,
    /// Index of the first edge owned by this node.
    pub first_edge: u32,
    /// Number of edges owned by this node.
    pub n_edges: u8,
    /// `F_EXPANDED | F_TERMINAL`.
    pub flags: u8,
    _pad: [u8; 2],
}

impl Node {
    pub fn new() -> Node {
        Node {
            visits: 0,
            sum: 0.0,
            term: 0.0,
            first_edge: 0,
            n_edges: 0,
            flags: 0,
            _pad: [0; 2],
        }
    }

    #[inline(always)]
    pub fn is_expanded(&self) -> bool {
        self.flags & F_EXPANDED != 0
    }

    #[inline(always)]
    pub fn is_terminal(&self) -> bool {
        self.flags & F_TERMINAL != 0
    }

    /// Mark the node terminal with the given value.
    #[inline]
    pub fn set_terminal(&mut self, value: f32) {
        self.term = value;
        self.flags |= F_TERMINAL;
    }
}

/// An edge of the search tree: one move of a node + the child's statistics.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct Edge {
    /// The move from the parent node to the child position.
    pub mv: Move,
    /// Index of the child node, or [`NO_CHILD`] while it is undeveloped.
    pub child: u32,
    /// Playouts that used this edge (= the child's `visits`, mirror invariant).
    pub visits: u32,
    /// Accumulated value **for the side to move in the child**.
    pub sum: f32,
    /// Policy prior (hook A2, câblé en phase 4): la probabilité du coup
    /// dans la distribution de prior du nœud parent. Utilisé par la
    /// sélection PUCT ; vaut 0 sur le chemin UCT (qui l'ignore).
    pub prior: f32,
}

impl Edge {
    fn new(mv: Move, prior: f32) -> Edge {
        Edge {
            mv,
            child: NO_CHILD,
            visits: 0,
            sum: 0.0,
            prior,
        }
    }
}

/// The two flat arenas of the search tree.
pub(crate) struct Arena {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

impl Arena {
    /// A fresh arena holding only the root node.
    pub fn new() -> Arena {
        Arena {
            nodes: vec![Node::new()],
            edges: Vec::new(),
        }
    }

    #[inline(always)]
    pub fn node(&self, i: u32) -> &Node {
        &self.nodes[i as usize]
    }

    #[inline(always)]
    pub fn node_mut(&mut self, i: u32) -> &mut Node {
        &mut self.nodes[i as usize]
    }

    #[inline(always)]
    pub fn edge(&self, i: u32) -> &Edge {
        &self.edges[i as usize]
    }

    #[inline(always)]
    pub fn edge_mut(&mut self, i: u32) -> &mut Edge {
        &mut self.edges[i as usize]
    }

    /// The edges owned by `node`, in generation order (contiguous range).
    #[inline]
    pub fn edges_of(&self, node: u32) -> &[Edge] {
        let n = self.nodes[node as usize];
        let start = n.first_edge as usize;
        &self.edges[start..start + n.n_edges as usize]
    }

    /// Allocate a new (undeveloped) node and return its index.
    pub fn alloc_node(&mut self) -> u32 {
        self.nodes.push(Node::new());
        (self.nodes.len() - 1) as u32
    }

    /// Attach `moves` to `node` as its edges, each with its **policy
    /// prior**. The edges are appended at the end of the edge arena, so the
    /// range stays contiguous.
    pub fn add_edges<I: IntoIterator<Item = (Move, f32)>>(&mut self, node: u32, moves: I) {
        let first = self.edges.len() as u32;
        let mut n = 0u8;
        for (mv, prior) in moves {
            self.edges.push(Edge::new(mv, prior));
            n += 1;
        }
        let nd = &mut self.nodes[node as usize];
        nd.first_edge = first;
        nd.n_edges = n;
        nd.flags |= F_EXPANDED;
    }

    /// Number of nodes in the arena (root included).
    pub fn len_nodes(&self) -> usize {
        self.nodes.len()
    }

    /// Number of edges in the arena.
    pub fn len_edges(&self) -> usize {
        self.edges.len()
    }
}
