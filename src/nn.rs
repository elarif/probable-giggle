//! La **valeur apprise** (phase NN) : un petit réseau de valeur, CPU-only.
//!
//! # Ce que c'est
//!
//! Un perceptron à **une couche cachée**, dont l'entrée est l'échiquier
//! et la sortie un scalaire dans `[-1, 1]` :
//!
//! ```text
//!   h[j] = b1[j] + Σ_pièces W1[plan][case][j] + W_trait[trait][j]
//!   h    = ReLU(h)
//!   y    = tanh(b2 + Σ_j W2[j]·h[j])
//! ```
//!
//! La première couche est donc une **table de positions apprise** : une
//! pièce sur une case contribue une ligne de poids. C'est exactement la
//! forme d'une PST, mais apprise au lieu d'écrite à la main — et c'est
//! aussi ce qui rend l'inférence **rapide** : la position est *creuse*
//! (≈ 32 pièces), on ne lit que les lignes des pièces présentes, jamais
//! les 769 entrées.
//!
//! # Pourquoi cette forme
//!
//! * **Coût de la feuille.** La valeur statique coûte ≈ 31 ns. Le réseau
//!   doit rester dans le même ordre de grandeur *relatif* à une feuille de
//!   MCTS (≈ 1 700 ns/playout) : une couche dense 769×N serait 30× plus
//!   lente que la feuille entière. La forme creuse coûte ≈ 32 lignes ×
//!   N flottants, soit quelques centaines de ns.
//! * **Portabilité.** Aucune dépendance, pas de GPU, pas d'AVX-512 : le
//!   calcul vectoriel est de l'AVX2 **optionnel** (détecté à l'exécution),
//!   avec un repli scalaire. L'EPYC de référence n'a pas d'AVX-512.
//! * **Convention.** La sortie est dans `[-1, 1]`, **côté trait** — la
//!   même convention que `eval::eval_for_mover` (normalisé) et que la
//!   feuille du MCTS. Le pont centipions est explicite et unique :
//!   [`NN_CP_SCALE`].
//!
//! # Ce que ce module ne fait pas
//!
//! Il ne change **rien** au core gelé, et il n'est **pas** câblé par
//! défaut : sans [`set_enabled`], la feuille reste la valeur statique,
//! bit-à-bit. Le câblage est explicite (option UCI `EvalNN`).

use crate::position::Position;
use crate::types::{BLACK, Color, WHITE};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// Le nombre de plans de pièces : 6 genres × 2 couleurs.
pub const PLANES: usize = 12;
/// Le nombre de cases d'un plan.
pub const SQ: usize = 64;
/// La taille de la couche cachée (le nombre de « tables de positions
/// apprises »).
pub const HIDDEN: usize = 64;
/// Le nombre de lignes de la couche cachée entrées dans le format du
/// fichier : `PLANES * SQ * HIDDEN`.
pub const W1_LEN: usize = PLANES * SQ * HIDDEN;

/// Le pont **centipions ↔ valeur normalisée**.
///
/// C'est le **seul** endroit où les deux échelles se touchent. La valeur
/// statique utilise le même pont (`mcts::EVAL_Q_CP_SCALE = 1/1000`) :
/// les deux valeurs sont donc directement comparables, et un remplacement
/// de la feuille ne change pas l'échelle des Q du MCTS.
pub const NN_CP_SCALE: f32 = 1000.0;

/// La version du format de fichier.
const FORMAT_VERSION: u32 = 1;
const MAGIC: &[u8; 8] = b"MYCHESSN";

