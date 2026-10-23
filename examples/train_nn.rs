//! L'**entraînement** du réseau de valeur (phase NN, tranche N3).
//!
//! # Le protocole
//!
//! * le corpus vient de `examples/gen_positions` (auto-play MCTS) ; chaque
//!   position porte le **résultat de sa partie** ;
//! * la cible est **côté trait**, dans `[-1, 1]` — la même échelle que
//!   la valeur du MCTS ;
//! * la découpe **train/test** se fait **par partie** (jamais par
//!   position) : sinon les positions d'une même partie se répondent
//!   mutuellement, et la mesure d'erreur serait une illusion ;
//! * l'optimiseur est **Adam**, implémenté ici (aucune dépendance) ;
//! * tout est **déterministe** : même corpus + même graine ⇒ mêmes
//!   poids.
//!
//! # Le gate
//!
//! L'erreur du réseau (RMSE sur le test) doit être **meilleure** que
//! celle de l'évaluation statique **calibrée** : on ajuste un seul
//! scalaire `a` sur l'ensemble d'entraînement (au sens des moindres
//! carrés) pour que l'évaluation statique soit dans la même échelle que
//! la cible, puis on compare les deux RMSE **sur le test**. C'est la
//! comparaison honnête : sans calibration, on comparerait deux échelles
//! différentes et le gate ne voudrait rien dire.
//!
//! # Usage
//!
//! ```text
//! cargo run --release --example train_nn -- <corpus> <poids> \
//!     [--epochs N] [--lr F] [--batch N] [--seed S] [--hidden N]
//! ```

use mychess::corpus::read_corpus;
use mychess::eval;
use mychess::mcts::Rng;
use mychess::nn::{pieces_of, Net, HIDDEN, NN_CP_SCALE};
use mychess::position::Position;

