//! Phase 5 — **la feuille tactiquement résolue** (quiescence-lite).
//!
//! ## Le problème
//!
//! Depuis A1, la valeur d'une feuille de l'arbre est l'**évaluation
//! statique** de la position où la descente s'arrête :
//!
//! ```text
//! leaf_value(pos) = eval_for_mover(pos)      // matériel + PST, en centipions
//! ```
//!
//! Cette position est **quelconque** : elle peut être au milieu d'un
//! échange de pièces. L'éval est alors **aveugle** à la capture qui va se
//! produire — c'est l'**effet horizon**, celui que la recherche en
//! quiescence (alpha-beta) résout en ne descendant que dans les positions
//! « agitées ».
//!
//! La mesure de la phase 5 (`examples/leaf_probe`) : sur un corpus de
//! positions prises dans un échange, l'écart entre l'éval de feuille
//! actuelle et la valeur après résolution des captures est de **351 cp en
//! moyenne**, et il dépasse **une tour dans 19 %** de ces positions.
//!
//! ## Ce que fait ce module
//!
//! `qvalue(pos, depth)` calcule la valeur de `pos` **pour le trait**, en
//! résolvant les échanges sur `depth` plies :
//!
//! ```text
//! qvalue(pos, 0) = eval_for_mover(pos)
//! qvalue(pos, d) = max( eval_for_mover(pos),
//!                       max over captures c of -qvalue(child(c), d-1) )
//! ```
//!
//! C'est la **quiescence classique**, avec la convention **stand-pat** :
//! le trait peut toujours **refuser** l'échange (l'éval statique reste
//! une option). Sans stand-pat, une position où le trait doit subir
//! l'échange serait surévaluée ; avec, la valeur est au moins l'éval.
//!
//! Le générateur utilisé est `movegen::generate_captures`, qui produit
//! les **captures et les promotions** (les promotions calmes comprises) et
//! rien d'autre : c'est le « générateur allégé » que la phase 2 avait
//! déjà produit pour la quiescence, et il n'avait encore **aucun
//! consommateur**.
//!
//! ## Deux conséquences assumées
//!
//! 1. **La feuille regarde un coup plus loin.** La valeur de la position
//!    est celle *après* le meilleur coup du trait à la feuille. L'arbre
//!    explorerait de lui-même certaines de ces tactiques s'il avait le
//!    budget : il y a donc un **double comptage** partiel, que seule la
//!    mesure tranche (c'est le gate G5 de la phase 5).
//! 2. **L'histoire n'est pas étendue** dans la recherche de quiescence :
//!    ni la règle des 50 coups ni la triple répétition ne sont rejouées
//!    sur les 1 à 4 plies de la quiescence. Une capture remet le compteur
//!    des 50 coups à zéro, et une répétition en 4 plies de captures est
//!    négligeable ; c'est la convention de la quiescence classique.
//!
//! ## Unités
//!
//! Tout est en **centipions**, comme `eval::eval_for_mover`. La
//! normalisation vers l'espace Q `[-1, 1]` reste faite par le pont
//! `mcts::eval_normalized` — ce module ne connaît pas le MCTS.

use crate::eval;
use crate::movegen::{MoveList, generate_captures, has_legal_move};
use crate::position::Position;

/// La valeur d'un **mat** en centipions, dans l'échelle de la quiescence.
///
/// Assez loin de toute évaluation statique (qui plafonne à quelques
/// milliers de centipions) pour que le mat domine le stand-pat, et assez
/// petit pour être saturé par le pont `cp -> Q` (qui borne à `[-1, 1]`).
pub const MATE_CP: i32 = 30_000;

/// La valeur d'une **pat** (le trait n'a plus de coup légal sans être en
/// échec) : nulle, comme la règle de nulle du moteur.
pub const STALEMATE_CP: i32 = 0;

/// La valeur de `pos` **pour le trait**, après avoir résolu les captures
/// sur `depth` plies.
///
/// * `depth == 0` : l'éval statique (comportement de la phase 4) ;
/// * pas de capture disponible : l'éval statique (stand-pat) ;
/// * sinon : le maximum entre l'éval statique et la meilleure chaîne de
///   captures, chaque plie de la chaîne étant Resolution jusqu'à la
///   quiescence.
///
/// Un mat ou une pat **découverts dans la chaîne** sont détectés : le
/// mat vaut `MATE_CP` pour le camp qui mate, la pat vaut `0`.
pub fn qvalue(pos: &Position, depth: u32) -> i32 {
    let stand_pat = eval::eval_for_mover(pos);
    if depth == 0 {
        return stand_pat;
    }
    let caps: MoveList = generate_captures(pos);
    if caps.is_empty() {
        return stand_pat;
    }
    let mut best = stand_pat;
    for mv in caps.iter() {
        // Le mode `CapturesOnly` du générateur est **bruité** : il émet
        // aussi les coups **calmes du roi** (et les promotions calmes, qui
        // sont voulues). On ne garde que ce qui est un **échange** :
        // capture, prise en passant, promotion.
        if !(mv.is_capture() || mv.is_promotion()) {
            continue;
        }
        let mut child = *pos;
        let undo = child.make(mv);
        let v = -qvalue_after(&child, depth - 1);
        child.unmake(mv, undo);
        if v > best {
            best = v;
        }
    }
    best
}

