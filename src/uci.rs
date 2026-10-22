//! Minimal UCI (STEP 0 deliverable): the only way fastchess can drive the
//! engine for SPRT matches.
//!
//! Supported: `uci`, `isready`, `ucinewgame`, `setoption` (accepted; knobs:
//! `Seed`, `MaxPlayoutPly`, `UCTC`, `Threads`), `position [startpos|fen <fen>] [moves ...]`,
//! `go [nodes N] [movetime T] [wtime W btime B winc I binc I] [infinite] [depth N]`,
//! `stop`, `quit`; `debug`/`ponder`/other tolerated as no-ops.
//!
//! v1 design notes:
//! * searches run on a worker thread; `stop` sets an AtomicBool polled every
//!   64 playouts; `isready` answers immediately (both protocol-correct);
//! * the game history (played moves) is replayed move-by-move through the
//!   frozen core and its Zobrist chain feeds the v1 triple-repetition rule;
//! * `go nodes N` = the fixed-nodes control used by the SPRT ladder (each
//!   playout counts as one node);
//! * tree is rebuilt per `go` (B2 adds reuse).

use crate::mcts::{Limits, Search};
use crate::movegen;
use crate::position::Position;
use crate::types::{Color, GenKind, Move};
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const DEFAULT_SEED: u64 = 0x1F2E_3D4C_5B6A_7988;
const DEFAULT_MAX_PLAYOUT_PLY: u32 = 100;
const NODE_BUDGET_SENTINEL: u64 = u64::MAX / 4;

pub struct UciState {
    pos: Position,
    /// Zobrist of every position in the replayed game (UCI replay), the last
    /// entry being the current position.
    history: Vec<u64>,
    seed: u64,
    max_playout_ply: u32,
    c_uct: f32,
    /// Number of parallel root workers (phase 3). 1 = mono-thread.
    threads: usize,
    worker: Option<std::thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}

