//! AB1/AB2/AB3 — l'alpha-beta du dépôt.
//!
//! Ce module est le **moteur de référence** du projet : il rend
//! mesurable l'affirmation « l'alpha-beta bat le MCTS à budget et à
//! évaluation identiques », que le tournoi du 2026-10-22 n'a pas pu
//! trancher (toutes les mesures du projet passaient par Stockfish, à
//! 950 Elo de distance).
//!
//! # Le contrat
//!
//! * **Évaluation identique** à la feuille du MCTS : `eval::eval_for_mover`
//!   (matériel + PST PeSTO-lite). C'est la condition pour que la
//!   comparaison AB-vs-MCTS soit une comparaison d'**algorithmes**.
//! * **Core gelé** : ce module n'ajoute rien à `position`, `movegen`,
//!   `types`, `zobrist`. Il vit *à côté* du MCTS, comme lui.
//! * **Score côté au trait** (convention du negamax) : la valeur rendue
//!   par l'API est celle du **joueur au trait dans la position racine**,
//!   en centipions ; un mat est `+MATE - n` (n = nombre de coups).
//!
//! # Les trois parties
//!
//! 1. **AB1** — le squelette : negamax fail-soft, fenêtre pleine,
//!    quiescence (stand-pat + captures, esquives en échec), règles de
//!    nulle (50 coups, répétition), mat/pat, ordre MVV-LVA.
//! 2. **AB2** — la table de transposition (`ab::tt`) : sondage, bornes
//!    exact/inférieure/supérieure, ajustement des scores de mat.
//! 3. **AB3** — la profondeur itérative et la gestion du temps : la
//!    meilleure itération **complète** est conservée, une itération
//!    interrompue par l'échéance est **jetée**.

use crate::ab::tt::{Flag, Tt};
use crate::eval;
use crate::movegen::{self, MoveList};
use crate::position::Position;
use crate::types::{GenKind, Move};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Le score d'un mat (côté au trait). Un mat en `n` coups vaut
/// `MATE - n`.
pub const MATE: i32 = 30_000;
/// La profondeur maximale de la pile de recherche.
pub const MAX_PLY: usize = 96;

/// Marge réservée à l'aller-retour UCI (le moteur doit répondre **avant**
/// que l'horloge de l'adversaire expire).
pub const TIME_MARGIN_MS: u128 = 5;

/// L'échéance n'est testée que tous les `1 << NODE_CHECK_BITS` nœuds :
/// `Instant::now()` coûte ~20 ns, on ne le paie pas à chaque nœud.
const NODE_CHECK_BITS: u32 = 10;
const NODE_CHECK_MASK: u64 = (1 << NODE_CHECK_BITS) - 1;

/// Le budget de la recherche.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// La profondeur maximale. `0` = « autant que le temps le permet ».
    pub max_depth: u32,
    /// Le budget **temps** (ms). `None` = pas de limite de temps.
    pub movetime_ms: Option<u128>,
    /// Le budget **nœuds**. `u64::MAX` = pas de limite.
    pub nodes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_depth: 0,
            movetime_ms: None,
            nodes: u64::MAX,
        }
    }
}

/// Le résultat d'une recherche.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// Le coup à jouer. Jamais `0000` sur une position jouable (le repli
    /// est le premier coup légal).
    pub best: Move,
    /// Le score, côté au trait, en centipions.
    pub score_cp: i32,
    /// Les nœuds visités.
    pub nodes: u64,
    /// La profondeur atteinte (la dernière itération **complète**).
    pub depth: u32,
    /// Le temps écoulé, en ms.
    pub elapsed_ms: u128,
    /// La variante principale (extraite de la TT).
    pub pv: Vec<Move>,
}

/// Une recherche d'alpha-beta.
pub struct Search {
    pos: Position,
    /// Les clés Zobrist de la partie jouée (la dernière est la position
    /// racine) — le matériel de la règle de répétition.
    game_hist: Vec<u64>,
    limits: Limits,
}