/// Un exemple d'entraînement.
struct Sample {
    /// Les pièces (plan, case), dans l'ordre canonique.
    pieces: Vec<(u32, u32)>,
    /// `true` si les blancs sont au trait.
    white_to_move: bool,
    /// La cible (côté trait, dans `[-1, 1]`).
    target: f32,
    /// La valeur statique normalisée (côté trait, dans `[-1, 1]).
    static_q: f32,
    /// L'index de la partie (pour la découpe par partie).
    game: u32,
    /// Le ply de la position dans sa partie (pour la récence).
    ply: u32,
    /// Le poids de l'exemple dans la perte (la **récence** : une
    /// position proche de la fin de la partie porte une information
    /// bien plus fiable sur la valeur qu'une position du début).
    weight: f32,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut positional: Vec<String> = Vec::new();
    let mut epochs = 30usize;
    let mut lr = 0.01f32;
    let mut batch = 32usize;
    let mut seed: u64 = 20261023;
    let mut hidden = HIDDEN;
    let mut test_every = 5usize; // une partie sur `test_every` est réservée au test
    let mut limit: Option<usize> = None;
    let mut linear = false;
    let mut recency: Option<f32> = None;
    let mut discount: f32 = 1.0;

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--epochs" => {
                epochs = args[i + 1].parse().expect("--epochs");
                i += 2;
            }
            "--lr" => {
                lr = args[i + 1].parse().expect("--lr");
                i += 2;
            }
            "--batch" => {
                batch = args[i + 1].parse().expect("--batch");
                i += 2;
            }
            "--seed" => {
                seed = args[i + 1].parse().expect("--seed");
                i += 2;
            }
            "--hidden" => {
                hidden = args[i + 1].parse().expect("--hidden");
                i += 2;
            }
            "--linear" => {
                linear = true;
                i += 1;
            }
            "--recency" => {
                recency = Some(args[i + 1].parse().expect("--recency"));
                i += 2;
            }
            "--discount" => {
                discount = args[i + 1].parse().expect("--discount");
                i += 2;
            }
            "--limit" => {
                limit = Some(args[i + 1].parse().expect("--limit"));
                i += 2;
            }
            "--test-every" => {
                test_every = args[i + 1].parse().expect("--test-every");
                i += 2;
            }
            "--check-grad" => {
                positional.push("--check-grad".to_string());
                i += 1;
            }
            a if a.starts_with("--") => {
                eprintln!("option inconnue : {a}");
                std::process::exit(2);
            }
            a => {
                positional.push(a.to_string());
                i += 1;
            }
        }
    }
    let check_only = positional.iter().any(|p| p == "--check-grad");
    if positional.len() < 2 && !check_only {
        eprintln!("usage : train_nn <corpus> <poids> [options]");
        std::process::exit(2);
    }
    let corpus_path = positional[0].clone();
    let out_path = positional.get(1).cloned().unwrap_or_else(|| "/tmp/value_v1.bin".to_string());

    // --- le corpus -------------------------------------------------------
    let records = read_corpus(std::path::Path::new(&corpus_path)).expect("lecture du corpus");
    let mut samples: Vec<Sample> = Vec::with_capacity(records.len());
    for r in &records {
        let pos = match Position::from_fen(&r.fen) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("FEN illisible (partie {}, ply {}) : {e}", r.game, r.ply);
                continue;
            }
        };
        let white_to_move = pos.side == mychess::types::Color::White;
        let static_q = (eval::eval_for_mover(&pos) as f32 / NN_CP_SCALE).clamp(-1.0, 1.0);
        samples.push(Sample {
            pieces: pieces_of(&pos),
            white_to_move,
            target: r.target(white_to_move),
            static_q,
            game: r.game,
            ply: r.ply,
            weight: 1.0,
        });
    }
    // La **récence** : le nombre de demi-coups qui séparent la position
    // de la fin de sa partie. On le déduit du corpus (les positions
    // d'une partie sont consécutives, la dernière a le plus grand ply).
    let has_continuous_label = records.first().and_then(|r| r.label).is_some();
    let mut last_ply: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    for r in &records {
        let e = last_ply.entry(r.game).or_insert(r.ply);
        if r.ply > *e {
            *e = r.ply;
        }
    }
    for s in samples.iter_mut() {
        let end = last_ply.get(&s.game).copied().unwrap_or(s.ply);
        let d = end.saturating_sub(s.ply) as f32;
        s.weight = match recency {
            Some(tau) if tau > 0.0 => (-d / tau).exp(),
            _ => 1.0,
        };
        // La décote ne s'applique qu'aux corpus annotés par le
        // **résultat de partie** : l'étiquette continue (la valeur de
        // la recherche) est déjà une valeur, la décoter serait
        // faux.
        if discount != 1.0 && !has_continuous_label {
            // La décote : une position à d demi-coups de la fin ne vaut
            // pas le résultat final, mais un résultat *affaibli*.
            s.target *= discount.powf(d);
        }
    }
    // La découpe est **par partie**.
    let mut train: Vec<usize> = Vec::new();
    let mut test: Vec<usize> = Vec::new();
    for (i, s) in samples.iter().enumerate() {
        if (s.game as usize) % test_every == 0 {
            test.push(i);
        } else {
            train.push(i);
        }
    }
    // Le mode « sur-apprentissage » : un petit sous-ensemble, pour
    // distinguer un **défaut** de l'optimiseur d'un simple réglage.
    if let Some(limit) = limit {
        train.truncate(limit);
        test.truncate(limit);
    }
    eprintln!(
        "corpus : {} enregistrements, {} exemples valides, {} train / {} test (découpe par partie, 1/{test_every})",
        records.len(),
        samples.len(),
        train.len(),
        test.len()
    );

    if std::env::args().any(|a| a == "--check-grad") {
        check_gradient(&samples, &train, hidden);
        return;
    }

    // --- l'initialisation -------------------------------------------------
    if linear {
        // Le mode **plat** : une seule unité cachée, sans ReLU — c'est
        // une **table de positions apprise** (768 paramètres).
        //
        // La couche de sortie est **figée à 1** : sans cela, le modèle
        // est *mal conditionné* (les poids des lignes et le poids de
        // sortie ne sont définis qu'à un facteur près), et Adam — qui
        // normalise chaque paramètre séparément — fait décroître le
        // poids de sortie jusqu'à zéro : le réseau n'apprend plus rien.
        // C'est un fait mesuré, pas une intuition.
        hidden = 1;
    }
    let mut net = Net::random(seed, hidden);
    net.relu = !linear;
    if linear {
        net.w2 = vec![1.0];
    }
    let mut adam = Adam::new(&net);
    let mut grads = Gradients::new(&net);

    // --- l'entraînement ---------------------------------------------------
    let mut rng = Rng::new(seed ^ 0x1234_5678_9ABC_DEF0);
    let mut order: Vec<usize> = train.clone();
    let mut step = 0u64;
    let mut history: Vec<(usize, f32, f32, f32)> = Vec::new();
    for epoch in 0..epochs {
        // Le mélange est déterministe (RNG de graine fixe).
        for i in (1..order.len()).rev() {
            let j = (rng.next_u64() % (i as u64 + 1)) as usize;
            order.swap(i, j);
        }
        let mut epoch_loss = 0f32;
        let mut n = 0usize;
        for chunk in order.chunks(batch) {
            grads.clear();
            for &i in chunk {
                let s = &samples[i];
                let (loss, g) = backward(&net, s);
                epoch_loss += loss;
                n += 1;
                grads.accumulate(s, &g);
            }
            step += 1;
            adam.apply(&mut net, &grads, lr, step);
        }
        let tr_loss = epoch_loss / n.max(1) as f32;
        if epoch % 3 == 0 || epoch == epochs - 1 {
            let (tr_rmse, te_rmse) = evaluate(&net, &samples, &train, &test);
            let (mean_out, mean_abs_w2, corr) = diagnostics(&net, &samples, &train);
            eprintln!(
                "epoch {epoch:3} : perte {tr_loss:.5} — RMSE train {tr_rmse:.4} — RMSE test {te_rmse:.4}                  — sortie moy {mean_out:+.3} — |w2| moy {mean_abs_w2:.4} — corr {corr:+.3}"
            );
            history.push((epoch, tr_loss, tr_rmse, te_rmse));
        }
    }

    // --- la mesure --------------------------------------------------------
    let (tr_rmse, te_rmse) = evaluate(&net, &samples, &train, &test);
    let (a, static_rmse_test, static_rmse_train) = calibrated_static(&samples, &train, &test);
    eprintln!();
    eprintln!("=== la mesure (gate de la tranche) ===");
    eprintln!("réseau      : RMSE train {tr_rmse:.4} — RMSE test {te_rmse:.4}");
    eprintln!(
        "éval statique calibrée (a = {a:.3}) : RMSE train {static_rmse_train:.4} — RMSE test {static_rmse_test:.4}"
    );
    let verdict = if te_rmse < static_rmse_test {
        "PASSÉ"
    } else {
        "ÉCHOUÉ"
    };
    eprintln!("gate (le réseau doit battre l'éval statique calibrée sur le test) : {verdict}");

    // --- l'écriture ------------------------------------------------------
    net.save(std::path::Path::new(&out_path)).expect("écriture des poids");
    eprintln!("poids écrits : {out_path}");

    // Le journal de la courbe, versionnable.
    let log_path = format!("{out_path}.log");
    let mut log = String::new();
    log.push_str("# entraînement du réseau de valeur (phase NN, tranche N3)\n");
    log.push_str(&format!("# corpus : {corpus_path}\n"));
    log.push_str(&format!(
        "# graine : {seed} — epoques : {epochs} — lr : {lr} — lot : {batch}\n"
    ));
    log.push_str(&format!("# couche cachee : {hidden}\n"));
    log.push_str("# epoch | perte | RMSE train | RMSE test\n");
    for (e, l, tr, te) in &history {
        log.push_str(&format!("{e} | {l:.6} | {tr:.5} | {te:.5}\n"));
    }
    log.push_str(&format!("# RMSE test reseau : {te_rmse:.5}\n"));
    log.push_str(&format!(
        "# RMSE test eval statique calibree : {static_rmse_test:.5} (a = {a:.4})\n"
    ));
    log.push_str(&format!("# gate : {verdict}\n"));
    std::fs::write(&log_path, log).expect("écriture du journal");
    eprintln!("journal écrit : {log_path}");
}

