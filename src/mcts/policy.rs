//! A2 — **priors de policy heuristiques** du MCTS (phase 4).
//!
//! La sélection UCT de la phase 3 ne savait **rien** des coups : les enfants
//! non visités étaient tirés **uniformément au hasard**, et les visités
//! étaient classés par `Q + C·sqrt(ln N / n)`. Une capture qui gagne une
//! dame et un coup calme qui perdait la partie étaient donc explorés avec
//! la même probabilité.
//!
//! La phase 4 passe à la sélection **PUCT** (Silver et al., AlphaZero) :
//!
//! ```text
//! s(a) = Q(a) + c_puct · P(a) · sqrt(N) / (1 + n(a))
//! ```
//!
//! où `P(a)` est le **prior** du coup `a` — une distribution de probabilité
//! sur les coups légaux du nœud. Le prior est calculé **une seule fois**, à
//! l'expansion du nœud, et stocké dans `Edge::prior` (le champ que l'arène
//! de la phase 3 réservait déjà pour ce hook).
//!
//! ## Le prior est-il un « tri MVV-LVA » ?
//!
//! C'est le **pendant exact** du tri heuristique des moteurs alpha-beta, mais
//! transplanté : en alpha-beta, l'ordre des coups décide de la *vitesse
//! d'élagage* ; en MCTS, il n'y a pas d'élagage — ce qui compte, c'est le
//! **quel sous-arbre est exploré en premier** (le prior) et **combien de
//! temps il est gardé** (le bonus d'exploration pondéré par le prior).
//!
//! Le score utilisé est le **MVV-LVA classique** : la valeur de la victime
//! compte dix fois, celle de l'attaquant une fois. Un pion qui prend une
//! dame (10·900 − 100 = 8900) prime donc sur une dame qui prend un pion
//! (10·100 − 900 = 100) — exactement la propriété recherchée.
//!
//! ## Pourquoi le tri n'est pas dans le générateur
//!
//! Trier les coups dans `movegen` coûterait un `O(n log n)` **à chaque
//! playout** (le générateur est rappelé à chaque expansion) pour un
//! bénéfice nul : en MCTS, l'ordre n'a de sens qu'une fois par nœud
//! développé. Le tri est donc fait **une fois**, dans le prior.
//!
//! ## Les garde-fous
//!
//! * le score est **saturé** (division par la référence MVV-LVA maximale)
//!   puis mélangé à un **plancher uniforme** (`PRIOR_EPS`) : la
//!   distribution ne peut pas s'effondrer sur un seul coup et aucun coup
//!   n'est jamais affamé ;
//! * la somme des priors vaut **exactement 1** (contrôlé par test) ;
//! * la racine du mode **parallèle** est perturbée par un bruit de Gumbel
//!   (`perturb`) : sans cela, tous les workers exploreraient le même arbre
//!   et le mode parallèle se réduirait au mono à `N/T` nœuds. C'est
//!   l'amorce de C1 (bruit de Dirichlet), dont les paramètres α/ε et la
//!   politique d'exploration du self-play restent à faire.

use crate::eval;
use crate::position::Position;
use crate::types::{Move, PAWN};

use super::Rng;

/// Mélange uniforme : chaque coup garde `PRIOR_EPS / n` de la masse, quel
/// que soit son score heuristique. C'est le filet de sécurité contre
/// l'affamation d'un coup dont le prior se serait effondré.
pub const PRIOR_EPS: f32 = 0.25;

/// Poids de la meilleure capture possible (un pion prend une dame) dans
/// l'échelle des « unités de prior ». Un coup calme vaut 0 unité.
pub const PRIOR_CAPTURE_WEIGHT: f32 = 4.0;

/// Poids d'une promotion en dame, dans la même échelle.
pub const PRIOR_PROMOTION_WEIGHT: f32 = 4.0;

