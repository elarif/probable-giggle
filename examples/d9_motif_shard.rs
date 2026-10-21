//! Runner d9 **décomposé par motif de check**, shardé et checkpointe.
//!
//! Le runner `d9detailed_shard` produit les 9 compteurs de la table CPW,
//! mais l'écart d9 porte sur deux colonnes (`discovery_checks` et
//! `double_checks`) et il faut savoir **quel motif** les produit. Ce
//! runner recompte les mêmes classes que `src/perft.rs` :
//!
//! ```text
//! if att.count_ones() >= 2   -> double_checks
//! else if att != moved_bit    -> discovery_checks
//! else                        -> direct
//! ```
//!
//! ...en découpant par motif : prise en passant, roque, et le reste.
//! C'est ce découpage qui doit expliquer l'écart CPW.
//!
//! Usage :
//!   d9motif-shard <fen> <depth> <threads> <split> <ckpt>
//!
//! Checkpoint : une ligne "task <idx> <v1> ... <v10>" par sous-arbre
//! terminé, `write_all` + `sync_data`, repris au restart (meme format que
//! `d9detailed_shard`, mais avec 10 compteurs au lieu de 9).

use mychess::movegen::{MoveList, attackers_to, generate};
use mychess::position::Position;
use mychess::types::GenKind;
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

// 10 compteurs : checks, direct, discovery, double, disc_ep, disc_castle,
// dbl_ep, dbl_castle, mates, ep_taken.
const NCOUNT: usize = 10;
const NAMES: [&str; NCOUNT] = [
    "checks",
    "direct",
    "discovery",
    "double",
    "discovery_ep",
    "discovery_castle",
    "double_ep",
    "double_castle",
    "mates",
    "ep",
];

/// Compteurs d'un sous-arbre.
#[derive(Clone, Copy, Default)]
struct Counts {
    v: [u64; NCOUNT],
}

impl Counts {
    fn add(&mut self, o: &Counts) {
        for i in 0..NCOUNT {
            self.v[i] += o.v[i];
        }
    }
}

/// Descend jusqu'à la feuille et classe chaque check.
fn perft_motif(pos: &mut Position, depth: u32, c: &mut Counts) {
    if depth == 0 {
        c.v[0] += 1;
        return;
    }
    let us = pos.side;
    let opp = us.opp();
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    for mv in list.iter() {
        let m = mv;
        let is_ep = m.is_ep();
        let is_castle = m.is_castle();
        let u = pos.make(m);
        if depth == 1 {
            if is_ep {
                c.v[9] += 1;
            }
            if pos.in_check(opp) {
                c.v[0] += 1;
                let att = attackers_to(pos, pos.king_sq(opp), us);
                let n = att.count_ones();
                if n >= 2 {
                    c.v[3] += 1;
                    if is_ep {
                        c.v[6] += 1;
                    }
                    if is_castle {
                        c.v[7] += 1;
                    }
                } else if att != (1u64 << m.to()) {
                    c.v[2] += 1;
                    if is_ep {
                        c.v[4] += 1;
                    }
                    if is_castle {
                        c.v[5] += 1;
                    }
                } else {
                    c.v[1] += 1;
                }
                let mut replies = MoveList::new();
                generate(pos, &mut replies, GenKind::All);
                if replies.is_empty() {
                    c.v[8] += 1;
                }
            }
        } else {
            perft_motif(pos, depth - 1, c);
        }
        pos.unmake(m, u);
    }
}

/// Liste deterministe de taches : `split` niveaux d'enumeration de coups.
fn enumerate(pos: &Position, depth_left: u32, split: u32, out: &mut Vec<(Position, u32)>) {
    assert!(depth_left > split, "split must leave leaf depth >= 1");
    if split == 0 {
        out.push((*pos, depth_left));
        return;
    }
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    for i in 0..list.len() {
        let mv = list.get(i);
        let mut p = *pos;
        p.make(mv);
        enumerate(&p, depth_left - 1, split - 1, out);
    }
}

fn load_ckpt(path: &str) -> BTreeMap<usize, [u64; NCOUNT]> {
    let mut map = BTreeMap::new();
    let mut s = String::new();
    if File::open(path)
        .and_then(|mut f| f.read_to_string(&mut s))
        .is_err()
    {
        return map;
    }
    for line in s.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() != 2 + NCOUNT || f[0] != "task" {
            eprintln!("# ckpt: stop at malformed/corrupt line");
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
        if !ok {
            eprintln!("# ckpt: bad values, stop");
            break;
        }
        map.insert(idx, vals);
    }
    map
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 6 {
        eprintln!("usage: d9motif-shard <fen> <depth> <threads> <split> <ckpt>");
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

    let mut totals = [0u64; NCOUNT];
    for v in loaded.values() {
        for i in 0..NCOUNT {
            totals[i] += v[i];
        }
    }

    let done = AtomicUsize::new(preloaded);
    let start = Instant::now();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&ckpt)
        .expect("ckpt");

    let pending: Vec<(usize, (Position, u32))> = tasks
        .iter()
        .enumerate()
        .filter(|(i, _)| !loaded.contains_key(i))
        .map(|(i, t)| (i, *t))
        .collect::<Vec<_>>();

    // Checkpoint par lots : chaque tache terminee est ecrite puis
    // synchronisee, donc un kill ne perd que le lot en cours (le run
    // d9 precedent a deja perdu 40 min de cette facon, deux fois).
    //
    // L'ecriture se fait sous un mutex, dans l'ordre des index, pour
    // que le fichier reste relisible meme tronque.
    let ckpt_mutex = std::sync::Mutex::new(());
    let mut results: Vec<(usize, [u64; NCOUNT])> = Vec::with_capacity(pending.len());

    for chunk in pending.chunks(8) {
        let batch: Vec<(usize, [u64; NCOUNT])> = pool.install(|| {
            chunk
                .par_iter()
                .map(|(idx, (p, d))| {
                    let mut pos = *p;
                    let mut c = Counts::default();
                    perft_motif(&mut pos, *d, &mut c);
                    (*idx, c.v)
                })
                .collect()
        });

        let mut lines = String::new();
        for (idx, v) in &batch {
            lines.push_str("task ");
            lines.push_str(&idx.to_string());
            for x in v.iter() {
                lines.push(' ');
                lines.push_str(&x.to_string());
            }
            lines.push('\n');
        }
        {
            let _guard = ckpt_mutex.lock().expect("ckpt lock");
            file.write_all(lines.as_bytes()).expect("ckpt write");
            file.sync_data().expect("ckpt sync");
        }
        results.extend(batch);

        let n = preloaded + results.len();
        done.store(n, Ordering::Relaxed);
        eprintln!(
            "# progress {n}/{total} elapsed {:.1}s",
            start.elapsed().as_secs_f64()
        );
    }

    for (_, v) in &results {
        for i in 0..NCOUNT {
            totals[i] += v[i];
        }
    }

    println!("depth {depth}");
    for i in 0..NCOUNT {
        println!("{:<18} {}", NAMES[i], totals[i]);
    }
    println!(
        "[{:.2}s, tasks {}/{}]",
        start.elapsed().as_secs_f64(),
        done.load(Ordering::Relaxed),
        total
    );
}