/// Le même contrat que [`qvalue`], mais **après** un coup : la position
/// est celle de l'adversaire, et la terminaison y est vérifiée avant
/// toute évaluation (un mat de l'adversaire vaut une pat pour lui, donc
/// un mat pour nous).
fn qvalue_after(pos: &Position, depth: u32) -> i32 {
    if !has_legal_move(pos) {
        // L'adversaire n'a plus de coup : mat s'il est en échec, pat sinon.
        return if pos.in_check(pos.side) {
            -MATE_CP
        } else {
            STALEMATE_CP
        };
    }
    qvalue(pos, depth)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fen(s: &str) -> Position {
        Position::from_fen(s).expect("valid FEN")
    }

    #[test]
    fn depth_zero_is_the_static_eval() {
        let p = fen("4k3/8/2p5/1P6/8/8/8/4K3 w - - 0 1");
        assert_eq!(qvalue(&p, 0), eval::eval_for_mover(&p));
    }

    #[test]
    fn a_quiet_position_keeps_the_static_eval() {
        // Position calme : aucune capture ni promotion disponible.
        let p = fen("4k3/8/8/8/3P4/8/8/4K3 w - - 0 1");
        let static_eval = eval::eval_for_mover(&p);
        assert_eq!(qvalue(&p, 1), static_eval);
        assert_eq!(qvalue(&p, 4), static_eval);
    }

    #[test]
    fn a_free_capture_is_seen() {
        // Le trait prend une dame : la valeur doit monter d'environ 900 cp
        // par rapport à l'éval statique (qui ne voit pas la capture).
        let p = fen("4k3/8/8/3q4/8/8/8/3QK3 w - - 0 1");
        let static_eval = eval::eval_for_mover(&p);
        let q = qvalue(&p, 1);
        assert!(
            q > static_eval + 800,
            "la capture gratuite doit etre vue : {q} contre {static_eval}"
        );
    }

    #[test]
    fn the_stand_pat_protects_the_trait() {
        // Le trait peut prendre un pion, mais la prise est **mauvaise** :
        // la dame adverse reprend. Sans stand-pat, la valeur serait
        // celle de la chaîne (−900 pour le trait) ; avec, le trait
        // refuse l'échange et la valeur reste l'éval statique.
        let p = fen("3qk3/8/8/3p4/4P3/8/8/4K3 w - - 0 1");
        let static_eval = eval::eval_for_mover(&p);
        let q = qvalue(&p, 2);
        assert_eq!(
            q, static_eval,
            "le stand-pat doit refuser l'echange mauvais : {q} contre {static_eval}"
        );
    }

    #[test]
    fn an_exchange_is_resolved_by_depth() {
        // Le pion blanc prend le cavalier (d5), mais le pion noir (c6)
        // reprend. La profondeur 1 ne voit **pas** la réplique, la
        // profondeur 2 la voit.
        let p = fen("4k3/8/2p5/3n4/4P3/8/8/4K3 w - - 0 1");
        let static_eval = eval::eval_for_mover(&p);
        let q1 = qvalue(&p, 1);
        let q2 = qvalue(&p, 2);
        assert!(
            q2 > static_eval,
            "l'echange resolu doit battre l'eval statique : {q2} contre {static_eval}"
        );
        assert!(
            q1 > q2,
            "la profondeur 1 ne voit pas la replique : {q1} contre {q2}"
        );
    }

    #[test]
    fn a_mate_found_in_the_chain_is_exact() {
        // Qxh7 est un mat : la quiescence doit le voir (la dame est
        // protégée par la tour h1, g8 et g7 sont couverts).
        let p = fen("7k/7p/8/8/8/3Q4/8/4K2R w - - 0 1");
        let q = qvalue(&p, 1);
        assert!(
            q >= MATE_CP,
            "le mat doit etre detecte dans la chaine : {q}"
        );
    }
}
