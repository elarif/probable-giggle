//! Chess engine core: board representation (bitboards), legal move generation,
//! make/unmake with incremental Zobrist, FEN, perft. No search / eval / UCI.

pub mod memo;
pub mod movegen;
pub mod perft;
pub mod position;
pub mod refgen;
pub mod slider;
pub mod tables;
pub mod types;
pub mod zobrist;

pub use position::Position;
pub use types::{Color, GenKind, Move, PieceKind, Square};
