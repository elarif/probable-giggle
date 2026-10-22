//! Phase 3 probe — nps and peak RSS of the MCTS search.
//!
//! The probe is **identical in the v1 and the phase-3 trees** (same source,
//! same arguments, same output format), so the two engines can be compared
//! by alternating runs in a single session: the machine is shared and never
//! idle, and an absolute number measured in another session is worthless.
//!
//! ```text
//! cargo run --release --example mcts_probe -- <nodes> <threads> [fen] [time:<ms>]
//! ```
//!
//! With `time:<ms>`, the search runs on a **time** budget instead of a node
//! budget: every worker runs until the shared deadline, so the total number
//! of nodes is the throughput figure (the phase-3 parallel mode).
//!
//! Output (one line, machine-readable):
//!
//! ```text
//! PROBE|nodes|elapsed_ms|nps|vmhwm_kb|best|score|tree_nodes|tree_edges
//! ```
//!
//! `vmhwm_kb` is the **peak** resident set of the process (`VmHWM` of
//! `/proc/self/status`), which is the memory the search actually cost.

use mychess::mcts::{Limits, Search};
use mychess::position::Position;
use std::sync::atomic::AtomicBool;

const DEFAULT_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// Peak resident set of the process, in kB (`VmHWM` of `/proc/self/status`).
fn vm_hwm_kb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmHWM:") {
            let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
            return digits.parse().unwrap_or(0);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let nodes: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(200_000);
    let threads: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let fen = args.get(3).map(String::as_str).unwrap_or(DEFAULT_FEN);
    // Optional `time:<ms>`: switch the search to a wall-clock budget.
    let movetime_ms = args
        .get(4)
        .and_then(|s| s.strip_prefix("time:"))
        .and_then(|s| s.parse().ok());

    let pos = Position::from_fen(fen).expect("valid FEN");
    let limits = Limits {
        nodes: if movetime_ms.is_some() {
            u64::MAX / 4
        } else {
            nodes
        },
        movetime_ms,
        max_playout_ply: 100,
        c_uct: std::f32::consts::SQRT_2,
    };
    let search = Search::new(pos, vec![pos.zobrist], limits, 42);
    let search = if threads > 1 {
        search.with_threads(threads)
    } else {
        search
    };
    let t0 = std::time::Instant::now();
    let o = search.run(&AtomicBool::new(false));
    let elapsed = t0.elapsed();
    let ms = elapsed.as_millis().max(1) as u64;
    let nps = o.nodes as f64 / (elapsed.as_secs_f64().max(1e-9));
    println!(
        "PROBE|{}|{}|{:.0}|{}|{}|{}|{}|{}",
        o.nodes,
        ms,
        nps,
        vm_hwm_kb(),
        o.best.uci(),
        o.score_cp,
        o.tree_nodes,
        o.tree_edges
    );
}
