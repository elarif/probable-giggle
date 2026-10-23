//! Chess engine core (board representation, bitboards, legal move generation,
//! make/unmake with incremental Zobrist, FEN, perft — FROZEN at `vcore-1.0`)
//! plus the MCTS phase additions: `eval` (PST hook), `mcts` (UCT v1), `uci`.

pub mod ab;
pub mod corpus;
pub mod eval;
pub mod mcts;
pub mod memo;
pub mod nn;
pub mod movegen;
pub mod perft;
pub mod position;
pub mod refgen;
pub mod slider;
pub mod tables;
pub mod types;
pub mod uci;
pub mod zobrist;

pub use position::Position;
pub use types::{Color, GenKind, Move, PieceKind, Square};
