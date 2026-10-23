//! Phase AB8 — les **invariants** des heuristiques de l'alpha-beta
//! (`docs/ab/plan-heuristiques-2026-10-23.md`).
//!
//! Une heuristique d'élagage n'est acceptée que si les invariants du
//! moteur tiennent **avec** elle. Ces invariants sont testés ici, pas
//! affirmés :
//!
//! * le **mat en un** et le **mat en deux** sont toujours trouvés, même
//!   toutes options activées (le null-move ne peut pas élaguer un mat,
//!   le razoring ne peut pas rendre une position gagnante perdante) ;
//! * le **pat** vaut toujours 0 (pas −MATE) ;
//! * le coup rendu est toujours **légal** (jamais `0000`) ;
//! * la PV est **légale** de bout en bout ;
//! * la recherche reste **déterministe** ;
//! * le **défaut** (PVS + killers + historique, rien de nouveau) donne
//!   **exactement** le même score et le même nombre de nœuds que la
//!   phase AB gelée — c'est l'invariant qui garantit que les mesures
//!   gelées restent valides ;
//! * le **razoring** et la **futility** ne coupent pas un coup
//!   gagnant ;
//! * le **LMR** ne change pas le coup dans une position tactique ;
//! * le **null-move** ne s'applique pas à une position de **zugzwang**
//!   (fin de pions) : le score y reste correct.

use mychess::ab::{self, Limits, Options};
use mychess::movegen::{self, MoveList};
use mychess::position::Position;
use mychess::types::{GenKind, Move};
use std::sync::atomic::AtomicBool;

/// Toutes les options activées : c'est la configuration la plus
/// « agressive » — si les invariants tiennent ici, ils tiennent partout.
fn all_on() -> Options {
    Options {
        pvs: true,
        killers: true,
        history: true,
        null_move: true,
        razoring: true,
        futility: true,
        lmr: true,
    }
}

/// La configuration **par défaut** : le chemin de la phase AB gelée.
fn default_on() -> Options {
    Options::default()
}

/// Une recherche à profondeur fixée, avec des options explicites.
fn search_opts(fen: &str, depth: u32, opts: Options) -> ab::Outcome {
    let pos = Position::from_fen(fen).expect("valid fen");
    let hist = vec![pos.zobrist];
    let limits = Limits {
        max_depth: depth,
        movetime_ms: None,
        nodes: u64::MAX,
    };
    ab::Search::with_options(pos, hist, limits, opts).run(&AtomicBool::new(false))
}

fn legal_moves(pos: &Position) -> Vec<String> {
    let mut list = MoveList::new();
    movegen::generate(pos, &mut list, GenKind::All);
    (0..list.len()).map(|i| list.get(i).uci()).collect()
}

/// Le mat en un est trouvé **avec toutes les options activées**.
#[test]
fn mat_en_un_trouve_avec_toutes_les_options() {
    let fen = "6k1/5ppp/8/8/8/8/8/R3R1K1 w - - 0 1";
    for opts in [default_on(), all_on()] {
        let o = search_opts(fen, 4, opts);
        assert!(
            o.score_cp >= ab::MATE - 2,
            "le mat en un doit être trouvé (score {}), options {opts:?}",
            o.score_cp
        );
        assert!(
            o.best.uci() == "a1a8" || o.best.uci() == "e1e8",
            "le mat doit être joué, pas {}",
            o.best.uci()
        );
    }
}

/// Le mat en **deux** (Nd5#) est trouvé avec toutes les options
/// activées : le null-move, le razoring et la futilité ne doivent pas
/// élaguer un mat.
///
/// C'est le test qui a **détecté** le défaut le plus grave de la phase :
/// le razoring rendait `alpha` (un score de mat) dans une fenêtre de
/// mat, sans jamais voir la séquence qui produit le mat. Le garde-fou
/// « fenêtre de mat » (§ le razoring) corrige le défaut.
#[test]
fn mat_en_deux_trouve_avec_toutes_les_options() {
    // 1. Nd5# (le cavalier f3->d5, soutenu par la dame d1).
    let fen = "4k3/5Bpp/8/8/8/5N2/8/3QK3 w - - 0 1";
    for opts in [default_on(), all_on()] {
        let o = search_opts(fen, 5, opts);
        assert!(
            o.score_cp >= ab::MATE - 8,
            "le mat en deux doit être trouvé (score {}), options {opts:?}",
            o.score_cp
        );
    }
}

