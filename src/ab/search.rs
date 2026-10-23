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
///
/// Elle est **appliquée par la couche UCI** (`uci::TIME_MARGIN_MS`) :
/// l'alpha-beta ne la soustrait pas une seconde fois, sinon le moteur
/// réfléchissait 5 ms de moins que le budget annoncé — et 5 ms de moins
/// que le MCTS, sur le même budget.
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
/// Le nombre maximal de **prises** que la quiescence explore.
///
/// # Pourquoi cette borne est nécessaire
///
/// La quiescence explore les prises **en série**, sans limite. Sur une
/// position riche en pions promouvables, l'arbre des séries de prises
/// est **combinatoire** : huit pions sur la 7ᵉ rangée ont chacun
/// plusieurs promotions, et chaque promotion **ajoute** une pièce
/// (elle n'en retire pas) — le nombre de pièces ne décroît donc pas, et
/// la borne « le matériel décroît » ne tient pas.
///
/// Mesuré (revue du 2026-10-23) : sur la position
/// `R3K2R/PPPBBPPP/2N2Q1p/1p2P3/3PN3/bn2pnp1/p1ppqpb1/r3k2r b KQkq - 0 1`
/// (kiwipete retourné), la **profondeur 1** ne se terminait pas après
/// 2 000 000 de nœuds : la recherche ne finissait pas, et le moteur
/// **bloquait** — une partie perdue sur le temps.
///
/// La borne est le compromis usuel : huit prises suffisent à résoudre
/// les combinaisons tactiques, et au-delà la valeur est dans le bruit.
const MAX_QUIESCENCE_CAPTURES: usize = 8;

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
        let mut tt = Tt::new();
        self.run_with_tt(stop, &mut tt)
    }

    /// La recherche, avec une **table de transposition partagée**.
    ///
    /// La table est normalement **créée par la recherche** (une table
    /// neuve à chaque `go`). Cette variante permet d'en **réutiliser
    /// une** — c'est ce qui rend testable l'invariant « la table de
    /// transposition ne change **pas** la valeur » : on peut purger la
    /// table entre deux recherches et comparer les scores.
    ///
    /// Sans cette variante, le test existant (`ab_tt_does_not_change_the_score`)
    /// comparait **deux recherches neuves** (donc deux tables neuves) :
    /// il ne vérifiait rien.
    pub fn run_with_tt(&self, stop: &AtomicBool, tt: &mut Tt) -> Outcome {
        let start = Instant::now();
        let mut ctx = Ctx {
            pos: self.pos,
            game_hist: self.game_hist.clone(),
            tt,
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
            // Le budget est pris **tel quel** : la marge d'aller-retour
            // est réservée par la couche UCI (`uci::TIME_MARGIN_MS`),
            // qui la soustrait une seule fois. La soustraire **encore**
            // ici retirait 5 ms de plus au moteur — et, surtout, rendait
            // l'alpha-beta et le MCTS **incomparables** (le MCTS, lui,
            // ne soustrait rien) : toute mesure de phase qui compare les
            // deux algorithmes était faussée de 5 ms.
            ctx.deadline = Some(start + std::time::Duration::from_millis(ms.max(1) as u64));
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

/// La valeur de la **quiescence** d'une position, seule (sans la negamax
/// au-dessus).
///
/// C'est une **sonde** : elle expose la feuille de l'alpha-beta pour que
/// les tests puissent vérifier son contrat sans passer par une
/// recherche complète. Le contrat qui compte : **la valeur d'une
/// position terminale ne dépend pas de la fenêtre** — un pat vaut 0
/// quelle que soit la fenêtre, parce que la terminalité est décidée
/// *avant* toute comparaison de scores.
///
/// Elle appelle exactement le même code que la recherche.
pub fn quiescence_of(pos: &Position, alpha: i32, beta: i32) -> i32 {
    let stop = AtomicBool::new(false);
    let mut tt = Tt::new();
    let mut ctx = Ctx {
        pos: *pos,
        game_hist: vec![pos.zobrist],
        tt: &mut tt,
        path: Vec::with_capacity(MAX_PLY),
        killers: vec![[Move::NONE; 2]; MAX_PLY + 1],
        history: vec![0u32; 64 * 64],
        scores: (0..=MAX_PLY + 1).map(|_| [0i32; MOVE_CAP]).collect(),
        pv_table: (0..=MAX_PLY).map(|_| Vec::new()).collect(),
        nodes: 0,
        node_limit: u64::MAX,
        deadline: None,
        stop: &stop,
        aborted: false,
        opts: Options::default(),
    };
    let (score, _, _) = ctx.quiescence(alpha, beta, 0, 0);
    score
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
struct Ctx<'a, 'b> {
    pos: Position,
    /// Les clés Zobrist de la partie (hors position courante).
    game_hist: Vec<u64>,
    /// La table de transposition : elle peut être **partagée** entre
    /// plusieurs recherches (voir [`Search::run_with_tt`]).
    tt: &'b mut Tt,
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

impl<'a, 'b> Ctx<'a, 'b> {
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
            return crate::nn::leaf_cp(&self.pos);
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
        //
        // La génération est **échelonnée** : les coups **tactiques**
        // (prises, promotions) d'abord, les coups **tranquilles** ensuite
        // — et ceux-ci ne sont générés que si les tactiques n'ont pas
        // suffi. Le générateur produisait jusqu'à 35 coups tranquilles
        // inutiles à *chaque* nœud, alors que la plupart des nœuds
        // coupent sur le premier coup.
        let mut list = MoveList::new();
        movegen::generate(&self.pos, &mut list, GenKind::CapturesOnly);
        let n_tactical = list.len();
        // Les coups tranquilles sont-ils déjà dans la liste ? (oui si
        // la position n'a aucun coup tactique : il faut alors les
        //_generate_ pour décider si la position est jouable.)
        let mut quiets_done = false;
        let mut phase_end;
        if n_tactical == 0 {
            movegen::generate(&self.pos, &mut list, GenKind::QuietsOnly);
            if list.is_empty() {
                return if self.pos.in_check(self.pos.side) {
                    -mate_at_ply
                } else {
                    0
                };
            }
            quiets_done = true;
            phase_end = list.len();
        } else {
            phase_end = n_tactical;
        }

        // Les règles de nulle.
        //
        // Le **matériel insuffisant** (K contre K, K+B contre K) est une
        // nulle : le MCTS l'applique (`mcts::terminal_value`), l'alpha-beta
        // non — les deux moteurs n'évaluaient donc pas la même position,
        // et le contrat de la phase AB (« la même évaluation pour les
        // deux algorithmes ») était rompu.
        if self.pos.halfmove >= 100 || self.is_repetition() || eval::insufficient_material(&self.pos)
        {
            return 0;
        }

        let key = self.pos.zobrist;
        let mut tt_best = Move::NONE;
        if let Some(p) = self.tt.probe(key) {
            tt_best = p.best;
            if p.depth as i32 >= depth {
                let s = score_from_tt(p.score as i32, ply);
                match p.flag {
                    Flag::Exact => {
                        // **La PV est semée avec le coup de la table.**
                        //
                        // Sans cela, une transposition rendue à la racine
                        // laissait la PV **vide** : le moteur rendait
                        // alors le repli (« le premier coup légal ») au
                        // lieu du meilleur coup connu — un coup
                        // arbitraire, joué avec la bonne valeur. C'est
                        // visible dès qu'une table est partagée entre
                        // deux recherches (le test
                        // `la_table_de_transposition_ne_change_pas_la_valeur`).
                        self.seed_pv(ply, p.best);
                        return s;
                    }
                    Flag::Lower => {
                        if s >= beta {
                            self.seed_pv(ply, p.best);
                            return s;
                        }
                        if s > alpha {
                            alpha = s;
                        }
                    }
                    Flag::Upper => {
                        if s <= alpha {
                            self.seed_pv(ply, p.best);
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
            let (sc, bm, fl) = self.quiescence(alpha, beta, ply, 0);
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
            && !self.opponent_has_capture()
        {
            let stand = crate::nn::leaf_cp(&self.pos);
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
            && !self.opponent_has_capture()
        {
            let stand = crate::nn::leaf_cp(&self.pos);
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
            && self.has_heavy_material()
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
        let mut order = [0u8; MOVE_CAP];
        self.score_and_order(ply, &list, 0, phase_end, &mut order, tt_best, killer0, killer1);

        let alpha_orig = alpha;
        let mut best_score = -MATE - 1;
        let mut best_move = Move::NONE;
        let mut flag = Flag::Upper;

        // La boucle parcourt les coups **par phases** : les tactiques
        // d'abord, puis — si aucune coupe n'est survenue — les
        // tranquilles, générés à la demande.
        let mut index = 0usize;
        loop {
            if index == phase_end {
                // La phase est épuisée : on passe aux coups tranquilles.
                if quiets_done {
                    break;
                }
                quiets_done = true;
                let start = list.len();
                movegen::generate(&self.pos, &mut list, GenKind::QuietsOnly);
                let end = list.len();
                if end == start {
                    break;
                }
                self.score_and_order(ply, &list, start, end, &mut order, tt_best, killer0, killer1);
                phase_end = end;
                if index == phase_end {
                    break;
                }
            }
            let idx = order[index];
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
            let can_reduce = lmr_can_reduce(
                &self.opts, mv, tt_best, index, ply, in_check, alpha, beta,
            );
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
            // Le coup suivant de la phase.
            index += 1;
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
    fn quiescence(
        &mut self,
        mut alpha: i32,
        beta: i32,
        ply: usize,
        captures: usize,
    ) -> (i32, Move, Flag) {
        if self.aborted {
            return (0, Move::NONE, Flag::Upper);
        }
        // **Garde-fou de profondeur** : la quiescence n'a pas de borne
        // de ply propre (elle n'est pas bornée par la profondeur
        // itérative). Sans ce filet, une série de prises-promotions
        // pourrait déborder la pile.
        if ply + 2 >= MAX_PLY {
            return (crate::nn::leaf_cp(&self.pos), Move::NONE, Flag::Exact);
        }
        // **Borne des prises** : au-delà, la position est rendue à sa
        // valeur statique (le « stand-pat »). C'est la borne qui rend
        // la quiescence **finie** sur les positions à promotions
        // combinatoires (cf. `MAX_QUIESCENCE_CAPTURES`).
        if captures >= MAX_QUIESCENCE_CAPTURES {
            return (crate::nn::leaf_cp(&self.pos), Move::NONE, Flag::Exact);
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
            return (crate::nn::leaf_cp(&self.pos), Move::NONE, Flag::Exact);
        }

        // L'alpha d'entrée : il sert à distinguer un **échec** (le score
        // rendu est une borne *supérieure*) d'une valeur exacte.
        let alpha_orig = alpha;

        let in_check = self.pos.in_check(self.pos.side);
        let mut list = MoveList::new();
        if in_check {
            movegen::generate(&self.pos, &mut list, GenKind::All);
            if list.is_empty() {
                // Aucun coup d'esquive : c'est un **mat**.
                return (-(MATE - ply as i32), Move::NONE, Flag::Exact);
            }
        } else {
            // Le **stand-pat** vient *avant* la liste : une position sans
            // prise n'est **pas** une nulle, c'est une position à
            // évaluer. (C'est ce que le mode `CapturesOnly` du
            // générateur masquait : il émettait aussi les coups
            // tranquilles du roi, donc la liste n'était jamais vide et le
            // stand-pat était toujours atteint — mais la quiescence
            // confondait alors « pas de prise » et « pat ».)
            let stand = crate::nn::leaf_cp(&self.pos);
            // **La terminalité passe AVANT la coupe « stand-pat ».**
            //
            // Une position sans aucun coup légal est un **pat** (nulle),
            // pas une position à évaluer. La coupe `stand >= beta`
            // précédait le test de légalité : une **capture qui menait au
            // pat** était alors notée « gagnante » (la valeur statique
            // de la position de pat, ≥ beta), et la nulle était perdue.
            // C'est la même famille de bug que le correctif du 2026-10-23
            // (le signe de la quiescence) : la terminalité doit être
            // décidée **avant** toute comparaison de scores.
            //
            // L'ordre n'est **pas** plus coûteux : la génération des
            // prises était faite de toute façon juste après — elle est
            // simplement anticipée. Le seul surcoût porte sur les nœuds
            // où la coupe s'appliquait (ils paient maintenant la
            // génération).
            movegen::generate(&self.pos, &mut list, GenKind::CapturesOnly);
            if list.is_empty() {
                // Pas de prise : la position n'est **terminale** que si
                // elle n'a plus aucun coup légal (pat). Sinon, c'est la
                // valeur statique qui répond.
                if !movegen::has_legal_move(&self.pos) {
                    return (0, Move::NONE, Flag::Exact);
                }
                if stand >= beta {
                    return (stand, Move::NONE, Flag::Lower);
                }
                return (stand, Move::NONE, Flag::Exact);
            }
            // Il y a au moins une prise : la position n'est pas
            // terminale, la coupe « stand-pat » est donc sûre.
            if stand >= beta {
                return (stand, Move::NONE, Flag::Lower);
            }
            if stand > alpha {
                alpha = stand;
            }
        }

        // The best move of the quiescence (for the PV). A stand-pat that
        // wins leaves it at `NONE`: the PV then stops there, which is the
        // honest thing to show — there is no capture to play.
        let mut best_move = Move::NONE;

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
            // **La valeur de l'enfant est celle de l'adversaire** : il faut
            // l'inverser pour la lire du point de vue du trait (negamax).
            // Sans cette inversion, la quiescence rendait la valeur *de
            // l'adversaire* : le moteur jugeait les positions calmes à
            // l'envers (une position perdante était notée gagnante).
            let (child_score, _, _) = self.quiescence(-beta, -alpha, ply + 1, captures + 1);
            let score = -child_score;
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


    /// Score et trie les coups `list[start..end]` : `order[start..end]`
    /// reçoit les **indices** des coups, triés par score décroissant —
    /// et, à score égal, par indice croissant : l'ordre est donc
    /// **total**, et le tri *instable* donne le même résultat qu'un tri
    /// stable.
    ///
    /// La méthode est appelée **une fois par phase** (tactique, puis
    /// tranquille) : c'est ce qui permet de ne générer les coups
    /// tranquilles qu'à la demande.
    fn score_and_order(
        &mut self,
        ply: usize,
        list: &MoveList,
        start: usize,
        end: usize,
        order: &mut [u8],
        tt_best: Move,
        killer0: Move,
        killer1: Move,
    ) {
        let use_history = self.opts.history;
        {
            let pos = &self.pos;
            let history = &self.history;
            let scores = &mut self.scores[ply];
            for i in start..end {
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
        for i in start..end {
            order[i] = i as u8;
        }
        {
            let scores = &self.scores[ply];
            order[start..end].sort_unstable_by(|&a, &b| {
                scores[b as usize]
                    .cmp(&scores[a as usize])
                    .then(a.cmp(&b))
            });
        }
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

    /// Le trait au trait a-t-il du **matériel lourd** (tour ou dame) ?
    ///
    /// Le coup gratuit est désactivé dans les fins de pions — c'est le
    /// zugzwang classique — **et** dans les fins de pièces **mineures**
    /// (cavalier ou fou contre roi) : dans ces finales, « passer » est
    /// presque toujours une perte, et le null-move y rend `beta` sans
    /// chercher : le moteur refuse alors un coup gagnant.
    ///
    /// Le garde-fou précédent (toute pièce hors pions) laissait passer
    /// ces finales : c'est le même bug de famille que le coup gratuit
    /// naïf, que le duel de la phase AB8 a mesuré **négatif**
    /// (−71 Elo) — mais la phase l'avait mesuré sur le débit, pas sur
    /// la force.
    fn has_heavy_material(&self) -> bool {
        has_heavy_material(&self.pos)
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

    /// L'**adversaire** a-t-il une capture disponible ?
    ///
    /// Le garde-fou « capture » de la futilité et du razoring doit
    /// regarder **les deux camps**. La futilité rend `beta` (« nous
    /// gagnons ») quand l'évaluation statique est très haute ; or
    /// l'évaluation statique compte les pièces *comme si elles étaient
    /// vivantes*. Si l'adversaire peut **prendre** une de nos pièces,
    /// la borne est fausse : la position est perdante, pas gagnante.
    ///
    /// Le trait adverse est simulé en inversant le **seul** côté au
    /// trait : la case de prise en passant est conservée (une prise en
    /// passant de l'adversaire doit être vue), et rien d'autre n'est
    /// touché.
    ///
    /// Le coût (une génération) n'est payé que sur le chemin **shallow**
    /// et seulement quand l'option est active — les options AB8 sont
    /// inactives par défaut.
    fn opponent_has_capture(&mut self) -> bool {
        let side = self.pos.side;
        self.pos.side = side.opp();
        let mut list = MoveList::new();
        movegen::generate(&self.pos, &mut list, GenKind::CapturesOnly);
        self.pos.side = side;
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
    ///
    /// # Règle : **trois** occurrences
    ///
    /// La nulle par répétition (règle FIDE) exige que la position soit
    /// apparue **trois fois** ; à la deuxième, la nulle peut être
    /// *réclamée*, elle n'est pas automatique.
    ///
    /// Le moteur traitait la **deuxième** occurrence comme une nulle
    /// automatique : il evaluait donc `0` une position qui n'était pas
    ///Drawing, et le MCTS (qui exige trois occurrences) ne traitait pas
    /// la même position de la même façon — ce qui **violait le contrat
    /// de la phase AB** (« les deux algorithmes voient la même
    /// évaluation »).
    ///
    /// Ici, on compte les occurrences **antérieures** : il en faut
    /// **deux** (la position courante étant la troisième).
    fn is_repetition(&self) -> bool {
        let key = self.pos.zobrist;
        let mut previous = 0usize;
        // La ligne de recherche : les ancêtres.
        for &k in self.path.iter() {
            if k == key {
                previous += 1;
                if previous >= 2 {
                    return true;
                }
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
                previous += 1;
                if previous >= 2 {
                    return true;
                }
            }
        }
        false
    }
    /// Semer la **ligne principale** d'un ply avec le coup connu.
    ///
    /// Quand la recherche rend la main sur une **transposition**, la
    /// ligne principale n'est pas reconstruite : elle est vide. Le coup
    /// de la table est la seule information connue — sans lui, le
    /// moteur rendrait le repli (le premier coup légal) au lieu du
    /// meilleur coup connu.
    fn seed_pv(&mut self, ply: usize, mv: Move) {
        if mv != Move::NONE {
            self.pv_table[ply].clear();
            self.pv_table[ply].push(mv);
        }
    }

}

/// Le trait au trait a-t-il du **matériel lourd** (tour ou dame) ?
///
/// Le coup gratuit (null-move) est désactivé dans les fins de pions
/// (zugzwang classique) **et** dans les fins de pièces **mineures**
/// (cavalier ou fou contre roi) : dans ces finales, « passer » est
/// presque toujours une perte, et le null-move y rend `beta` sans
/// chercher — le moteur refuse alors un coup gagnant.
fn has_heavy_material(pos: &Position) -> bool {
    let us = pos.side;
    (pos.pieces(us, crate::types::ROOK) | pos.pieces(us, crate::types::QUEEN)) != 0
}

/// Le coup peut-il être cherché à **profondeur réduite** (LMR) ?
///
/// La fonction est **pure** et **testable** : les invariants du LMR
/// sont des invariants de *cette* fonction, pas des invariants de la
/// boucle de recherche — c'est la raison de l'extraire.
///
/// # Les invariants
///
/// * le coup de la **TT** n'est **jamais** réduit : c'est le meilleur
///   coup connu pour la position, et sa valeur peut n'être qu'une
///   *borne* (une recherche à fenêtre nulle qui échoue ne donne pas la
///   valeur exacte) ; le réduire produirait un choix faux. La
///   génération **échelonnée** rend cette condition nécessaire : le
///   coup de la TT peut être un coup *tranquille*, donc porter un
///   index > 0 dans la phase tranquille — le test `index > 0` ne
///   l'excluait pas ;
/// * le coup de la **PV** (ply 0) n'est **jamais** réduit : la ligne
///   principale est la seule dont la valeur est garantie complète ;
/// * une **fenêtre de mat** n'est jamais réduite (le mat serait perdu) ;
/// * les coups **tactiques** (prises, promotions) ne sont jamais
///   réduits.
fn lmr_can_reduce(
    opts: &Options,
    mv: Move,
    tt_best: Move,
    index: usize,
    ply: usize,
    in_check: bool,
    alpha: i32,
    beta: i32,
) -> bool {
    opts.lmr
        && index > 0
        && !in_check
        && !mv.is_capture()
        && !mv.is_promotion()
        && mv != tt_best
        && ply > 0
        && alpha.abs() < LMR_SAFE_LIMIT
        && beta.abs() < LMR_SAFE_LIMIT
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

#[cfg(test)]
mod tests {
    //! Les invariants **internes** de la recherche.
    //!
    //! Les invariants qui se lisent dans une recherche complète (« le
    //! score ne bouge pas ») sont tested dans `tests/ab_proprietes.rs` ;
    //! ceux qui tiennent à **une condition** (le LMR, le null-move) sont
    //! ici, sur la fonction pure qui les porte : c'est le seul endroit
    //! où on peut les énoncer **sans** dépendre du reste de la boucle.

    use super::*;

    fn opts() -> Options {
        Options {
            lmr: true,
            ..Options::default()
        }
    }

    /// Un coup **tranquille**, ni de la TT, hors PV, dans une fenêtre
    /// normale : le LMR **s'applique**.
    #[test]
    fn lmr_reduces_a_plain_quiet_move() {
        let mv = Move::new(0, 1, 0); // un coup tranquille quelconque
        assert!(lmr_can_reduce(
            &opts(),
            mv,
            Move::NONE,
            3,   // index
            2,   // ply
            false,
            0,
            100
        ));
    }

    /// Le coup de la **table de transposition** n'est **jamais** réduit.
    ///
    /// C'est l'invariant que la génération échelonnée avait cassé : le
    /// coup de la TT peut être *tranquille*, donc porter un index > 0
    /// dans la phase tranquille — le test `index > 0` ne l'excluait pas.
    #[test]
    fn lmr_never_reduces_the_tt_move() {
        let mv = Move::new(0, 1, 0);
        assert!(!lmr_can_reduce(
            &opts(),
            mv,
            mv,   // le coup de la TT
            7,    // index > 0 (phase tranquille)
            2,
            false,
            0,
            100
        ));
    }

    /// Le coup de la **PV** (ply 0) n'est **jamais** réduit.
    #[test]
    fn lmr_never_reduces_the_pv_move() {
        let mv = Move::new(0, 1, 0);
        assert!(!lmr_can_reduce(
            &opts(), mv, Move::NONE, 5, /* ply */ 0, false, 0, 100
        ));
    }

    /// Un coup **tactique** n'est jamais réduit, ni dans une **fenêtre
    /// de mat**, ni en échec.
    #[test]
    fn lmr_never_reduces_a_tactical_or_mate_move() {
        let mv = Move::new(0, 1, 0);
        // une prise (le drapeau « capture » est le bit 2 des drapeaux)
        let capture = Move::new(0, 1, 4);
        assert!(!lmr_can_reduce(
            &opts(),
            capture,
            Move::NONE,
            5,
            2,
            false,
            0,
            100
        ));
        // une fenêtre de mat
        assert!(!lmr_can_reduce(
            &opts(),
            mv,
            Move::NONE,
            5,
            2,
            false,
            MATE - 10,
            MATE
        ));
        // le trait est en échec
        assert!(!lmr_can_reduce(
            &opts(),
            mv,
            Move::NONE,
            5,
            2,
            true,
            0,
            100
        ));
    }

    /// L'option éteinte, rien n'est réduit.
    #[test]
    fn lmr_is_off_when_the_option_is_off() {
        let mv = Move::new(0, 1, 0);
        let off = Options::default();
        assert!(!lmr_can_reduce(
            &off, mv, Move::NONE, 5, 2, false, 0, 100
        ));
    }

    /// Le **matériel lourd** : une tour ou une dame.
    ///
    /// Le null-move est désactivé sans matériel lourd : dans une finale
    /// de pièces mineures, « passer » est un zugzwang — le null-move
    /// rendrait `beta` sans chercher, et le moteur refuserait un coup
    /// gagnant.
    #[test]
    fn the_heavy_material_is_a_rook_or_a_queen() {
        // cavalier contre roi : pas de matériel lourd
        let pos = Position::from_fen("8/8/8/4k3/8/8/8/K3N3 w - - 0 1").unwrap();
        assert!(!has_heavy_material(&pos));
        // fou contre roi : pas de matériel lourd
        let pos = Position::from_fen("8/8/8/4k3/8/8/8/K3B3 w - - 0 1").unwrap();
        assert!(!has_heavy_material(&pos));
        // tour contre roi : matériel lourd
        let pos = Position::from_fen("8/8/8/4k3/8/8/8/K3R3 w - - 0 1").unwrap();
        assert!(has_heavy_material(&pos));
        // dame contre roi : matériel lourd
        let pos = Position::from_fen("8/8/8/4k3/8/8/8/K3Q3 w - - 0 1").unwrap();
        assert!(has_heavy_material(&pos));
        // pions seuls : pas de matériel lourd
        let pos = Position::from_fen("8/8/8/4k3/8/8/4P3/K7 w - - 0 1").unwrap();
        assert!(!has_heavy_material(&pos));
    }
}