/// La **vérification numérique** du gradient : on compare le gradient
/// analytique (celui du backward) au gradient par différences finies.
///
/// C'est l'instrument qui a permis de corriger une divergence de
/// l'entraînement : sans lui, on.scrollait sur une perte qui augmentait.
fn check_gradient(samples: &[Sample], train: &[usize], hidden: usize) {
    let net = Net::random(12345, hidden);
    let s = &samples[train[0]];
    let (_, g) = backward(&net, s);
    let eps = 1e-3f32;
    let mut checked = 0usize;
    // b1[0], w2[0], b2, w_side[side+0], et une ligne de pièce.
    let mut targets: Vec<(&str, usize, f32, f32)> = Vec::new();
    targets.push(("b1[0]", 0, net.b1[0], g.dh[0]));
    targets.push(("w2[0]", 0, net.w2[0], g.dy * g.h[0]));
    targets.push(("b2", 0, net.b2, g.dy));
    let side = if s.white_to_move { 0usize } else { hidden };
    targets.push(("w_side", side, net.w_side[side], g.dh[0]));
    if let Some(&(plane, sq)) = s.pieces.first() {
        let base = (plane as usize * 64 + sq as usize) * hidden;
        targets.push(("w1", base, net.w1[base], g.dh[0]));
    }
    for (name, idx, w, analytic) in targets {
        let mut net2 = net.clone();
        match name {
            "b1[0]" => net2.b1[idx] = w + eps,
            "w2[0]" => net2.w2[idx] = w + eps,
            "b2" => net2.b2 = w + eps,
            "w_side" => net2.w_side[idx] = w + eps,
            _ => net2.w1[idx] = w + eps,
        }
        let lp = net2.forward_pieces(&s.pieces, s.white_to_move) - s.target;
        let mut net3 = net.clone();
        match name {
            "b1[0]" => net3.b1[idx] = w - eps,
            "w2[0]" => net3.w2[idx] = w - eps,
            "b2" => net3.b2 = w - eps,
            "w_side" => net3.w_side[idx] = w - eps,
            _ => net3.w1[idx] = w - eps,
        }
        let lm = net3.forward_pieces(&s.pieces, s.white_to_move) - s.target;
        let numeric = (lp * lp - lm * lm) / (2.0 * eps);
        println!(
            "{name}[{idx}] : analytique {analytic:+.6} — numérique {numeric:+.6} — écart {:+.2e}",
            (analytic - numeric).abs()
        );
        checked += 1;
    }
    println!("{checked} paramètres vérifiés");
}

