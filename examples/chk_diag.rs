//! Diagnostic M4-réconcile v3 : sous-classes des feuilles « en échec » pour
//! expliquer les deltas discovery/double vs CPW, avec le test géométrique
//! correct : un échec « passe par » un carré libéré (origine du coup, ou
//! carré du pion capturé en ep) si ce carré est sur la ligne attaquant→roi
//! (strictement entre les deux).
//!
//! Prédictions (convention CPW : les échecs passant par le carré du pion
//! capturé n'alimentent PAS les colonnes Discovery/Double) :
//!   d8 : singles thru_capt == 4,   doubles thru_capt == 0
//!   d9 : singles thru_capt == 2037, doubles thru_capt == 767715
//!
//! usage: chk_diag <fen> <depth> <threads> <split>

use mychess::movegen::{MoveList, attackers_to, generate};
use mychess::position::{Position, castle_rook_squares};
use mychess::types::*;
use rayon::prelude::*;

#[derive(Default, Clone, Copy)]
struct Cats {
    checks: u64,
    disc_single: u64, // n==1, att != 1<<to (notre definition discovery)
    disc_direct: u64, // n==1, att == 1<<to
    dbl2: u64,
    dbl3: u64,
    dbln: u64,
    // ep : dernier coup = capture ep
    ep_s: u64,      // singles ep (n==1, checker != pion)
    ep_s_origin: u64,  // rayon passe par l'origine seulement
    ep_s_capt: u64,    // rayon passe par le carre du pion capture
    ep_s_both: u64,    // les deux
    ep_s_other: u64,   // aucun des deux (doit rester ~0)
    ep_d: u64,      // doubles+ ep (n>=2)
    ep_d_capt: u64,    // un des attaquants passe par le carre capture
    ep_d_onlyorigin: u64, // doubles ep sans checker thru-capt
    ep_d_other: u64,   // ep doubles sans aucun checker thru (knight? doive ~0)
    ep_direct: u64, // ep singles d'echec direct (pion attaque le roi)
    // non-ep, pour reference
    np_dbl: u64,
    np_disc: u64,
    // roque / promotion (d8 : promos = 0 ; le « 4 » pourrait etre du roque)
    cs_s: u64,          // singles roque (n==1, checker != pieces deplacees)
    cs_s_origin: u64,   // rayon par l'origine du ROI seulement
    cs_s_rookdirect: u64, // n==1 && att == {tour arrivee} : suspect du delta disc
    cs_s_other: u64,
    cs_d: u64,          // doubles+ roque
    cs_rook_direct: u64, // n>=2, la tour arrivee figure dans att
    pr_dbl: u64,        // promos n>=2
    pr_direct: u64,     // promos n==1, piece promue attaque le roi
    pr_disc: u64,       // promos n==1, discovery
    pr_none: u64,       // promos n==0 avec... (att vide) — doit rester 0
    np_disc_bucket: [u64; 18], // disc non-ep decomposition: kind*3 + flag(quiet/cap/dpush)
}

impl Cats {
    fn add(&mut self, o: &Cats) {
        self.checks += o.checks;
        self.disc_single += o.disc_single;
        self.disc_direct += o.disc_direct;
        self.dbl2 += o.dbl2;
        self.dbl3 += o.dbl3;
        self.dbln += o.dbln;
        self.ep_s += o.ep_s;
        self.ep_s_origin += o.ep_s_origin;
        self.ep_s_capt += o.ep_s_capt;
        self.ep_s_both += o.ep_s_both;
        self.ep_s_other += o.ep_s_other;
        self.ep_d += o.ep_d;
        self.ep_d_capt += o.ep_d_capt;
        self.ep_d_onlyorigin += o.ep_d_onlyorigin;
        self.ep_d_other += o.ep_d_other;
        self.ep_direct += o.ep_direct;
        self.np_dbl += o.np_dbl;
        self.np_disc += o.np_disc;
        self.cs_s += o.cs_s;
        self.cs_s_origin += o.cs_s_origin;
        self.cs_s_rookdirect += o.cs_s_rookdirect;
        self.cs_s_other += o.cs_s_other;
        self.cs_d += o.cs_d;
        self.cs_rook_direct += o.cs_rook_direct;
        self.pr_dbl += o.pr_dbl;
        self.pr_direct += o.pr_direct;
        self.pr_disc += o.pr_disc;
        self.pr_none += o.pr_none;
        for i in 0..18 {
            self.np_disc_bucket[i] += o.np_disc_bucket[i];
        }
    }
}

