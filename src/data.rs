//! Saving chosen result tables as CSV or tab-separated text.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::figures::RatioSpec;
use crate::results::{pretty_line, pretty_reaction, Results, Table};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DataSet {
    Summary,
    Structure,
    Lines,
    Ratios,
    Emissivity,
    Ions,
    Continuum,
    Grains,
    HeatCool,
    Chemistry,
}

impl DataSet {
    pub const ALL: [DataSet; 10] = [
        DataSet::Summary,
        DataSet::Structure,
        DataSet::Lines,
        DataSet::Ratios,
        DataSet::Emissivity,
        DataSet::Ions,
        DataSet::Continuum,
        DataSet::Grains,
        DataSet::HeatCool,
        DataSet::Chemistry,
    ];
    pub fn label(self) -> &'static str {
        match self {
            DataSet::Summary => "Summary (fronts, temperatures)",
            DataSet::Structure => "Structure vs depth (overview)",
            DataSet::Lines => "Line intensities",
            DataSet::Ratios => "Line ratios",
            DataSet::Emissivity => "Line emissivity vs depth",
            DataSet::Ions => "Ionisation fractions (per element)",
            DataSet::Continuum => "Continuum (SED)",
            DataSet::Grains => "Grain temperatures",
            DataSet::HeatCool => "Heating and cooling",
            DataSet::Chemistry => "Reaction rates (per species)",
        }
    }
}

struct Writer {
    sep: &'static str,
    ext: &'static str,
}

impl Writer {
    fn cell(&self, s: &str) -> String {
        if self.sep == "," && (s.contains(',') || s.contains('"') || s.contains(' ')) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    }
    fn table(&self, cols: &[String], data: &[Vec<f64>]) -> String {
        let mut s = String::new();
        let head: Vec<String> = cols.iter().map(|c| self.cell(c.trim())).collect();
        let _ = writeln!(s, "{}", head.join(self.sep));
        let n = data.iter().map(|c| c.len()).max().unwrap_or(0);
        for i in 0..n {
            let row: Vec<String> = data.iter().map(|c| c.get(i).map(|v| format!("{v:.6e}")).unwrap_or_default()).collect();
            let _ = writeln!(s, "{}", row.join(self.sep));
        }
        s
    }
    fn tbl(&self, t: &Table) -> String {
        self.table(&t.cols, &t.data)
    }
}