/// Les diagnostics : la moyenne des sorties, la taille des poids de
/// sortie, et la corrélation sortie/cible.
fn diagnostics(net: &Net, samples: &[Sample], idx: &[usize]) -> (f32, f32, f32) {
    // Le nombre d'unités **actives** (ReLU) : s'il est nul, le réseau
    // n'exprime rien (le défaut hunted : la ReLU tue tout le gradient).
    let mut active = 0f64;
    let mut total = 0f64;
    let mut pre = 0f64;
    for &i in idx.iter().take(200) {
        let s = &samples[i];
        let hidden = net.hidden();
        let mut h = vec![0f32; hidden];
        h.copy_from_slice(&net.b1[..hidden]);
        for &(plane, sq) in &s.pieces {
            let base = (plane as usize * 64 + sq as usize) * hidden;
            for j in 0..hidden {
                h[j] += net.w1[base + j];
            }
        }
        let side = if s.white_to_move { 0usize } else { hidden };
        for j in 0..hidden {
            h[j] += net.w_side[side + j];
        }
        for v in h.iter() {
            pre += *v as f64;
            total += 1.0;
            if *v > 0.0 {
                active += 1.0;
            }
        }
    }
    let mean_w1: f64 = net.w1.iter().map(|w| *w as f64).sum::<f64>() / net.w1.len() as f64;
    let mean_b1: f64 = net.b1.iter().map(|w| *w as f64).sum::<f64>() / net.b1.len() as f64;
    let mean_side: f64 = net.w_side.iter().map(|w| *w as f64).sum::<f64>() / net.w_side.len() as f64;
    eprintln!(
        "   [diag] unités actives : {active}/{total} — moyenne pré-activation : {:+.4} — w1 moy {mean_w1:+.4} — b1 moy {mean_b1:+.4} — side moy {mean_side:+.4} — pieces {}",
        pre / total.max(1.0),
        samples[idx[0]].pieces.len()
    );
    let mut sum = 0f64;
    let mut n = 0f64;
    let mut sx = 0f64;
    let mut sy = 0f64;
    let mut sxx = 0f64;
    let mut syy = 0f64;
    let mut sxy = 0f64;
    for &i in idx {
        let s = &samples[i];
        let y = net.forward_pieces(&s.pieces, s.white_to_move) as f64;
        let t = s.target as f64;
        sum += y;
        n += 1.0;
        sx += y;
        sy += t;
        sxx += y * y;
        syy += t * t;
        sxy += y * t;
    }
    let cov = sxy / n - (sx / n) * (sy / n);
    let varx = (sxx / n - (sx / n) * (sx / n)).max(0.0);
    let vary = (syy / n - (sy / n) * (sy / n)).max(0.0);
    let corr = if varx > 0.0 && vary > 0.0 {
        (cov / (varx.sqrt() * vary.sqrt())) as f32
    } else {
        0.0
    };
    let mean_abs_w2 = net.w2.iter().map(|w| w.abs()).sum::<f32>() / net.w2.len() as f32;
    ((sum / n) as f32, mean_abs_w2, corr)
}