fn file_of(sq: u64) -> i32 {
    (sq % 8) as i32
}
fn rank_of(sq: u64) -> i32 {
    (sq / 8) as i32
}

/// Le rayon attaquant->roi (carres strictement interieurs) intersecte-t-il v ?
/// (c attaque k par construction : att contient le bit c.)
fn ray_between_hits(c: u64, k: u64, v: u64) -> bool {
    let (cr, cf, kr, kf) = (rank_of(c), file_of(c), rank_of(k), file_of(k));
    let (dr, df) = (kr - cr, kf - cf);
    if dr == 0 && df == 0 {
        return false;
    }
    let (step, steps) = if dr == 0 {
        (if df > 0 { 1i64 } else { -1 }, df.abs() - 1)
    } else if df == 0 {
        (if dr > 0 { 8i64 } else { -8 }, dr.abs() - 1)
    } else if dr.abs() == df.abs() {
        let sr = if dr > 0 { 8i64 } else { -8 };
        let sf = if df > 0 { 1i64 } else { -1 };
        (sr + sf, dr.abs() - 1)
    } else {
        return false; // non aligne (cavalier, etc.)
    };
    let mut sq = c as i64;
    for _ in 0..steps {
        sq += step;
        if v & (1u64 << sq) != 0 {
            return true;
        }
    }
    false
}

fn piece_char(pos: &Position, sq: u64) -> char {
    let m = pos.piece_on(sq as u8);
    if m == 0 {
        return '.';
    }
    let code = (m - 1) as usize;
    let kinds = ["p", "n", "b", "r", "q", "k"];
    let mut ch = kinds[code % 6].chars().next().unwrap();
    if code < 6 {
        ch = ch.to_ascii_uppercase();
    }
    ch
}

fn fen_of(pos: &Position) -> String {
    let mut rows = Vec::new();
    for r in (0..8).rev() {
        let mut row = String::new();
        let mut empty = 0u8;
        for f in 0..8 {
            let ch = piece_char(pos, (r * 8 + f) as u64);
            if ch == '.' {
                empty += 1;
            } else {
                if empty > 0 {
                    row.push_str(&empty.to_string());
                    empty = 0;
                }
                row.push(ch);
            }
        }
        if empty > 0 {
            row.push_str(&empty.to_string());
        }
        rows.push(row);
    }
    rows.join("/")
}

fn sqn(sq: u64) -> String {
    format!(
        "{}{}",
        (b'a' + (sq % 8) as u8) as char,
        (b'1' + (sq / 8) as u8) as char
    )
}

fn sq_names(att: u64) -> String {
    (0..64)
        .filter(|&i| att & (1u64 << i) != 0)
        .map(|i| sqn(i))
        .collect::<Vec<_>>()
        .join(",")
}

fn dump_ep(
    pos: &Position,
    att: u64,
    n: usize,
    us: Color,
    mv: Move,
    origin_c: u64,
    capt_c: u64,
    singles: bool,
) {
    // dumps limites : 12 pour les singles thru-capt, 8 pour les doubles.
    static SEEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let cap = if singles { 12 } else { 8 };
    if SEEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= cap {
        return;
    }
    let ksq = pos.king_sq(us.opp()) as u64;
    eprintln!(
        "DIAG (n={n}) att=[{}] move {}x{} ep(origin={},capt={}) king={}, thru_origin(first)={}, thru_capt(first)={}, fen=[{}]",
        sq_names(att),
        sqn(mv.from() as u64),
        sqn(mv.to() as u64),
        sqn(origin_c),
        sqn(capt_c),
        sqn(ksq),
        ray_between_hits(first_bit(att), ksq, 1u64 << origin_c),
        ray_between_hits(first_bit(att), ksq, 1u64 << capt_c),
        fen_of(pos)
    );
}

#[inline]
fn first_bit(b: u64) -> u64 {
    b.trailing_zeros() as u64
}