impl Search {
    pub fn new(pos: Position, game_hist: Vec<u64>, limits: Limits) -> Self {
        Search {
            pos,
            game_hist,
            limits,
        }
    }

    /// La recherche. `stop` permet l'interruption UCI : l'itération en
    /// cours est abandonnée et la **dernière itération complète** est
    /// rendue.
    pub fn run(&self, stop: &AtomicBool) -> Outcome {
        let start = Instant::now();
        let mut ctx = Ctx {
            pos: self.pos,
            game_hist: self.game_hist.clone(),
            tt: Tt::new(),
            path: Vec::with_capacity(MAX_PLY),
            killers: vec![[Move::NONE; 2]; MAX_PLY + 1],
            history: vec![0u32; 64 * 64],
            pv_table: (0..=MAX_PLY).map(|_| Vec::new()).collect(),
            nodes: 0,
            node_limit: self.limits.nodes,
            deadline: None,
            stop,
            aborted: false,
        };
        if let Some(ms) = self.limits.movetime_ms {
            let budget = ms.saturating_sub(TIME_MARGIN_MS).max(1);
            ctx.deadline = Some(start + std::time::Duration::from_millis(budget as u64));
        }

        let max_depth = if self.limits.max_depth > 0 {
            self.limits.max_depth
        } else {
            64
        };

        let mut best = Move::NONE;
        let mut score = 0;
        let mut depth_done = 0u32;
        let mut pv: Vec<Move> = Vec::new();

        for d in 1..=max_depth {
            let s = ctx.negamax(-MATE - 1, MATE + 1, d as i32, 0);
            if ctx.aborted {
                // Itération **incomplète** : on la jette (AB3). Le score
                // d'une itération interrompue est faux — c'est exactement
                // le bug que la phase 5 du MCTS a appris.
                break;
            }
            score = s;
            depth_done = d;
            pv = ctx.pv_table[0].clone();
            if let Some(first) = pv.first().copied() {
                best = first;
            }
            // Un mat trouvé ne peut pas être amélioré : on s'arrête.
            if s.abs() >= MATE - MAX_PLY as i32 {
                break;
            }
            if ctx.nodes >= ctx.node_limit {
                break;
            }
        }

        // Repli : une position jouable doit toujours recevoir un coup
        // **légal** (même contrat que le correctif UCI du 2026-10-22).
        if best == Move::NONE {
            best = first_legal_move(&self.pos);
        }

        Outcome {
            best,
            score_cp: score,
            nodes: ctx.nodes,
            depth: depth_done,
            elapsed_ms: start.elapsed().as_millis(),
            pv,
        }
    }
}

/// Le premier coup légal de la position (`0000` si elle est terminale).
pub fn first_legal_move(pos: &Position) -> Move {
    let list = movegen::generate_all(pos);
    if list.is_empty() {
        Move::NONE
    } else {
        list.get(0)
    }
}

/// L'état de la recherche (le « contexte »).
struct Ctx<'a> {
    pos: Position,
    /// Les clés Zobrist de la partie (hors position courante).
    game_hist: Vec<u64>,
    tt: Tt,
    /// Les clés Zobrist des positions **ancêtres** de la ligne de
    /// recherche (la règle de répétition).
    path: Vec<u64>,
    /// Les **killer moves** : les deux coups qui ont caused une coupe
    /// (`beta move`) à cette profondeur, les premiers essayés après les
    /// captures (AB4).
    killers: Vec<[Move; 2]>,
    /// L'**heuristique d'historique** : un bonus par couple
    /// (départ, arrivée), incrémenté quand le coup cause une coupe
    /// (AB4). `history[from * 64 + to]`.
    history: Vec<u32>,
    /// La **table PV** (ligne principale) : `pv_table[ply]` est la
    /// meilleure ligne connue depuis la profondeur `ply`. Elle est
    /// reconstruite pendant la recherche — l'extraire de la TT serait
    /// fragile (une collision d'adressage tronque la chaîne).
    pv_table: Vec<Vec<Move>>,
    nodes: u64,
    node_limit: u64,
    deadline: Option<Instant>,
    stop: &'a AtomicBool,
    aborted: bool,
}