/// La RMSE sur un ensemble.
fn rmse(net: &Net, samples: &[Sample], idx: &[usize]) -> f32 {
    let mut acc = 0f64;
    for &i in idx {
        let s = &samples[i];
        let y = net.forward_pieces(&s.pieces, s.white_to_move).clamp(-1.0, 1.0);
        let d = (y as f64 - s.target as f64).powi(2);
        acc += d;
    }
    (acc / idx.len().max(1) as f64).sqrt() as f32
}

fn evaluate(net: &Net, samples: &[Sample], train: &[usize], test: &[usize]) -> (f32, f32) {
    (rmse(net, samples, train), rmse(net, samples, test))
}

/// La calibration de l'évaluation statique : un scalaire `a` ajusté sur
/// l'ensemble d'entraînement (moindres carrés), puis les deux RMSE.
fn calibrated_static(samples: &[Sample], train: &[usize], test: &[usize]) -> (f32, f32, f32) {
    let mut num = 0f64;
    let mut den = 0f64;
    for &i in train {
        let s = &samples[i];
        num += (s.static_q as f64) * (s.target as f64);
        den += (s.static_q as f64).powi(2);
    }
    let a = if den > 0.0 { (num / den) as f32 } else { 0.0 };
    let mut acc_tr = 0f64;
    for &i in train {
        let s = &samples[i];
        acc_tr += ((a * s.static_q) as f64 - s.target as f64).powi(2);
    }
    let mut acc_te = 0f64;
    for &i in test {
        let s = &samples[i];
        acc_te += ((a * s.static_q) as f64 - s.target as f64).powi(2);
    }
    (
        a,
        (acc_te / test.len().max(1) as f64).sqrt() as f32,
        (acc_tr / train.len().max(1) as f64).sqrt() as f32,
    )
}

/// Les gradients d'un exemple (le réseau est petit : on recalcule la
/// forward et on en déduit le gradient).
struct Backprop {
    /// Le gradient de la couche de sortie : `dy` (dL/dz de la sortie).
    dy: f32,
    /// Le gradient de la couche cachée (après le ReLU) : `dh[j]`.
    dh: Vec<f32>,
    /// La couche cachée **activée** (ReLU appliqué) : `h[j]`.
    h: Vec<f32>,
}