fn dump_castle(
    pos: &Position,
    att: u64,
    n: usize,
    us: Color,
    rook_from: usize,
    rook_dst: usize,
    dbl: bool,
) {
    // budget : tous les n==1 sont rares → 24 ; doubles 12
    static SEEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let cap = if dbl { 12 } else { 24 };
    if SEEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= cap {
        return;
    }
    let ksq = pos.king_sq(us.opp()) as u64;
    eprintln!(
        "DIAG castle(dbl={} n={}) att=[{}] rook={}{}->{}{} king={} rook_checks={} fen=[{}]",
        dbl,
        n,
        sq_names(att),
        sqn(rook_from as u64),
        if pos.piece_on(rook_from as u8) == 0 { "!" } else { "" },
        sqn(rook_dst as u64),
        if rook_dst != 0 && att & (1u64 << rook_dst) != 0 { "" } else { "" },
        sqn(ksq),
        att & (1u64 << rook_dst) != 0,
        fen_of(pos)
    );
}

fn dump_promo(pos: &Position, att: u64, n: usize, us: Color, mv: Move, disc: bool) {
    static SEEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    if SEEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 12 {
        return;
    }
    let ksq = pos.king_sq(us.opp()) as u64;
    eprintln!(
        "DIAG promo(disc={} n={}) move {}x{}promo king={} att=[{}] fen=[{}]",
        disc,
        n,
        sqn(mv.from() as u64),
        sqn(mv.to() as u64),
        sqn(ksq),
        sq_names(att),
        fen_of(pos)
    );
}

// dumps des buckets rares : 2 slots par bucket (18 buckets * 2 bits)
static DUMP_BITS_A: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static DUMP_BITS_B: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[inline]
fn grab_dump_slot(bucket: usize) -> bool {
    let atomic = if bucket < 9 { &DUMP_BITS_A } else { &DUMP_BITS_B };
    let bit = 1u64 << ((bucket % 9) * 2);
    let prev = atomic.fetch_or(bit, std::sync::atomic::Ordering::Relaxed);
    prev & bit == 0
}

