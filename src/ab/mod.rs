//! AB — l'alpha-beta du dépôt, moteur de **référence** du projet.
//!
//! Pourquoi ce module existe : le tournoi round-robin du 2026-10-22
//! (`docs/elo/README.md`) a mesuré `mychess` à ≈ 1296 Elo et `Reckless
//! v0.1.0` à ≈ 2249, mais **toutes** les mesures du projet passaient par
//! Stockfish, à 950 Elo de distance. Sans opposant local, rien ne
//! pouvait trancher cette question : *l'alpha-beta bat-il le MCTS à
//! budget et à évaluation identiques ?*
//!
//! C'est ce module qui rend la question mesurable. Il est périmètre par
//! `docs/ab/plan-alpha-beta-2026-10-22.md` :
//!
//! * **AB1** — le squelette negamax (quiescence, règles de nulle, mat et
//!   pat, ordre MVV-LVA) ;
//! * **AB2** — la table de transposition (`ab::tt`) ;
//! * **AB3** — la profondeur itérative et la gestion du temps.
//!
//! Le contrat : **même évaluation** que la feuille du MCTS
//! (`eval::eval_for_mover`), **core gelé** (rien n'est ajouté à
//! `position`, `movegen`, `types`, `zobrist`), et le chemin MCTS reste
//! **inchangé** par défaut (l'option UCI `Search` vaut `mcts`).

pub mod search;
pub mod tt;

pub use search::{Limits, MATE, MAX_PLY, Outcome, Search, first_legal_move};
pub use tt::{Flag, Probe, Tt};