impl<'a> Ctx<'a> {
    /// Le negamax, fenêtre `[alpha, beta)`, score côté au trait.
    fn negamax(&mut self, mut alpha: i32, mut beta: i32, depth: i32, ply: usize) -> i32 {
        if self.aborted {
            return 0;
        }
        if self.nodes >= self.node_limit {
            self.aborted = true;
            return 0;
        }
        self.nodes += 1;
        if self.nodes & NODE_CHECK_MASK == 0 {
            if self.stop.load(Ordering::Relaxed) {
                self.aborted = true;
                return 0;
            }
            if let Some(d) = self.deadline
                && Instant::now() >= d
            {
                self.aborted = true;
                return 0;
            }
        }
        if ply + 2 >= MAX_PLY {
            return eval::eval_for_mover(&self.pos);
        }

        // La ligne principale de ce nœud est reconstruite : le nœud
        // enfant la remplira à son tour.
        self.pv_table[ply].clear();

        // **Élagage par distance au mat** : si le_parent cherche un score
        // déjà meilleur qu'un mat à cette profondeur, la recherche peut
        // s'arrêter.
        let mate_at_ply = MATE - ply as i32;
        if alpha >= mate_at_ply {
            return alpha;
        }
        if beta <= -mate_at_ply {
            return beta;
        }

        // Les coups légaux : le mat et le pat se décident **ici** (avant
        // les règles de nulle : un mat gagné au 100e demi-coup est un
        // mat, pas une nulle — même convention que `mcts::terminal_value`).
        let mut list = MoveList::new();
        movegen::generate(&self.pos, &mut list, GenKind::All);
        if list.is_empty() {
            return if self.pos.in_check(self.pos.side) {
                -mate_at_ply
            } else {
                0
            };
        }

        // Les règles de nulle.
        if self.pos.halfmove >= 100 || self.is_repetition() {
            return 0;
        }

        let key = self.pos.zobrist;
        let mut tt_best = Move::NONE;
        if let Some(p) = self.tt.probe(key) {
            tt_best = p.best;
            if p.depth as i32 >= depth {
                let s = score_from_tt(p.score as i32, ply);
                match p.flag {
                    Flag::Exact => return s,
                    Flag::Lower => {
                        if s >= beta {
                            return s;
                        }
                        if s > alpha {
                            alpha = s;
                        }
                    }
                    Flag::Upper => {
                        if s <= alpha {
                            return s;
                        }
                        if s < beta {
                            beta = s;
                        }
                    }
                }
            }
        }

        if depth <= 0 {
            // The frontier node is **stored too**: the PV is extracted from
            // the chain of TT entries, and a frontier that is not stored
            // truncates the PV to a single move.
            let (sc, bm) = self.quiescence(alpha, beta, ply);
            self.tt
                .store(key, 0, score_to_tt(sc, ply) as i16, Flag::Exact, bm);
            return sc;
        }

        // L'ordre des coups (AB4) : coup de la TT, puis MVV-LVA, puis
        // les killer moves, puis les coups tranquilles classés par
        // l'heuristique d'historique. L'ordre **est** le premier levier
        // de débit d'un alpha-beta.
        let killer0 = self.killers[ply][0];
        let killer1 = self.killers[ply][1];
        let mut moves: Vec<(i32, Move)> = Vec::with_capacity(list.len());
        for i in 0..list.len() {
            let mv = list.get(i);
            let mut sc = self.mvv_lva(mv);
            if mv == tt_best && tt_best != Move::NONE {
                sc += 1_000_000;
            } else if mv == killer0 || mv == killer1 {
                sc += 50_000;
            } else if !mv.is_capture() {
                sc += (self.history[history_index(mv)] as i32) / 1024;
            }
            moves.push((sc, mv));
        }
        moves.sort_by(|a, b| b.0.cmp(&a.0));

        let alpha_orig = alpha;
        let mut best_score = -MATE - 1;
        let mut best_move = Move::NONE;
        let mut flag = Flag::Upper;

        for (index, (_, mv)) in moves.iter().enumerate() {
            let mv = *mv;
            let key_here = self.pos.zobrist;
            let undo = self.pos.make(mv);
            self.path.push(key_here);
            // **PVS** (AB4) : le premier coup est cherché avec la fenêtre
            // pleine, les suivants avec une fenêtre **nulle** ; si le
            // résultat dépasse `alpha`, la recherche est relancée avec la
            // fenêtre pleine. C'est le gain de débit le plus rentable de
            // l'alpha-beta.
            let score = if index == 0 {
                -self.negamax(-beta, -alpha, depth - 1, ply + 1)
            } else {
                let narrow = -self.negamax(-alpha - 1, -alpha, depth - 1, ply + 1);
                if narrow > alpha && narrow < beta {
                    -self.negamax(-beta, -alpha, depth - 1, ply + 1)
                } else {
                    narrow
                }
            };
            self.path.pop();
            self.pos.unmake(mv, undo);
            if self.aborted {
                return 0;
            }
            if score > best_score {
                best_score = score;
                best_move = mv;
                // La ligne principale : le coup courant suivi de la
                // ligne de l'enfant.
                self.pv_table[ply].clear();
                self.pv_table[ply].push(mv);
                let child = self.pv_table[ply + 1].clone();
                self.pv_table[ply].extend_from_slice(&child);
            }
            if score > alpha {
                alpha = score;
            }
            if alpha >= beta {
                flag = Flag::Lower;
                // Le coup qui coupe est un **killer move** et alimente
                // l'historique : les deux heuristiques apprennent des
                // coups qui ont fait gagner du matériel.
                if !mv.is_capture() {
                    self.killers[ply] = [mv, self.killers[ply][0]];
                    let idx = history_index(mv);
                    self.history[idx] =
                        self.history[idx].saturating_add(depth as u32 * depth as u32);
                }
                break;
            }
        }

        if best_score <= alpha_orig {
            flag = Flag::Upper;
        } else if flag != Flag::Lower {
            flag = Flag::Exact;
        }

        self.tt.store(
            key,
            depth as i16,
            score_to_tt(best_score, ply) as i16,
            flag,
            best_move,
        );
        best_score
    }

