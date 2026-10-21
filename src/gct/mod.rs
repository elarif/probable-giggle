//! Portage Rust de la série « Grand Chess Tree » (7 posts), phase 1.
//!
//! Ce module est une **réécriture fidèle** de la chaîne de génération de
//! coups légaux décrite par le post, et non une optimisation de
//! `crate::movegen`. Il est volontairement redondant avec lui : il sert
//! d'oracle croisé, puis de base propre pour les améliorations de phase 2.
//!
//! Correspondance avec les posts :
//!   - Part 4 → [`attacks`] : décalages, `SquaresBetween`, `IsAttackedBy` ;
//!   - Part 7 → [`legality`] : les trois bitboards `danger`, `checkers`,
//!     `pins` (trois passes séparées, comme le post) ;
//!   - Part 7 → [`generate`] : le générateur légal, dans l'ordre du post ;
//!   - Part 7 → [`perft`] : perft bulk au bord.
//!
//! Écarts assumés vis-à-vis du listing C# (tous documentés au site d'appel) :
//!   - le listing ne compile pas tel quel (`Span<ulong>` au lieu de
//!     `ulong[]`, `GenEnPassant` appelé avec 4 arguments pour 6 déclarés,
//!     `Rank3`/`Rank6` absents de la classe `Bitboards`, `EmitTargets`
//!     appelé depuis une classe qui ne le déclare pas, `delegate AttackFn`) ;
//!     le portage corrige, il ne recopie pas ;
//!   - pas de `delegate` pour l'attaque des fous/tours : appels directs
//!     monomorphisés ;
//!   - la dame est générée en un passage (`bishop_attacks | rook_attacks`)
//!     au lieu de deux ;
//!   - `pin_lines[sq] = u64::MAX` pour les pièces non clouées, ce qui rend le
//!     masquage inconditionnel (`targets &= pin_lines[sq]`) ;
//!   - les tables d'attaque sous-jacentes sont celles de `mychess`
//!     (`crate::slider`, `crate::tables`) : les deux chemins slider restent
//!     bit-identiques à `movegen`.
//!
//! Ré-exports de la surface publique, pour que la phase 2 puisse écrire
//! `use crate::gct::{generate, perft};` sans traverser les sous-modules. Le
//! module `generate` garde son nom : la fonction `generate` est dans le
//! namespace des valeurs, le module dans celui des types, donc les deux
//! coexistent (`crate::gct::generate::generate` reste valide).
pub mod attacks;
pub mod generate;
pub mod legality;
pub mod perft;

pub use attacks::{is_attacked_by, squares_between};
pub use generate::generate;
pub use legality::{compute_checkers, compute_pins, king_danger};
pub use perft::{perft, perft_detailed};