impl Default for UciState {
    fn default() -> Self {
        let pos = Position::startpos();
        UciState {
            history: vec![pos.zobrist],
            pos,
            seed: DEFAULT_SEED,
            max_playout_ply: DEFAULT_MAX_PLAYOUT_PLY,
            c_uct: std::f32::consts::SQRT_2,
            threads: 1,
            worker: None,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Run the UCI loop over stdin.
pub fn run() {
    use std::io::BufRead;
    let stdin = std::io::stdin();
    let mut st = UciState::default();
    for line in stdin.lock().lines() {
        let line = line.unwrap_or_default();
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !handle(&mut st, line) {
            break;
        }
    }
    st.stop.store(true, Ordering::Relaxed);
    if let Some(w) = st.worker.take() {
        let _ = w.join();
    }
}

/// One-shot stdout write: lock held ONLY for the line (the search worker
/// prints to stdout concurrently; holding it across a `quit`-join would
/// deadlock both sides).
fn out_line(line: &str) {
    let mut o = std::io::stdout().lock();
    let _ = writeln!(o, "{line}");
    let _ = o.flush();
}

fn handle(st: &mut UciState, line: &str) -> bool {
    let toks: Vec<&str> = line.split_whitespace().collect();
    match toks.first().copied() {
        Some("uci") => {
            out_line("id name mychess-mcts v1");
            out_line("id author probable-giggle phase-MCTS");
            out_line(&format!(
                "option name Seed type string default {:#x}",
                st.seed
            ));
            out_line(&format!(
                "option name MaxPlayoutPly type spin default {} min 10 max 1000",
                st.max_playout_ply
            ));
            out_line(&format!(
                "option name UCTC type string default {}",
                st.c_uct
            ));
            out_line(&format!(
                "option name Threads type spin default {} min 1 max 64",
                st.threads
            ));
            out_line("uciok");
        }
        Some("isready") => {
            out_line("readyok");
        }
        Some("ucinewgame") => {
            stop_worker(st);
            st.pos = Position::startpos();
            st.history = vec![st.pos.zobrist];
        }
        Some("setoption") => set_option(st, &toks),
        Some("position") => position(st, &toks),
        Some("go") => {
            stop_worker(st);
            let go = parse_go(&toks, &st.pos.side);
            st.stop = Arc::new(AtomicBool::new(false));
            let stop = st.stop.clone();
            let pos = st.pos;
            let hist = st.history.clone();
            let (seed, mpl, c_uct) = (st.seed, st.max_playout_ply, st.c_uct);
            let threads = st.threads;
            st.worker = Some(std::thread::spawn(move || {
                let search = Search::new(
                    pos,
                    hist,
                    Limits {
                        nodes: go.nodes,
                        movetime_ms: go.movetime_ms,
                        max_playout_ply: mpl,
                        c_uct,
                    },
                    seed,
                )
                .with_threads(threads);
                let o = search.run(&stop);
                let nps = if o.elapsed_ms > 0 {
                    o.nodes as f64 / (o.elapsed_ms as f64 / 1000.0)
                } else {
                    o.nodes as f64
                };
                println!(
                    "info depth {} score cp {} nodes {} nps {:.0} time {} string mcts-v1 UCT threads {}",
                    pseudo_depth(o.nodes),
                    o.score_cp,
                    o.nodes,
                    nps,
                    o.elapsed_ms,
                    threads
                );
                println!("bestmove {}", move_uci(o.best));
            }));
        }
        Some("stop") => st.stop.store(true, Ordering::Relaxed),
        Some("quit") => {
            stop_worker(st);
            return false;
        }
        Some("debug") | Some("ponder") | Some("register") | Some("wait") | None => {}
        Some(_) => {} // tolerant: unknown commands ignored
    }
    true
}

fn stop_worker(st: &mut UciState) {
    if let Some(w) = st.worker.take() {
        st.stop.store(true, Ordering::Relaxed);
        let _ = w.join();
    }
}

/// sqrt(visits) as a stable depth proxy (v1 has no ply depth).
fn pseudo_depth(nodes: u64) -> u64 {
    (nodes as f64).sqrt().round() as u64
}

fn move_uci(m: Move) -> String {
    if m == Move::NONE {
        "0000".to_string()
    } else {
        m.uci()
    }
}

fn set_option(st: &mut UciState, toks: &[&str]) {
    // setoption name <Name> value <Value...>
    let (mut n, mut v) = (String::new(), String::new());
    let mut mode = 0; // 0: before name, 1: reading name, 2: reading value
    for &t in &toks[1..] {
        match (mode, t) {
            (0, "name") => mode = 1,
            (1, "value") | (0, "value") => mode = 2,
            (1, t) => {
                if !n.is_empty() {
                    n.push(' ');
                }
                n.push_str(t);
            }
            (2, t) => {
                if !v.is_empty() {
                    v.push(' ');
                }
                v.push_str(t);
            }
            _ => {}
        }
    }
    match (n.as_str(), v.as_str()) {
        ("Seed", _) => {
            st.seed = u64::from_str_radix(v.trim_start_matches("0x"), 16)
                .or_else(|_| v.parse())
                .unwrap_or(st.seed)
        }
        ("MaxPlayoutPly", _) => {
            st.max_playout_ply = v
                .parse()
                .map(|x: u64| x.min(1000) as u32)
                .unwrap_or(st.max_playout_ply)
        }
        ("UCTC", _) => st.c_uct = v.parse().unwrap_or(st.c_uct),
        ("Threads", _) => {
            st.threads = v
                .parse::<usize>()
                .map(|x: usize| x.clamp(1, 64))
                .unwrap_or(st.threads)
        }
        (name, value) => eprintln!("uci: ignored option {name:?} = {value:?}"),
    }
}

fn position(st: &mut UciState, toks: &[&str]) {
    let mut pos: Option<Position> = None;
    let mut moves_from = None;
    if toks.get(1) == Some(&"startpos") {
        pos = Some(Position::startpos());
        if toks.get(2) == Some(&"moves") {
            moves_from = Some(3);
        }
    } else if toks.get(1) == Some(&"fen") {
        let fen_end = toks
            .iter()
            .position(|&t| t == "moves")
            .unwrap_or(toks.len());
        // FEN = piece placement + side + castling + ep (+ optional counters
        // which the engine core tolerates as missing).
        let fen = toks[2..fen_end.max(3)].join(" ");
        match Position::from_fen(&fen) {
            Ok(p) => pos = Some(p),
            Err(e) => {
                eprintln!("uci: invalid fen ({e:?})");
                return;
            }
        }
        if fen_end < toks.len() {
            moves_from = Some(fen_end + 1);
        }
    }
    if let Some(mut p) = pos {
        st.history = vec![p.zobrist];
        if let Some(from) = moves_from {
            for &tok in toks.get(from..).unwrap_or(&[]) {
                apply_move(&mut p, &mut st.history, tok);
            }
        }
        st.pos = p;
    }
}

fn apply_move(p: &mut Position, hist: &mut Vec<u64>, tok: &str) {
    let mut list = movegen::MoveList::new();
    movegen::generate(p, &mut list, GenKind::All);
    for i in 0..list.len() {
        let m = list.get(i);
        if m.uci() == tok {
            p.make(m);
            hist.push(p.zobrist);
            return;
        }
    }
    eprintln!("uci: unknown move {tok}"); // tolerated, no crash
}

struct GoLimits {
    nodes: u64,
    movetime_ms: Option<u128>,
}

fn parse_go(toks: &[&str], side: &Color) -> GoLimits {
    let mut nodes: u64 = 10_000; // default budget (protocol: `go` must move)
    let mut movetime_ms: Option<u128> = None;
    let (mut my_time, mut my_inc): (u128, u128) = (0, 0);
    let mut is_white = *side == Color::White;
    let mut infinite = false;
    let mut i = 1;
    while i < toks.len() {
        let key = toks[i];
        let next = toks.get(i + 1).copied();
        let val = next.and_then(|v| v.parse::<u128>().ok());
        match key {
            "nodes" => {
                if let Some(v) = val {
                    nodes = v.min(NODE_BUDGET_SENTINEL as u128) as u64;
                }
                i += 2;
            }
            "movetime" => {
                movetime_ms = val;
                i += 2;
            }
            "wtime" => {
                if is_white {
                    my_time = val.unwrap_or(0);
                }
                i += 2;
            }
            "btime" => {
                if !is_white {
                    my_time = val.unwrap_or(0);
                }
                i += 2;
            }
            "winc" => {
                if is_white {
                    my_inc = val.unwrap_or(0);
                }
                i += 2;
            }
            "binc" => {
                if !is_white {
                    my_inc = val.unwrap_or(0);
                }
                i += 2;
            }
            "white" => {
                is_white = true;
                i += 1;
            }
            "black" => {
                is_white = false;
                i += 1;
            }
            "infinite" => {
                infinite = true;
                i += 1;
            }
            "depth" => {
                if let Some(v) = val {
                    // generous mapping, v1: ~30k playouts per "ply depth"
                    nodes = v.min(1_000_000).saturating_mul(30_000) as u64;
                }
                i += 2;
            }
            _ => {
                i += 1; // tolerate ponder/perft/searchmoves…
            }
        }
    }
    if let Some(mt) = movetime_ms {
        return GoLimits {
            nodes: NODE_BUDGET_SENTINEL,
            movetime_ms: Some(mt),
        };
    }
    if my_time > 0 {
        // TC: my remaining time / 30 + 2/3 of my increment; capped at 10 s
        // per move (safe margins under fastchess wtime/btime, no flagging).
        let budget = my_time
            .saturating_div(30)
            .saturating_add(my_inc.saturating_mul(2) / 3)
            .clamp(50, 10_000);
        return GoLimits {
            nodes: NODE_BUDGET_SENTINEL,
            movetime_ms: Some(budget),
        };
    }
    if infinite {
        // Until `stop`: unbounded node sentinel + no wall-clock cap.
        return GoLimits {
            nodes: NODE_BUDGET_SENTINEL,
            movetime_ms: None,
        };
    }
    GoLimits {
        nodes,
        movetime_ms: None,
    }
}
