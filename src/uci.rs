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
/// Le chemin **par défaut** du fichier de poids de la valeur apprise
/// (phase NN). Un chemin relatif est relatif au répertoire courant du
/// moteur : un duel doit donc passer un chemin **absolu**.
const DEFAULT_NN_FILE: &str = "models/value_v1.bin";
const NODE_BUDGET_SENTINEL: u64 = u64::MAX / 4;

/// Le budget de nœuds **par défaut** d'un `go` sans budget explicite.
///
/// C'est le même que celui du MCTS (10 000 nœuds) : un `go` nu doit
/// faire jouer le moteur, mais **borné** — un `go` nu ne doit pas
/// transformer l'alpha-beta en recherche infinie.
const DEFAULT_NODE_BUDGET: u64 = 10_000;

/// **Phase 5 — marge de temps.** Le budget de réflexion est toujours
/// *strictement* inférieur au temps annoncé, pour que la réponse (`bestmove`)
/// et l'écriture sur stdout soient encore dans l'horloge quand le
/// controleur l'attend. La phase 4 avait un plancher de 50 ms qui
/// **dépassait** l'horloge (158/200 parties adjugées en TC 1+0.01) ; la
/// phase 5 borne par le bas à 1 ms et retranche cette marge par le haut.
const TIME_MARGIN_MS: u128 = 5;