/// La forward + la backward d'un exemple.
fn backward(net: &Net, s: &Sample) -> (f32, Backprop) {
    let hidden = net.hidden();
    let mut h = vec![0f32; hidden];
    h.copy_from_slice(&net.b1[..hidden]);
    for &(plane, sq) in &s.pieces {
        let base = (plane as usize * 64 + sq as usize) * hidden;
        let row = &net.w1[base..base + hidden];
        for j in 0..hidden {
            h[j] += row[j];
        }
    }
    let side = if s.white_to_move { 0usize } else { hidden };
    for j in 0..hidden {
        h[j] += net.w_side[side + j];
    }
    // **La ReLU est appliquée avant la couche de sortie** — exactement
    // comme dans `nn::Net::finish`. (Défaut corrigé : l'entraînement
    // optimisait une fonction *différente* de celle qui est déployée,
    // et le gradient ne correspondait à rien ; c'est la vérification
    // numérique (`--check-grad`) qui l'a montré.)
    let mut z = net.b2;
    let mut activated = vec![0f32; hidden];
    for j in 0..hidden {
        let a = if net.relu && h[j] <= 0.0 { 0.0 } else { h[j] };
        activated[j] = a;
        z += net.w2[j] * a;
    }
    // La sortie est **linéaire** : pas de `tanh` (qui saturerait sur
    // les cibles ±1 et annulerait le gradient).
    let y = z;
    let loss = s.weight * (y - s.target).powi(2);
    // dL/dz = 2 w (y - t)
    let dy = 2.0 * s.weight * (y - s.target);
    // dh[j] = dy * w2[j] * relu'(h)
    let mut dh = vec![0f32; hidden];
    for j in 0..hidden {
        let active = !net.relu || h[j] > 0.0;
        dh[j] = if active { dy * net.w2[j] } else { 0.0 };
    }
    (
        loss,
        Backprop {
            dy,
            dh,
            h: activated,
        },
    )
}

/// L'accumulateur de gradients d'un lot.
///
/// Le gradient de `w1` est **creux** : seules les lignes des pièces
/// présentes dans le lot reçoivent un gradient. Les lignes touchées
/// sont mémorisées (pour n'appliquer l'update d'Adam que là).
struct Gradients {
    w1: Vec<f32>,
    b1: Vec<f32>,
    w_side: Vec<f32>,
    w2: Vec<f32>,
    b2: f32,
    /// Les lignes de `w1` touchées par le lot (base des lignes).
    touched: Vec<usize>,
    /// Le drapeau « déjà touchée ».
    flags: Vec<bool>,
    /// Le nombre d'exemples du lot (le gradient est une moyenne).
    n: usize,
}

impl Gradients {
    fn new(net: &Net) -> Gradients {
        let hidden = net.b1.len();
        Gradients {
            w1: vec![0f32; net.w1.len()],
            b1: vec![0f32; hidden],
            w_side: vec![0f32; net.w_side.len()],
            w2: vec![0f32; hidden],
            b2: 0.0,
            touched: Vec::new(),
            flags: vec![false; net.w1.len() / hidden],
            n: 0,
        }
    }

    fn clear(&mut self) {
        self.w1.fill(0.0);
        self.b1.fill(0.0);
        self.w_side.fill(0.0);
        self.w2.fill(0.0);
        self.b2 = 0.0;
        for &base in &self.touched {
            self.flags[base / self.b1.len()] = false;
        }
        self.touched.clear();
        self.n = 0;
    }

    fn accumulate(&mut self, s: &Sample, g: &Backprop) {
        let hidden = self.b1.len();
        self.n += 1;
        for j in 0..hidden {
            self.b1[j] += g.dh[j];
            self.w2[j] += g.dy * g.h[j];
        }
        self.b2 += g.dy;
        let side = if s.white_to_move { 0usize } else { hidden };
        for j in 0..hidden {
            self.w_side[side + j] += g.dh[j];
        }
        // Les lignes des pièces : chaque contributeur vaut 1.
        for &(plane, sq) in &s.pieces {
            let row = plane as usize * 64 + sq as usize;
            let base = row * hidden;
            if !self.flags[row] {
                self.flags[row] = true;
                self.touched.push(base);
            }
            for j in 0..hidden {
                self.w1[base + j] += g.dh[j];
            }
        }
    }
}

