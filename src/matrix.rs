//! Developer check: input files for every choice of the panel, to run through CLOUDY and compare
//! with what the checks before a run say.

use std::path::Path;

use crate::model::{Abundances, DensityLaw, Geometry, Grains, Params, Sed, SED_EXAMPLE};

fn base(quick: bool) -> Params {
    let mut p = Params::default();
    p.custom_sed.text = SED_EXAMPLE.to_string();
    if quick {
        p.stop.use_zones = true;
        p.stop.zones = 3;
        p.iterations = 1;
    }
    p
}

fn cases(quick: bool) -> Vec<(String, Params)> {
    let mut v: Vec<(String, Params)> = vec![];
    let b = || base(quick);
    // every combination of the choices
    for sed in Sed::ALL {
        for geo in [Geometry::Slab, Geometry::Sphere] {
            for law in [DensityLaw::Constant, DensityLaw::ConstantPressure] {
                for ab in Abundances::ALL {
                    for gr in Grains::ALL {
                        let mut p = b();
                        p.sed = sed;
                        p.geometry = geo;
                        p.density_law = law;
                        p.abundances = ab;
                        p.grains = gr;
                        v.push((format!("grid {sed:?} {geo:?} {law:?} {ab:?} {gr:?}"), p));
                    }
                }
            }
        }
    }
    // each slider and limit at its ends
    let mut one = |name: &str, f: &dyn Fn(&mut Params)| {
        let mut p = b();
        f(&mut p);
        v.push((name.to_string(), p));
    };
    for h in [-2.0, 0.0, 4.0, 6.0, 8.0, 10.0] {
        one(&format!("hden {h}"), &|p| p.log_hden = h);
    }
    for u in [-6.0, -4.0, 0.0, 1.0] {
        one(&format!("logU {u}"), &|p| p.log_u = u);
    }
    for t in [5000.0, 10000.0, 200000.0] {
        one(&format!("Tbb {t}"), &|p| p.bb_temp = t);
    }
    for g in [-2.0, 3.0, 6.0] {
        one(&format!("ism G {g}"), &|p| {
            p.sed = Sed::IsmField;
            p.ism_log_g = g;
        });
    }
    for r in [10.0, 14.0, 22.0] {
        one(&format!("sphere r {r}"), &|p| {
            p.geometry = Geometry::Sphere;
            p.log_radius = r;
        });
    }
    for z in [-2.0, 1.0] {
        for gr in [Grains::None, Grains::IsmPah] {
            one(&format!("metals {z} {gr:?}"), &|p| {
                p.log_metals = z;
                p.grains = gr;
            });
        }
    }
    one("turbulence 20", &|p| p.turbulence = 20.0);
    one("no cosmic rays", &|p| p.cosmic_rays = false);
    one("stop temp 1", &|p| p.stop.temp = 1.0);
    one("stop temp 1e5", &|p| p.stop.temp = 1e5);
    one("stop temp off", &|p| p.stop.use_temp = false);
    one("stop av 0.001", &|p| p.stop.av = 0.001);
    one("stop av 1000", &|p| p.stop.av = 1000.0);
    for (c, on) in [(15.0, true), (26.0, true)] {
        one(&format!("stop column {c}"), &|p| {
            p.stop.use_column = on;
            p.stop.log_column = c;
        });
    }
    for t in [5.0, 24.0] {
        one(&format!("stop thickness {t}"), &|p| {
            p.stop.use_thickness = true;
            p.stop.log_thickness = t;
        });
        one(&format!("stop thickness {t} sphere"), &|p| {
            p.stop.use_thickness = true;
            p.stop.log_thickness = t;
            p.geometry = Geometry::Sphere;
        });
    }
    for e in [-10.0, 0.0] {
        one(&format!("stop efrac {e}"), &|p| {
            p.stop.use_efrac = true;
            p.stop.log_efrac = e;
        });
    }
    one("stop zones 1", &|p| p.stop.zones = 1);
    for h in [-1.8, -2.0] {
        for gr in [Grains::Ism, Grains::Orion, Grains::IsmPah] {
            one(&format!("low density {h} {gr:?}"), &|p| {
                p.log_hden = h;
                p.grains = gr;
            });
        }
    }
    one("stop av 1000 deep", &|p| p.stop.av = 1000.0);
    one("no stop at all", &|p| {
        p.stop.use_temp = false;
        p.stop.use_av = false;
        p.stop.use_zones = false;
    });
    one("iterations 3", &|p| p.iterations = 3);
    one("iterations 20", &|p| p.iterations = 20);
    one("converge 2", &|p| {
        p.converge = true;
        p.converge_max = 2;
    });
    one("converge 100 err 0.001", &|p| {
        p.converge = true;
        p.converge_max = 100;
        p.converge_error = 0.001;
    });
    one("converge err 1", &|p| {
        p.converge = true;
        p.converge_error = 1.0;
    });
    one("all lines incl H2", &|p| p.lines.iter_mut().for_each(|l| l.enabled = true));
    one("no lines", &|p| p.lines.iter_mut().for_each(|l| l.enabled = false));
    one("no chemistry", &|p| p.save_chemistry = false);
    one("40 species", &|p| {
        p.chem_species = crate::species::known("").into_iter().take(40).collect::<Vec<_>>().join(", ");
    });
    // the corners
    for h in [-2.0, 10.0] {
        for u in [-6.0, 1.0] {
            for (sn, sed, t) in [("bb5000", Sed::Blackbody, 5000.0), ("bb2e5", Sed::Blackbody, 2e5), ("agn", Sed::Agn, 0.0), ("custom", Sed::Custom, 0.0)] {
                one(&format!("corner hden {h} logU {u} {sn}"), &|p| {
                    p.log_hden = h;
                    p.log_u = u;
                    p.sed = sed;
                    if t > 0.0 {
                        p.bb_temp = t;
                    }
                });
            }
        }
        for g in [-2.0, 6.0] {
            one(&format!("corner ism hden {h} G {g}"), &|p| {
                p.sed = Sed::IsmField;
                p.log_hden = h;
                p.ism_log_g = g;
            });
        }
    }
    one("corner metals 1 pah hden 10 logU 1", &|p| {
        p.log_metals = 1.0;
        p.grains = Grains::IsmPah;
        p.log_hden = 10.0;
        p.log_u = 1.0;
    });
    one("corner pressure sphere r10 logU 1", &|p| {
        p.density_law = DensityLaw::ConstantPressure;
        p.geometry = Geometry::Sphere;
        p.log_radius = 10.0;
        p.log_u = 1.0;
    });
    v
}

/// Write one folder per case, and `cases.tsv` with what the checks say about it.
pub fn write(out: &Path, quick: bool) {
    let _ = std::fs::create_dir_all(out);
    let mut index = String::new();
    for (k, (name, p)) in cases(quick).into_iter().enumerate() {
        let dir = out.join(format!("{k:03}"));
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("model.in"), p.input_file());
        for (rel, content) in p.extra_files().unwrap_or_default() {
            let f = dir.join(rel);
            if let Some(d) = f.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(f, content);
        }
        let probs = p.problems();
        let errors: Vec<String> = probs.iter().filter(|x| x.error).map(|x| x.text.chars().take(70).collect()).collect();
        let warns = probs.iter().filter(|x| !x.error).count();
        let li = p.log_intensity().map(|x| format!("{x:.3}")).unwrap_or_default();
        index.push_str(&format!("{k:03}\t{name}\t{}\t{warns}\t{li}\t{}\n", errors.len(), errors.join(" | ")));
    }
    let _ = std::fs::write(out.join("cases.tsv"), index);
}
