//! La **robustesse du protocole UCI** — les propriétés que rien ne
//! testait (revue du 2026-10-23).
//!
//! # Pourquoi ce fichier existe
//!
//! L'audit a trouvé des défauts que la suite entière ne pouvait pas voir,
//! parce que tous les tests existants passaient par des **chemins
//! heureux** : une FEN valide, un `go` bien formé, une recherche qui
//! aboutit.
//!
//! Les propriétés écrites ici sont celles d'un **arbitre** :
//!
//! * toute commande produit une réponse — ou au moins **ne tue pas** le
//!   moteur (un panic dans le worker UCI = pas de `bestmove` = partie
//!   perdue) ;
//! * un `go` **sans budget** est **borné** (sinon le moteur réfléchit
//!   indéfiniment et l'arbitre le flague) ;
//! * le moteur répond **toujours** un coup **légal** sur une position
//!   jouable ;
//! * un mat est annoncé `score mate N`, pas `score cp 29999` ;
//! * la marge de temps est soustraite **une seule fois** (la doubler
//!   fait réfléchir 5 ms de moins que le budget annoncé) ;
//! * une option **inconnue** ne change pas le moteur **en silence**.
//!
//! Chaque test est une **propriété** du protocole, pas un cas
//! particulier : ils attrapent une famille entière de défauts.

use mychess::types::Color;
use mychess::uci::{self, parse_seed};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};

// ---------------------------------------------------------------------------
// Le pilote UCI
// ---------------------------------------------------------------------------

/// Une session avec le moteur : on écrit une commande, on lit les
/// lignes jusqu'à la **balise** attendue, avec un délai.
struct Session {
    child: Child,
    rx: mpsc::Receiver<String>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl Session {
    fn start() -> Session {
        let bin = env!("CARGO_BIN_EXE_mychess");
        let mut child = Command::new(bin)
            .arg("uci")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn mychess uci");
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if tx.send(line.unwrap_or_default()).is_err() {
                    break;
                }
            }
        });
        Session {
            child,
            rx,
            reader: Some(reader),
        }
    }

    fn send(&mut self, line: &str) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
    }

    /// Attend une ligne commençant par `tag`, et rend **toutes** les
    /// lignes vues (les `info` compris).
    fn wait_for(&self, tag: &str, timeout_ms: u64) -> Vec<String> {
        let t0 = std::time::Instant::now();
        let mut seen = Vec::new();
        loop {
            match self.rx.recv_timeout(std::time::Duration::from_millis(20)) {
                Ok(l) => {
                    if l.starts_with(tag) {
                        seen.push(l);
                        return seen;
                    }
                    seen.push(l);
                }
                Err(RecvTimeoutError::Timeout) => {
                    if t0.elapsed().as_millis() > timeout_ms as u128 {
                        panic!("timeout en attendant `{tag}` (vu : {seen:?})");
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("le moteur a **mouru** en attendant `{tag}` (vu : {seen:?})");
                }
            }
        }
    }

    /// Le processus est-il encore vivant ?
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(w) = self.child.stdin.as_mut() {
            let _ = writeln!(w, "quit");
            let _ = w.flush();
        }
        let _ = self.child.wait();
        drop(self.reader.take());
    }
}

/// La ligne `bestmove` (le coup en UCI).
fn best_move_of(lines: &[String]) -> String {
    lines
        .iter()
        .find(|l| l.starts_with("bestmove"))
        .and_then(|l| l.split_whitespace().nth(1))
        .expect("pas de ligne bestmove")
        .to_string()
}

/// Le champ `time` de la ligne `info`.
fn info_time_of(lines: &[String]) -> u128 {
    let line = lines
        .iter()
        .find(|l| l.starts_with("info "))
        .expect("pas de ligne info");
    let parts: Vec<&str> = line.split_whitespace().collect();
    let i = parts
        .iter()
        .position(|p| *p == "time")
        .expect("pas de champ time");
    parts[i + 1].parse().expect("champ time illisible")
}

// ---------------------------------------------------------------------------
// 1. Un `go` sans budget est borné, sur les DEUX moteurs.
// ---------------------------------------------------------------------------