/// La référence MVV-LVA : `10 · dame − pion` = 8900 centipions. Le score
/// est divisé par cette valeur, ce qui le **sature** : la meilleure capture
/// possible vaut exactement `PRIOR_CAPTURE_WEIGHT` unités.
const MVV_LVA_MAX: f32 = 8900.0;

/// La valeur d'une dame, référentiel des promotions.
const QUEEN_VALUE: f32 = 900.0;

/// La température du softmax des priors, en « unités de prior ». `1.0` :
/// une prise de dame (4 unités) vaut ~0,5 de probabilité.
pub const PRIOR_TAU: f32 = 1.0;

/// Le type de pièce sur une case (`None` si la case est vide).
///
/// Le mailbox code une pièce par `1 + kind + 6·couleur` ; `kind` est donc
/// `code % 6` (cf. `eval::eval_white`, même convention).
#[inline]
fn piece_kind_at(pos: &Position, sq: u8) -> Option<usize> {
    let m = pos.mailbox[sq as usize];
    if m == 0 {
        None
    } else {
        Some(((m - 1) % 6) as usize)
    }
}

/// La valeur (en centipions) de la pièce **capturée** par `mv`.
#[inline]
fn victim_value(pos: &Position, mv: Move) -> f32 {
    if mv.is_ep() {
        // En passant, le pion capturé n'est pas sur la case d'arrivée.
        return eval::material(PAWN) as f32;
    }
    piece_kind_at(pos, mv.to())
        .map(|k| eval::material(k) as f32)
        .unwrap_or(0.0)
}

/// La valeur (en centipions) de la pièce qui **joue** `mv` (l'attaquant).
#[inline]
fn attacker_value(pos: &Position, mv: Move) -> f32 {
    piece_kind_at(pos, mv.from())
        .map(|k| eval::material(k) as f32)
        .unwrap_or(0.0)
}

/// Le score heuristique d'un coup, en « unités de prior » (0 = coup calme).
///
/// * capture : `PRIOR_CAPTURE_WEIGHT · (10·victime − attaquant) / 8900` ;
/// * promotion : `PRIOR_PROMOTION_WEIGHT · valeur / 900` ;
/// * une promotion-capture additionne les deux.
pub fn raw_units(pos: &Position, mv: Move) -> f32 {
    let mut u = 0.0;
    if mv.is_capture() {
        let mvv = 10.0 * victim_value(pos, mv) - attacker_value(pos, mv);
        u += PRIOR_CAPTURE_WEIGHT * (mvv / MVV_LVA_MAX);
    }
    if let Some(k) = mv.promotion_kind() {
        u += PRIOR_PROMOTION_WEIGHT * (eval::material(k.idx()) as f32 / QUEEN_VALUE);
    }
    u
}

