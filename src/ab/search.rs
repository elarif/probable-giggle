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

/// La capacité du tampon d'ordre des coups (phase perf).
///
/// Elle est égale à la capacité de la `MoveList` du core (256) : une
/// position légale n'a jamais plus de 218 coups, la liste du core ne
/// peut donc pas en contenir plus. Le tampon est indexé par des `u8`,
/// d'où la limite à 256.
const MOVE_CAP: usize = 256;

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

/// Les **options de recherche** de l'alpha-beta (phase AB8,
/// `docs/ab/plan-heuristiques-2026-10-23.md`).
///
/// Les trois premières briques — PVS, killer moves, heuristique
/// d'historique — sont celles de la phase AB gelée : elles sont
/// **actives par défaut**, et le chemin par défaut est donc celui de la
/// phase AB (PVS + killers + historique).
///
/// Les trois suivantes — null-move, razoring/futility, LMR — sont
/// **nouvelles** et **inactives par défaut** : chacune doit gagner sa
/// place sur des **mesures** (débit, profondeur, score, amplification
/// du bruit), pas sur une réputation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// **PVS** (principal variation search) : les coups suivants la
    /// racine sont cherchés à fenêtre nulle, puis re-cherchés si le
    /// coup dépasse `alpha`.
    pub pvs: bool,
    /// **Killer moves** : les deux coups qui ont causé une coupe à
    /// cette profondeur sont essayés en tête de l'ordre.
    pub killers: bool,
    /// **Heuristique d'historique** : un bonus incrémental par
    /// couple (départ, arrivée).
    pub history: bool,
    /// **Null-move** : un coup gratuit (`R = 2`) est offert à
    /// l'adversaire ; s'il ne peut pas gagner, la position est
    /// gagnante à cette profondeur.
    pub null_move: bool,
    /// **Razoring** : à faible profondeur, un score statique très bas
    /// est vérifié à profondeur réduite avant d'être rendu.
    pub razoring: bool,
    /// **Futility** : à faible profondeur, un score statique très haut
    /// est rendu sans chercher les coups.
    pub futility: bool,
    /// **LMR** (late move reductions) : les coups tranquilles, loin dans
    /// l'ordre, sont cherchés à profondeur réduite, puis re-cherchés à
    /// pleine profondeur si le coup promet.
    pub lmr: bool,
}

impl Default for Options {
    /// Le **défaut** est le chemin de la phase AB gelée : PVS + killers
    /// + historique, et rien de nouveau. Les mesures déjà publiées
    /// (duel de la phase AB, `examples/eval_error`) restent donc
    /// reproductibles.
    fn default() -> Self {
        Options {
            pvs: true,
            killers: true,
            history: true,
            null_move: false,
            razoring: false,
            futility: false,
            lmr: false,
        }
    }
}

/// La profondeur minimale du **null-move** : en dessous, un coup gratuit
/// n'apporte rien à l'élagage.
const NULL_MIN_DEPTH: i32 = 3;
/// La réduction appliquée au null-move (`R = 2`).
const NULL_REDUCTION: i32 = 2;
/// La profondeur maximale à laquelle le **razoring** et la **futility**
/// s'appliquent.
const SHALLOW_MAX_DEPTH: i32 = 2;
/// La marge du **razoring** : sous `alpha − marge`, le score statique est
/// si bas qu'une vérification à profondeur réduite suffit.
const RAZOR_MARGIN: i32 = 300;
/// La marge de la **futility** : au-dessus de `beta + marge`, le score
/// statique est si haut que les coups tranquilles ne peuvent pas le
/// faire perdre.
const FUTILITY_MARGIN: i32 = 200;
/// La **zone sûre** du LMR : au-delà de cette marge (en centipions), la
/// position est si favorable — ou si défavorable — que la profondeur
/// gagnée ne change plus la décision. Le LMR n'y réduit pas.
const LMR_SAFE_LIMIT: i32 = 1000;

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
    /// Les **options de recherche** (phase AB8) : chaque heuristique est
    /// activable **seule**, pour être mesurée seule.
    options: Options,
}

impl Search {
    pub fn new(pos: Position, game_hist: Vec<u64>, limits: Limits) -> Self {
        Search::with_options(pos, game_hist, limits, Options::default())
    }