/// Write the chosen tables to `dir`, every file name starting with `prefix`.
pub fn save(r: &Results, sets: &[DataSet], dir: &Path, prefix: &str, tsv: bool, ratios: &[RatioSpec]) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    let w = if tsv { Writer { sep: "\t", ext: "txt" } } else { Writer { sep: ",", ext: "csv" } };
    let mut out = vec![];
    let mut put = |name: &str, text: String| -> Result<(), String> {
        let p = dir.join(format!("{prefix}_{name}.{}", w.ext));
        std::fs::write(&p, text).map_err(|e| format!("Cannot write {}: {e}", p.display()))?;
        out.push(p);
        Ok(())
    };
    for set in sets {
        match set {
            DataSet::Summary => {
                let mut s = String::new();
                let mut kv = |k: &str, v: String| {
                    let _ = writeln!(s, "{}{}{}", w.cell(k), w.sep, w.cell(&v));
                };
                kv("quantity", "value".into());
                kv("model", r.deck.title.clone());
                kv("zones", r.depth.len().to_string());
                kv("thickness_cm", r.depth.last().map(|d| format!("{d:.6e}")).unwrap_or_default());
                let opt = |v: Option<f64>| v.map(|x| format!("{x:.6e}")).unwrap_or_else(|| "none".into());
                kv("ionisation_front_depth_cm", opt(r.fronts.ionisation));
                kv("ionisation_front_AV", opt(r.fronts.ionisation.and_then(|d| r.av_at(d))));
                kv("dissociation_front_depth_cm", opt(r.fronts.dissociation));
                kv("dissociation_front_AV", opt(r.fronts.dissociation.and_then(|d| r.av_at(d))));
                kv("C_CO_transition_depth_cm", opt(r.fronts.co));
                kv("mean_Te_HII_K", opt(r.mean_te_hii()));
                kv("total_AV", opt(r.av.as_ref().and_then(|a| a.last().copied())));
                kv("total_NH_cm-2", opt(r.column.last().copied()));
                put("summary", s)?;
            }
            DataSet::Structure => put("structure", w.tbl(&r.overview))?,
            DataSet::Lines => {
                let mut s = String::new();
                let _ = writeln!(s, "{}", ["label", "name", "intensity_erg_cm-2_s-1", &format!("I/I({})", r.line_ref)].map(|c| w.cell(c)).join(w.sep));
                for l in &r.lines {
                    let _ = writeln!(s, "{}{sep}{}{sep}{:.6e}{sep}{:.6e}", w.cell(l.label.trim()), w.cell(&pretty_line(&l.label)), l.intensity, l.relative, sep = w.sep);
                }
                put("lines", s)?;
            }
            DataSet::Ratios => {
                let mut s = String::new();
                let _ = writeln!(s, "{}", ["numerator", "denominator", "ratio"].join(w.sep));
                for sp in ratios {
                    let v = match (r.line(&sp.num), r.line(&sp.den)) {
                        (Some(a), Some(b)) if b.intensity > 0.0 => format!("{:.6e}", a.intensity / b.intensity),
                        _ => "n/a".into(),
                    };
                    let _ = writeln!(s, "{}{sep}{}{sep}{v}", w.cell(&pretty_line(&sp.num)), w.cell(&pretty_line(&sp.den)), sep = w.sep);
                }
                put("ratios", s)?;
            }
            DataSet::Emissivity => {
                if let Some(t) = &r.emissivity {
                    put("emissivity", w.tbl(t))?;
                }
            }
            DataSet::Ions => {
                for (el, t) in &r.elements {
                    put(&format!("ions_{el}"), w.tbl(t))?;
                }
            }
            DataSet::Continuum => {
                if let Some(t) = &r.continuum {
                    // keep the numeric columns only
                    let keep: Vec<usize> = (0..t.cols.len()).filter(|i| t.data[*i].iter().any(|v| v.is_finite())).collect();
                    let cols: Vec<String> = keep.iter().map(|i| if *i == 0 { "wavelength_micron".to_string() } else { t.cols[*i].clone() }).collect();
                    let data: Vec<Vec<f64>> = keep.iter().map(|i| t.data[*i].clone()).collect();
                    put("continuum", w.table(&cols, &data))?;
                }
            }
            DataSet::Grains => {
                if let Some(t) = &r.grains {
                    put("grain_temperature", w.tbl(t))?;
                }
            }
            DataSet::HeatCool => {
                for (name, a) in [("heating", &r.heating), ("cooling", &r.cooling)] {
                    if let Some(a) = a {
                        let mut cols = vec!["depth_cm".to_string(), "total".to_string()];
                        let mut data = vec![a.depth.clone(), a.total.clone()];
                        for (n, v) in &a.agents {
                            cols.push(n.trim().to_string());
                            data.push(v.clone());
                        }
                        put(name, w.table(&cols, &data))?;
                    }
                }
            }
            DataSet::Chemistry => {
                for c in &r.chemistry {
                    let f = crate::model::species_file(&c.species);
                    for (kind, t) in [("formation", &c.formation), ("destruction", &c.destruction)] {
                        if let Some(t) = t {
                            let cols: Vec<String> = t.cols.iter().enumerate().map(|(i, x)| if i == 0 { "depth_cm".into() } else { pretty_reaction(x) }).collect();
                            put(&format!("rates_{f}_{kind}"), w.table(&cols, &t.data))?;
                        }
                    }
                }
            }
        }
    }
    Ok(out)
}
