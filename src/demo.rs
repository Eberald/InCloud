//! Synthetic, CLOUDY-formatted output used to preview the plots without running CLOUDY.
//! The numbers are illustrative only (simple analytic profiles), not a CLOUDY calculation.

use std::fmt::Write as _;
use std::path::Path;

use crate::model::{Geometry, Params, Sed};

fn sig(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

pub fn write_demo(p: &Params, dir: &Path) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let prefix = p.file_prefix();
    let n = 10f64.powf(p.log_hden);
    let u = 10f64.powf(p.log_u);
    let lu = p.log_u + 2.0; // relative to log U = -2
    let grains = p.has_grains();
    let av_per_nh = if grains { 1.0 / 1.87e21 } else { 1.0 / 1.87e23 };
    let alpha_b = 2.6e-13;
    let d_if = (u * 3e10 / (n * alpha_b)) * if grains { 0.8 } else { 1.0 };
    let av_if = n * d_if * av_per_nh;
    let av_df = (av_if + (1.8 + 0.5 * lu - 0.25 * (p.log_hden - 2.0)).clamp(0.4, 6.0)).max(av_if + 0.3);
    let av_co = av_df + 1.5;
    let depth_of_av = |av: f64| if av <= av_if { d_if * av / av_if.max(1e-30) } else { d_if + (av - av_if) / (n * av_per_nh) };

    // where to stop
    let st = &p.stop;
    let mut d_end = f64::MAX;
    if st.use_av {
        d_end = d_end.min(depth_of_av(st.av));
    }
    if st.use_thickness {
        d_end = d_end.min(10f64.powf(st.log_thickness));
    }
    if st.use_column {
        d_end = d_end.min(10f64.powf(st.log_column) / n);
    }
    if st.use_temp && st.temp > 2000.0 || st.use_efrac {
        d_end = d_end.min(d_if * 1.05);
    }
    if d_end == f64::MAX {
        d_end = depth_of_av(10.0);
    }
    let d_df = depth_of_av(av_df);
    let d_co = depth_of_av(av_co);

    // depth grid: logarithmic + refined around the fronts
    let d0 = d_if * 1e-4;
    let mut grid: Vec<f64> = (0..260).map(|i| d0 * (d_end / d0).powf(i as f64 / 259.0)).collect();
    for (c, w) in [(d_if, 0.25 * d_if), (d_df, 0.3 * (d_df - d_if).max(d_if * 0.1)), (d_co, 0.3 * (d_co - d_if).max(d_if * 0.1))] {
        for i in 0..80 {
            let v = c - w + 2.0 * w * i as f64 / 79.0;
            if v > d0 && v < d_end {
                grid.push(v);
            }
        }
    }
    grid.sort_by(|a, b| a.partial_cmp(b).unwrap());
    grid.dedup_by(|a, b| (*a - *b).abs() < *b * 1e-6);

    let t_star = if p.sed == Sed::Blackbody { p.bb_temp } else { 6e4 };
    let t_hii = 7400.0 * (1.0 + 0.06 * lu) * (1.0 + 0.12 * ((t_star - 4e4) / 4e4)) * (1.0 - 0.25 * p.log_metals);
    let r0 = if p.geometry == Geometry::Sphere { 10f64.powf(p.log_radius) } else { 0.0 };

    struct Z {
        d: f64,
        av: f64,
        hp: f64,
        h0: f64,
        h2: f64,
        co: f64,
        te: f64,
        ne: f64,
        g_o: f64,
        g_n: f64,
        g_s: f64,
        g_he: f64,
        g_c: f64,
    }
    let mut zs = vec![];
    for &d in &grid {
        let av = if d <= d_if { n * d * av_per_nh } else { av_if + (d - d_if) * n * av_per_nh };
        let w_if = 0.025 * d_if;
        let hp = (sig(-(d - d_if) / w_if)).max(2e-5 * (1.0 - sig((av - av_co) / 0.3)) + 1e-7);
        let h2 = (1.0 - hp) * sig((av - av_df) / 0.18);
        let h0 = (1.0 - hp - h2).max(1e-6);
        let co = sig((av - av_co) / 0.22);
        let t_pdr = 18.0 + 380.0 * (-(av - av_if).max(0.0) / 0.55).exp() + 40.0 * (1.0 - sig((av - av_df) / 0.3));
        let bump = 1.0 + 0.12 * (-((d - 0.9 * d_if) / (0.08 * d_if)).powi(2)).exp();
        let te = hp * t_hii * bump * (1.0 + 0.03 * (d / d_if)) + (1.0 - hp) * t_pdr;
        let ne = n * (1.1 * hp + 1.4e-4 * (1.0 - co) + 1e-8);
        let g = |f: f64, wf: f64| sig(-(d - f * d_if) / (wf * d_if));
        zs.push(Z {
            d,
            av,
            hp,
            h0,
            h2,
            co,
            te,
            ne,
            g_o: g(0.72 + 0.05 * lu, 0.06),
            g_n: g(0.62 + 0.05 * lu, 0.06),
            g_s: g(0.80 + 0.03 * lu, 0.05),
            g_he: g(0.86 + 0.04 * lu.min(1.0) - 0.15 * (4e4 - t_star).max(0.0) / 1e4, 0.02),
            g_c: g(0.84, 0.05),
        });
    }

    let lg = |v: f64| v.max(1e-37).log10();
    // ---- overview
    let mut s = String::from("#depth\tTe\tHtot\thden\teden\t2H_2/H\tHI\tHII\tHeI\tHeII\tHeIII\tCO/C\tC1\tC2\tC3\tC4\tO1\tO2\tO3\tO4\tO5\tO6\tH2O/O\tAV(point)\tAV(extend)\n");
    for z in &zs {
        let he2 = z.hp * z.g_he;
        let c2 = z.hp * (1.0 - z.g_c) + (1.0 - z.hp) * (1.0 - z.co) * (1.0 - sig((z.av - av_co + 0.4) / 0.2) * 0.3);
        let c3 = z.hp * z.g_c;
        let o3 = z.hp * z.g_o;
        let o2 = z.hp * (1.0 - z.g_o);
        let htot = n * n * (z.hp * 2.4e-24 + (1.0 - z.hp) * 4e-26 * (-1.8 * (z.av - av_if).max(0.0)).exp() + 2e-28);
        let _ = writeln!(
            s,
            "{:.4e}\t{:.4e}\t{:.4e}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{:.4}",
            z.d, z.te, htot, lg(n), lg(z.ne), lg(z.h2), lg(z.h0), lg(z.hp), lg(1.0 - he2), lg(he2), lg(1e-5 * z.hp),
            lg(z.co), lg((1.0 - c2 - c3 - z.co).max(1e-8)), lg(c2), lg(c3), lg(1e-4 * z.hp),
            lg(1.0 - z.hp), lg(o2), lg(o3), lg(1e-4 * z.hp), -8.0, -9.0, lg(1e-4 * z.co), z.av, z.av
        );
    }
    std::fs::write(dir.join(format!("{prefix}.ovr")), &s).map_err(|e| e.to_string())?;

    // ---- elements (linear fractions)
    let elem = |head: &str, f: &dyn Fn(&Z) -> Vec<f64>| -> String {
        let mut s = format!("#depth\t{head}\n");
        for z in &zs {
            let v = f(z);
            let tot: f64 = v.iter().sum::<f64>().max(1e-30);
            let _ = write!(s, "{:.4e}", z.d);
            for x in v {
                let _ = write!(s, "\t{:.4e}", (x / tot).max(1e-12));
            }
            s.push('\n');
        }
        s
    };
    let files: Vec<(&str, String)> = vec![
        ("H", elem("H\tH+\tH2", &|z| vec![z.h0, z.hp, z.h2])),
        ("He", elem("He\tHe+\tHe+2", &|z| vec![1.0 - z.hp * z.g_he, z.hp * z.g_he, 1e-5 * z.hp])),
        ("C", elem("C\tC+\tC+2\tC+3\tCO", &|z| {
            let c2 = z.hp * z.g_c;
            let c3 = 0.02 * c2;
            let cp = z.hp * (1.0 - z.g_c) + (1.0 - z.hp) * (1.0 - z.co) * (1.0 - 0.6 * sig((z.av - av_df) / 0.4));
            let c0 = ((1.0 - z.hp) * (1.0 - z.co) - (1.0 - z.hp) * (1.0 - z.co) * (1.0 - 0.6 * sig((z.av - av_df) / 0.4))).max(1e-6);
            vec![c0, cp, c2, c3, z.co * (1.0 - z.hp)]
        })),
        ("N", elem("N\tN+\tN+2\tN+3", &|z| vec![1.0 - z.hp, z.hp * (1.0 - z.g_n), z.hp * z.g_n * 0.97, z.hp * z.g_n * 0.03])),
        ("O", elem("O\tO+\tO+2\tO+3", &|z| vec![1.0 - z.hp, z.hp * (1.0 - z.g_o), z.hp * z.g_o * 0.99, z.hp * z.g_o * 0.01])),
        ("S", elem("S\tS+\tS+2\tS+3", &|z| {
            let sp_pdr = (1.0 - z.hp) * (1.0 - sig((z.av - av_if - 1.2) / 0.25));
            vec![(1.0 - z.hp) - sp_pdr + 1e-6, z.hp * (1.0 - z.g_s) + sp_pdr, z.hp * z.g_s * 0.9, z.hp * z.g_s * 0.1]
        })),
    ];
    for (sym, txt) in files {
        std::fs::write(dir.join(format!("{prefix}.ele_{sym}")), txt).map_err(|e| e.to_string())?;
    }

    // ---- grain temperature
    if grains {
        let mut s = String::from("#depth");
        for m in ["gra-ism", "sil-ism"] {
            for i in 1..=5 {
                let _ = write!(s, "\t{m}{i:02}");
            }
        }
        if p.grains == crate::model::Grains::IsmPah {
            for i in 1..=3 {
                let _ = write!(s, "\tpah-c{i:02}");
            }
        }
        s.push('\n');
        for z in &zs {
            let att = (-1.8 * (z.av - av_if).max(0.0) / 6.0).exp();
            let _ = write!(s, "{:.4e}", z.d);
            for (base, _) in [(78.0, 0), (58.0, 1)] {
                for i in 1..=5 {
                    let size_f = 1.0 + 0.09 * (5 - i) as f64;
                    let t = (base * (1.0 + 0.12 * lu) * size_f * att).max(9.0 + i as f64);
                    let _ = write!(s, "\t{:.3e}", t);
                }
            }
            if p.grains == crate::model::Grains::IsmPah {
                for i in 1..=3 {
                    let t = (110.0 * (1.0 + 0.1 * (3 - i) as f64) * att).max(14.0);
                    let _ = write!(s, "\t{:.3e}", t);
                }
            }
            s.push('\n');
        }
        std::fs::write(dir.join(format!("{prefix}.gtemp")), s).map_err(|e| e.to_string())?;
    }

    // ---- heating / cooling
    let mut het = String::from("#depth cm\tTemp K\tHtot erg/cm3/s\tCtot erg/cm3/s\theat fracs\n");
    let mut col = String::from("#depth cm\tTemp K\tHtot erg/cm3/s\tCtot erg/cm3/s\tcool fracs\n");
    for z in &zs {
        let htot = n * n * (z.hp * 2.4e-24 + (1.0 - z.hp) * 4e-26 * (-1.8 * (z.av - av_if).max(0.0)).exp() + 2e-28 / (1.0 + z.av));
        let hii = z.hp;
        let pdr = (1.0 - z.hp) * (1.0 - z.h2);
        let mol = z.h2;
        let heat = [
            ("H  1", 0.86 * hii),
            ("He 1", 0.10 * hii),
            ("GrnP", 0.04 * hii + 0.85 * pdr + 0.35 * mol),
            ("H2  ", 0.10 * pdr + 0.1 * mol),
            ("CR H", 0.05 * pdr + 0.55 * mol),
        ];
        let cool = [
            ("O  3", 0.32 * hii * (0.5 + 0.5 * z.g_o)),
            ("O  2", 0.22 * hii * (1.0 - 0.5 * z.g_o)),
            ("N  2", 0.12 * hii),
            ("S  2", 0.05 * hii),
            ("H FF", 0.12 * hii),
            ("Ne 2", 0.08 * hii),
            ("S  3", 0.09 * hii),
            ("C  2", 0.55 * pdr + 0.05 * mol),
            ("O  1", 0.40 * pdr),
            ("Si 2", 0.05 * pdr),
            ("CO  ", 0.6 * mol),
            ("GrGH", 0.35 * mol),
        ];
        let line = |head: &mut String, items: &[(&str, f64)]| {
            let tot: f64 = items.iter().map(|x| x.1).sum::<f64>().max(1e-30);
            let _ = write!(head, "{:.4e}\t{:.4e}\t{:.4e}\t{:.4e}", z.d, z.te, htot, htot);
            for (l, v) in items {
                if v / tot > 0.005 {
                    let _ = write!(head, "\t{l}\t{:.3}", v / tot);
                }
            }
            head.push('\n');
        };
        line(&mut het, &heat);
        line(&mut col, &cool);
    }
    std::fs::write(dir.join(format!("{prefix}.het")), het).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{prefix}.col")), col).map_err(|e| e.to_string())?;

    // ---- chemistry: formation / destruction rates (illustrative reaction sets)
    for sp in p.chemistry_species() {
        type Prof = Box<dyn Fn(&Z) -> f64>;
        let g = move |z: &Z| (-1.8 * (z.av - av_if).max(0.0)).exp() * (1.0 - z.hp) + 1e-6;
        let (form, dest): (Vec<(&str, Prof)>, Vec<(&str, Prof)>) = match sp.as_str() {
            "H2" => (
                vec![
                    ("H,H=>H2", Box::new(move |z: &Z| 3e-17 * n * n * z.h0 * z.h0.max(0.05))),
                    ("H-,H=>H2,e-", Box::new(move |z: &Z| 1e-19 * n * n * z.h0 * z.ne / n)),
                    ("H2+,H=>H+,H2", Box::new(move |z: &Z| 1e-21 * n * n * z.hp * z.h0)),
                ],
                vec![
                    ("PHOTON,H2=>H,H", Box::new(move |z: &Z| 5e-11 * n * z.h2 * g(z) * 1e2)),
                    ("CRPHOT,H2=>e-,H2+", Box::new(move |z: &Z| 2e-16 * n * z.h2)),
                    ("H+,H2=>H,H2+", Box::new(move |z: &Z| 1e-10 * n * n * z.hp * z.h2 * 1e-3)),
                ],
            ),
            "CO" => (
                vec![
                    ("C+,OH=>H+,CO", Box::new(move |z: &Z| 1e-13 * n * n * (1.0 - z.co) * z.h2 * 1e-4)),
                    ("HCO+,e-=>H,CO", Box::new(move |z: &Z| 1e-14 * n * z.ne * z.co * 1e-2)),
                    ("C,O=>PHOTON,CO", Box::new(move |z: &Z| 1e-20 * n * n * (1.0 - z.hp))),
                ],
                vec![
                    ("PHOTON,CO=>C,O", Box::new(move |z: &Z| 2e-10 * n * z.co * 1e-4 * g(z) * 1e3)),
                    ("He+,CO=>He,C+,O", Box::new(move |z: &Z| 1e-15 * n * n * z.co * 1e-4)),
                ],
            ),
            "C+" => (
                vec![
                    ("PHOTON,C=>e-,C+", Box::new(move |z: &Z| 3e-10 * n * 1e-4 * (1.0 - z.co) * (g(z) + z.hp))),
                    ("He+,CO=>He,C+,O", Box::new(move |z: &Z| 1e-15 * n * n * z.co * 1e-4)),
                ],
                vec![
                    ("e-,C+=>PHOTON,C", Box::new(move |z: &Z| 4e-12 * z.ne * n * 1e-4 * (1.0 - z.co))),
                    ("H2,C+=>PHOTON,CH2+", Box::new(move |z: &Z| 1e-16 * n * n * z.h2 * 1e-4)),
                ],
            ),
            "OH" => (
                vec![
                    ("O,H2=>H,OH", Box::new(move |z: &Z| 1e-18 * n * n * z.h2 * (z.te / 300.0).min(3.0))),
                    ("H3O+,e-=>H2,OH", Box::new(move |z: &Z| 1e-20 * n * z.ne * z.h2)),
                ],
                vec![
                    ("PHOTON,OH=>H,O", Box::new(move |z: &Z| 4e-10 * n * 1e-7 * g(z) * z.h2)),
                    ("C+,OH=>H+,CO", Box::new(move |z: &Z| 1e-13 * n * n * (1.0 - z.co) * z.h2 * 1e-4)),
                ],
            ),
            other => {
                let base = other.trim_end_matches(|c: char| c == '+' || c.is_ascii_digit()).to_string();
                let lower = if other.ends_with('+') && !other.contains("+2") { base.clone() } else { format!("{base}+") };
                let f1: &'static str = Box::leak(format!("PHOTON,{lower}=>e-,{other}").into_boxed_str());
                let d1: &'static str = Box::leak(format!("e-,{other}=>PHOTON,{lower}").into_boxed_str());
                let d2: &'static str = Box::leak(format!("H,{other}=>H+,{lower}").into_boxed_str());
                (
                    vec![(f1, Box::new(move |z: &Z| 1e-12 * n * z.hp + 1e-20))],
                    vec![(d1, Box::new(move |z: &Z| 3e-13 * z.ne * n * z.hp + 1e-20)), (d2, Box::new(move |z: &Z| 1e-14 * n * n * z.h0 * z.hp))],
                )
            }
        };
        let f = crate::model::species_file(&sp);
        for (set, ext) in [(&form, "chf"), (&dest, "chd")] {
            let mut s = String::from("#Depth");
            for (name, _) in set.iter() {
                let _ = write!(s, "\t{name}");
            }
            s.push('\n');
            for z in &zs {
                let _ = write!(s, "{:.5e}", z.d);
                for (_, fun) in set.iter() {
                    let _ = write!(s, "\t{:.3e}", fun(z).max(0.0));
                }
                s.push('\n');
            }
            std::fs::write(dir.join(format!("{prefix}.{ext}_{f}")), s).map_err(|e| e.to_string())?;
        }
    }

    // ---- emissivities (log10 of 4 pi j), scaled so that the integrated ratios are plausible
    let labels = p.line_labels();
    if !labels.is_empty() {
        let shape = |lab: &str, z: &Z| -> f64 {
            let l = lab.replace(' ', "").replace("Blnd5875", "He15875").replace("Blnd6300", "O16300");
            let np = n * z.hp;
            let t4 = (z.te / 1e4).max(1e-3);
            let coll = |e: f64| z.ne * (-e / z.te).exp() / z.te.sqrt();
            let pdr_n = n * (1.0 - z.hp);
            if l.starts_with("H1") {
                z.ne * np * t4.powf(-0.9)
            } else if l.starts_with("He1") {
                z.ne * np * z.g_he * t4.powf(-0.8)
            } else if l.starts_with("O3") {
                coll(if l.contains("4363") { 62000.0 } else if l.ends_with('m') { 150.0 } else { 29000.0 }) * np * z.g_o
            } else if l.starts_with("Ne3") {
                coll(if l.ends_with('m') { 900.0 } else { 37000.0 }) * np * z.g_o
            } else if l.starts_with("Ar3") || l.starts_with("S3") {
                coll(if l.ends_with('m') { 700.0 } else { 20000.0 }) * np * z.g_s
            } else if l.starts_with("N2") {
                coll(if l.ends_with('m') { 70.0 } else { 22000.0 }) * np * (1.0 - z.g_n)
            } else if l.starts_with("O2") {
                coll(39000.0) * np * (1.0 - z.g_o)
            } else if l.starts_with("S2") {
                coll(21000.0) * (np * (1.0 - z.g_s) + 0.05 * pdr_n * (1.0 - sig((z.av - av_if - 1.2) / 0.25)))
            } else if l.starts_with("Ne2") {
                coll(1100.0) * np * (1.0 - z.g_o)
            } else if l.starts_with("O1") {
                if l.ends_with('m') {
                    pdr_n * pdr_n * (1.0 - z.h2 * 0.5) * (-(if l.contains("145") { 330.0 } else { 228.0 }) / z.te).exp()
                } else {
                    pdr_n * n * z.ne / n * (-22800.0 / z.te).exp() * 50.0 * (-(z.d - d_if).abs() / (0.05 * d_if)).exp()
                }
            } else if l.starts_with("C2") {
                n * n * (1.0 - z.co) * ((1.0 - z.hp) + 0.2 * z.hp * (1.0 - z.g_c)) * (-91.0 / z.te).exp()
            } else if l.starts_with("Si2") {
                n * n * (0.3 * z.hp + (1.0 - z.hp) * (1.0 - z.co)) * (-413.0 / z.te).exp()
            } else if l.starts_with("H2") {
                n * n * z.h2 * (-((z.av - av_df) / 0.35).powi(2)).exp()
            } else if l.starts_with("CO") {
                n * n * z.co * (1.0 - z.hp)
            } else {
                z.ne * np
            }
        };
        // target ratios relative to H-beta (illustrative)
        let target = |lab: &str| -> f64 {
            let l = lab.replace(' ', "").replace("Blnd5875", "He15875").replace("Blnd6300", "O16300");
            let m = 10f64.powf(p.log_metals);
            let sx = |k: f64| 10f64.powf(k * lu);
            if l.starts_with("H16562") || l.starts_with("H16563") {
                2.86
            } else if l.starts_with("H14861") {
                1.0
            } else if l.starts_with("He1") {
                0.11
            } else if l.starts_with("O35006") || l.starts_with("O35007") {
                (3.0 * sx(0.8) * m).min(8.0)
            } else if l.starts_with("O34958") || l.starts_with("O34959") {
                (3.0 * sx(0.8) * m).min(8.0) / 2.98
            } else if l.starts_with("O34363") {
                0.012 * sx(0.9)
            } else if l.starts_with("O388") {
                0.9 * sx(0.6)
            } else if l.starts_with("N26583") || l.starts_with("N26584") {
                0.75 * sx(-0.4) * m
            } else if l.starts_with("N26548") {
                0.25 * sx(-0.4) * m
            } else if l.starts_with("N2205") {
                0.02 * m
            } else if l.starts_with("O23726") {
                1.1 * sx(-0.5) * m
            } else if l.starts_with("O23728") || l.starts_with("O23729") {
                1.4 * sx(-0.5) * m
            } else if l.starts_with("S26716") {
                0.32 * sx(-0.5) * m
            } else if l.starts_with("S26730") || l.starts_with("S26731") {
                0.24 * sx(-0.5) * m
            } else if l.starts_with("O16300") {
                0.04 * sx(-0.4) * m
            } else if l.starts_with("Ne33868") || l.starts_with("Ne33869") {
                0.22 * sx(0.6) * m
            } else if l.starts_with("Ar3") {
                0.11 * m
            } else if l.starts_with("Ne212") {
                0.35 * sx(-0.4) * m
            } else if l.starts_with("Ne315") {
                0.45 * sx(0.5) * m
            } else if l.starts_with("S318") {
                0.4 * m
            } else if l.starts_with("Si234") {
                0.35 * m
            } else if l.starts_with("O163") {
                0.7 * m
            } else if l.starts_with("O1145") {
                0.05 * m
            } else if l.starts_with("C2157") || l.starts_with("C2158") {
                0.45 * m
            } else if l.starts_with("H22") {
                0.008
            } else if l.starts_with("CO") {
                2e-5 * m
            } else {
                0.05
            }
        };
        let weight = |d: f64| if r0 > 0.0 { ((r0 + d) / r0).powi(2) } else { 1.0 };
        let trapz = |y: &[f64]| -> f64 {
            let mut s = 0.0;
            for i in 1..zs.len() {
                s += 0.5 * (y[i] + y[i - 1]) * (zs[i].d - zs[i - 1].d);
            }
            s
        };
        let hb_shape: Vec<f64> = zs.iter().map(|z| shape("H  1 4861", z) * weight(z.d)).collect();
        let hb_int = trapz(&hb_shape).max(1e-300);
        // physical normalisation of H-beta: 4 pi j = 1.24e-25 ne np t4^-0.9
        let hb_scale = 1.24e-25;
        let mut s = String::from("#depth");
        for l in &labels {
            let _ = write!(s, "\t{l}");
        }
        s.push('\n');
        let mut cols: Vec<Vec<f64>> = vec![];
        for l in &labels {
            let raw: Vec<f64> = zs.iter().map(|z| shape(l, z).max(0.0)).collect();
            let w: Vec<f64> = raw.iter().zip(&zs).map(|(v, z)| v * weight(z.d)).collect();
            let int = trapz(&w).max(1e-300);
            let k = target(l) * hb_int / int * hb_scale;
            cols.push(raw.iter().map(|v| v * k).collect());
        }
        for (i, z) in zs.iter().enumerate() {
            let _ = write!(s, "{:.4e}", z.d);
            for c in &cols {
                let _ = write!(s, "\t{:.4}", lg(c[i].max(1e-37)));
            }
            s.push('\n');
        }
        std::fs::write(dir.join(format!("{prefix}.ems")), s).map_err(|e| e.to_string())?;
    }

    // ---- continuum (microns, nu F_nu)
    let fbol = u * n * 3e10 * 3.2e-11 * 1.6;
    let mut s = String::from("#Cont  nu\tincident\ttrans\tDiffOut\tnet trans\treflc\ttotal\treflin\toutlin\tlineID\tcont\tnLine\n");
    let av_tot = zs.last().map(|z| z.av).unwrap_or(0.0);
    let n_h0: f64 = zs.windows(2).map(|w| 0.5 * (w[0].h0 + w[1].h0) * n * (w[1].d - w[0].d)).sum();
    let l_abs = fbol * 0.75;
    let k = 1.380649e-16;
    let h = 6.62607e-27;
    for i in 0..900 {
        let lam = 10f64.powf(-3.5 + 7.5 * i as f64 / 899.0); // micron
        let nu = 2.998e14 / lam;
        let inc = match p.sed {
            Sed::Blackbody | Sed::Custom => {
                let x = h * nu / (k * t_star);
                if x > 700.0 { 0.0 } else { fbol * 15.0 / std::f64::consts::PI.powi(4) * x.powi(4) / x.exp_m1() }
            }
            Sed::Agn => {
                let x = h * nu / (k * 1.5e5);
                let bump = if x > 700.0 { 0.0 } else { x.powf(1.5) * (-x).exp() };
                let xr = 0.12 * (lam / 1e-3).powf(0.1) * (-(lam / 3.0)).exp();
                fbol * (bump * 1.5 + xr * (-(1e-4 / lam)).exp())
            }
            Sed::IsmField => {
                let x = h * nu / (k * 1.2e4);
                if x > 700.0 { 0.0 } else { fbol * 0.6 * x.powi(4) / x.exp_m1() * (-(0.0912 / lam).powi(8)).exp() }
            }
        };
        let tau_dust = if grains { av_tot / 1.086 * (0.55 / lam.max(0.05)).powf(1.4) } else { 0.0 };
        let tau_h = if lam < 0.0912 { n_h0 * 6.3e-18 * (lam / 0.0912).powi(3) + 1e-3 } else { 0.0 };
        let trans = inc * (-(tau_dust + tau_h).min(700.0)).exp();
        // nebular continuum (Balmer jump + free-free) and dust emission
        let neb = fbol * 2e-3 * (lam / 0.36).powf(-0.8) * if lam < 0.3646 { 0.4 } else { 1.0 } * (-(0.0912 / lam).powi(4)).exp();
        let mut dust = 0.0;
        if grains {
            for (td, f) in [(70.0 * (1.0 + 0.1 * lu), 0.25), (28.0, 0.75)] {
                let x = h * nu / (k * td);
                // modified blackbody beta = 2, normalised to unit integral in ln nu
                let shape = if x > 700.0 { 0.0 } else { x.powi(6) / x.exp_m1() / 726.0 };
                dust += l_abs * 0.6 * f * shape;
            }
            if p.grains == crate::model::Grains::IsmPah {
                for (c, wdt, a) in [(3.3, 0.04, 0.02), (6.2, 0.1, 0.08), (7.7, 0.25, 0.15), (8.6, 0.15, 0.05), (11.3, 0.2, 0.08), (12.7, 0.2, 0.04)] {
                    dust += l_abs * a * (-((lam - c) / wdt).powi(2)).exp();
                }
            }
        }
        let diff = neb + dust;
        let _ = writeln!(
            s,
            "{:.5e}\t{:.4e}\t{:.4e}\t{:.4e}\t{:.4e}\t{:.4e}\t{:.4e}\t{:.4e}\t{:.4e}\t \t1.0\t1.0",
            lam, inc, trans, diff, trans + diff, 0.0, trans + diff, 0.0, 0.0
        );
    }
    std::fs::write(dir.join(format!("{prefix}.con")), s).map_err(|e| e.to_string())?;

    std::fs::write(dir.join(format!("{prefix}.in")), p.input_file()).map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join(format!("{prefix}.out")),
        "INCLOUD-DEMO: synthetic illustrative output, not a CLOUDY calculation.\n Cloudy exited OK\n",
    )
    .map_err(|e| e.to_string())?;
    Ok(prefix)
}
