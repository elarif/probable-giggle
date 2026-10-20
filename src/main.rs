use mychess::perft::{divide, divide2, perft, perft_detailed, perft_parallel};
use mychess::position::Position;
use std::time::Instant;

fn usage() -> ! {
    eprintln!(
        "usage:\n  mychess uci                              # UCI engine (fastchess driver)\n  mychess perft <fen> <depth> [--detailed]\n  mychess divide <fen> <depth> [--depth2]\n  mychess sweep <fen> <depth> <threads_csv> [--reps N]\n  mychess bench"
    );
    std::process::exit(2);
}

/// Report a bad argument on stderr and exit with the usage code, instead of
/// panicking with a backtrace.
fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("error: {msg}");
    usage();
}

/// Parse a FEN argument, reporting the parse error instead of panicking.
fn parse_fen(fen: &str) -> Position {
    match Position::from_fen(fen) {
        Ok(p) => p,
        Err(e) => fail(format!("invalid FEN {fen:?}: {e:?}")),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
    }
    // M0: record which slider dispatch path is active on this hardware.
    eprintln!("[{}]", mychess::slider::describe());
    match args[1].as_str() {
        "uci" => mychess::uci::run(),
        "perft" => {
            if args.len() < 4 {
                usage();
            }
            let fen = &args[2];
            let depth: u32 = match args[3].parse() {
                Ok(d) => d,
                Err(e) => fail(format!("invalid depth {:?}: {e}", args[3])),
            };
            let detailed = args.iter().any(|a| a == "--detailed");
            let memo_on = args.iter().any(|a| a == "--memo");
            let mut pos = parse_fen(fen);
            let t = Instant::now();
            if memo_on {
                let log2: u32 = args
                    .iter()
                    .position(|a| a == "--memo-log2")
                    .and_then(|i| args.get(i + 1))
                    .map(|s| match s.parse() {
                        Ok(v) => v,
                        Err(e) => fail(format!("invalid --memo-log2 {s:?}: {e}")),
                    })
                    .unwrap_or(28);
                eprintln!(
                    "memo: {} entries x 24B = {:.2} GiB, min_depth=3",
                    1u64 << log2,
                    ((1u64 << log2) * 24) as f64 / (1u64 << 30) as f64
                );
                let mut memo = mychess::memo::Memo::new(log2);
                let n = mychess::memo::perft_memo(&mut pos, depth, &mut memo);
                let s = t.elapsed().as_secs_f64();
                println!(
                    "perft({depth}) = {n}  [memo: fills={} hits={}] [{:.2}s, {:.1} Mnodes/s]",
                    memo.fills,
                    memo.hits,
                    s,
                    n as f64 / s / 1e6
                );
                return;
            }
            if detailed {
                let c = perft_detailed(&mut pos, depth);
                let s = t.elapsed().as_secs_f64();
                println!("depth {depth}");
                println!("nodes            {:>18}", c.nodes);
                println!("captures         {:>18}", c.captures);
                println!("ep               {:>18}", c.ep);
                println!("castles          {:>18}", c.castles);
                println!("promotions       {:>18}", c.promotions);
                println!("checks           {:>18}", c.checks);
                println!("discovery_checks {:>18}", c.discovery_checks);
                println!("double_checks    {:>18}", c.double_checks);
                println!("mates            {:>18}", c.mates);
                println!("[{:.2}s, {:.1} Mnodes/s]", s, c.nodes as f64 / s / 1e6);
            } else {
                let n = perft(&mut pos, depth);
                let s = t.elapsed().as_secs_f64();
                println!(
                    "perft({depth}) = {n}  [{:.2}s, {:.1} Mnodes/s]",
                    s,
                    n as f64 / s / 1e6
                );
            }
        }
        "divide" => {
            if args.len() < 4 {
                usage();
            }
            let fen = &args[2];
            let depth: u32 = match args[3].parse() {
                Ok(d) => d,
                Err(e) => fail(format!("invalid depth {:?}: {e}", args[3])),
            };
            let depth2 = args.iter().any(|a| a == "--depth2");
            let mut pos = parse_fen(fen);
            let t = Instant::now();
            let n = if depth2 {
                divide2(&mut pos, depth)
            } else {
                divide(&mut pos, depth)
            };
            println!(
                "{} = {n}  [{:.2}s]",
                if depth2 { "divide2" } else { "divide" },
                t.elapsed().as_secs_f64()
            );
        }
        "sweep" => {
            // Re-baseline helper: perft over a thread sweep, each leg repeated
            // `reps` times round-robin (A/B/A/B friendly).
            if args.len() < 5 {
                usage();
            }
            let fen = &args[2];
            let depth: u32 = match args[3].parse() {
                Ok(d) => d,
                Err(e) => fail(format!("invalid depth {:?}: {e}", args[3])),
            };
            let threads: Vec<usize> = args[4]
                .split(',')
                .map(|t| match t.trim().parse() {
                    Ok(v) => v,
                    Err(e) => fail(format!("invalid thread count {t:?}: {e}")),
                })
                .collect();
            let reps: usize = args
                .iter()
                .position(|a| a == "--reps")
                .and_then(|i| args.get(i + 1))
                .map(|s| match s.parse() {
                    Ok(v) => v,
                    Err(e) => fail(format!("invalid --reps {s:?}: {e}")),
                })
                .unwrap_or(1);
            let pos = parse_fen(fen);
            for rep in 0..reps {
                for t in &threads {
                    let (n, el) = if *t == 1 {
                        let mut p = pos;
                        let s = Instant::now();
                        let n = perft(&mut p, depth);
                        (n, s.elapsed())
                    } else {
                        let s = Instant::now();
                        let n = perft_parallel(&pos, depth, *t);
                        (n, s.elapsed())
                    };
                    println!(
                        "rep{} threads={:<2} perft({depth}) = {n}  [{:.3}s, {:.1} Mnps]",
                        rep,
                        t,
                        el.as_secs_f64(),
                        n as f64 / el.as_secs_f64() / 1e6
                    );
                }
            }
        }
        "bench" => {
            let startpos = Position::startpos();
            let t = Instant::now();
            let n6 = perft(&mut { startpos }, 6);
            println!(
                "perft(6) 1-thread = {n6}  [{:.2}s, {:.1} Mnodes/s]",
                t.elapsed().as_secs_f64(),
                n6 as f64 / t.elapsed().as_secs_f64() / 1e6
            );
            let threads = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1);
            let t = Instant::now();
            let n7 = perft_parallel(&startpos, 7, threads);
            println!(
                "perft(7) {threads}-thread = {n7}  [{:.2}s, {:.1} Mnodes/s]",
                t.elapsed().as_secs_f64(),
                n7 as f64 / t.elapsed().as_secs_f64() / 1e6
            );
        }
        _ => usage(),
    }
}