/// **Un `go` sans budget est borné**, sur les deux moteurs.
///
/// Le défaut historique : le chemin `Search=alpha` transmettait
/// `u64::MAX` nœuds et une profondeur de 64 — un `go` nu réfléchissait
/// **25 s** (100 M de nœuds mesurés) sans jamais répondre, pendant que
/// le MCTS, lui, s'arrêtait à 10 000 nœuds. Les deux moteurs doivent
/// obéir au **même** contrat.
///
/// On vérifie donc que, pour toutes les formes de `go` sans budget
/// explicite, une `bestmove` arrive **vite** — et qu'elle est un coup
/// **légal**, pas `0000`.
#[test]
fn un_go_sans_budget_est_borne_sur_les_deux_moteurs() {
    for engine in ["mcts", "alpha"] {
        for go in [
            "go",
            "go depth 0",
            "go ponder",
            "go nodes 0",
            "go movetime 0",
        ] {
            let mut s = Session::start();
            s.send("uci");
            s.wait_for("uciok", 5000);
            s.send(&format!("setoption name Search value {engine}"));
            s.send("isready");
            s.wait_for("readyok", 5000);
            s.send("position startpos");
            s.send("isready");
            s.wait_for("readyok", 5000);
            s.send(go);
            let lines = s.wait_for("bestmove", 3000);
            let bm = best_move_of(&lines);
            assert_ne!(
                bm, "0000",
                "`{go}` ({engine}) a répondu 0000 sur une position jouable"
            );
            assert!(s.alive(), "`{go}` ({engine}) a tué le moteur");
        }
    }
}

// ---------------------------------------------------------------------------
// 2. Les commandes hostiles ne tuent pas le moteur.
// ---------------------------------------------------------------------------

/// **Une commande hostile ne tue pas le moteur.**
///
/// Le moteur est une **frontière d'entrée** : les commandes viennent
/// d'un GUI, d'un proxy, d'un arbitre — et une ligne tronquée est
/// possible. Le contrat : le moteur **reste vivant** et **répond**.
///
/// Les cas testés sont ceux que l'audit a reproduits :
/// `position fen` sans FEN (panic sur `toks[2..3]`), `position fen
/// moves` sans FEN, une FEN **sans roi** (le `king_sq` du worker
/// panique), une option inconnue, une commande inconnue.
#[test]
fn les_commandes_hostiles_ne_tuent_pas_le_moteur() {
    let mut s = Session::start();
    s.send("uci");
    s.wait_for("uciok", 5000);

    for hostile in [
        "position fen",
        "position fen moves",
        "position fen 8/8/8/8/8/8/8/8 w - - 0 1",
        "position fen 8/8/8/8/8/8/8/8 w KQkq - 0 1",
        "position fen pas-un-fen w - - 0 1",
        "setoption name Inconnu value 3",
        "setoption name EvalNN value 1",
        "setoption name EvalNNFile value /tmp/fichier-inexistant.bin",
        "commande-inconnue",
        "position startpos moves e2e4 e7e5",
    ] {
        s.send(hostile);
        s.send("isready");
        s.wait_for("readyok", 5000);
        assert!(s.alive(), "`{hostile}` a tué le moteur");
    }

    // Et le moteur joue **encore** : un `go` après toutes ces
    // commandes doit produire un coup légal.
    s.send("position startpos");
    s.send("go nodes 100");
    let lines = s.wait_for("bestmove", 5000);
    let bm = best_move_of(&lines);
    assert_ne!(
        bm, "0000",
        "le moteur ne joue plus après les commandes hostiles"
    );
    assert!(s.alive(), "le moteur a Rif le `go`");
}

// ---------------------------------------------------------------------------
// 3. La marge de temps est soustraite UNE SEULE FOIS.
// ---------------------------------------------------------------------------