/// Le réseau de valeur.
#[derive(Clone, Debug)]
pub struct Net {
    /// Les poids de la couche cachée : `W1[plan][case][j]`, rangé
    /// plan → case → unité cachée (c'est cet ordre qui rend le parcours
    /// « pièce par pièce » possible).
    pub w1: Vec<f32>,
    /// Le biais de la couche cachée.
    pub b1: Vec<f32>,
    /// La feature « trait » : deux lignes (blancs / noirs).
    pub w_side: Vec<f32>,
    /// Les poids de la couche de sortie.
    pub w2: Vec<f32>,
    /// Le biais de la couche de sortie.
    pub b2: f32,
    /// L'activation de la couche cachée : `true` = ReLU (le réseau
    /// « profond »), `false` = linéaire (le réseau **plat** : la
    /// première couche est alors une **table de positions apprise**,
    /// sans non-linéarité).
    ///
    /// Le mode plat n'est pas un jouet : c'est le modèle le plus
    /// **régularisé** qui soit (768 paramètres au lieu de 49 345),
    /// donc le bon point de départ pour mesurer ce qu'un réseau
    /// **appris** apporte face à une table écrite à la main — quand les
    /// étiquettes sont bruitées (résultat de partie), la capacité
    /// supplémentaire ne fait que sur-apprendre.
    pub relu: bool,
}

impl Net {
    /// Un réseau **vide** (tous les poids nuls) : la sortie vaut
    /// `tanh(b2)`. C'est le point de départ « sans connaissance ».
    pub fn zeros(hidden: usize) -> Net {
        Net {
            w1: vec![0.0; PLANES * SQ * hidden],
            b1: vec![0.0; hidden],
            w_side: vec![0.0; 2 * hidden],
            w2: vec![0.0; hidden],
            b2: 0.0,
            relu: true,
        }
    }

    /// Un réseau **aléatoire mais déterministe** (graine fixe) : le point
    /// de départ de l'entraînement.
    pub fn random(seed: u64, hidden: usize) -> Net {
        let mut rng = crate::mcts::Rng::new(seed);
        let mut net = Net::zeros(hidden);
        // Écart-type ≈ 1/√fan-in pour la couche cachée (≈ 1/√32 pour les
        // lignes de pièce, qui reçoivent ≈ 32 contributions).
        let s1 = (1.0f32 / 32.0f32).sqrt();
        for w in net.w1.iter_mut() {
            *w = s1 * uniform(&mut rng);
        }
        for w in net.b1.iter_mut() {
            *w = s1 * uniform(&mut rng);
        }
        for w in net.w_side.iter_mut() {
            *w = s1 * uniform(&mut rng);
        }
        let s2 = (1.0f32 / hidden as f32).sqrt();
        for w in net.w2.iter_mut() {
            *w = s2 * uniform(&mut rng);
        }
        net.b2 = s2 * uniform(&mut rng);
        net
    }

    /// Le nombre d'unités cachées du réseau.
    pub fn hidden(&self) -> usize {
        self.b1.len()
    }

    /// La **valeur** d'une position : la sortie de la forward,
    /// **découpée** dans `[-1, 1]`. C'est la valeur que le moteur
    /// consomme (elle est donc toujours dans la même échelle que la
    /// valeur statique).
    #[inline]
    pub fn value(&self, pos: &Position) -> f32 {
        self.forward(pos).clamp(-1.0, 1.0)
    }

    /// La **forward** sur une position (sortie **brute**, non découpée).
    #[inline]
    pub fn forward(&self, pos: &Position) -> f32 {
        let hidden = self.hidden();
        let mut h = vec![0.0f32; hidden];
        self.accumulate(pos, &mut h);
        self.finish(&mut h, pos.side == Color::White)
    }

    /// La **forward** sur une liste de pièces (plan, case) — c'est la
    /// forme qu'utilise l'entraînement (les pièces sont pré-extraites).
    ///
    /// L'ordre d'accumulation est **le même** que celui de [`Net::forward`]
    /// (genre, puis couleur, puis case croissante) : les deux chemins
    /// rendent donc la **même** valeur, bit à bit (l'addition flottante
    /// n'est pas associative, l'ordre compte).
    pub fn forward_pieces(&self, pieces: &[(u32, u32)], white_to_move: bool) -> f32 {
        let hidden = self.hidden();
        let mut h = vec![0.0f32; hidden];
        h.copy_from_slice(&self.b1[..hidden]);
        for &(plane, sq) in pieces {
            let base = (plane as usize * SQ + sq as usize) * hidden;
            add_slice(&mut h, &self.w1[base..base + hidden]);
        }
        self.finish(&mut h, white_to_move)
    }

