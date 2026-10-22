//! AB2 — la table de transposition de l'alpha-beta.
//!
//! Une entrée fait **16 octets** :
//!
//! ```text
//! key   (8)  la clé Zobrist COMPLÈTE — c'est elle qui détecte les
//!              collisions d'index (l'adresse n'en est qu'un hachage)
//! score (2)  le score, en centipions, ajusté à la profondeur de stockage
//! depth (2)  la profondeur de la recherche qui a produit l'entrée
//! best  (2)  le meilleur coup (0 = « entree vide »)
//! flag  (1)  la nature de la borne : exact / inférieure / supérieure
//! ```
//!
//! La table est **mappée d'avance** (un `Vec` de `1 << TT_BITS`
//! entrées) : aucune allocation pendant la recherche, et l'adressage est
//! un simple masque. Le remplacement est *depth-preferred* : une entrée
//! plus profonde n'est pas écrasée par une entrée moins profonde de la
//! **même** position (une clé différente, elle, écrase toujours — c'est
//! le remplacement direct, classique).

use crate::types::Move;

/// Adresse de la table : `1 << TT_BITS` entrées.
pub const TT_BITS: u32 = 19;
/// Capacité en entrées (524 288).
pub const TT_SIZE: usize = 1 << TT_BITS;

/// La nature de la borne stockée.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flag {
    /// La valeur est **exacte** (le nœud a été recherché entièrement).
    Exact,
    /// Borne **inférieure** : la vraie valeur est ≥ celle-ci (le nœud a
    /// coupé, « fail high »).
    Lower,
    /// Borne **supérieure** : la vraie valeur est ≤ celle-ci (le nœud a
    /// échoué, « fail low »).
    Upper,
}

impl Flag {
    fn code(self) -> u8 {
        match self {
            Flag::Exact => 1,
            Flag::Lower => 2,
            Flag::Upper => 3,
        }
    }

    fn from_code(c: u8) -> Option<Flag> {
        match c {
            1 => Some(Flag::Exact),
            2 => Some(Flag::Lower),
            3 => Some(Flag::Upper),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
struct Entry {
    key: u64,
    score: i16,
    depth: i16,
    best: u16,
    flag: u8,
}

/// Ce que la table rend ausovant une **sonde** réussie.
#[derive(Clone, Copy, Debug)]
pub struct Probe {
    pub score: i16,
    pub depth: i16,
    pub best: Move,
    pub flag: Flag,
}

pub struct Tt {
    entries: Box<[Entry]>,
}

impl Default for Tt {
    fn default() -> Self {
        Self::new()
    }
}

impl Tt {
    pub fn new() -> Self {
        let empty = Entry {
            key: 0,
            score: 0,
            depth: 0,
            best: 0,
            flag: 0,
        };
        Tt {
            entries: vec![empty; TT_SIZE].into_boxed_slice(),
        }
    }

    /// Vide la table (nouveau `ucinewgame`).
    pub fn clear(&mut self) {
        let empty = Entry {
            key: 0,
            score: 0,
            depth: 0,
            best: 0,
            flag: 0,
        };
        for e in self.entries.iter_mut() {
            *e = empty;
        }
    }

    /// La sonde : `None` si l'emplacement est vide ou porte une autre
    /// position (collision d'adresse — la clé complète tranche).
    pub fn probe(&self, key: u64) -> Option<Probe> {
        let e = &self.entries[(key as usize) & (TT_SIZE - 1)];
        if e.flag == 0 || e.key != key {
            return None;
        }
        let flag = Flag::from_code(e.flag)?;
        Some(Probe {
            score: e.score,
            depth: e.depth,
            best: Move(e.best),
            flag,
        })
    }

    /// L'écriture. *Depth-preferred* : une entrée existante de la
    /// **même** position et **plus profonde** est conservée.
    pub fn store(&mut self, key: u64, depth: i16, score: i16, flag: Flag, best: Move) {
        let idx = (key as usize) & (TT_SIZE - 1);
        let e = &mut self.entries[idx];
        if e.flag != 0 && e.key == key && e.depth > depth {
            return; // une borne plus profonde vaut mieux qu'une moins profonde
        }
        e.key = key;
        e.score = score;
        e.depth = depth;
        e.best = best.0;
        e.flag = flag.code();
    }
}