    /// La même recherche, avec des **options de recherche** explicites
    /// (phase AB8, `docs/ab/plan-heuristiques-2026-10-23.md`).
    ///
    /// C'est le point d'entrée des mesures : le reste du moteur est
    /// inchangé, seule la liste des briques actives change.
    pub fn with_options(
        pos: Position,
        game_hist: Vec<u64>,
        limits: Limits,
        options: Options,
    ) -> Self {
        Search {
            pos,
            game_hist,
            limits,
            options,
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
            scores: (0..=MAX_PLY + 1).map(|_| [0i32; MOVE_CAP]).collect(),
            pv_table: (0..=MAX_PLY).map(|_| Vec::new()).collect(),
            nodes: 0,
            node_limit: self.limits.nodes,
            deadline: None,
            stop,
            aborted: false,
            opts: self.options,
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
    ///
    /// L'échelle est calibrée pour que le bonus soit **visible** dans
    /// l'ordre des coups : l'incrément vaut `profondeur² × 4` et le
    /// bonus lu est divisé par 8. (Avant la phase AB8, le bonus était
    /// divisé par 1024 : à la profondeur atteinte — 4 à 5 plies — le
    /// bonus valait **toujours zéro**, et l'heuristique d'historique
    /// était du code mort. C'est la décomposition d'AB4 qui l'a
    /// révélé.)
    history: Vec<u32>,
    /// Le tampon des **scores d'ordre**, un par ply (phase perf).
    ///
    /// Avant, chaque nœud allouait un `Vec<(i32, Move)>` sur le tas
    /// (malloc + free + le tampon temporaire du tri stable) : à un
    /// million de nœuds, c'est un million d'allocations. Le tampon est
    /// ici alloué **une fois par recherche** et réutilisé d'un nœud à
    /// l'autre.
    ///
    /// Un ply n'est pas réutilisé par une récursion plus profonde : la
    /// negamax du ply `p` n'utilise que `scores[p]`, et ses enfants
    /// utilisent `scores[p+1]`, `scores[p+2]`… La quiescence utilise le
    /// tampon du **même** ply que la negamax qui l'appelle — mais cette
    /// negamax là rend la main à la quiescence *avant* de trier ses
    /// coups, donc avant de toucher à son tampon.
    scores: Vec<[i32; MOVE_CAP]>,
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
    /// Les options de recherche (phase AB8).
    opts: Options,
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

        // Une fenêtre **nulle** (largeur 1) ne donne qu'une **borne** : le
        // score qui en sort ne peut pas être stocké comme `Exact`.
        //
        // C'est la correction qui rend le **LMR** et le **razoring**
        // utilisables : une recherche à fenêtre nulle qui dépasse `beta`
        // donne une borne *inférieure*, pas la valeur exacte de la
        // position. Stocker cette borne comme exacte ferait falsifier
        // les transpositions Ultérieures.
        let null_window = beta == alpha + 1;
        let beta_orig = beta;

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
            let (sc, bm, fl) = self.quiescence(alpha, beta, ply);
            self.tt
                .store(key, 0, score_to_tt(sc, ply) as i16, fl, bm);
            return sc;
        }

        // ---- les élagages « de surface » (phase AB8) ----
        //
        // L'indicateur « en échec » est un test d'attaque : il n'est
        // calculé que si une heuristique en a besoin.
        let in_check = if self.opts.null_move
            || self.opts.razoring
            || self.opts.futility
            || self.opts.lmr
        {
            self.pos.in_check(self.pos.side)
        } else {
            false
        };

        // **Futility** : le score statique est si haut que, même en
        // donnant une pièce, la position resterait au-dessus de `beta`.
        // On rend `beta` sans chercher les coups.
        //
        // **Garde-fou « capture »** : une heuristique qui élague sur
        // l'évaluation statique ne doit pas s'appliquer quand une
        // **capture** est disponible : l'évaluation statique compte les
        // pièces *comme si elles étaient vivantes*, et c'est
        // précisément là qu'elle se trompe le plus. Sur une position
        // comme « une dame sous les yeux mais non encore prise », la
        // futilité rendrait `beta` et perdrait la dame.
        //
        // **Garde-fou « fenêtre de mat »** : un score de mat n'est pas
        // comparable à une marge en centipions. Élaguer sur
        // l'évaluation dans une fenêtre de mat fait **perdre le mat** :
        // le razoring rendrait `alpha` (un score de mat) sans jamais
        // voir la séquence qui le produit.
        let mate_window =
            alpha.abs() >= MATE - MAX_PLY as i32 || beta.abs() >= MATE - MAX_PLY as i32;
        if self.opts.futility
            && depth <= SHALLOW_MAX_DEPTH
            && !in_check
            && !mate_window
            && !self.has_capture()
        {
            let stand = eval::eval_for_mover(&self.pos);
            if stand - FUTILITY_MARGIN >= beta {
                // La borne n'est **pas** stockée : elle repose sur
                // l'évaluation statique, pas sur une recherche. La
                // stocker comme exacte tronquerait les transpositions
                // ultérieures. (Le coût : la position est re-cherchée
                // quand elle est atteinte à nouveau.)
                return beta;
            }
        }

        // **Razoring** : le score statique est si bas qu'il vaut la peine
        // de le vérifier à profondeur réduite avant de le rendre. Si la
        // vérification échoue (`fail low`), l.eval est confirmé : la
        // position est mauvaise pour le trait, on rend `alpha`.
        //
        // Mêmes garde-fous que pour la futilité (« capture » et
        // « fenêtre de mat »).
        if self.opts.razoring
            && depth <= SHALLOW_MAX_DEPTH
            && !in_check
            && !mate_window
            && !self.has_capture()
        {
            let stand = eval::eval_for_mover(&self.pos);
            if stand + RAZOR_MARGIN < alpha {
                let s = self.negamax(alpha, alpha + 1, depth - 1, ply);
                if self.aborted {
                    return 0;
                }
                if s <= alpha {
                    // La borne est stockée à la **profondeur réduite** :
                    // c'est le seul niveau où elle a été prouvée.
                    self.tt.store(
                        key,
                        (depth - 1) as i16,
                        score_to_tt(alpha, ply) as i16,
                        Flag::Upper,
                        Move::NONE,
                    );
                    return alpha;
                }
            }
        }

        // **Null-move** : on offre un coup gratuit à l'adversaire. Si la
        // position reste bonne pour le trait malgré ce cadeau, elle est
        // gagnante à cette profondeur : on rend `beta`.
        //
        // Deux garde-fous : la position ne doit pas être en échec (un
        // échec rend le coup gratuit illegal), et le trait doit avoir
        // autre chose que des pions (sinon la partie de pions est un
        // **zugzwang** : « passer » y est un perte).
        if self.opts.null_move
            && depth >= NULL_MIN_DEPTH
            && !in_check
            && self.pos.ep == crate::types::NO_EP
            && self.has_non_pawn_material()
        {
            let saved_ep = self.make_null();
            let s = -self.negamax(-beta, -beta + 1, depth - 1 - NULL_REDUCTION, ply + 1);
            self.unmake_null(saved_ep);
            if self.aborted {
                return 0;
            }
            if s >= beta {
                // La borne est stockée à la **profondeur réduite** : c'est
                // le seul niveau où la recherche a été faite.
                self.tt.store(
                    key,
                    (depth - 1 - NULL_REDUCTION) as i16,
                    score_to_tt(beta, ply) as i16,
                    Flag::Lower,
                    Move::NONE,
                );
                return beta;
            }
        }

        // L'ordre des coups (AB4) : coup de la TT, puis MVV-LVA, puis
        // les killer moves, puis les coups tranquilles classés par
        // l'heuristique d'historique. L'ordre **est** le premier levier
        // de débit d'un alpha-beta.
        let killer0 = if self.opts.killers {
            self.killers[ply][0]
        } else {
            Move::NONE
        };
        let killer1 = if self.opts.killers {
            self.killers[ply][1]
        } else {
            Move::NONE
        };
        // **Pas d'allocation par nœud** (phase perf, 2026-10-23).
        //
        // Les scores vivent dans un tampon **par ply**, réutilisé d'un
        // nœud à l'autre ; l'ordre est un tableau d'**indices** sur la
        // pile. Le tri est *instable* — mais sur un ordre **total**
        // (score décroissant, puis indice croissant) : le résultat est
        // donc **identique** à celui du tri stable d'avant, coup par
        // coup. C'est ce qui permet de supprimer l'allocation *et* le
        // tampon temporaire du tri.
        //
        // L'emprunt est scindé (`&self.pos`, `&self.history`,
        // `&mut self.scores[ply]`) : ce sont trois champs disjoints.
        let n = list.len();
        debug_assert!(n <= MOVE_CAP, "plus de coups que le tampon d'ordre");
        let use_history = self.opts.history;
        {
            let pos = &self.pos;
            let history = &self.history;
            let scores = &mut self.scores[ply];
            for i in 0..n {
                let mv = list.get(i);
                let mut sc = mvv_lva(pos, mv);
                if mv == tt_best && tt_best != Move::NONE {
                    sc += 1_000_000;
                } else if mv == killer0 || mv == killer1 {
                    sc += 50_000;
                } else if use_history && !mv.is_capture() {
                    sc += (history[history_index(mv)] as i32) / 8;
                }
                scores[i] = sc;
            }
        }
        let mut order = [0u8; MOVE_CAP];
        for i in 0..n {
            order[i] = i as u8;
        }
        {
            let scores = &self.scores[ply];
            order[..n].sort_unstable_by(|&a, &b| {
                scores[b as usize]
                    .cmp(&scores[a as usize])
                    .then(a.cmp(&b))
            });
        }

        let alpha_orig = alpha;
        let mut best_score = -MATE - 1;
        let mut best_move = Move::NONE;
        let mut flag = Flag::Upper;

        for (index, &idx) in order[..n].iter().enumerate() {
            let mv = list.get(idx as usize);
            let key_here = self.pos.zobrist;
            let undo = self.pos.make(mv);
            self.path.push(key_here);
            // **PVS** (AB4) : le premier coup est cherché avec la fenêtre
            // pleine, les suivants avec une fenêtre **nulle** ; si le
            // résultat dépasse `alpha`, la recherche est relancée avec la
            // fenêtre pleine. C'est le gain de débit le plus rentable de
            // l'alpha-beta.
            //
            // **LMR** (phase AB8) : un coup **tranquille**, loin dans
            // l'ordre, est d'abord cherché à **profondeur réduite** ;
            // s'il promet (il dépasse `alpha`), il est rejoué à pleine
            // profondeur. Le gain de profondeur est donc **payé** seulement
            // quand le coup le mérite.
            let full_depth = depth - 1;
            // **LMR** : un coup **tranquille**, loin dans l'ordre, est
            // d'abord cherché à **profondeur réduite** ; s'il promet (il
            // dépasse `alpha`), il est rejoué à pleine profondeur. Le
            // gain de profondeur est donc **payé** seulement quand le
            // coup le mérite.
            //
            // Deux garde-fous : le LMR ne réduit pas dans une **fenêtre
            // de mat** (il perdrait le mat) ni dans une position où le
            // trait est **largement devant** (la zone « sûre » : à ce
            // niveau, la profondeur gagnée ne change pas la décision).
            let can_reduce = self.opts.lmr
                && index > 0
                && !in_check
                && !mv.is_capture()
                && !mv.is_promotion()
                && alpha.abs() < LMR_SAFE_LIMIT
                && beta.abs() < LMR_SAFE_LIMIT;
            let reduced = if can_reduce {
                (full_depth - self.lmr_reduction(depth, index)).max(1)
            } else {
                full_depth
            };
            let score = if index == 0 {
                -self.negamax(-beta, -alpha, reduced, ply + 1)
            } else if reduced < full_depth {
                // **LMR** : le coup est cherché à profondeur réduite, à
                // fenêtre nulle. S'il promet, il est rejoué à pleine
                // profondeur, avec la fenêtre pleine.
                let narrow = -self.negamax(-alpha - 1, -alpha, reduced, ply + 1);
                if narrow > alpha {
                    -self.negamax(-beta, -alpha, full_depth, ply + 1)
                } else {
                    narrow
                }
            } else if self.opts.pvs {
                let narrow = -self.negamax(-alpha - 1, -alpha, full_depth, ply + 1);
                if narrow > alpha && narrow < beta {
                    -self.negamax(-beta, -alpha, full_depth, ply + 1)
                } else {
                    narrow
                }
            } else {
                -self.negamax(-beta, -alpha, full_depth, ply + 1)
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
                    if self.opts.killers {
                        self.killers[ply] = [mv, self.killers[ply][0]];
                    }
                    if self.opts.history {
                        let idx = history_index(mv);
                        self.history[idx] =
                            self.history[idx]
                                .saturating_add(depth as u32 * depth as u32 * 4);
                    }
                }
                break;
            }
        }

