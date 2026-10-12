use mychess::perft::{divide, divide2, perft, perft_detailed, perft_parallel};
use mychess::position::Position;
use std::time::Instant;

fn usage() -> ! {
    eprintln!(
        "usage:\n  mychess perft <fen> <depth> [--detailed]\n  mychess divide <fen> <depth> [--depth2]\n  mychess bench"
    );
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
    }
    match args[1].as_str() {
        "perft" => {
            if args.len() < 4 {
                usage();
            }
            let fen = &args[2];
            let depth: u32 = args[3].parse().expect("depth");
            let detailed = args.iter().any(|a| a == "--detailed");
            let mut pos = Position::from_fen(fen).expect("bad fen");
            let t = Instant::now();
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
                println!(
                    "[{:.2}s, {:.1} Mnodes/s]",
                    s,
                    c.nodes as f64 / s / 1e6
                );
            } else {
                let n = perft(&mut pos, depth);
                let s = t.elapsed().as_secs_f64();
                println!("perft({depth}) = {n}  [{:.2}s, {:.1} Mnodes/s]", s, n as f64 / s / 1e6);
            }
        }
        "divide" => {
            if args.len() < 4 {
                usage();
            }
            let fen = &args[2];
            let depth: u32 = args[3].parse().expect("depth");
            let depth2 = args.iter().any(|a| a == "--depth2");
            let mut pos = Position::from_fen(fen).expect("bad fen");
            let t = Instant::now();
            let n = if depth2 { divide2(&mut pos, depth) } else { divide(&mut pos, depth) };
            println!("{} = {n}  [{:.2}s]", if depth2 { "divide2" } else { "divide" }, t.elapsed().as_secs_f64());
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
            let threads =
                std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
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