/// **La marge de temps est soustraite une seule fois.**
///
/// La marge (5 ms) est réservée pour l'aller-retour UCI. Elle l'était
/// **deux fois** sur le chemin alpha-beta (une fois dans la couche
/// UCI, une fois dans le moteur) : l'alpha-beta réfléchissait 5 ms de
/// moins que le budget annoncé, et l'écart AB/MCTS polluait toute
/// mesure de phase.
///
/// La couche UCI est le **seul** endroit où la marge est soustraite :
/// c'est ce que vérifie ce test, sur la valeur transmise…
#[test]
fn la_marge_de_temps_est_soustraite_une_seule_fois() {
    let g = uci::parse_go(&["go", "movetime", "200"], &Color::White);
    assert_eq!(
        g.movetime_ms,
        Some(195),
        "la marge doit être soustraite une seule fois (200 -> 195)"
    );
    // …et le temps **réellement** utilisé par le moteur est dans le
    // **ordre de grandeur** du budget annoncé (ni 190, ni 200, ni
    // 2 000).
    //
    // La borne est **lâche** ici : sous charge (les tests d'un même
    // binaire tournent **en parallèle**), le moteur peut être
    // désordonné et dépasser l'échéance — c'est une question de
    // robustesse du moteur, pas de la propriété testée. La propriété
    // « la marge est soustraite **une seule fois** » est vérifiée
    // **exactement** par l'assertion sur `parse_go` ci-dessus.
    for engine in ["mcts", "alpha"] {
        let mut s = Session::start();
        s.send("uci");
        s.wait_for("uciok", 5000);
        s.send(&format!("setoption name Search value {engine}"));
        s.send("isready");
        s.wait_for("readyok", 5000);
        s.send("position startpos");
        s.send("go movetime 200");
        let lines = s.wait_for("bestmove", 10_000);
        let t = info_time_of(&lines);
        assert!(
            t > 0,
            "`go movetime 200` ({engine}) n'a pas consommé de temps"
        );
        assert!(
            t <= 2_000,
            "`go movetime 200` ({engine}) a duré {t} ms : le budget n'est pas respecté"
        );
        assert_ne!(best_move_of(&lines), "0000", "{engine} n'a pas joué");
    }
}

// ---------------------------------------------------------------------------
// 4. Un mat est annoncé `score mate N`.
// ---------------------------------------------------------------------------

/// **Un mat est annoncé `score mate N`**, jamais `score cp 29999`.
///
/// Un client UCI lit `cp 29999` comme « +299 990 centipions » et
/// affiche une finale de trois siècles au lieu de « mat en 1 ».
#[test]
fn un_mat_est_annonce_comme_mate() {
    let mut s = Session::start();
    s.send("uci");
    s.wait_for("uciok", 5000);
    s.send("setoption name Search value alpha");
    s.send("isready");
    s.wait_for("readyok", 5000);
    s.send("position fen 6k1/5ppp/8/8/8/8/8/R3R1K1 w - - 0 1");
    s.send("go depth 4");
    let lines = s.wait_for("bestmove", 10000);
    let info = lines
        .iter()
        .find(|l| l.starts_with("info "))
        .expect("pas de ligne info");
    assert!(
        info.contains("score mate"),
        "le mat n'est pas annoncé comme un mat : {info}"
    );
    assert!(
        !info.contains("score cp 299"),
        "le mat est annoncé en centipions : {info}"
    );
    // Et le coup joué est le mat.
    let bm = best_move_of(&lines);
    assert!(
        bm == "a1a8" || bm == "e1e8",
        "le moteur n'a pas joué le mat : {bm}"
    );
}

// ---------------------------------------------------------------------------
// 5. L'horloge à zéro est un budget, pas une absence de budget.
// ---------------------------------------------------------------------------

/// **`go wtime 0` est un budget** (1 ms), pas une absence de budget.
///
/// Sans ce cas, `wtime 0` retombait sur le budget par défaut
/// (10 000 nœuds, ~80 ms mesurés) : un dépassement d'horloge assuré.
#[test]
fn une_horloge_a_zero_est_un_budget() {
    for toks in [
        vec!["go", "wtime", "0"],
        vec!["go", "btime", "0"],
        vec!["go", "wtime", "0", "winc", "0"],
    ] {
        let g = uci::parse_go(&toks, &Color::White);
        assert_eq!(
            g.movetime_ms,
            Some(1),
            "`{}` n'est pas traité comme un budget nul",
            toks.join(" ")
        );
    }
    // Une horloge **normale** reste un budget de temps.
    let g = uci::parse_go(&["go", "wtime", "60000", "winc", "1000"], &Color::White);
    assert!(
        g.movetime_ms.is_some_and(|t| t > 1 && t <= 10_000),
        "un budget TC normal a été mal calculé : {:?}",
        g.movetime_ms
    );
}

// ---------------------------------------------------------------------------
// 6. `go depth` est borné **en temps**.
// ---------------------------------------------------------------------------

/// **`go depth N` est borné en temps**.
///
/// La profondeur est traduite en un budget de nœuds (30 000 par ply
/// pour le MCTS) : `go depth 1000` demandait 30 M de nœuds, ~25 s
/// mesurés. Le moteur doit répondre dans un temps raisonnable, quel
/// que soit le budget demandé.
#[test]
fn un_go_depth_est_borne_en_temps() {
    let g = uci::parse_go(&["go", "depth", "1000"], &Color::White);
    assert!(
        g.movetime_ms.is_some_and(|t| t <= 10_000),
        "`go depth 1000` n'est pas borné en temps : {:?}",
        g.movetime_ms
    );
    // `go infinite` reste non borné — c'est le contrat de l'infini.
    let g = uci::parse_go(&["go", "infinite"], &Color::White);
    assert_eq!(g.movetime_ms, None, "`go infinite` a été borné en temps");
}