    /// L'accumulation de la couche cachée (les lignes des pièces + le
    /// biais), sans la non-linéarité.
    fn accumulate(&self, pos: &Position, h: &mut [f32]) {
        let hidden = self.hidden();
        h.copy_from_slice(&self.b1[..hidden]);
        for kind in 0..6 {
            for (ci, color) in [(0usize, WHITE), (1usize, BLACK)] {
                let mut bb = pos.bb_piece[kind] & pos.bb_color[color];
                while bb != 0 {
                    let sq = bb.trailing_zeros() as usize;
                    let base = (ci * 6 + kind) * SQ * hidden + sq * hidden;
                    add_slice(h, &self.w1[base..base + hidden]);
                    bb &= bb - 1;
                }
            }
        }
    }

    /// La fin de la forward : ReLU, puis la couche de sortie.
    ///
    /// La sortie est **linéaire** (pas un `tanh`) : la valeur d'une
    /// position est une *espérance* de résultat, qui peut valoir ±1, et
    /// le `tanh` sature exactement sur ces cibles — son gradient
    /// s'annule et l'entraînement ne démarre pas (constaté, puis
    /// corrigé). La saturation est assuree par le **découpage** à
    /// l'inférence (comme l'évaluation statique, qui est découpée en
    /// ±1 par le pont centipions), jamais par la fonction.
    fn finish(&self, h: &mut [f32], white_to_move: bool) -> f32 {
        let hidden = self.hidden();
        let side = if white_to_move { 0usize } else { hidden };
        add_slice(h, &self.w_side[side..side + hidden]);
        if self.relu {
            for v in h.iter_mut() {
                if *v < 0.0 {
                    *v = 0.0;
                }
            }
        }
        let mut z = self.b2;
        for j in 0..hidden {
            z += self.w2[j] * h[j];
        }
        z
    }

    /// Écrit le réseau dans un fichier (format binaire, versionné).
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        let mut buf: Vec<u8> =
            Vec::with_capacity(20 + 4 * (self.w1.len() + self.b1.len() + self.w_side.len() + self.w2.len() + 1));
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        buf.extend_from_slice(&(self.hidden() as u32).to_le_bytes());
        // Les drapeaux : bit 0 = ReLU.
        buf.extend_from_slice(&(self.relu as u32).to_le_bytes());
        for arr in [&self.w1, &self.b1, &self.w_side, &self.w2] {
            for v in arr {
                buf.extend_from_slice(&v.to_le_bytes());
            }
        }
        buf.extend_from_slice(&self.b2.to_le_bytes());
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir).map_err(|e| format!("création de {} : {e}", dir.display()))?;
        }
        std::fs::write(path, &buf).map_err(|e| format!("écriture de {} : {e}", path.display()))
    }

    /// Lit un réseau depuis un fichier.
    pub fn load(path: &std::path::Path) -> Result<Net, String> {
        let buf = std::fs::read(path).map_err(|e| format!("lecture de {} : {e}", path.display()))?;
        if buf.len() < 16 || &buf[0..8] != MAGIC {
            return Err(format!("{} n'est pas un fichier de poids (magic invalide)", path.display()));
        }
        let version = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        if version != FORMAT_VERSION {
            return Err(format!("version de format {version} (attendue {FORMAT_VERSION})"));
        }
        let hidden = u32::from_le_bytes(buf[12..16].try_into().unwrap()) as usize;
        let flags = u32::from_le_bytes(buf[16..20].try_into().unwrap());
        if hidden == 0 || hidden > 4096 {
            return Err(format!("taille de couche cachée invalide : {hidden}"));
        }
        let need = 4 * (PLANES * SQ * hidden + hidden + 2 * hidden + hidden + 1);
        if buf.len() < 20 + need {
            return Err(format!(
                "fichier tronqué : {} octets, {need} attendus",
                buf.len() - 20
            ));
        }
        let mut off = 20usize;
        let mut read_arr = |n: usize| -> Vec<f32> {
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                v.push(f32::from_le_bytes(buf[off..off + 4].try_into().unwrap()));
                off += 4;
            }
            v
        };
        let w1 = read_arr(PLANES * SQ * hidden);
        let b1 = read_arr(hidden);
        let w_side = read_arr(2 * hidden);
        let w2 = read_arr(hidden);
        let b2 = f32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
        Ok(Net {
            w1,
            b1,
            w_side,
            w2,
            b2,
            relu: flags & 1 != 0,
        })
    }
}

