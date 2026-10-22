//! Diagnostic — **erreur d'évaluation de la feuille** (effet horizon).
//!
//! La feuille du MCTS est évaluée par l'éval statique (matériel + PST) à
//! une position *quelconque* : si cette position est au milieu d'un échange
//! de pièces, l'éval ne voit pas la capture qui va se passer et se trompe.
//! C'est l'équivalent MCTS de l'effet horizon que la quiescence (recherche
//! captures-seulement) résout en alpha-beta.
//!
//! Ce probe mesure l'**erreur de la feuille** :
//!
//! * `v_static` = `eval_for_mover(pos)` — l'éval actuelle ;
//! * `v_q(d)` = la valeur de la même position après avoir **résolu les
//!   captures** sur `d` plies (recherche captures-seulement avec
//!   stand-pat, `mcts::qsearch::qvalue`), toujours du point de vue du
//!   trait.
//!
//! L'écart `|v_static − v_q(d)|` est l'erreur commise par la feuille
//! actuelle. Un corpus de positions est tiré de playouts aléatoires depuis
//! la position initiale (le même type de corpus que `mcts_agreement`).
//!
//! ```text
//! cargo run --release --example leaf_probe -- [positions] [plies_min] [plies_max]
//! ```

use mychess::eval;
use mychess::mcts::qsearch::qvalue;
use mychess::movegen::{MoveList, generate_all, generate_captures};
use mychess::position::Position;
use mychess::types::Move;

/// Générateur pseudo-aléatoire minimal (xorshift64*), suffisant pour
/// l'échantillonnage.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// Un coup légal au hasard.
fn random_move(pos: &Position, rng: &mut Rng) -> Option<Move> {
    let list: MoveList = generate_all(pos);
    if list.is_empty() {
        return None;
    }
    Some(list.as_slice()[rng.below(list.len())])
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n_positions: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(2_000);
    let plies_min: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(6);
    let plies_max: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(28);

    let mut rng = Rng(0x00C0_FFEE_1234_5678);
    let mut sum_abs = [0i64; 3];
    let mut sum_signed = [0i64; 3];
    let mut over_pawn = [0usize; 3];
    let mut over_rook = [0usize; 3];
    let mut with_capture = 0usize;
    let mut sampled = 0usize;
    let mut diffs: [Vec<i32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    // Deux populations : les positions **prises dans un échange** (le
    // dernier coup joué était une capture — c'est là que l'effet horizon
    // mord) et les autres.
    let mut pop: Vec<bool> = Vec::new();

    for _ in 0..n_positions {
        // --- un playout aléatoire, on garde une position au milieu ------
        let mut pos = Position::startpos();
        let plies = plies_min + rng.below(plies_max.saturating_sub(plies_min) + 1);
        let mut ok = true;
        let mut last_was_capture = false;
        for _ in 0..plies {
            match random_move(&pos, &mut rng) {
                Some(mv) => {
                    last_was_capture = mv.is_capture();
                    pos.make(mv);
                }
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        sampled += 1;
        pop.push(last_was_capture);

        let v_static = eval::eval_for_mover(&pos);
        let caps: MoveList = generate_captures(&pos);
        if !caps.is_empty() {
            with_capture += 1;
        }
        for (i, d) in [1u32, 2, 4].iter().enumerate() {
            let v_q = qvalue(&pos, *d);
            let diff = (v_static - v_q).abs() as i64;
            sum_abs[i] += diff;
            sum_signed[i] += (v_static - v_q) as i64;
            if diff > 100 {
                over_pawn[i] += 1;
            }
            if diff > 500 {
                over_rook[i] += 1;
            }
            diffs[i].push(diff as i32);
        }
    }

    println!("positions echantillonnees : {sampled}");
    println!(
        "positions avec au moins une capture disponible : {with_capture} ({:.1} %)",
        100.0 * with_capture as f64 / sampled.max(1) as f64
    );
    println!();
    println!(
        "{:<6} {:>12} {:>12} {:>12} {:>10} {:>10}",
        "prof", "ecart moyen", "ecart median", "biais moyen", ">1 pion", ">1 tour"
    );
    for (i, d) in [1u32, 2, 4].iter().enumerate() {
        let mean = sum_abs[i] as f64 / sampled.max(1) as f64;
        let bias = sum_signed[i] as f64 / sampled.max(1) as f64;
        let mut v = diffs[i].clone();
        v.sort_unstable();
        let median = if v.is_empty() {
            0.0
        } else {
            v[v.len() / 2] as f64
        };
        println!(
            "{:<6} {:>12.1} {:>12.1} {:>12.1} {:>9.1}% {:>9.1}%",
            format!("q{d}"),
            mean,
            median,
            bias,
            100.0 * over_pawn[i] as f64 / sampled.max(1) as f64,
            100.0 * over_rook[i] as f64 / sampled.max(1) as f64
        );
    }
    println!();
    println!("ecart moyen et biais en centipions (positif = l'eval statique surestime)");

    // --- les positions prises dans un échange (effet horizon) -------------
    let n_in = pop.iter().filter(|&&x| x).count();
    let n_out = sampled - n_in;
    if n_in > 0 {
        println!();
        println!("positions prises dans un echange (dernier coup = capture) : {n_in}");
        for (i, d) in [1u32, 2, 4].iter().enumerate() {
            let sel: Vec<i64> = diffs[i]
                .iter()
                .zip(pop.iter())
                .filter(|&(_, &p)| p)
                .map(|(d, _)| *d as i64)
                .collect();
            let mean = sel.iter().sum::<i64>() as f64 / sel.len().max(1) as f64;
            let big = sel.iter().filter(|&&x| x > 500).count();
            println!(
                "  q{d} : ecart moyen {mean:8.1} cp   > 1 tour : {:5.1} %",
                100.0 * big as f64 / sel.len().max(1) as f64
            );
        }
    }
    if n_out > 0 {
        println!();
        println!("positions hors echange : {n_out}");
        for (i, d) in [1u32, 2, 4].iter().enumerate() {
            let sel: Vec<i64> = diffs[i]
                .iter()
                .zip(pop.iter())
                .filter(|&(_, &p)| !p)
                .map(|(d, _)| *d as i64)
                .collect();
            let mean = sel.iter().sum::<i64>() as f64 / sel.len().max(1) as f64;
            let big = sel.iter().filter(|&&x| x > 500).count();
            println!(
                "  q{d} : ecart moyen {mean:8.1} cp   > 1 tour : {:5.1} %",
                100.0 * big as f64 / sel.len().max(1) as f64
            );
        }
    }
}