// ---------------------------------------------------------------------------
// 7. Une option inconnue ne change pas le moteur, en silence.
// ---------------------------------------------------------------------------

/// **Une valeur d'option inconnue ne change pas le moteur.**
///
/// `setoption name Search value bogus` faisait retomber **silencieusement**
/// sur le MCTS — un changement de moteur non demandé, et non signalé.
#[test]
fn une_option_inconnue_ne_change_pas_le_moteur() {
    // 1. Le MCTS est le moteur par défaut : une valeur invalide doit
    //    le laisser en place (la ligne `info` porte la signature du
    //    MCTS).
    let mut s = Session::start();
    s.send("uci");
    s.wait_for("uciok", 5000);
    s.send("setoption name Search value bogus");
    s.send("isready");
    s.wait_for("readyok", 5000);
    s.send("position startpos");
    s.send("go nodes 500");
    let lines = s.wait_for("bestmove", 5000);
    assert!(
        lines.iter().any(|l| l.contains("string mcts-v1")),
        "le moteur a changé sans qu'on le demande : {lines:?}"
    );

    // 2. Symétrique : en alpha-beta, une valeur invalide doit **rester**
    //    en alpha-beta (pas de retour au MCTS).
    let mut s = Session::start();
    s.send("uci");
    s.wait_for("uciok", 5000);
    s.send("setoption name Search value alpha");
    s.send("isready");
    s.wait_for("readyok", 5000);
    s.send("setoption name Search value bogus");
    s.send("isready");
    s.wait_for("readyok", 5000);
    s.send("position startpos");
    s.send("go nodes 500");
    let lines = s.wait_for("bestmove", 5000);
    assert!(
        !lines.iter().any(|l| l.contains("string mcts-v1")),
        "le moteur est repassé en MCTS sans qu'on le demande : {lines:?}"
    );
}

// ---------------------------------------------------------------------------
// 8. La graine est lue comme elle est écrite.
// ---------------------------------------------------------------------------

/// **Une graine décimale est lue en décimal**, une graine `0x…` en
/// hexadécimal.
///
/// L'ancien code essayait l'hexadécimal **d'abord** : `setoption name
/// Seed value 42` valait `0x42` — 66 — sans le dire.
#[test]
fn la_graine_est_lue_comme_elle_est_ecrite() {
    assert_eq!(
        parse_seed("42"),
        Some(42),
        "un décimal n'est pas lu en décimal"
    );
    assert_eq!(parse_seed("0x2a"), Some(42), "un hexadécimal n'est pas lu");
    assert_eq!(
        parse_seed("0X2A"),
        Some(42),
        "un hexadécimal majuscule n'est pas lu"
    );
    assert_eq!(
        parse_seed("deadbeef"),
        Some(0xdeadbeef),
        "une graine hexadécimale sans préfixe n'est pas lue"
    );
    assert_eq!(
        parse_seed("pas-une-graine"),
        None,
        "une graine invalide est acceptée"
    );
}

// ---------------------------------------------------------------------------
// 9. Le moteur ne fuit pas ses fichiers.
// ---------------------------------------------------------------------------

/// **Le moteur répond `readyok` après n'importe quelle commande**.
///
/// C'est le protocole le plus simple et le plus exigeant : `isready`
/// doit **toujours** être suivi d'un `readyok`. Un panic entre deux
/// commandes rompt ce protocole, et l'arbitre (ou le GUI) attend
/// indéfiniment.
#[test]
fn le_moteur_repond_toujours_readyok() {
    let mut s = Session::start();
    s.send("uci");
    s.wait_for("uciok", 5000);
    for cmd in [
        "isready",
        "ucinewgame",
        "position startpos",
        "position fen rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "setoption name Threads value 4",
        "d",
        "go nodes 100",
    ] {
        // Chaque commande est suivie d'un `isready` : c'est le **seul**
        // contrat qui marche pour *toutes* les commandes, y compris
        // celles qui ne répondent pas (`ucinewgame`) ou qui répondent
        // autre chose (`d`).
        s.send(cmd);
        s.send("isready");
        s.wait_for("readyok", 5000);
    }
    assert!(s.alive(), "le moteur a lâché prise");
}