fn walk(pos: &mut Position, depth: u32, cats: &mut Cats, path: &mut Vec<(u8, u8, u8)>) {
    if depth == 0 {
        return;
    }
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    let us = pos.side;
    for mv in list.iter() {
        let is_ep = mv.is_ep();
        let is_castle = mv.is_castle();
        let is_promo = mv.is_promotion();
        let mkind = pos.piece_on(mv.from()) as usize; // mailbox (code+1), avant make
        let u = pos.make(mv);
        path.push((mv.from(), mv.to(), mv.flags()));
        if depth == 1 {
            let opp = us.opp();
            if pos.in_check(opp) {
                cats.checks += 1;
                let ks = pos.king_sq(opp) as u64;
                let att = attackers_to(pos, ks as u8, us);
                let n = att.count_ones() as usize;
                let mover_checks = att & (1u64 << mv.to()) != 0;
                if n == 1 {
                    if mover_checks {
                        cats.disc_direct += 1;
                        if is_ep {
                            cats.ep_direct += 1;
                        }
                    } else {
                        cats.disc_single += 1;
                        if is_ep {
                            cats.ep_s += 1;
                            let origin = 1u64 << mv.from();
                            let capt = 1u64
                                << (mv.to() as i64 - if us == Color::White { 8 } else { -8 });
                            let (t_o, t_c) = (
                                ray_between_hits(first_bit(att), ks, origin),
                                ray_between_hits(first_bit(att), ks, capt),
                            );
                            if t_c {
                                cats.ep_s_capt += 1;
                            }
                            if t_o {
                                cats.ep_s_origin += 1;
                            }
                            if t_o && t_c {
                                cats.ep_s_both += 1;
                            }
                            if !t_o && !t_c {
                                cats.ep_s_other += 1;
                            }
                            if t_c {
                                dump_ep(pos, att, n, us, mv, mv.from() as u64, capt.trailing_zeros() as u64, true);
                            }
                        } else {
                            // decouverte non-ep : bucket par piece/flag du coup
                            cats.np_disc += 1;
                            let kind = (mkind - 1) % 6;
                            let flag = if mv.is_capture() {
                                1
                            } else if mv.is_double_push() {
                                2
                            } else {
                                0
                            };
                            let bucket = kind * 3 + flag;
                            cats.np_disc_bucket[bucket] += 1;
                            if grab_dump_slot(bucket) {
                                let kp = ["p", "n", "b", "r", "q", "k"][kind];
                                let fl = ["q", "c", "d"][flag];
                                eprintln!(
                                    "DIAG npdisc({}{}) path=[{}] att=[{}] king={} fen=[{}]",
                                    kp,
                                    fl,
                                    path.iter()
                                        .map(|(f, t, fg)| format!(
                                            "{}{}->{}{}(flag{})",
                                            (b'a' + (f % 8)) as char,
                                            (b'1' + f / 8) as char,
                                            (b'a' + (t % 8)) as char,
                                            (b'1' + t / 8) as char,
                                            fg
                                        ))
                                        .collect::<Vec<_>>()
                                        .join(" "),
                                    sq_names(att),
                                    sqn(ks),
                                    fen_of(pos)
                                );
                            }
                        }
                    }
                } else {
                    match n {
                        2 => cats.dbl2 += 1,
                        3 => cats.dbl3 += 1,
                        _ => cats.dbln += 1,
                    }
                    cats.np_dbl += 1;
                    if is_ep {
                        cats.np_dbl -= 1;
                        cats.ep_d += 1;
                        let origin = 1u64 << mv.from();
                        let capt =
                            1u64 << (mv.to() as i64 - if us == Color::White { 8 } else { -8 });
                        // un des attaquants passe-t-il par le carre capture ?
                        let mut any_capt = false;
                        let mut all_explained = true;
                        for i in 0..64 {
                            let bit = 1u64 << i;
                            if att & bit != 0 {
                                let t_o = ray_between_hits(i, ks, origin);
                                let t_c = ray_between_hits(i, ks, capt);
                                if t_c {
                                    any_capt = true;
                                }
                                if !t_c && !t_o && bit != 1u64 << mv.to() {
                                    // attaquants autres que le pion deplace
                                    // non expliques par une ligne liberee
                                    all_explained = false;
                                }
                            }
                        }
                        if any_capt {
                            cats.ep_d_capt += 1;
                        } else if all_explained {
                            cats.ep_d_onlyorigin += 1;
                        } else {
                            cats.ep_d_other += 1;
                        }
                        dump_ep(
                            pos,
                            att,
                            n,
                            us,
                            mv,
                            mv.from() as u64,
                            capt.trailing_zeros() as u64,
                            false,
                        );
                    }
                }
                if is_castle {
                    let ci = us.idx();
                    let king_side = mv.to() > mv.from();
                    let (rook_from, rook_dst) = castle_rook_squares(ci, king_side);
                    if n == 1 && !mover_checks {
                        cats.cs_s += 1;
                        if att == (1u64 << rook_dst) {
                            // echec direct par la tour arrivee seule
                            cats.cs_s_rookdirect += 1;
                            dump_castle(pos, att, n, us, rook_from, rook_dst, false);
                        } else {
                            cats.cs_s_other += 1;
                            dump_castle(pos, att, n, us, rook_from, rook_dst, false);
                        }
                    } else {
                        let king_checks = mover_checks;
                        let rook_checks = att & (1u64 << rook_dst) != 0;
                        if n >= 2 {
                            cats.cs_d += 1;
                            if rook_checks {
                                cats.cs_rook_direct += 1;
                            }
                            dump_castle(pos, att, n, us, rook_from, rook_dst, true);
                        }
                        let _ = king_checks;
                    }
                }
                if is_promo {
                    if n >= 2 {
                        cats.pr_dbl += 1;
                        dump_promo(pos, att, n, us, mv, false);
                    } else if n == 1 {
                        if mover_checks {
                            cats.pr_direct += 1;
                        } else {
                            cats.pr_disc += 1;
                            dump_promo(pos, att, n, us, mv, true);
                        }
                    } else {
                        cats.pr_none += 1;
                        dump_promo(pos, att, n, us, mv, false);
                    }
                }
            }
        } else {
            walk(pos, depth - 1, cats, path);
        }
        path.pop();
        pos.unmake(mv, u);
    }
}

fn enumerate(pos: &Position, depth_left: u32, split: u32, out: &mut Vec<(Position, u32)>) {
    assert!(depth_left >= split + 1, "split must leave leaf depth >= 1");
    if split == 0 {
        out.push((*pos, depth_left));
        return;
    }
    let mut list = MoveList::new();
    generate(pos, &mut list, GenKind::All);
    for i in 0..list.len() {
        let mv: Move = list.get(i);
        let mut p = *pos;
        p.make(mv);
        enumerate(&p, depth_left - 1, split - 1, out);
    }
}