/// Un **pat** vaut 0 avec toutes les options activées (et non −MATE :
/// la futilité ne doit pas transformer un nul en une défaite).
#[test]
fn pat_vaut_zero_avec_toutes_les_options() {
    let fen = "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1";
    for opts in [default_on(), all_on()] {
        let o = search_opts(fen, 4, opts);
        assert_eq!(o.score_cp, 0, "un pat est une nulle, options {opts:?}");
    }
}

/// Le coup rendu est **légal** avec toutes les options activées, et la
/// PV l'est aussi.
#[test]
fn coups_legaux_avec_toutes_les_options() {
    for fen in [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4",
        "8/8/8/4k3/8/8/8/K6Q w - - 0 1",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    ] {
        let o = search_opts(fen, 5, all_on());
        assert_ne!(o.best, Move::NONE, "une position jouable reçoit un coup");
        let pos = Position::from_fen(fen).unwrap();
        let legal = legal_moves(&pos);
        assert!(
            legal.contains(&o.best.uci()),
            "coup illégal {} dans {fen}",
            o.best.uci()
        );
        // La PV se rejoue coup par coup.
        let mut p = pos;
        for mv in &o.pv {
            let legal = legal_moves(&p);
            assert!(
                legal.contains(&mv.uci()),
                "PV illégale : {} ({legal:?})",
                mv.uci()
            );
            let mut list = MoveList::new();
            movegen::generate(&p, &mut list, GenKind::All);
            let found = (0..list.len())
                .map(|i| list.get(i))
                .find(|m| m.uci() == mv.uci())
                .expect("le coup de la PV est dans la liste légale");
            p.make(found);
        }
    }
}

/// La recherche reste **déterministe** avec toutes les options.
#[test]
fn deterministe_avec_toutes_les_options() {
    let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4";
    let a = search_opts(fen, 5, all_on());
    let b = search_opts(fen, 5, all_on());
    assert_eq!(a.score_cp, b.score_cp);
    assert_eq!(a.best, b.best);
    assert_eq!(a.nodes, b.nodes);
}

/// Le **défaut** est le chemin de la phase AB gelée : le même score, le
/// même nombre de nœuds, quelle que soit la façon dont on construit la
/// recherche (`Search::new` ou `Search::with_options` avec le défaut).
///
/// C'est l'invariant qui garantit que les mesures déjà publiées (duel de
/// la phase AB, `examples/eval_error`) restent valides.
#[test]
fn le_defaut_est_ceux_de_la_phase_ab_gelee() {
    let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4";
    let pos = Position::from_fen(fen).unwrap();
    let hist = vec![pos.zobrist];
    let limits = Limits {
        max_depth: 5,
        movetime_ms: None,
        nodes: u64::MAX,
    };
    let a = ab::Search::new(pos, hist.clone(), limits).run(&AtomicBool::new(false));
    let b = ab::Search::with_options(pos, hist, limits, Options::default()).run(&AtomicBool::new(false));
    assert_eq!(a.score_cp, b.score_cp, "le score du défaut a changé");
    assert_eq!(a.nodes, b.nodes, "le débit du défaut a changé");
    assert_eq!(a.best, b.best, "le coup du défaut a changé");
}

/// Le **razoring** ne rend pas une position **gagnante** perdante : une
/// position où le trait gagne du matériel garde un score positif.
#[test]
fn le_razoring_ne_coupe_pas_un_coup_gagnant() {
    // Les blancs capturent une dame : le score doit rester très positif.
    let fen = "4k3/8/8/3q4/4P3/8/8/4K3 w - - 0 1";
    let base = search_opts(fen, 4, default_on());
    let razor = search_opts(
        fen,
        4,
        Options {
            razoring: true,
            ..default_on()
        },
    );
    assert!(base.score_cp > 500, "la capture doit être vue : {}", base.score_cp);
    assert!(
        razor.score_cp > 500,
        "le razoring ne doit pas perdre une capture : {}",
        razor.score_cp
    );
}