/// La distribution de prior des coups d'un nœud : un vecteur de
/// probabilités **strictement positives** dont la somme vaut **1**.
///
/// La forme est un **softmax** sur les unités heuristiques, mélangé à un
/// plancher uniforme :
///
/// ```text
/// P_i = (1 − ε)·exp(u_i/τ) / Σ_j exp(u_j/τ) + ε/n
/// ```
///
/// La température `τ` fixe l'amplitude de la distribution : `τ = 1` unité
/// donne à une prise de dame (~4 unités) une probabilité de l'ordre de
/// 0,5, les coups calmes se partageant le reste. Un `τ` plus grand
/// aplatit la distribution (un prior uniforme), un `τ` plus petit la
/// concentre.
pub fn priors(pos: &Position, moves: &[Move]) -> Vec<f32> {
    let n = moves.len();
    if n == 0 {
        return Vec::new();
    }
    let mut units = Vec::with_capacity(n);
    let mut max_u = f32::MIN;
    for &mv in moves {
        let u = raw_units(pos, mv);
        if u > max_u {
            max_u = u;
        }
        units.push(u);
    }
    let mut w = Vec::with_capacity(n);
    let mut sum = 0.0f32;
    for u in units {
        // The maximum is subtracted before the exponential: the sum stays
        // finite whatever the number of captures.
        let e = ((u - max_u) / PRIOR_TAU).exp();
        w.push(e);
        sum += e;
    }
    let uniform = PRIOR_EPS / n as f32;
    let mut out = Vec::with_capacity(n);
    for v in w {
        out.push((1.0 - PRIOR_EPS) * (v / sum) + uniform);
    }
    out
}
/// Perturbe les priors d'un bruit de **Gumbel** (`P' ∝ P·exp(g)`) et
/// renormalise.
///
/// C'est le mécanisme de **diversité** du mode parallèle : chaque worker
/// tire son propre bruit (sa propre graine) et explore donc un ordre
/// d'expansion racine différent. Le chemin mono ne l'utilise **pas** : il
/// reste propre et déterministe.
///
/// `scale = 0` ne fait rien.
pub fn perturb(priors: &mut [f32], rng: &mut Rng, scale: f32) {
    if scale <= 0.0 || priors.len() < 2 {
        return;
    }
    let mut w = Vec::with_capacity(priors.len());
    let mut sum = 0.0f32;
    for &p in priors.iter() {
        // u ∈ (0, 1) : les extrémités sont écrêtées pour que log-log reste
        // fini.
        let u = (rng.next_u64() as f64 / (u64::MAX as f64 + 1.0)).clamp(1e-9, 1.0 - 1e-9);
        let g = -(-u.ln()).ln() as f32; // Gumbel(0, 1)
        let e = (p.max(1e-9).ln() + scale * g).exp();
        w.push(e);
        sum += e;
    }
    for (p, v) in priors.iter_mut().zip(w) {
        *p = v / sum;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::movegen::{MoveList, generate};
    use crate::types::GenKind;

    fn legal(pos: &Position) -> Vec<Move> {
        let mut l = MoveList::new();
        generate(pos, &mut l, GenKind::All);
        l.iter().collect()
    }

    #[test]
    fn priors_sum_to_one() {
        let pos = Position::startpos();
        let moves = legal(&pos);
        let p = priors(&pos, &moves);
        assert_eq!(p.len(), moves.len());
        let sum: f32 = p.iter().sum();
        assert!(
            (sum - 1.0).abs() < 1e-5,
            "the priors must sum to 1, got {sum}"
        );
        assert!(p.iter().all(|&x| x > 0.0), "every prior must be positive");
    }

    #[test]
    fn mvv_lva_order_is_respected() {
        // White: Pb5, Qc2, Ke1. Black: Pc6, Ke8. White can play bxc6 (a
        // pawn taking a queen) and Qxc6 (a queen taking a queen): MVV-LVA
        // must rank the pawn capture first.
        let pos = Position::from_fen("4k3/8/2p5/1P6/8/8/2Q5/4K3 w - - 0 1").unwrap();
        let moves = legal(&pos);
        let p = priors(&pos, &moves);
        let get = |uci: &str| {
            let i = moves
                .iter()
                .position(|m| m.uci() == uci)
                .unwrap_or_else(|| panic!("{uci} must be legal"));
            p[i]
        };
        let pawn_takes_queen = get("b5c6");
        let queen_takes_queen = get("c2c6");
        let quiet = get("e1e2");
        assert!(
            pawn_takes_queen > queen_takes_queen,
            "MVV-LVA: a pawn taking a queen ({pawn_takes_queen}) must beat a queen taking a queen ({queen_takes_queen})"
        );
        assert!(
            queen_takes_queen > quiet,
            "a capture must beat a quiet move ({queen_takes_queen} vs {quiet})"
        );
    }

    #[test]
    fn perturbation_keeps_a_distribution() {
        let pos = Position::startpos();
        let moves = legal(&pos);
        let mut p = priors(&pos, &moves);
        let mut rng = Rng::new(7);
        perturb(&mut p, &mut rng, 1.0);
        let sum: f32 = p.iter().sum();
        assert!(
            (sum - 1.0).abs() < 1e-5,
            "the perturbed priors sum to {sum}"
        );
        assert!(p.iter().all(|&x| x > 0.0));
    }
}
