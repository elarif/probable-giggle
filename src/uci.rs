//! Minimal UCI (STEP 0 deliverable): the only way fastchess can drive the
//! engine for SPRT matches.
//!
//! Supported: `uci`, `isready`, `ucinewgame`, `setoption` (accepted; knobs:
//! `Seed`, `MaxPlayoutPly`, `UCTC`, `Threads`, `Cpuct`, `RootNoise`,
//! `Policy`, `LeafQ`), `position [startpos|fen <fen>] [moves ...]`,
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

use crate::ab;
use crate::mcts::{Limits, Policy, Search};
use crate::movegen;
use crate::position::Position;
use crate::types::{Color, GenKind, Move};
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const DEFAULT_SEED: u64 = 0x1F2E_3D4C_5B6A_7988;
const DEFAULT_MAX_PLAYOUT_PLY: u32 = 100;
const NODE_BUDGET_SENTINEL: u64 = u64::MAX / 4;

/// **Phase 5 — marge de temps.** Le budget de réflexion est toujours
/// *strictement* inférieur au temps annoncé, pour que la réponse (`bestmove`)
/// et l'écriture sur stdout soient encore dans l'horloge quand le
/// controleur l'attend. La phase 4 avait un plancher de 50 ms qui
/// **dépassait** l'horloge (158/200 parties adjugées en TC 1+0.01) ; la
/// phase 5 borne par le bas à 1 ms et retranche cette marge par le haut.
const TIME_MARGIN_MS: u128 = 5;

/// L'algorithme de recherche piloté par l'option UCI `Search`.
///
/// * `Mcts` — le MCTS du projet (le **défaut**, inchangé : toutes les
///   mesures gelées des phases 3–5 restent reproductibles) ;
/// * `Alpha` — l'alpha-beta de `crate::ab` (le moteur de **référence**,
///   `docs/ab/plan-alpha-beta-2026-10-22.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKind {
    Mcts,
    Alpha,
}

impl Default for SearchKind {
    fn default() -> Self {
        SearchKind::Mcts
    }
}