    /// La quiescence : stand-pat + captures ; en échec, les esquives.
    ///
    /// L'évaluation n'est **jamais** lue sur une position « agitée » :
    /// c'est l'effet horizon que la phase 5 du MCTS a voulu corriger
    /// (et qui a coûté −118 à −241 Elo).
    fn quiescence(&mut self, mut alpha: i32, beta: i32, ply: usize) -> (i32, Move) {
        if self.aborted {
            return (0, Move::NONE);
        }
        if self.nodes >= self.node_limit {
            self.aborted = true;
            return (0, Move::NONE);
        }
        self.nodes += 1;
        if self.nodes & NODE_CHECK_MASK == 0 {
            if self.stop.load(Ordering::Relaxed) {
                self.aborted = true;
                return (0, Move::NONE);
            }
            if let Some(d) = self.deadline
                && Instant::now() >= d
            {
                self.aborted = true;
                return (0, Move::NONE);
            }
        }
        if ply + 2 >= MAX_PLY {
            return (eval::eval_for_mover(&self.pos), Move::NONE);
        }

        let in_check = self.pos.in_check(self.pos.side);
        let mut list = MoveList::new();
        if in_check {
            movegen::generate(&self.pos, &mut list, GenKind::All);
        } else {
            movegen::generate(&self.pos, &mut list, GenKind::CapturesOnly);
        }
        if list.is_empty() {
            let v = if in_check { -(MATE - ply as i32) } else { 0 };
            return (v, Move::NONE);
        }

        // The best move of the quiescence (for the PV). A stand-pat that
        // wins leaves it at `NONE`: the PV then stops there, which is the
        // honest thing to show — there is no capture to play.
        let mut best_move = Move::NONE;
        if !in_check {
            let stand = eval::eval_for_mover(&self.pos);
            if stand >= beta {
                return (stand, Move::NONE);
            }
            if stand > alpha {
                alpha = stand;
            }
        }

        let mut moves: Vec<(i32, Move)> = Vec::with_capacity(list.len());
        for i in 0..list.len() {
            let mv = list.get(i);
            moves.push((self.mvv_lva(mv), mv));
        }
        moves.sort_by(|a, b| b.0.cmp(&a.0));

        for (_, mv) in moves {
            let undo = self.pos.make(mv);
            let (score, _) = self.quiescence(-beta, -alpha, ply + 1);
            self.pos.unmake(mv, undo);
            if self.aborted {
                return (0, Move::NONE);
            }
            if score >= beta {
                return (score, mv);
            }
            if score > alpha {
                alpha = score;
                best_move = mv;
            }
        }
        (alpha, best_move)
    }