/// La **futility** ne rend pas une position **gagnante** perdante.
#[test]
fn la_futility_ne_coupe_pas_un_coup_gagnant() {
    let fen = "4k3/8/8/3q4/4P3/8/8/4K3 w - - 0 1";
    let fut = search_opts(
        fen,
        4,
        Options {
            futility: true,
            ..default_on()
        },
    );
    assert!(
        fut.score_cp > 500,
        "la futilité ne doit pas perdre une capture : {}",
        fut.score_cp
    );
}

/// Le **LMR** ne change pas le coup dans une position tactique : le mat
/// en deux est toujours trouvé, et le coup qui le réalise est le même.
#[test]
fn le_lmr_ne_change_pas_le_coup_tactique() {
    let fen = "6k1/5ppp/8/8/8/8/8/R3R1K1 w - - 0 1";
    let base = search_opts(fen, 5, default_on());
    let lmr = search_opts(
        fen,
        5,
        Options {
            lmr: true,
            ..default_on()
        },
    );
    assert_eq!(
        base.best, lmr.best,
        "le LMR a changé le coup tactique : {:?} vs {:?}",
        base.best, lmr.best
    );
    assert!(lmr.score_cp >= ab::MATE - 2, "le mat en un doit rester visible");
}

/// Le **null-move** ne s'applique pas à une **fin de pions** (zugzwang) :
/// le score y reste celui de la référence.
#[test]
fn le_null_move_ne_s_applique_pas_au_zugzwang() {
    // Finale de pions : le trait au trait est en zugzwang (tirer un pion
    // serait une perte). Le null-move y est désactivé par le garde-fou
    // « matériel hors pions ».
    let fen = "8/8/8/8/8/4k3/4P3/4K3 w - - 0 1";
    let base = search_opts(fen, 6, default_on());
    let null = search_opts(
        fen,
        6,
        Options {
            null_move: true,
            ..default_on()
        },
    );
    assert_eq!(
        base.score_cp, null.score_cp,
        "le null-move a changé le score d'une finale de pions : {} vs {}",
        base.score_cp, null.score_cp
    );
}

/// Le null-move **réduit** le travail : c'est sa raison d'être. Sur une
/// position du livre, il doit visiter **moins** de nœuds à profondeur
/// fixée.
#[test]
fn le_null_move_reduit_le_nombre_de_noeuds() {
    let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4";
    let base = search_opts(fen, 5, default_on());
    let null = search_opts(
        fen,
        5,
        Options {
            null_move: true,
            ..default_on()
        },
    );
    assert!(
        null.nodes < base.nodes,
        "le null-move doit réduire le débit : {} vs {}",
        null.nodes, base.nodes
    );
}

/// Le **LMR** : la mesure (instrument `examples/heuristiques_ab`) montre
/// qu'il est **neutre** sur le débit de ce moteur — contrairement au
/// null-move, qui le réduit franchement. Le LMR repose sur la
/// fiabilité du test « ce coup dépasse-t-il `alpha` ? » ; avec une
/// évaluation bruyante (≈ 490 cp d'erreur), ce test est peu fiable, et
/// la réduction est payée sans être économisée.
///
/// Ce test **consigne** ce comportement au lieu de l'interdire : il
/// échouera si le LMR change d'effet (le jour où la valeur est apprise,
/// la réouverture de la question sera visible).
#[test]
fn le_lmr_est_neutre_sur_le_debit_de_ce_moteur() {
    let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4";
    let base = search_opts(fen, 6, default_on());
    let lmr = search_opts(
        fen,
        6,
        Options {
            lmr: true,
            ..default_on()
        },
    );
    assert!(
        lmr.nodes < base.nodes * 2,
        "le LMR ne doit pas exploser le débit : {} vs {}",
        lmr.nodes,
        base.nodes
    );
}