pub struct UciState {
    pos: Position,
    /// Zobrist of every position in the replayed game (UCI replay), the last
    /// entry being the current position.
    history: Vec<u64>,
    seed: u64,
    max_playout_ply: u32,
    c_uct: f32,
    /// PUCT exploration constant (phase 4).
    c_puct: f32,
    /// Scale of the root perturbation in the parallel mode (phase 4).
    root_noise: f32,
    /// The selection rule (phase 4): `puct` (default) or `uct`.
    policy: Policy,
    /// Quiescence-lite depth at the leaf (phase 5): `0` = static
    /// evaluation only (phase-4 behaviour, bit-for-bit), `d > 0` = the
    /// exchanges standing on the leaf are resolved on `d` plies.
    leaf_q: u32,
    /// Number of parallel root workers (phase 3). 1 = mono-thread.
    threads: usize,
    /// The search algorithm (AB): `mcts` (default) or `alpha`.
    search: SearchKind,
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
            c_puct: crate::mcts::DEFAULT_C_PUCT,
            root_noise: crate::mcts::DEFAULT_ROOT_NOISE,
            policy: Policy::default(),
            leaf_q: 0,
            threads: 1,
            search: SearchKind::default(),
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
            out_line(&format!(
                "option name Cpuct type string default {}",
                st.c_puct
            ));
            out_line(&format!(
                "option name RootNoise type string default {}",
                st.root_noise
            ));
            out_line("option name Policy type string default puct");
            out_line(&format!(
                "option name LeafQ type spin default {} min 0 max 8",
                st.leaf_q
            ));
            // L'alpha-beta de référence (`docs/ab/plan-alpha-beta-2026-10-22.md`).
            // Le **défaut reste `mcts`** : les mesures gelées des phases
            // 3–5 (et le tournoi du 2026-10-22) doivent rester
            // reproductibles à l'identique.
            out_line("option name Search type string default mcts");
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
            let (c_puct, root_noise) = (st.c_puct, st.root_noise);
            let policy = st.policy;
            let leaf_q = st.leaf_q;
            let threads = st.threads;
            let kind = st.search;
            st.worker = Some(std::thread::spawn(move || {
                // ---- l'alpha-beta de référence (phase AB) ----
                // `docs/ab/plan-alpha-beta-2026-10-22.md`. Le chemin MCTS
                // ci-dessous est **inchangé** : c'est lui qui porte les
                // mesures gelées des phases 3 à 5.
                if kind == SearchKind::Alpha {
                    let limits = ab::Limits {
                        max_depth: go.depth.unwrap_or(0),
                        movetime_ms: go.movetime_ms,
                        // The node budget is only used when it was
                        // **explicitly** requested (`go nodes N`): the
                        // depth -> nodes mapping of the MCTS must not
                        // become the alpha-beta's node limit.
                        nodes: if go.nodes_given && go.nodes < NODE_BUDGET_SENTINEL {
                            go.nodes
                        } else {
                            u64::MAX
                        },
                    };
                    let search = ab::Search::new(pos, hist, limits);
                    let o = search.run(&stop);
                    // Même contrat que le MCTS : une position jouable
                    // reçoit toujours un coup **légal**.
                    let best = if o.best == Move::NONE {
                        ab::first_legal_move(&pos)
                    } else {
                        o.best
                    };
                    let nps = if o.elapsed_ms > 0 {
                        o.nodes as f64 / (o.elapsed_ms as f64 / 1000.0)
                    } else {
                        o.nodes as f64
                    };
                    let pv: Vec<String> = o.pv.iter().map(|m| m.uci()).collect();
                    println!(
                        "info depth {} score cp {} nodes {} nps {:.0} time {} pv {}",
                        o.depth,
                        o.score_cp,
                        o.nodes,
                        nps,
                        o.elapsed_ms,
                        pv.join(" ")
                    );
                    println!("bestmove {}", move_uci(best));
                    return;
                }
                let search = Search::new(
                    pos,
                    hist,
                    Limits {
                        nodes: go.nodes,
                        movetime_ms: go.movetime_ms,
                        max_playout_ply: mpl,
                        c_uct,
                        c_puct,
                        root_noise,
                        leaf_q,
                    },
                    seed,
                )
                .with_threads(threads)
                .with_policy(policy);
                let o = search.run(&stop);
                // Une recherche **arrêtée** avant de développer la racine ne
                // produit aucun coup. Répondre `0000` dans une position
                // jouable est un **coup illégal** : fastchess (et tout
                // autre arbitre) le compte comme tel, et la partie est
                // perdue sur une position qui était jouable. `0000` n'est
                // une réponse correcte que dans une position **terminale**
                // (mat ou pat), où il n'y a aucun coup à jouer.
                let best = if o.best == Move::NONE {
                    fallback_move(&pos)
                } else {
                    o.best
                };
                let nps = if o.elapsed_ms > 0 {
                    o.nodes as f64 / (o.elapsed_ms as f64 / 1000.0)
                } else {
                    o.nodes as f64
                };
                println!(
                    "info depth {} score cp {} nodes {} nps {:.0} time {} string mcts-v1 policy {} leafq {} threads {}",
                    pseudo_depth(o.nodes),
                    o.score_cp,
                    o.nodes,
                    nps,
                    o.elapsed_ms,
                    match policy {
                        Policy::Puct => "puct",
                        Policy::Uct => "uct",
                    },
                    leaf_q,
                    threads
                );
                println!("bestmove {}", move_uci(best));
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

/// Le coup à jouer quand la recherche n'a rien produit.
///
/// `0000` n'est une réponse UCI correcte que dans une position
/// **terminale** (mat ou pat) : il n'y a alors aucun coup à jouer. Dans
/// une position jouable, `0000` est un **coup illégal** — l'adversaire
/// le signale et la partie est perdue sur une position qui était
/// jouable.
///
/// La recherche peut ne rien produire quand elle est **arrêtée** avant
/// de développer la racine : c'est exactement ce que fait l'arbitre de
/// fastchess quand il constate un dépassement de temps — il envoie
/// `stop` au moteur trop lent et attend son `bestmove`. Le moteur doit
/// alors répondre un coup **légal** (le meilleur qu'il ait), pas `0000`.
///
/// Le repli est le **premier coup légal** de la position : la recherche
/// n'ayant rien produit, il n'y a pas de « meilleur » coup à estimer,
/// et l'ordre des coups légaux est déterministe (donc reproductible).
pub fn fallback_move(pos: &Position) -> Move {
    let list = movegen::generate_all(pos);
    if list.is_empty() {
        // Position terminale : `0000` est la bonne réponse.
        Move::NONE
    } else {
        list.get(0)
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
        ("Cpuct", _) => st.c_puct = v.parse().unwrap_or(st.c_puct),
        ("RootNoise", _) => st.root_noise = v.parse().unwrap_or(st.root_noise),
        ("Policy", _) => {
            st.policy = match v.trim().to_ascii_lowercase().as_str() {
                "uct" => Policy::Uct,
                "puct" => Policy::Puct,
                _ => st.policy,
            }
        }
        ("LeafQ", _) => {
            st.leaf_q = v
                .parse::<u64>()
                .map(|x: u64| x.min(8) as u32)
                .unwrap_or(st.leaf_q)
        }
        ("Threads", _) => {
            st.threads = v
                .parse::<usize>()
                .map(|x: usize| x.clamp(1, 64))
                .unwrap_or(st.threads)
        }
        // L'alpha-beta de référence (`docs/ab/plan-alpha-beta-2026-10-22.md`).
        ("Search", _) => {
            st.search = match v.trim().to_ascii_lowercase().as_str() {
                "alpha" | "alphabeta" | "alpha-beta" => SearchKind::Alpha,
                _ => SearchKind::Mcts,
            }
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

/// The budget decided by `go`: a node budget **or** a wall-clock budget.
/// `movetime_ms == None` means "no wall-clock cap" (`go infinite`).
///
/// Public for the phase-5 tests: the time budget is a **contract** with
/// the opponent's clock, and it must be testable without spawning a
/// process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoLimits {
    pub nodes: u64,
    pub movetime_ms: Option<u128>,
    /// The requested depth (`go depth N`).
    ///
    /// The MCTS path ignores it (it maps a depth to a node budget, its
    /// historical behaviour) ; the **alpha-beta** path uses it as its
    /// maximum depth.
    pub depth: Option<u32>,
    /// Whether the node budget was **explicitly** requested (`go nodes N`).
    ///
    /// It matters for the alpha-beta path: the MCTS maps a *depth* to a
    /// node budget (30 000 nœuds par ply), and that derived budget must
    /// **not** become the alpha-beta's node limit — it would abort the
    /// depth-6 iteration at 180 000 nœuds.
    pub nodes_given: bool,
}

/// Parse a `go` command into a budget.
///
/// **Phase 5 — le budget temps est borné par le temps restant.** Une
/// marge de [`TIME_MARGIN_MS`] est réservée pour l'aller-retour UCI et
/// la première allocation de l'arène ; le plancher de 50 ms de la phase 4
/// (qui faisait réfléchir le moteur **plus longtemps que son horloge**)
/// a disparu.
pub fn parse_go(toks: &[&str], side: &Color) -> GoLimits {
    let mut nodes: u64 = 10_000; // default budget (protocol: `go` must move)
    let mut movetime_ms: Option<u128> = None;
    let mut depth: Option<u32> = None;
    let mut nodes_given = false;
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
                    nodes_given = true;
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
                    // The alpha-beta path takes the depth at face value.
                    depth = Some(v.min(128) as u32);
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
            movetime_ms: Some(mt.saturating_sub(TIME_MARGIN_MS).max(1)),
            depth,
            nodes_given,
        };
    }
    if my_time > 0 {
        // TC: my remaining time / 30 + 2/3 of my increment; capped at 10 s
        // per move.
        let raw = my_time
            .saturating_div(30)
            .saturating_add(my_inc.saturating_mul(2) / 3);
        // **Phase 5 — correctif du budget temps.** Deux règles :
        //
        // 1. le budget ne doit **jamais** dépasser le temps qui reste
        //    (moins la marge) : l'ancien plancher de 50 ms faisait
        //    réfléchir le moteur plus longtemps que son horloge, et le
        //    moteur se faisait flaguer (158/200 parties adjugées sur
        //    dépassement en TC 1+0.01, cf.
        //    `docs/mcts/phase4-2026-10-22.md`) ;
        // 2. quand la formule TC ne donne **rien** (moins de 30 ms de
        //    temps restant et pas d'incrément), on consomme le temps
        //    restant plutôt que de se contenter de 1 ms : c'est le
        //    régime où le moteur flaguait.
        let available = my_time.saturating_sub(TIME_MARGIN_MS).max(1);
        let budget = if raw == 0 {
            available
        } else {
            raw.clamp(1, 10_000).min(available)
        };
        return GoLimits {
            nodes: NODE_BUDGET_SENTINEL,
            movetime_ms: Some(budget),
            depth,
            nodes_given,
        };
    }
    if infinite {
        // Until `stop`: unbounded node sentinel + no wall-clock cap.
        return GoLimits {
            nodes: NODE_BUDGET_SENTINEL,
            movetime_ms: None,
            depth,
            nodes_given,
        };
    }
    GoLimits {
        nodes,
        movetime_ms: None,
        depth,
        nodes_given,
    }
}