    /// MVV-LVA : `10 × valeur de la victime − valeur de l'agresseur`,
    /// plus un bonus de promotion. L'ordre des coups est *le* premier
    /// levier de débit d'un alpha-beta.
    fn mvv_lva(&self, mv: Move) -> i32 {
        let victim = if mv.is_ep() {
            eval::MAT_PAWN
        } else {
            let code = self.pos.piece_on(mv.to());
            if code == 0 {
                0
            } else {
                piece_value(((code - 1) % 6) as usize)
            }
        };
        let attacker_code = self.pos.piece_on(mv.from());
        let attacker = if attacker_code == 0 {
            0
        } else {
            piece_value(((attacker_code - 1) % 6) as usize)
        };
        let promo = if mv.is_promotion() { 800 } else { 0 };
        10 * victim + promo - attacker
    }

    /// La position courante est-elle une **répétition** ? (même joueur au
    /// trait, mêmes droits de roque, même case de prise en passant —
    /// c'est ce que la clé Zobrist garantit.)
    fn is_repetition(&self) -> bool {
        let key = self.pos.zobrist;
        // La ligne de recherche : les ancêtres.
        for &k in self.path.iter() {
            if k == key {
                return true;
            }
        }
        // L'historique de la partie, **hors** la position courante (la
        // dernière entrée est la position racine).
        //
        // Seules les **100 dernières positions** sont regardées : au-delà,
        // une répétition est impossible (la règle des 50 coups borne la
        // fenêtre de répétition à 100 demi-coups). Sans cette borne, le
        // coût de la recherche était linéaire en la longueur de la
        // partie — à 300 coups, chaque nœud payait 300 comparaisons.
        for &k in self.game_hist.iter().rev().skip(1).take(100) {
            if k == key {
                return true;
            }
        }
        false
    }
}

/// L'index de la table d'historique d'un coup (`from * 64 + to`).
fn history_index(mv: Move) -> usize {
    mv.from() as usize * 64 + mv.to() as usize
}

/// La valeur d'une pièce, en centipions (le roi n'est jamais capturé :
/// sa valeur est un garde-fou).
fn piece_value(kind: usize) -> i32 {
    match kind {
        0 => eval::MAT_PAWN,
        1 => eval::MAT_KNIGHT,
        2 => eval::MAT_BISHOP,
        3 => eval::MAT_ROOK,
        4 => eval::MAT_QUEEN,
        _ => 20_000,
    }
}

/// L'ajustement des scores de mat au stockage en TT : un score de mat
/// encode la **distance** au mat, or la distance dépend de la ply. On la
/// ramène à une référence stable avant de stocker, on la restaure à la
/// lecture.
fn score_to_tt(score: i32, ply: usize) -> i32 {
    if score >= MATE - MAX_PLY as i32 {
        score + ply as i32
    } else if score <= -MATE + MAX_PLY as i32 {
        score - ply as i32
    } else {
        score
    }
}

fn score_from_tt(score: i32, ply: usize) -> i32 {
    if score >= MATE - MAX_PLY as i32 {
        score - ply as i32
    } else if score <= -MATE + MAX_PLY as i32 {
        score + ply as i32
    } else {
        score
    }
}