/// L'optimiseur **Adam** (aucune dépendance externe).
struct Adam {
    m_w1: Vec<f32>,
    v_w1: Vec<f32>,
    m_b1: Vec<f32>,
    v_b1: Vec<f32>,
    m_w_side: Vec<f32>,
    v_w_side: Vec<f32>,
    m_w2: Vec<f32>,
    v_w2: Vec<f32>,
    m_b2: f32,
    v_b2: f32,
}

const BETA1: f32 = 0.9;
const BETA2: f32 = 0.999;
const EPS: f32 = 1e-8;

impl Adam {
    fn new(net: &Net) -> Adam {
        Adam {
            m_w1: vec![0.0; net.w1.len()],
            v_w1: vec![0.0; net.w1.len()],
            m_b1: vec![0.0; net.b1.len()],
            v_b1: vec![0.0; net.b1.len()],
            m_w_side: vec![0.0; net.w_side.len()],
            v_w_side: vec![0.0; net.w_side.len()],
            m_w2: vec![0.0; net.w2.len()],
            v_w2: vec![0.0; net.w2.len()],
            m_b2: 0.0,
            v_b2: 0.0,
        }
    }

    fn apply(&mut self, net: &mut Net, grads: &Gradients, lr: f32, step: u64) {
        let bc1 = 1.0 - BETA1.powi(step as i32);
        let bc2 = 1.0 - BETA2.powi(step as i32);
        let hidden = net.b1.len();
        let scale = 1.0 / grads.n.max(1) as f32;
        // Les lignes touchées de `w1`.
        for &base in &grads.touched {
            for j in 0..hidden {
                let idx = base + j;
                let g = grads.w1[idx] * scale;
                self.m_w1[idx] = BETA1 * self.m_w1[idx] + (1.0 - BETA1) * g;
                self.v_w1[idx] = BETA2 * self.v_w1[idx] + (1.0 - BETA2) * g * g;
                let m = self.m_w1[idx] / bc1;
                let v = self.v_w1[idx] / bc2;
                net.w1[idx] -= lr * m / (v.sqrt() + EPS);
            }
        }
        for j in 0..hidden {
            let g = grads.b1[j] * scale;
            self.m_b1[j] = BETA1 * self.m_b1[j] + (1.0 - BETA1) * g;
            self.v_b1[j] = BETA2 * self.v_b1[j] + (1.0 - BETA2) * g * g;
            let m = self.m_b1[j] / bc1;
            let v = self.v_b1[j] / bc2;
            net.b1[j] -= lr * m / (v.sqrt() + EPS);

            if net.relu {
                // En mode **plat**, la couche de sortie est figée (cf.
                // l'initialisation) : sans cela, le modèle est mal
                // conditionné et n'apprend rien.
                let g = grads.w2[j] * scale;
                self.m_w2[j] = BETA1 * self.m_w2[j] + (1.0 - BETA1) * g;
                self.v_w2[j] = BETA2 * self.v_w2[j] + (1.0 - BETA2) * g * g;
                let m = self.m_w2[j] / bc1;
                let v = self.v_w2[j] / bc2;
                net.w2[j] -= lr * m / (v.sqrt() + EPS);
            }
        }
        for j in 0..2 * hidden {
            let g = grads.w_side[j] * scale;
            self.m_w_side[j] = BETA1 * self.m_w_side[j] + (1.0 - BETA1) * g;
            self.v_w_side[j] = BETA2 * self.v_w_side[j] + (1.0 - BETA2) * g * g;
            let m = self.m_w_side[j] / bc1;
            let v = self.v_w_side[j] / bc2;
            net.w_side[j] -= lr * m / (v.sqrt() + EPS);
        }
        let g = grads.b2 * scale;
        self.m_b2 = BETA1 * self.m_b2 + (1.0 - BETA1) * g;
        self.v_b2 = BETA2 * self.v_b2 + (1.0 - BETA2) * g * g;
        let m = self.m_b2 / bc1;
        let v = self.v_b2 / bc2;
        net.b2 -= lr * m / (v.sqrt() + EPS);
    }
}