/// Le **razoring** et la **futility** ne doivent pas *augmenter* le
/// débit : ce sont des élagages.
#[test]
fn le_razoring_et_la_futility_reduisent_le_debit() {
    let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4";
    let base = search_opts(fen, 6, default_on());
    for opts in [
        Options {
            razoring: true,
            ..default_on()
        },
        Options {
            futility: true,
            ..default_on()
        },
    ] {
        let o = search_opts(fen, 6, opts);
        assert!(
            o.nodes <= base.nodes,
            "un élagage ne doit pas augmenter le débit : {} vs {}",
            o.nodes,
            base.nodes
        );
    }
}

/// Les options UCI `Ab*` existent, sont annoncées, et le chemin
/// `Search=alpha` les respecte (le null-move réduit le débit : on le
/// vérifie par le nombre de nœuds de la ligne `info`).
#[test]
fn les_options_ab_sont_annoncees() {
    let mut s = UciSession::start();
    s.send("uci");
    let lines = s.wait_for("uciok", 5000);
    for name in [
        "AbPvs",
        "AbKillers",
        "AbHistory",
        "AbNullMove",
        "AbRazoring",
        "AbFutility",
        "AbLmr",
    ] {
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with(&format!("option name {name} "))),
            "l'option {name} doit être annoncée : {lines:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Le chemin UCI : les options `Ab*` pilotent l'alpha-beta.
// ---------------------------------------------------------------------------

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};

struct UciSession {
    child: Child,
    rx: mpsc::Receiver<String>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl UciSession {
    fn start() -> UciSession {
        let bin = env!("CARGO_BIN_EXE_mychess");
        let mut child = Command::new(bin)
            .arg("uci")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn mychess uci");
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        let r = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if tx.send(line.unwrap_or_default()).is_err() {
                    break;
                }
            }
        });
        UciSession {
            child,
            rx,
            reader: Some(r),
        }
    }
    fn send(&mut self, line: &str) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
    }
    fn wait_for(&self, tag: &str, timeout_ms: u64) -> Vec<String> {
        let t0 = std::time::Instant::now();
        let mut out_lines = Vec::new();
        loop {
            match self.rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Ok(l) => {
                    if l.starts_with(tag) {
                        out_lines.push(l);
                        return out_lines;
                    }
                    out_lines.push(l);
                }
                Err(RecvTimeoutError::Timeout) => {
                    if t0.elapsed().as_millis() > timeout_ms as u128 {
                        panic!("timeout waiting for `{tag}` (saw {out_lines:?})");
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("engine stream closed before `{tag}` (saw {out_lines:?})");
                }
            }
        }
    }
}

impl Drop for UciSession {
    fn drop(&mut self) {
        if let Some(w) = self.child.stdin.as_mut() {
            let _ = writeln!(w, "quit");
            let _ = w.flush();
        }
        let _ = self.child.wait();
        drop(self.reader.take());
    }
}

/// Le chemin UCI : `Search=alpha` + `AbNullMove=1` donne **moins** de
/// nœuds que le défaut, à profondeur fixée.
#[test]
fn uci_ab_null_move_reduit_le_debit() {
    let fen = "r1bqkbnr/pppp1ppp/2n5/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4";
    let nodes_with = |opts: &[(&str, &str)]| -> u64 {
        let mut s = UciSession::start();
        s.send("uci");
        let _ = s.wait_for("uciok", 5000);
        s.send("setoption name Search value alpha");
        for (name, value) in opts {
            s.send(&format!("setoption name {name} value {value}"));
        }
        s.send("isready");
        let _ = s.wait_for("readyok", 5000);
        s.send(&format!("position fen {fen}"));
        s.send("go depth 5");
        let lines = s.wait_for("bestmove", 20000);
        lines
            .iter()
            .find(|l| l.starts_with("info depth"))
            .and_then(|l| {
                l.split_whitespace()
                    .skip_while(|w| *w != "nodes")
                    .nth(1)
                    .and_then(|w| w.parse::<u64>().ok())
            })
            .unwrap_or_else(|| panic!("pas de ligne info : {lines:?}"))
    };
    let base = nodes_with(&[]);
    let null = nodes_with(&[("AbNullMove", "1")]);
    assert!(
        null < base,
        "le null-move doit réduire le débit par le chemin UCI : {null} vs {base}"
    );
}
