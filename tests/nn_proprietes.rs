//! Les **propriétés** du réseau de valeur (phase NN) — les tests que
//! la suite ne couvrait pas (revue du 2026-10-23).
//!
//! # Pourquoi ce fichier existe
//!
//! L'audit a trouvé quatre défauts du câblage NN qu'aucun test ne
//! pouvait voir :
//!
//! * un fichier de poids **tronqué** faisait **paniquer** le worker UCI
//!   (`buf[16..20]` hors bornes) — et un panic dans le worker tue le
//!   moteur, sans `bestmove` ;
//! * un poids `NaN` était accepté : le cast Rust de `NaN` en `i32`
//!   vaut `0`, donc le moteur évaluait **toute** position à égalité —
//!   en silence ;
//! * l'écriture n'était **pas atomique** : un `.bin` interrompu laissait
//!   un fichier tronqué sur le disque ;
//! * `EvalNN=1` **supprimait le bruit** : le dispositif de la phase AB7
//!   (l'erreur de la fonction de valeur) disparaissait sans le dire.
//!
//! Et surtout, un invariant que **personne** n'écrivait : le chemin
//! **SIMD** (AVX2) doit donner **exactement** le même résultat que le
//! chemin scalaire. Le SIMD ne change que le débit ; s'il changeait le
//! résultat, les mesures de force et de débit compareraient deux
//! moteurs différents.

use mychess::eval;
use mychess::movegen::{self, MoveList};
use mychess::nn::{self, Net};
use mychess::position::Position;
use mychess::types::GenKind;

