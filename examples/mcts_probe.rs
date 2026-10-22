//! Phase 3 probe — nps and peak RSS of the MCTS search.
//!
//! The probe is **identical in the v1 and the phase-3 trees** (same source,
//! same arguments, same output format), so the two engines can be compared
//! by alternating runs in a single session: the machine is shared and never
//! idle, and an absolute number measured in another session is worthless.
//!
//! ```text
//! cargo run --release --example mcts_probe -- <nodes> <threads> [fen] [time:<ms>] [policy]
//! ```
//!
//! With `time:<ms>`, the search runs on a **time** budget instead of a node
//! budget: every worker runs until the shared deadline, so the total number
//! of nodes is the throughput figure (the phase-3 parallel mode).
//!
//! `policy` is `puct` (the phase-4 default) or `uct` (the frozen v1
//! selection); the two are measured **in the same session**, so the
//! comparison is not polluted by a machine that drifted between runs.
//!
//! `leafq:<d>` (phase 5) sets the depth of the **tactical resolution of
//! the leaf** (`0` = the static evaluation alone, i.e. the phase-4
//! behaviour). The option is recognised **wherever** it appears in the
//! command line.
//!
//! Output (one line, machine-readable):
//!
//! ```text
//! PROBE|nodes|elapsed_ms|nps|vmhwm_kb|best|score|tree_nodes|tree_edges|policy|leaf_q
//! ```
//!
//! `vmhwm_kb` is the **peak** resident set of the process (`VmHWM` of
//! `/proc/self/status`), which is the memory the search actually cost.

use mychess::mcts::{Limits, Policy, Search};
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
    // The selection rule: `puct` (phase 4, the default) or `uct` (the
    // frozen v1 rule).
    let policy = match args.get(5).map(String::as_str) {
        Some("uct") => Policy::Uct,
        _ => Policy::Puct,
    };
    let policy_name = match policy {
        Policy::Uct => "uct",
        Policy::Puct => "puct",
    };
    // Phase 5: the depth of the tactical resolution of the leaf. The
    // option is recognised wherever it appears.
    let leaf_q: u32 = args
        .iter()
        .find_map(|a| a.strip_prefix("leafq:"))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

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
        c_puct: mychess::mcts::DEFAULT_C_PUCT,
        root_noise: mychess::mcts::DEFAULT_ROOT_NOISE,
        leaf_q,
    };
    let search = Search::new(pos, vec![pos.zobrist], limits, 42).with_policy(policy);
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
        "PROBE|{}|{}|{:.0}|{}|{}|{}|{}|{}|{policy_name}|{leaf_q}",
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