fn main() {
    // auto-test du rayon (avant tout compte)
    {
        let mut ok = true;
        // diagonale a5 -> e1 traverse b4 ; pas c4 ; ni h5->d1 (autre diagonale)
        ok &= ray_between_hits(32, 4, 1 << 25);
        ok &= !ray_between_hits(32, 4, 1 << 26);
        ok &= !ray_between_hits(39, 3, 1 << 60); // h5->d1, cible hors diagonale
        ok &= ray_between_hits(39, 3, 1 << 21); // h5->d1 passe par f3
        // ligne : a1 -> h1 ; colonne : a1 -> a8
        ok &= ray_between_hits(0, 7, 1 << 4);
        ok &= ray_between_hits(0, 56, 1 << 24);
        ok &= !ray_between_hits(0, 56, 1 << 25); // b5 pas sur la colonne a
        // cavalier : b1 -> d2 (1 -> 11) jamais « a travers »
        ok &= !ray_between_hits(1, 11, 1 << 17);
        // adjacent : echec de contact, rien entre
        ok &= !ray_between_hits(3, 4, 1 << 11);
        assert!(ok, "ray_between_hits selftest FAILED");
        eprintln!("# selftest ray: OK");
    }
    let args: Vec<String> = std::env::args().collect();
    let fen = args.get(1).cloned().expect("fen");
    let depth: u32 = args.get(2).map(|s| s.parse().unwrap()).expect("depth");
    let threads: usize = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(8);
    let split: u32 = args.get(4).map(|s| s.parse().unwrap()).unwrap_or(2);

    eprintln!("[{}]", mychess::slider::describe());
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads.max(1)).build().unwrap();

    let pos = Position::from_fen(&fen).unwrap();
    let mut tasks: Vec<(Position, u32)> = Vec::new();
    enumerate(&pos, depth, split, &mut tasks);
    let total = tasks.len();
    eprintln!("# diag: {total} tasks (depth {depth}, split {split})");

    let cats = std::sync::Mutex::new(Cats::default());
    let t0 = std::time::Instant::now();
    pool.install(|| {
        tasks.into_par_iter().for_each(|(mut sub, dleft)| {
            let mut c = Cats::default();
            let mut path: Vec<(u8, u8, u8)> = Vec::with_capacity(16);
            walk(&mut sub, dleft, &mut c, &mut path);
            cats.lock().unwrap().add(&c);
        });
    });

    let c = cats.into_inner().unwrap();
    for (k, v) in [
        ("checks", c.checks),
        ("disc_single", c.disc_single),
        ("disc_direct", c.disc_direct),
        ("dbl2", c.dbl2),
        ("dbl3", c.dbl3),
        ("dbln", c.dbln),
        ("ep_s", c.ep_s),
        ("ep_s_origin", c.ep_s_origin),
        ("ep_s_capt", c.ep_s_capt),
        ("ep_s_both", c.ep_s_both),
        ("ep_s_other", c.ep_s_other),
        ("ep_direct", c.ep_direct),
        ("ep_d", c.ep_d),
        ("ep_d_capt", c.ep_d_capt),
        ("ep_d_onlyorigin", c.ep_d_onlyorigin),
        ("ep_d_other", c.ep_d_other),
        ("np_disc", c.np_disc),
        ("np_dbl", c.np_dbl),
        ("cs_s", c.cs_s),
        ("cs_s_rookdirect", c.cs_s_rookdirect),
        ("cs_s_other", c.cs_s_other),
        ("cs_d", c.cs_d),
        ("cs_rook_direct", c.cs_rook_direct),
        ("pr_dbl", c.pr_dbl),
        ("pr_direct", c.pr_direct),
        ("pr_disc", c.pr_disc),
        ("pr_none", c.pr_none),
        ("our_double", c.dbl2 + c.dbl3 + c.dbln),
    ] {
        println!("{k:<16} {v:>14}");
    }
    for i in 0..18 {
        let kinds = ["p", "n", "b", "r", "q", "k"];
        let flags = ["quiet", "cap", "dpush"];
        println!(
            "npdisc_{}_{} {:>10}",
            kinds[i / 3],
            flags[i % 3],
            c.np_disc_bucket[i]
        );
    }
    eprintln!("# diag: done in {:.1}s", t0.elapsed().as_secs_f64());
}