/// Les pièces d'une position, dans l'ordre canonique
/// (genre, couleur, case croissante) : c'est l'ordre d'accumulation de
/// la couche cachée, donc l'ordre qui garantit l'identité bit à bit
/// entre [`Net::forward`] et [`Net::forward_pieces`].
pub fn pieces_of(pos: &Position) -> Vec<(u32, u32)> {
    let mut out = Vec::with_capacity(32);
    for kind in 0..6 {
        for (ci, color) in [(0usize, WHITE), (1usize, BLACK)] {
            let mut bb = pos.bb_piece[kind] & pos.bb_color[color];
            while bb != 0 {
                let sq = bb.trailing_zeros();
                out.push(((ci * 6 + kind) as u32, sq));
                bb &= bb - 1;
            }
        }
    }
    out
}

/// L'index du plan d'une pièce (genre, couleur).
#[inline]
pub fn plane_of(kind: usize, color_idx: usize) -> usize {
    color_idx * 6 + kind
}

/// Un nombre uniforme dans `[-1, 1]` (le RNG du MCTS, déterministe).
fn uniform(rng: &mut crate::mcts::Rng) -> f32 {
    let h = rng.next_u64();
    ((h >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) as f32
}

// ---- L'accumulation vectorielle ---------------------------------------------

/// AVX2 disponible à l'exécution ?
fn has_avx2() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("avx2")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// `dst += src`, en AVX2 si l processeur le permet, en scalaire sinon.
///
/// C'est le cœur de l'inférence : la position est creuse, on additionne
/// une trentaine de lignes de `HIDDEN` flottants. Le repli scalaire
/// donne **exactement** le même résultat (l'addition IEEE est
/// déterministe et l'ordre est le même) — le SIMD ne change que le
/// débit.
#[inline]
fn add_slice(dst: &mut [f32], src: &[f32]) {
    debug_assert_eq!(dst.len(), src.len());
    #[cfg(target_arch = "x86_64")]
    {
        if has_avx2() {
            // SAFETY : `avx2` est détecté à l'exécution, et les deux
            // tranches ont la même longueur (vérifié par `debug_assert` et
            // par les appelants, qui lisent toujours des tranches
            // complètes de `HIDDEN` flottants).
            unsafe { add_slice_avx2(dst, src) };
            return;
        }
    }
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d += *s;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn add_slice_avx2(dst: &mut [f32], src: &[f32]) {
    use std::arch::x86_64::*;
    let n = dst.len();
    let mut i = 0usize;
    while i + 8 <= n {
        // SAFETY : `i + 8 <= n`, les deux pointeurs sont valides sur
        // `n` flottants, et l'alignement n'est pas requis par
        // `loadu`/`storeu`.
        unsafe {
            let d = _mm256_loadu_ps(dst.as_ptr().add(i));
            let s = _mm256_loadu_ps(src.as_ptr().add(i));
            _mm256_storeu_ps(dst.as_ptr().add(i) as *mut f32, _mm256_add_ps(d, s));
        }
        i += 8;
    }
    while i < n {
        // SAFETY : `i < n`.
        unsafe {
            *dst.get_unchecked_mut(i) += *src.get_unchecked(i);
        }
        i += 1;
    }
}

// ---- L'état global (le câblage) ---------------------------------------------

static ENABLED: AtomicBool = AtomicBool::new(false);
static NET: RwLock<Option<Arc<Net>>> = RwLock::new(None);

/// Active ou désactive la valeur apprise.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Le réseau est-il **activé** ?
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Charge un réseau et l'active.
pub fn load_and_enable(path: &std::path::Path) -> Result<(), String> {
    let net = Net::load(path)?;
    let mut guard = NET.write().map_err(|_| "verrou du réseau empoisonné".to_string())?;
    *guard = Some(Arc::new(net));
    ENABLED.store(true, Ordering::Relaxed);
    Ok(())
}

/// Décharge le réseau et désactive la valeur apprise.
pub fn unload() {
    if let Ok(mut guard) = NET.write() {
        *guard = None;
    }
    ENABLED.store(false, Ordering::Relaxed);
}

/// La valeur apprise, en **centipions** (côté trait), ou `None` si le
/// réseau n'est pas chargé/activé.
///
/// Le pont est [`NN_CP_SCALE`] : `+1.0` (le réseau est certain de la
/// victoire du trait) vaut `+1000` centipions, comme la valeur statique
/// après son propre pont. Les deux valeurs sont donc **comparables**,
/// et le remplacement de la feuille ne change pas l'échelle du MCTS.
pub fn value_cp(pos: &Position) -> Option<i32> {
    if !enabled() {
        return None;
    }
    let guard = NET.read().ok()?;
    let net = guard.as_ref()?;
    let v = net.value(pos);
    Some((v * NN_CP_SCALE).round() as i32)
}

/// La **valeur de la feuille** : la valeur apprise si le réseau est
/// activé, l'évaluation statique sinon.
///
/// C'est le **point de câblage unique** de la phase : le MCTS et
/// l'alpha-beta passent tous les deux par ici, donc les deux algorithmes
/// voient **exactement** la même valeur — c'est la condition pour que
/// la comparaison AB-vs-MCTS reste une comparaison d'algorithmes.
///
/// Sans réseau (le défaut), la fonction rend `eval::eval_for_mover` :
/// le comportement du moteur est **bit-à-bit** celui d'avant la phase.
#[inline]
pub fn leaf_cp(pos: &Position) -> i32 {
    match value_cp(pos) {
        Some(cp) => cp,
        None => crate::eval::eval_for_mover(pos),
    }
}

/// La valeur apprise **par lot** : le contrat de l'inférence SIMD.
///
/// Comme [`crate::eval::eval_batch`], le lot est évalué position par
/// position, et chaque valeur est celle de [`value_cp`] pour la
/// position correspondante. Le batch existe pour que l'inférence
/// puisse réutiliser les poids (qui tiennent dans le cache) — le gain
/// de débit est mesuré, pas supposé.
pub fn value_batch_cp(positions: &[Position]) -> Option<Vec<i32>> {
    if !enabled() {
        return None;
    }
    let guard = NET.read().ok()?;
    let net = guard.as_ref()?;
    Some(
        positions
            .iter()
            .map(|p| (net.value(p) * NN_CP_SCALE).round() as i32)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::Position;

    fn positions() -> Vec<Position> {
        let fens = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 b - - 0 1",
        ];
        fens.iter().map(|f| Position::from_fen(f).unwrap()).collect()
    }

    /// Les deux chemins de la forward (par position, par liste de pièces)
    /// doivent rendre **la même** valeur, bit à bit.
    #[test]
    fn forward_and_forward_pieces_agree() {
        let net = Net::random(0xC0FFEE, HIDDEN);
        for pos in positions() {
            let a = net.forward(&pos);
            let b = net.forward_pieces(&pieces_of(&pos), pos.side == Color::White);
            assert_eq!(a.to_bits(), b.to_bits(), "les deux forward divergent");
        }
    }

    /// La **valeur** (celle que le moteur consomme) est **toujours** dans
    /// `[-1, 1]`, même pour un réseau saturé.
    #[test]
    fn the_value_is_always_in_range() {
        let net = Net::random(7, HIDDEN);
        for pos in positions() {
            let v = net.value(&pos);
            assert!((-1.0..=1.0).contains(&v), "valeur hors bornes : {v}");
        }
        let mut net = Net::random(11, HIDDEN);
        for w in net.w1.iter_mut() {
            *w *= 50.0;
        }
        for pos in positions() {
            let v = net.value(&pos);
            assert!((-1.0..=1.0).contains(&v));
        }
    }

    /// Le réseau **vide** est neutre : la valeur vaut 0.
    #[test]
    fn a_zero_network_is_neutral() {
        let net = Net::zeros(HIDDEN);
        for pos in positions() {
            assert_eq!(net.forward(&pos), 0.0);
            assert_eq!(net.value(&pos), 0.0);
        }
    }

    /// Le mode **plat** (pas de ReLU) est bien celui du modèle : une
    /// table de positions apprise, linéaire.
    #[test]
    fn the_flat_mode_is_linear() {
        let mut net = Net::random(3, HIDDEN);
        net.relu = false;
        // Deux positions dont la différence de pièces est connue : la
        // sortie doit être **linéaire** en cette différence.
        // La valeur du mode plat est, **par construction**, la formule
        // lineaire : `b2 + Σ_j w2[j]·(b1[j] + Σ_pièces w1[p][case][j] + w_trait[j])`.
        // On la recalcule a la main et on la compare : c'est la preuve
        // que le mode plat est bien une **table de positions apprise**.
        let pos = Position::startpos();
        let hidden = net.hidden();
        let mut expected = net.b2;
        for j in 0..hidden {
            let mut h = net.b1[j] + net.w_side[j];
            for &(plane, sq) in &pieces_of(&pos) {
                h += net.w1[(plane as usize * SQ + sq as usize) * hidden + j];
            }
            expected += net.w2[j] * h;
        }
        let got = net.forward(&pos);
        assert!(
            (got - expected).abs() < 1e-3,
            "le mode plat ne calcule pas la formule lineaire : {got} != {expected}"
        );
    }

    /// L'aller-retour sur fichier ne change rien (le format est
    /// normatif : il porte les poids de la phase).
    #[test]
    fn save_load_roundtrip() {
        let net = Net::random(0x5EED, HIDDEN);
        let dir = std::env::temp_dir().join("mychess-nn-test");
        let path = dir.join("net.bin");
        net.save(&path).unwrap();
        let back = Net::load(&path).unwrap();
        for pos in positions() {
            assert_eq!(
                net.forward(&pos).to_bits(),
                back.forward(&pos).to_bits(),
                "l'aller-retour sur fichier a changé la valeur"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// La feature « trait » est **effectivement** lue : sans elle, le
    /// réseau ne pourrait pas distinguer « les blancs sont au trait »
    /// de « les noirs sont au trait » — or la convention de la phase est
    /// **côté trait**. C'est donc une propriété structurante du réseau.
    #[test]
    fn the_side_feature_is_used() {
        let mut net = Net::zeros(HIDDEN);
        for j in 0..HIDDEN {
            net.w_side[HIDDEN + j] = 0.5; // le trait « noirs »
            net.w2[j] = 1.0;
        }
        let mut pos = Position::startpos();
        let white = net.forward(&pos);
        pos.side = Color::Black;
        let black = net.forward(&pos);
        assert!(black > white, "la feature « trait » n'influence pas la valeur");
    }
}