/// Le **plafond temporel** d'un `go depth N`.
///
/// `go depth N` n'est pas un budget de temps : c'est un budget de
/// *profondeur*. Mais la profondeur est traduite en un budget de nœuds
/// (30 000 par ply pour le MCTS), et `go depth 1000` demandait 30 M de
/// nœuds — ~25 s mesurés, sans réponse. Le plafond est celui du budget
/// TC (10 s par coup) : il ne mord pas sur les usages normaux
/// (`go depth 4`, `go depth 5`) et borne les depths absurdes.
const DEPTH_TIME_CAP_MS: u128 = 10_000;

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
    /// Phase AB7 : l'amplitude du bruit de la valeur (en centipions).
    /// `0` = pas de bruit (le défaut, et l'état bit-à-bit d'avant).
    noise_cp: u32,
    /// Phase AB7 : la graine du champ de bruit, **indépendante** de la
    /// graine de recherche.
    noise_seed: u64,
    /// Phase AB8 : les **options de recherche** de l'alpha-beta — chaque
    /// heuristique est activable **seule**, pour être mesurée seule.
    ab_options: ab::Options,
    /// Phase NN : le chemin du fichier de poids de la valeur apprise.
    /// Il doit être fixé **avant** `EvalNN=1`.
    nn_file: String,
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
            noise_cp: 0,
            noise_seed: crate::eval::DEFAULT_NOISE_SEED,
            ab_options: ab::Options::default(),
            nn_file: DEFAULT_NN_FILE.to_string(),
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
            // Phase AB7 : le bruit de la valeur (le dispositif causal).
            // `EvalNoise` est l'amplitude **en centipions** ; `0` = pas de
            // bruit (le défaut : les mesures gelées des phases 3 à 5
            // restent bit-à-bit).
            out_line("option name EvalNoise type spin default 0 min 0 max 2000");
            out_line(&format!(
                "option name EvalNoiseSeed type string default {:#x}",
                st.noise_seed
            ));
            // Phase AB8 : les options de recherche de l'alpha-beta. Les
            // trois premières sont **actives par défaut** (c'est le chemin
            // de la phase AB gelée) ; les trois autres sont **inactives** :
            // chacune doit gagner sa place sur des mesures.
            out_line("option name AbPvs type spin default 1 min 0 max 1");
            out_line("option name AbKillers type spin default 1 min 0 max 1");
            out_line("option name AbHistory type spin default 1 min 0 max 1");
            out_line("option name AbNullMove type spin default 0 min 0 max 1");
            out_line("option name AbRazoring type spin default 0 min 0 max 1");
            out_line("option name AbFutility type spin default 0 min 0 max 1");
            out_line("option name AbLmr type spin default 0 min 0 max 1");
            // Phase NN : la **valeur apprise**. `EvalNN` vaut 0 par
            // défaut : le moteur se comporte alors **bit-à-bit** comme
            // avant la phase (la valeur statique reste la référence).
            // `EvalNNFile` est le chemin du fichier de poids ; il doit
            // être fixé **avant** `EvalNN=1`.
            out_line("option name EvalNN type spin default 0 min 0 max 1");
            out_line(&format!(
                "option name EvalNNFile type string default {}",
                st.nn_file
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
            let (c_puct, root_noise) = (st.c_puct, st.root_noise);
            let policy = st.policy;
            let leaf_q = st.leaf_q;
            let threads = st.threads;
            let kind = st.search;
            let ab_options = st.ab_options;
            st.worker = Some(std::thread::spawn(move || {
                // ---- l'alpha-beta de référence (phase AB) ----
                // `docs/ab/plan-alpha-beta-2026-10-22.md`. Le chemin MCTS
                // ci-dessous est **inchangé** : c'est lui qui porte les
                // mesures gelées des phases 3 à 5.
                if kind == SearchKind::Alpha {
                    // Le budget de l'alpha-beta est **toujours borné**.
                    //
                    // Avant, un `go` **sans budget** (ni temps, ni nœuds,
                    // ni profondeur) transmettait `u64::MAX` de nœuds et
                    // une profondeur de 64 : la recherche ne s'arrêtait
                    // jamais (mesuré : 100 M de nœuds, 25 s, et toujours
                    // pas de `bestmove`). Le MCTS, lui, applique un budget
                    // par défaut de 10 000 nœuds : les deux moteurs
                    // doivent obéir au **même** contrat — `go` sans
                    // budget = budget par défaut.
                    //
                    // `go depth N` reste un budget **de profondeur** (les
                    // tests de la phase AB s'appuient dessus), mais la
                    // profondeur est plafonnée par `MAX_PLY` : au-delà,
                    // la recherche rend l'évaluation statique sans
                    // regarder la position (les itérations 97+ sont
                    // vides).
                    let depth_req = go.depth.filter(|d| *d > 0);
                    let limits = ab::Limits {
                        max_depth: depth_req.map(|d| d.min(ab::MAX_PLY as u32)).unwrap_or(0),
                        movetime_ms: go.movetime_ms,
                        // The node budget is only used when it was
                        // **explicitly** requested (`go nodes N`): the
                        // depth -> nodes mapping of the MCTS must not
                        // become the alpha-beta's node limit. When a
                        // *time* budget is given, the time is the bound
                        // and the node count is free.
                        nodes: if go.movetime_ms.is_some() || depth_req.is_some() {
                            u64::MAX
                        } else if go.nodes_given && go.nodes < NODE_BUDGET_SENTINEL {
                            go.nodes
                        } else {
                            DEFAULT_NODE_BUDGET
                        },
                    };
                    let search = ab::Search::with_options(pos, hist, limits, ab_options);
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
                    // **Un mat s'annonce `score mate N`**, pas `score cp
                    // 29999` : un client UCI (un GUI, un analyseur)
                    // lit `cp 29999` comme « +299 990 centipions » et
                    // affiche une finale de trois siècles au lieu de
                    // « mat en 1 ». La conversion est celle de la norme
                    // UCI : le nombre de **coups** (plies arrondies) qui
                    // séparent du mat.
                    //
                    // Et quand **aucune itération n'a pu être complétée**
                    // (budget de nœuds nul, temps expiré avant la
                    // première itération), le score rendu n'est pas
                    // « 0 » — ce serait mentir sur la position : c'est
                    // l'évaluation statique de la racine, qui est
                    // exactement ce que le moteur sait.
                    let score = if o.depth == 0 {
                        crate::eval::eval_for_mover(&pos)
                    } else {
                        o.score_cp
                    };
                    let score_str = match mate_in_moves(score) {
                        Some(m) => format!("mate {m}"),
                        None => format!("cp {score}"),
                    };
                    println!(
                        "info depth {} score {} nodes {} nps {:.0} time {} pv {}",
                        o.depth,
                        score_str,
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

/// L'alpha-beta de référence (`docs/ab/plan-alpha-beta-2026-10-22.md`).
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
            st.seed = parse_seed(&v).unwrap_or(st.seed)
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
        // Le bruit de la valeur (phase AB7). L'amplitude et la graine
        // sont **indépendantes** de la graine de recherche (`Seed`) : le
        // champ de bruit doit être le même d'un point de la courbe à
        // l'autre, quelle que soit la graine du moteur.
        ("EvalNoise", _) => {
            st.noise_cp = v.parse::<u32>().map(|x| x.min(2000)).unwrap_or(st.noise_cp);
            crate::eval::set_noise(st.noise_cp, st.noise_seed);
        }
        ("EvalNoiseSeed", _) => {
            st.noise_seed = parse_seed(&v).unwrap_or(st.noise_seed);
            crate::eval::set_noise(st.noise_cp, st.noise_seed);
        }
        // L'alpha-beta de référence (`docs/ab/plan-alpha-beta-2026-10-22.md`).
        ("Search", _) => {
            // Une valeur **inconnue** ne doit pas changer le moteur
            // **en silence** : `setoption name Search value bogus`
            // faisait retomber sur le MCTS, quel que soit le moteur
            // actif — un changement de moteur non demandé, et non
            // signalé. La valeur courante est **conservée**.
            st.search = match v.trim().to_ascii_lowercase().as_str() {
                "alpha" | "alphabeta" | "alpha-beta" => SearchKind::Alpha,
                "mcts" | "montecarlo" | "mcts-v1" => SearchKind::Mcts,
                _ => st.search,
            }
        }
        // Phase AB8 : les options de recherche de l'alpha-beta. Elles
        // n'ont d'effet que sur le chemin `Search=alpha` ; le chemin
        // MCTS est inchangé.
        ("AbPvs", _) => st.ab_options.pvs = parse_bool(&v, st.ab_options.pvs),
        ("AbKillers", _) => st.ab_options.killers = parse_bool(&v, st.ab_options.killers),
        ("AbHistory", _) => st.ab_options.history = parse_bool(&v, st.ab_options.history),
        ("AbNullMove", _) => st.ab_options.null_move = parse_bool(&v, st.ab_options.null_move),
        ("AbRazoring", _) => st.ab_options.razoring = parse_bool(&v, st.ab_options.razoring),
        ("AbFutility", _) => st.ab_options.futility = parse_bool(&v, st.ab_options.futility),
        ("AbLmr", _) => st.ab_options.lmr = parse_bool(&v, st.ab_options.lmr),
        // Phase NN : la valeur apprise. `EvalNN=1` **charge** le réseau
        // (depuis `EvalNNFile`) et l'active ; `EvalNN=0` le décharge et
        // rend la feuille à l'évaluation statique — bit-à-bit comme
        // avant la phase. Un échec de chargement **n'arrête pas** le
        // moteur : il reste sur la valeur statique, et le dit.
        ("EvalNN", _) => {
            let on = parse_bool(&v, false);
            if on {
                let path = st.nn_file.clone();
                match crate::nn::load_and_enable(std::path::Path::new(&path)) {
                    Ok(()) => eprintln!("uci: valeur apprise active ({path})"),
                    Err(e) => eprintln!("uci: valeur apprise indisponible ({e}) — feuille statique"),
                }
            } else {
                crate::nn::unload();
            }
        }
        ("EvalNNFile", _) => {
            st.nn_file = v.trim().to_string();
        }
        (name, value) => eprintln!("uci: ignored option {name:?} = {value:?}"),
    }
}

/// Le nombre de **coups** qui séparent du mat, si le score est un score
/// de mat ; `None` sinon.
///
/// La conversion est celle de la norme UCI : le score de mat du moteur
/// est `MATE - n` où `n` est le **ply** du mat, et l'UCI compte les
/// **coups** (une paire de plies). `+29999` est donc « mat en 1 »,
/// `-29997` est « mat en 2 ».
fn mate_in_moves(score: i32) -> Option<i32> {
    let mate = ab::MATE;
    let max_ply = ab::MAX_PLY as i32;
    if score >= mate - max_ply {
        Some((mate - score + 1) / 2)
    } else if score <= -mate + max_ply {
        Some(-((mate + score + 1) / 2))
    } else {
        None
    }
}

/// Un booléen d'option UCI : `0`/`1`, `false`/`true`, `non`/`oui`.
/// Une valeur illisible **laisse l'option inchangée** (elle n'est pas
/// appliquée silencieusement à l'inverse de ce qui est demandé).
fn parse_bool(v: &str, current: bool) -> bool {
    match v.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => current,
    }
}

/// Lit une **graine** (`Seed`, `EvalNoiseSeed`).
///
/// La convention annoncée par l'option est hexadécimale (`0x…`), mais
/// une valeur **décimale** doit être lue comme telle : l'ancien code
/// essayait l'hexadécimal **d'abord**, donc `setoption name Seed value
/// 42` valait `0x42` — 66 — sans le dire. La lecture est donc :
///
/// * `0x…` → hexadécimal (la convention de l'option) ;
/// * sinon **décimal** ;
/// * sinon hexadécimal (une graine en hexadécimal sans préfixe).
pub fn parse_seed(v: &str) -> Option<u64> {
    let t = v.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok();
    }
    if let Ok(d) = t.parse::<u64>() {
        return Some(d);
    }
    u64::from_str_radix(t, 16).ok()
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
        // **Un FEN absent ne doit pas paniquer** : `position fen` (sans
        // FEN) faisait panic le moteur (`toks[2..3]` sur un slice de
        // longueur 2). Un panic dans la boucle UCI tue le moteur : pas
        // de `bestmove`, et l'arbitre attend indéfiniment.
        if toks.len() < 3 {
            eprintln!("uci: `position fen` sans FEN — la position est inchangée");
            return;
        }
        let fen_end = toks
            .iter()
            .position(|&t| t == "moves")
            .unwrap_or(toks.len());
        // FEN = piece placement + side + castling + ep (+ optional counters
        // which the engine core tolerates as missing).
        let fen = toks[2..fen_end.max(3).min(toks.len())].join(" ");
        match Position::from_fen(&fen) {
            Ok(p) => {
                // **Un roi par camp** : le protocole UCI exige une
                // position *jouable*. Le core définit et tolère une
                // position sans roi (tests/malformed_fen.rs), mais le
                // moteur ne peut pas y jouer : la recherche y appelle
                // `king_sq` (un `debug_assert` paniquait en build de
                // debug) et le repli renvoie `0000` — un coup illégal
                // sur une position que l'arbitre croyait jouable.
                if !p.has_king(crate::types::Color::White) || !p.has_king(crate::types::Color::Black) {
                    eprintln!("uci: FEN sans roi — la position est inchangée");
                    return;
                }
                pos = Some(p)
            }
            Err(e) => {
                eprintln!("uci: invalid fen ({e:?})");
                return;
            }
        }        if fen_end < toks.len() {
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
    // L'horloge a-t-elle été **fournie** ? `go wtime 0` est un budget
    // (le moteur n'a plus de temps), pas une absence de budget : sans
    // ce drapeau, `wtime 0` retombait sur le budget par défaut
    // (10 000 nœuds, ~80 ms mesurés) — un dépassement d'horloge
    // assuré.
    let mut clock_given = false;
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
                clock_given = true;
                i += 2;
            }
            "btime" => {
                if !is_white {
                    my_time = val.unwrap_or(0);
                }
                clock_given = true;
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
                    // **La profondeur nulle n'est pas une profondeur** :
                    // `go depth 0` est un `go` **sans budget de
                    // profondeur** — le budget par défaut s'applique
                    // (10 000 nœuds). Avant, la profondeur nulle était
                    // traduite en un budget de nœuds **nul** pour le
                    // MCTS (le moteur ne cherchait pas du tout) et en
                    // « profondeur 64 » pour l'alpha-beta (une recherche
                    // de plusieurs secondes). Les deux moteurs doivent
                    // obéir au **même** contrat.
                    if v > 0 {
                        // generous mapping, v1: ~30k playouts per "ply depth"
                        nodes = v.min(1_000_000).saturating_mul(30_000) as u64;
                        // The alpha-beta path takes the depth at face value.
                        depth = Some(v.min(128) as u32);
                    }
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
    if my_time > 0 || clock_given {
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
    // `go depth N` : le MCTS mappe la profondeur à un budget de nœuds
    // (30 000 par ply), donc `go depth 1000` = 30 M de nœuds, ~25 s
    // mesurés. Un **plafond temporel** borne la course : le moteur
    // doit répondre dans un temps raisonnable, quel que soit le
    // budget demandé. (Le plafond de 10 s est celui du budget TC.)
    let movetime_ms = if depth.is_some() {
        Some(DEPTH_TIME_CAP_MS)
    } else {
        None
    };
    GoLimits {
        nodes,
        movetime_ms,
        depth,
        nodes_given,
    }
}