        // Le drapeau de la TT : une fenêtre **nulle** ne donne qu'une
        // **borne** (inférieure si la recherche a dépassé `beta`,
        // supérieure sinon) — jamais la valeur exacte.
        if null_window {
            flag = if best_score >= beta_orig {
                Flag::Lower
            } else {
                Flag::Upper
            };
        } else if best_score <= alpha_orig {
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
    fn quiescence(&mut self, mut alpha: i32, beta: i32, ply: usize) -> (i32, Move, Flag) {
        if self.aborted {
            return (0, Move::NONE, Flag::Upper);
        }
        if self.nodes >= self.node_limit {
            self.aborted = true;
            return (0, Move::NONE, Flag::Upper);
        }
        self.nodes += 1;
        if self.nodes & NODE_CHECK_MASK == 0 {
            if self.stop.load(Ordering::Relaxed) {
                self.aborted = true;
                return (0, Move::NONE, Flag::Upper);
            }
            if let Some(d) = self.deadline
                && Instant::now() >= d
            {
                self.aborted = true;
                return (0, Move::NONE, Flag::Upper);
            }
        }
        if ply + 2 >= MAX_PLY {
            return (eval::eval_for_mover(&self.pos), Move::NONE, Flag::Exact);
        }

        // L'alpha d'entrée : il sert à distinguer un **échec** (le score
        // rendu est une borne *supérieure*) d'une valeur exacte.
        let alpha_orig = alpha;

        let in_check = self.pos.in_check(self.pos.side);
        let mut list = MoveList::new();
        if in_check {
            movegen::generate(&self.pos, &mut list, GenKind::All);
        } else {
            movegen::generate(&self.pos, &mut list, GenKind::CapturesOnly);
        }
        if list.is_empty() {
            let v = if in_check { -(MATE - ply as i32) } else { 0 };
            return (v, Move::NONE, Flag::Exact);
        }

        // The best move of the quiescence (for the PV). A stand-pat that
        // wins leaves it at `NONE`: the PV then stops there, which is the
        // honest thing to show — there is no capture to play.
        let mut best_move = Move::NONE;
        if !in_check {
            let stand = eval::eval_for_mover(&self.pos);
            if stand >= beta {
                return (stand, Move::NONE, Flag::Lower);
            }
            if stand > alpha {
                alpha = stand;
            }
        }

        // Le même traitement que dans la negamax : **aucune allocation**
        // (tampon par ply + tri sur un ordre total).
        let n = list.len();
        debug_assert!(n <= MOVE_CAP, "plus de coups que le tampon d'ordre");
        {
            let pos = &self.pos;
            let scores = &mut self.scores[ply];
            for i in 0..n {
                scores[i] = mvv_lva(pos, list.get(i));
            }
        }
        let mut order = [0u8; MOVE_CAP];
        for i in 0..n {
            order[i] = i as u8;
        }
        {
            let scores = &self.scores[ply];
            order[..n].sort_unstable_by(|&a, &b| {
                scores[b as usize]
                    .cmp(&scores[a as usize])
                    .then(a.cmp(&b))
            });
        }

        for &idx in order[..n].iter() {
            let mv = list.get(idx as usize);
            let undo = self.pos.make(mv);
            let (score, _, _) = self.quiescence(-beta, -alpha, ply + 1);
            self.pos.unmake(mv, undo);
            if self.aborted {
                return (0, Move::NONE, Flag::Upper);
            }
            if score >= beta {
                return (score, mv, Flag::Lower);
            }
            if score > alpha {
                alpha = score;
                best_move = mv;
            }
        }
        // Un échec (le score rendu est l'alpha d'entrée) n'est qu'une
        // **borne supérieure** : le stocker comme exact ferait
        // sous-estimer la position dans toutes les transpositions
        // ultérieures. C'est la même correction que pour la fenêtre
        // nulle, appliquée à la quiescence.
        let flag = if alpha <= alpha_orig {
            Flag::Upper
        } else {
            Flag::Exact
        };
        (alpha, best_move, flag)
    }


    /// Le **coup gratuit** (null-move) : le trait passe son tour.
    ///
    /// C'est implémenté **côté AB** : le core gelé n'a pas de « passe »,
    /// et il ne doit pas en avoir. On bascule donc le trait et la clé
    /// Zobrist — la clé de la position *après* le coup gratuit est celle
    /// de la position *avant*, avec le trait opposé.
    ///
    /// L'horloge des 50 coups n'est **pas** incrémentée : le coup gratuit
    /// n'est pas un coup, et le traiter comme une nulle par la règle des
    /// 50 coups produirait des coupes fausses. La case de prise en
    /// passant est neutralisée : après un coup gratuit, le droit de prise
    /// en passant est *caduc* (aucun coup réel n'a été joué) — sans quoi
    /// l'adversaire pourrait « capturer en passant » un pion qui n'a pas
    /// avancé.
    fn make_null(&mut self) -> u8 {
        let saved_ep = self.pos.ep;
        self.pos.side = self.pos.side.opp();
        self.pos.zobrist ^= crate::zobrist::ZOBRIST.side;
        self.pos.ep = crate::types::NO_EP;
        saved_ep
    }

    /// L'inverse de [`Ctx::make_null`].
    fn unmake_null(&mut self, saved_ep: u8) {
        self.pos.side = self.pos.side.opp();
        self.pos.zobrist ^= crate::zobrist::ZOBRIST.side;
        self.pos.ep = saved_ep;
    }

    /// Le trait au trait a-t-il du **matériel** hors pions ? Dans une
    /// fin de pions, un coup gratuit est un **zugzwang** : « passer »
    /// serait une perte. Le null-move y est donc désactivé.
    fn has_non_pawn_material(&self) -> bool {
        let us = self.pos.side;
        (self.pos.pieces(us, crate::types::KNIGHT)
            | self.pos.pieces(us, crate::types::BISHOP)
            | self.pos.pieces(us, crate::types::ROOK)
            | self.pos.pieces(us, crate::types::QUEEN))
            != 0
    }

    /// Le trait au trait a-t-il une **capture** disponible ?
    ///
    /// Une heuristique qui élague sur l'évaluation statique (razoring,
    /// futilité) ne doit pas s'appliquer quand une capture existe :
    /// l'évaluation statique compte les pièces *comme si elles étaient
    /// vivantes*, et c'est exactement là qu'elle se trompe le plus.
    /// Sans ce garde-fou, la futilité rendrait `beta` sur une position
    /// où une dame est « prise mais pas encore comptée comme prise ».
    fn has_capture(&self) -> bool {
        let mut list = MoveList::new();
        movegen::generate(&self.pos, &mut list, GenKind::CapturesOnly);
        !list.is_empty()
    }

    /// La **réduction de profondeur** du LMR : les coups tranquilles,
    /// loin dans l'ordre, sont cherchés moins profond.
    ///
    /// La table est volontairement simple (elle n'est pas « apprise ») :
    /// la phase mesure l'effet **moyen** de la réduction, pas sa qualité
    /// fine. Une table apprise serait une phase à part.
    fn lmr_reduction(&self, depth: i32, index: usize) -> i32 {
        let mut r = 1;
        if depth >= 4 {
            r += 1;
        }
        if depth >= 6 {
            r += 1;
        }
        if index >= 3 {
            r += 1;
        }
        if index >= 6 {
            r += 1;
        }
        r
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

/// MVV-LVA : `10 × valeur de la victime − valeur de l'agresseur`, plus
/// un bonus de promotion.
///
/// C'est une **fonction libre** (et non une méthode) pour que
/// l'appelant puisse emprunter `&self.pos` et `&mut self.scores` **en
/// même temps** : ce sont deux champs disjoints de `self`, et le
/// borrow-checker l'accepte.
fn mvv_lva(pos: &Position, mv: Move) -> i32 {
    let victim = if mv.is_ep() {
        eval::MAT_PAWN
    } else {
        let code = pos.piece_on(mv.to());
        if code == 0 {
            0
        } else {
            piece_value(((code - 1) % 6) as usize)
        }
    };
    let attacker_code = pos.piece_on(mv.from());
    let attacker = if attacker_code == 0 {
        0
    } else {
        piece_value(((attacker_code - 1) % 6) as usize)
    };
    let promo = if mv.is_promotion() { 800 } else { 0 };
    10 * victim + promo - attacker
}