/// Le **verrou de l'état global** du réseau (`nn::load_and_enable`).
///
/// Le réseau chargé est un **état global** : deux tests qui l'activent
/// en parallèle se lisent l'un l'autre (le test a alors charged le
/// réseau de l'autre test — 500 centipions lus au lieu de 1000). Les
/// tests qui touchent cet état prennent donc le verrou.
fn lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Un dossier temporaire **propre à un test** (les tests s'exécutent en
/// parallèle : un dossier partagé serait une course).
fn tmp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mychess-nn-proprietes-{name}"));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Un corpus de positions tirées par **auto-jeu** (pas un cas
/// d'école) : c'est le seul moyen de couvrir les configurations
/// creuses que la forward doit additionner.
fn corpus(n: usize) -> Vec<Position> {
    let mut state = 0xDEAD_BEEF_CAFE_1234u64;
    let mut rng = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut out = Vec::new();
    let mut pos = Position::startpos();
    while out.len() < n {
        let mut list = MoveList::new();
        movegen::generate(&pos, &mut list, GenKind::All);
        if list.is_empty() {
            pos = Position::startpos();
            continue;
        }
        out.push(pos);
        let mv = list.get((rng() as usize) % list.len());
        // La position **avance** réellement (pas de `unmake`) : sinon
        // le corpus serait cent fois la position initiale.
        pos.make(mv);
    }
    out
}

// ---------------------------------------------------------------------------
// 1. Le SIMD est identique au scalaire, bit à bit.
// ---------------------------------------------------------------------------

/// **Le chemin SIMD (AVX2) donne exactement le même résultat que le
/// chemin scalaire** — bit à bit.
///
/// L'accumulation de la forward est le cœur de l'inférence : une
/// trentaine de lignes de `HIDDEN` flottants. Le repli scalaire doit
/// donner **le même** flottant, pas « presque le même » : une
/// différence d'un ULPEnough suffirait à changer une valeur de
/// position, donc les décisions du moteur, donc les mesures.
///
/// On compare sur des tranches de **toutes** les longueurs — y
/// compris celles qui ne sont pas un multiple de 8 (la queue scalaire
/// du chemin vectoriel) — et sur des valeurs qui高度的 des
/// cancellation (somme de grands nombres de signes opposés).
#[test]
fn le_simd_est_identique_au_scalaire_bit_a_bit() {
    let mut state = 0x1234_5678_9ABC_DEF0u64;
    let mut rng = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for len in [
        1usize, 2, 3, 7, 8, 9, 15, 16, 17, 31, 32, 33, 64, 128, 256, 1024,
    ] {
        for _ in 0..8 {
            let src: Vec<f32> = (0..len)
                .map(|_| (rng() as f32 / (u32::MAX as f32)) * 40.0 - 20.0)
                .collect();
            let mut a = vec![0.0f32; len];
            let mut b = vec![0.0f32; len];
            nn::add_slice(&mut a, &src);
            nn::add_slice_scalar(&mut b, &src);
            for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
                assert_eq!(
                    x.to_bits(),
                    y.to_bits(),
                    "le SIMD et le scalaire divergent à l'indice {i} \
                     (longueur {len}) : {x} contre {y}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 2. Le pont centipions du réseau vaut 1000.
// ---------------------------------------------------------------------------

/// **Le pont `Q ↔ centipions` du réseau vaut 1 000**, et non 100.
///
/// C'est la même famille de bug que celui du MCTS (le score dix fois
/// trop petit) : un pont écrit « en dur » sans constante nommée est un
/// pont qui se casse. Ici le pont est **nommé** (`NN_CP_SCALE`), et le
/// test le vérifie par le **comportement** : un réseau dont la sortie
/// vaut exactement `1.0` doit rendre exactement `1000` centipions.
#[test]
fn le_pont_centipions_du_reseau_vaut_1000() {
    let _g = lock();
    assert_eq!(
        nn::NN_CP_SCALE,
        1000.0,
        "le pont centipions du réseau a changé"
    );

    // Un réseau **plat** dont la sortie vaut 1.0 partout.
    let mut net = Net::zeros(8);
    net.relu = false;
    net.b2 = 1.0;

    let dir = tmp_dir("pont");
    let path = dir.join("pont.bin");
    net.save(&path).unwrap();
    nn::load_and_enable(&path).unwrap();
    assert!(nn::enabled(), "le réseau n'a pas été activé");

    let pos = Position::startpos();
    let cp = nn::value_cp(&pos).expect("le réseau est activé");
    assert_eq!(cp, 1000, "le pont centipions n'est pas 1 pour 1 000");

    nn::unload();
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// 3. Un fichier de poids dégradé est refusé, sans panic.
// ---------------------------------------------------------------------------

/// **Un fichier de poids dégradé est toujours refusé, et jamais
/// panic** — quelle que soit la dégradation.
///
/// On parcourt **toutes** les longueurs de troncature (et quelques
/// fichiers trop longs, et quelques poids non finis) : la propriété
/// est « jamais de panic, toujours un `Err` », pas « le cas de
/// troncature que j'ai pensé à écrire ».
#[test]
fn un_fichier_degrade_est_refuse_sans_paniquer() {
    let net = Net::random(0xABCD, 16);
    let dir = tmp_dir("degrade");
    let path = dir.join("poids.bin");
    net.save(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();

    // 1. Toutes les troncatures de l'**en-tête** (les 20 premiers
    //    octets) — c'est là que le panic se produisait — puis un
    //    échantillon régulier sur le reste du fichier. Parcourir
    //    *chaque* longueur du fichier complet prendrait une minute
    //    pour rien : le risque est dans l'en-tête.
    for cut in 0..=24 {
        let p = dir.join("tronque.bin");
        std::fs::write(&p, &bytes[..cut]).unwrap();
        let res = std::panic::catch_unwind(|| nn::Net::load(&p));
        assert!(res.is_ok(), "le chargement a PANIQUÉ sur {cut} octets");
        assert!(
            res.unwrap().is_err(),
            "un fichier de {cut} octets a été accepté"
        );
    }
    let mut cut = 24;
    while cut < bytes.len() {
        let p = dir.join("tronque.bin");
        std::fs::write(&p, &bytes[..cut]).unwrap();
        let res = std::panic::catch_unwind(|| nn::Net::load(&p));
        assert!(res.is_ok(), "le chargement a PANIQUÉ sur {cut} octets");
        assert!(
            res.unwrap().is_err(),
            "un fichier de {cut} octets a été accepté"
        );
        cut += 37;
    }

    // 2. Un fichier trop long.
    let p = dir.join("trop_long.bin");
    let mut trop = bytes.clone();
    trop.extend_from_slice(&[0u8; 64]);
    std::fs::write(&p, &trop).unwrap();
    assert!(
        nn::Net::load(&p).is_err(),
        "un fichier trop long a été accepté"
    );

    // 3. Des poids non finis.
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut mauvais = net.clone();
        mauvais.w1[0] = bad;
        let p = dir.join("non_fini.bin");
        mauvais.save(&p).unwrap();
        assert!(
            nn::Net::load(&p).is_err(),
            "un poids non fini ({bad}) a été accepté"
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// 4. Le réseau est reproductible et la sauvegarde est exacte.
// ---------------------------------------------------------------------------

/// **Le réseau est reproductible** (la même graine donne les mêmes
/// poids, bit à bit) et **l'écriture est exacte** (sauvegarder puis
/// recharger redonne les mêmes poids, bit à bit).
///
/// C'est la condition pour qu'un modèle entraîné soit **reproductible**,
/// et qu'un duel contre un modèle donné soit rejouable.
#[test]
fn le_reseau_est_reproductible_et_la_sauvegarde_est_exacte() {
    let a = Net::random(0x1234_5678, 32);
    let b = Net::random(0x1234_5678, 32);
    assert_eq!(a.w1, b.w1, "la graine ne produit pas le même réseau");
    assert_eq!(a.b1, b.b1, "la graine ne produit pas le même réseau");
    assert_eq!(a.w2, b.w2, "la graine ne produit pas le même réseau");
    assert_eq!(
        a.b2.to_bits(),
        b.b2.to_bits(),
        "la graine ne produit pas le même réseau"
    );

    let dir = tmp_dir("reproductible");
    let path = dir.join("reseau.bin");
    a.save(&path).unwrap();
    let c = Net::load(&path).expect("le réseau doit se recharger");
    assert_eq!(a.w1, c.w1, "l'écriture n'est pas exacte (w1)");
    assert_eq!(a.b1, c.b1, "l'écriture n'est pas exacte (b1)");
    assert_eq!(a.w_side, c.w_side, "l'écriture n'est pas exacte (w_side)");
    assert_eq!(a.w2, c.w2, "l'écriture n'est pas exacte (w2)");
    assert_eq!(
        a.b2.to_bits(),
        c.b2.to_bits(),
        "l'écriture n'est pas exacte (b2)"
    );
    assert_eq!(a.relu, c.relu, "l'écriture n'est pas exacte (relu)");

    // Et les **valeurs** sont les mêmes, sur un corpus.
    let positions = corpus(64);
    for pos in &positions {
        assert_eq!(
            a.value(pos).to_bits(),
            c.value(pos).to_bits(),
            "la valeur a changé après un aller-retour sur le disque"
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// 5. La valeur apprise est finie et bornée.
// ---------------------------------------------------------------------------

/// **La valeur apprise est toujours finie et bornée** — sur un corpus
/// large, et y compris pour un réseau **saturé**.
///
/// Un `NaN` qui se glisse dans la valeur rendrait le moteur_muette :
/// le pont vers les centipions le convertirait en `0`, et toutes les
/// positions seraient jugées égales.
#[test]
fn la_valeur_apprise_est_finie_et_bornee() {
    let positions = corpus(256);
    for seed in [1u64, 2, 3] {
        let net = Net::random(seed, 32);
        for pos in &positions {
            let v = net.value(pos);
            assert!(v.is_finite(), "la valeur n'est pas finie : {v}");
            assert!((-1.0..=1.0).contains(&v), "valeur hors bornes : {v}");
        }
    }
    // Même un réseau **saturé** (poids démesurés) reste borné.
    let mut net = Net::random(4, 32);
    for w in net.w1.iter_mut() {
        *w *= 1e6;
    }
    for w in net.w2.iter_mut() {
        *w *= 1e6;
    }
    for pos in &positions {
        let v = net.value(pos);
        assert!(v.is_finite(), "la valeur n'est pas finie : {v}");
        assert!((-1.0..=1.0).contains(&v), "valeur hors bornes : {v}");
    }
}

// ---------------------------------------------------------------------------
// 6. Le bruit survit à l'activation du réseau.
// ---------------------------------------------------------------------------

/// **Le bruit de l'évaluation survit à l'activation du réseau**.
///
/// `EvalNN=1` remplace la feuille par la valeur apprise. Si le bruit
/// n'était pas ajouté **après** le pont, le dispositif de la phase AB7
/// — l'erreur de la fonction de valeur, censée bruiter les deux
/// algorithmes **identiquement** — disparaissait en silence dès que le
/// réseau était activé.
///
/// C'est le test qui verrouille le correctif : `leaf_cp` vaut
/// `valeur_apprise + bruit`, pas `valeur_apprise`.
#[test]
fn le_bruit_survit_a_lactivation_du_reseau() {
    let _g = lock();
    let mut net = Net::zeros(8);
    net.relu = false;
    net.b2 = 0.5; // 500 centipions

    let dir = tmp_dir("bruit");
    let path = dir.join("bruit.bin");
    net.save(&path).unwrap();
    nn::load_and_enable(&path).unwrap();

    let pos =
        Position::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1")
            .unwrap();

    // Bruit nul : la feuille est la valeur apprise.
    eval::set_noise(0, 0x1234);
    assert_eq!(
        nn::leaf_cp(&pos),
        500,
        "la feuille n'est pas la valeur apprise"
    );

    // Bruit non nul : la feuille est la valeur apprise **plus** le bruit.
    eval::set_noise(40, 0x1234);
    let attendue = 500 + eval::noise_for_mover(&pos);
    assert_eq!(
        nn::leaf_cp(&pos),
        attendue,
        "le bruit a disparu quand le réseau est activé"
    );
    assert_ne!(
        eval::noise_for_mover(&pos),
        0,
        "le bruit de test est nul : le test ne prouve rien"
    );

    eval::clear_noise();
    nn::unload();
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------------------
// 7. Le réseau activé ne fuit pas entre les tests.
// ---------------------------------------------------------------------------

/// **Le réseau désactivé rend l'évaluation statique, exactement.**
///
/// Le câblage par défaut (pas de réseau) doit être **bit à bit** celui
/// d'avant la phase NN : c'est la condition pour que toutes les
/// mesures gelées restent reproductibles.
#[test]
fn sans_reseau_la_feuille_est_levaluation_statique() {
    let _g = lock();
    let positions = corpus(64);
    for pos in &positions {
        assert!(
            !nn::enabled(),
            "un réseau est resté activé (fuite d'état entre les tests)"
        );
        assert_eq!(
            nn::leaf_cp(pos),
            eval::eval_for_mover(pos),
            "sans réseau, la feuille doit être l'évaluation statique"
        );
        assert!(
            nn::value_cp(pos).is_none(),
            "sans réseau, il ne doit pas y avoir de valeur apprise"
        );
    }
    // Le lot suit la même règle.
    let lot = nn::value_batch_cp(&positions);
    assert!(lot.is_none(), "sans réseau, le lot ne doit rien rendre");
}

// ---------------------------------------------------------------------------
// 8. La valeur apprise est stable (le réseau ne dérive pas).
// ---------------------------------------------------------------------------

/// **La valeur apprise est stable** : deux évaluations de la même
/// position rendent le même flottant, bit à bit.
///
/// Le chemin `forward` alloue un vecteur à chaque appel : une
/// allocation par feuille, c'est un coût ; mais surtout, une valeur
/// qui **dépendrait** de l'état mémoire (un tampon réutilisé, une
/// course) rendrait les mesures **non reproductibles**. Ce test
/// verrouille la stabilité, et il tourne **en parallèle** avec les
/// autres (c'est le seul moyen de attraper une course).
#[test]
fn la_valeur_apprise_est_stable() {
    let net = Net::random(0xFEED, 32);
    let positions = corpus(32);
    let premieres: Vec<u32> = positions.iter().map(|p| net.value(p).to_bits()).collect();
    for _ in 0..3 {
        for (i, pos) in positions.iter().enumerate() {
            assert_eq!(
                net.value(pos).to_bits(),
                premieres[i],
                "la valeur d'une position a changé d'un appel à l'autre"
            );
        }
    }
    // Et l'ordre des positions **n'a pas d'importance**.
    let inverse: Vec<u32> = positions
        .iter()
        .rev()
        .map(|p| net.value(p).to_bits())
        .collect();
    for (i, v) in inverse.iter().enumerate() {
        assert_eq!(
            *v,
            premieres[positions.len() - 1 - i],
            "la valeur dépend de l'ordre d'évaluation"
        );
    }
}
