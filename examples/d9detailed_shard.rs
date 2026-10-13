//! Checkpoint-resumable root-split PARALLEL `perft_detailed` runner.
//!
//! Hardening of `d9detailed` for the GATE 2.5 M4 gate run: the workspace was
//! hibernated twice killing the multi-hour run with zero progress persisted
//! (slaml/d9_detailed.log.killed-*). This runner splits the root the exact
//! same way the committed `d9detailed` does (see its docs: `split` levels of
//! move enumeration, subtrees handed to the frozen `perft::perft_detailed`),
//! so the task set is deterministic, but it CHECKPOINTS every completed
//! subtree to `<ckpt>` ("task <idx> <9 counters>", single `write_all` +
//! `sync_data`) and SKIPS loaded tasks on restart. Summing the completed-task
//! ledger over the full deterministic task list is exactly the same fold as
//! the one-shot reducer (integer addition over independent subtrees —
//! shard==serial validated d5/d6/d7 before the gate run).
//!
//! Usage: `mychess-d9detailed-shard <fen> <depth> <threads> <split> <ckpt>`
//! Prints the same table shape as `mychess perft <fen> <depth> --detailed`.
//! Progress goes to stderr as "# progress ..." lines (never starting with a
//! counter keyword, so `reconcile.sh`'s awk `$1 == key` stays unambiguous).

use mychess::movegen::{MoveList, generate};
use mychess::perft::perft_detailed;
use mychess::position::Position;
use mychess::types::{GenKind, Move};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

const NCOUNT: usize = 9;
const NAMES: [&str; 9] = [
    "nodes",
    "captures",
    "ep",
    "castles",
    "promotions",
    "checks",
    "discovery_checks",
    "double_checks",
    "mates",
];

/// Deterministic task list: `split` levels of move enumeration, each task =
/// (position, perft_detailed depth = depth - split).
fn enumerate(pos: &Position, depth_left: u32, split: u32, out: &mut Vec<(Position, u32)>) {
    assert!(depth_left >= split + 1, "split must leave leaf depth >= 1");
    if split == 0 {
        out.push((*pos, depth_left));
        return;
    }
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    for i in 0..list.len() {
        let mv: Move = list.get(i);
        let mut p = *pos;
        p.make(mv);
        enumerate(&p, depth_left - 1, split - 1, out);
    }
}

/// Parse "task <idx> <n> <c> <ep> <cs> <pr> <ch> <dc> <dbl> <mt>" lines.
/// Stops at the first malformed line (a kill can truncate the tail write).
fn load_ckpt(path: &str) -> BTreeMap<usize, [u64; NCOUNT]> {
    let mut map = BTreeMap::new();
    let mut s = String::new();
    if File::open(path).and_then(|mut f| f.read_to_string(&mut s)).is_err() {
        return map;
    }
    for line in s.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() != 2 + NCOUNT || f[0] != "task" {
            eprintln!("# ckpt: stop at malformed/corrupt line (likely tail truncation)");
            break;
        }
        let idx: usize = match f[1].parse() {
            Ok(i) => i,
            Err(_) => {
                eprintln!("# ckpt: bad index, stop");
                break;
            }
        };
        let mut vals = [0u64; NCOUNT];
        let ok = f[2..]
            .iter()
            .zip(vals.iter_mut())
            .all(|(t, v)| match t.parse::<u64>() {
                Ok(x) => {
                    *v = x;
                    true
                }
                Err(_) => false,
            });
        if !ok || f.len() - 2 != NCOUNT {
            eprintln!("# ckpt: bad values, stop");
            break;
        }
        map.insert(idx, vals); // last line wins
    }
    map
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 6 {
        eprintln!("usage: d9detailed-shard <fen> <depth> <threads> <split> <ckpt>");
        std::process::exit(2);
    }
    let fen = args[1].clone();
    let depth: u32 = args[2].parse().expect("depth");
    let threads: usize = args[3].parse().expect("threads");
    let split: u32 = args[4].parse().expect("split");
    let ckpt = args[5].clone();

    eprintln!("[{}]", mychess::slider::describe());
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .build()
        .unwrap();

    let pos = Position::from_fen(&fen).unwrap();
    let mut tasks: Vec<(Position, u32)> = Vec::new();
    enumerate(&pos, depth, split, &mut tasks);
    let total = tasks.len();

    let loaded = load_ckpt(&ckpt);
    let preloaded = loaded.len();
    eprintln!("# resume: {preloaded}/{total} tasks loaded from ckpt ({ckpt})");

    let t0 = Instant::now();
    // Completion ledger: loaded entries ∪ every freshly computed task.
    let ledger = std::sync::Mutex::new(loaded);
    let done = AtomicUsize::new(preloaded);
    let ckpt_file =
        std::sync::Mutex::new(OpenOptions::new().create(true).append(true).open(&ckpt).unwrap());

    pool.install(|| {
        (0..total).into_par_iter().for_each(|idx| {
            if ledger.lock().unwrap().contains_key(&idx) {
                return;
            }
            let (sub, dleft) = tasks[idx];
            let mut p = sub;
            let c = perft_detailed(&mut p, dleft);
            let vals: [u64; NCOUNT] = [
                c.nodes, c.captures, c.ep, c.castles, c.promotions, c.checks,
                c.discovery_checks, c.double_checks, c.mates,
            ];
            // checkpoint line FIRST, ledger insertion AFTER: a crash between
            // the two only costs an idempotent recompute of the task.
            let line = format!(
                "task {} {}\n",
                idx,
                vals.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ")
            );
            {
                let mut g = ckpt_file.lock().unwrap();
                g.write_all(line.as_bytes()).expect("checkpoint append failed");
                g.sync_data().expect("checkpoint sync failed");
            }
            ledger.lock().unwrap().insert(idx, vals);
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            if n % 8 == 0 || n == total {
                eprintln!(
                    "# progress {n}/{total} elapsed {:.1}s",
                    t0.elapsed().as_secs_f64()
                );
            }
        });
    });

    let map = ledger.into_inner().unwrap();
    let mut sum = [0u64; NCOUNT];
    for v in map.values() {
        for (i, v) in v.iter().enumerate() {
            sum[i] += v;
        }
    }

    let s = t0.elapsed().as_secs_f64();
    println!("depth {depth}");
    for (i, name) in NAMES.iter().enumerate() {
        println!("{name:<16} {:>18}", sum[i]);
    }
    println!(
        "[{:.2}s, {:.1} Mnodes/s, tasks {}/{} ({preloaded} ckpt-resumed)]",
        s,
        sum[0] as f64 / s / 1e6,
        map.len(),
        total,
    );
}