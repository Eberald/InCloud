//! Reading CLOUDY "save" files and deriving physical quantities
//! (ionisation / dissociation fronts, H II – PDR – molecular regions, line intensities).

use std::path::{Path, PathBuf};

use crate::detect::{self, Files, Kind};
use crate::model::{read_deck, DeckInfo};
use crate::runinfo::{self, Level, RunInfo};

/// A tab-separated CLOUDY table: column names and column-major numeric data.
#[derive(Clone, Debug, Default)]
pub struct Table {
    pub cols: Vec<String>,
    pub data: Vec<Vec<f64>>,
}

impl Table {
    pub fn rows(&self) -> usize {
        self.data.first().map(|c| c.len()).unwrap_or(0)
    }
    pub fn find(&self, names: &[&str]) -> Option<usize> {
        for n in names {
            let n = n.to_ascii_lowercase();
            if let Some(i) = self.cols.iter().position(|c| c.trim().to_ascii_lowercase() == n) {
                return Some(i);
            }
        }
        for n in names {
            let n = n.to_ascii_lowercase();
            if let Some(i) = self.cols.iter().position(|c| c.trim().to_ascii_lowercase().starts_with(&n)) {
                return Some(i);
            }
        }
        None
    }
    pub fn col(&self, names: &[&str]) -> Option<&Vec<f64>> {
        self.find(names).map(|i| &self.data[i])
    }
}

fn parse_num(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    t.parse::<f64>().ok().or_else(|| {
        // Fortran-style exponents like 1.0D+02 or 1.0-100
        let t2 = t.replace(['D', 'd'], "E");
        t2.parse::<f64>().ok()
    })
}

/// Parse a CLOUDY save file. Keeps the last block (last iteration) when several are present.
pub fn parse_table(text: &str) -> Table {
    let mut header: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<f64>> = Vec::new();
    let mut prev_first: Option<f64> = None;
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with('#') {
            if line.chars().all(|c| c == '#' || c.is_whitespace()) {
                // iteration separator
                rows.clear();
                prev_first = None;
                continue;
            }
            let h: Vec<String> = line.trim_start_matches('#').split('\t').map(|s| s.trim().to_string()).collect();
            if h.len() > 1 {
                header = h;
                rows.clear();
                prev_first = None;
            }
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let vals: Vec<f64> = fields.iter().map(|f| parse_num(f).unwrap_or(f64::NAN)).collect();
        if vals.first().map(|v| v.is_nan()).unwrap_or(true) {
            continue;
        }
        // a depth that goes back down means a new iteration started
        if let (Some(p), Some(v)) = (prev_first, vals.first()) {
            if header.first().map(|h| h.to_ascii_lowercase().starts_with("depth")).unwrap_or(false) && *v < p * 0.999 {
                rows.clear();
            }
        }
        prev_first = vals.first().copied();
        rows.push(vals);
    }
    let ncol = rows.iter().map(|r| r.len()).max().unwrap_or(0).max(header.len());
    let mut cols = header;
    while cols.len() < ncol {
        cols.push(format!("col{}", cols.len()));
    }
    let mut data = vec![Vec::with_capacity(rows.len()); ncol];
    for r in &rows {
        for (j, d) in data.iter_mut().enumerate() {
            d.push(r.get(j).copied().unwrap_or(f64::NAN));
        }
    }
    Table { cols, data }
}

/// Values of a "fraction-like" column, linear. CLOUDY often writes log10 values;
/// a positive physical quantity with any negative entry must be logarithmic.
pub fn to_linear(v: &[f64]) -> Vec<f64> {
    if v.iter().any(|x| x.is_finite() && *x < 0.0) {
        v.iter().map(|x| 10f64.powf(*x)).collect()
    } else {
        v.to_vec()
    }
}

/// Older CLOUDY versions write log10 densities in the overview, C25 writes linear ones.
/// When the input deck is known, compare with its `hden` value to decide.
pub fn density_is_log(v: &[f64], deck_log_hden: Option<f64>) -> bool {
    let mut f: Vec<f64> = v.iter().cloned().filter(|x| x.is_finite()).collect();
    if f.is_empty() {
        return false;
    }
    f.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med = f[f.len() / 2];
    if let Some(lh) = deck_log_hden {
        let as_log = (med - lh).abs();
        let as_lin = if med > 0.0 { (med.log10() - lh).abs() } else { f64::MAX };
        return as_log < as_lin;
    }
    f.iter().any(|x| *x < 0.0) || f.last().copied().unwrap_or(0.0) < 30.0
}

fn density_linear(v: &[f64], is_log: bool) -> Vec<f64> {
    if is_log {
        v.iter().map(|x| 10f64.powf(*x)).collect()
    } else {
        v.to_vec()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Agents {
    pub depth: Vec<f64>,
    pub total: Vec<f64>,
    /// (agent label, rate per zone in erg cm^-3 s^-1)
    pub agents: Vec<(String, Vec<f64>)>,
}

/// Parse `save heating` / `save cooling` output: depth, Te, Htot, Ctot, then label/fraction pairs.
pub fn parse_agents(text: &str, heating: bool) -> Agents {
    let mut out = Agents::default();
    let mut rows: Vec<(f64, f64, Vec<(String, f64)>)> = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 4 {
            continue;
        }
        let (Some(d), Some(h), Some(c)) = (parse_num(f[0]), parse_num(f[2]), parse_num(f[3])) else {
            continue;
        };
        if let Some((pd, _, _)) = rows.last() {
            if d < pd * 0.999 {
                rows.clear();
            }
        }
        let total = if heating { h } else { c };
        let mut pairs = Vec::new();
        let mut label: Option<String> = None;
        for tok in &f[4..] {
            let t = tok.trim();
            if t.is_empty() {
                continue;
            }
            if let Some(v) = parse_num(t) {
                if let Some(l) = label.take() {
                    pairs.push((l, v));
                }
            } else {
                // a token may hold "label value" together
                let mut parts: Vec<&str> = t.split_whitespace().collect();
                // only when the last part is clearly a fraction ("0.82", "8.2e-01"):
                // labels such as "H  1" end with an integer ion stage
                let looks_value = parts.last().map(|p| p.contains('.') || p.contains('e') || p.contains('E')).unwrap_or(false);
                if parts.len() > 1 && looks_value {
                    if let Some(v) = parts.last().and_then(|p| parse_num(p)) {
                        parts.pop();
                        pairs.push((parts.join(" "), v));
                        continue;
                    }
                }
                label = Some(t.to_string());
            }
        }
        rows.push((d, total, pairs));
    }
    let mut names: Vec<String> = Vec::new();
    for (_, _, p) in &rows {
        for (n, _) in p {
            let n = n.trim().to_string();
            if !names.contains(&n) {
                names.push(n);
            }
        }
    }
    out.depth = rows.iter().map(|r| r.0).collect();
    out.total = rows.iter().map(|r| r.1).collect();
    for n in names {
        let v: Vec<f64> = rows
            .iter()
            .map(|(_, tot, p)| {
                let fr: f64 = p.iter().filter(|(k, _)| k.trim() == n).map(|(_, v)| *v).sum();
                fr * tot
            })
            .collect();
        out.agents.push((n, v));
    }
    // most important agents first
    out.agents.sort_by(|a, b| {
        let ma = a.1.iter().zip(&out.total).map(|(x, t)| x / t.max(1e-300)).fold(0.0, f64::max);
        let mb = b.1.iter().zip(&out.total).map(|(x, t)| x / t.max(1e-300)).fold(0.0, f64::max);
        mb.partial_cmp(&ma).unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

#[derive(Clone, Debug)]
pub struct LineIntensity {
    pub label: String,
    /// Integrated intensity (erg cm^-2 s^-1, per unit area of the illuminated face)
    pub intensity: f64,
    /// Relative to H-beta (or to the strongest line when H-beta is missing)
    pub relative: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Fronts {
    pub ionisation: Option<f64>,
    pub dissociation: Option<f64>,
    pub co: Option<f64>,
}

#[derive(Clone, Debug, Default)]
pub struct Results {
    pub dir: PathBuf,
    pub prefix: String,
    pub demo: bool,
    pub deck: DeckInfo,
    pub input_text: String,

    pub depth: Vec<f64>,
    pub te: Vec<f64>,
    pub hden: Vec<f64>,
    pub eden: Vec<f64>,
    pub x_hplus: Vec<f64>,
    pub x_h0: Vec<f64>,
    pub x_h2: Vec<f64>,
    pub co_c: Option<Vec<f64>>,
    pub av: Option<Vec<f64>>,
    pub av_estimated: bool,
    pub column: Vec<f64>,

    pub elements: Vec<(String, Table)>,
    pub continuum: Option<Table>,
    pub grains: Option<Table>,
    pub heating: Option<Agents>,
    pub cooling: Option<Agents>,
    pub emissivity: Option<Table>,
    pub lines: Vec<LineIntensity>,
    pub line_ref: String,

    pub fronts: Fronts,
    pub exited_ok: Option<bool>,
    pub warnings: Vec<String>,
    /// the whole overview table, converted to linear values
    pub overview: Table,
    pub chemistry: Vec<Chem>,
    /// everything the text output says about the run
    pub info: RunInfo,
    /// only the text output exists: there is nothing to plot
    pub info_only: bool,
    /// the runs found in the same folder
    pub siblings: Vec<String>,
    /// where each kind of data was found
    pub files: Files,
}

/// The runs in a folder, newest first: the names that have a .out or a .ovr file.
pub fn runs_in(dir: &Path) -> Vec<String> {
    let mut found: Vec<(std::time::SystemTime, String)> = vec![];
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if !p.extension().map(|x| x == "out" || x == "ovr" || x == "in").unwrap_or(false) {
            continue;
        }
        let Some(stem) = p.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
        let time = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
        match found.iter_mut().find(|f| f.1 == stem) {
            Some(f) => f.0 = f.0.max(time),
            None => found.push((time, stem)),
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    found.into_iter().map(|f| f.1).collect()
}

/// Reaction rates of one species (cm^-3 s^-1 per reaction, versus depth).
#[derive(Clone, Debug, Default)]
pub struct Chem {
    pub species: String,
    pub formation: Option<Table>,
    pub destruction: Option<Table>,
}

/// One reaction's share of the formation or destruction of a species over a depth range.
#[derive(Clone, Debug)]
pub struct ReactionShare {
    pub reaction: String,
    pub integrated: f64,
    pub fraction: f64,
}

impl Chem {
    /// Reactions integrated over [d0, d1], largest first.
    pub fn shares(&self, formation: bool, d0: f64, d1: f64) -> Vec<ReactionShare> {
        let Some(t) = (if formation { self.formation.as_ref() } else { self.destruction.as_ref() }) else { return vec![] };
        let d = &t.data[0];
        let mut out = vec![];
        for c in 1..t.cols.len() {
            let mut s = 0.0;
            for i in 1..d.len() {
                let (a, b) = (d[i - 1].max(d0), d[i].min(d1));
                if b > a {
                    let (y0, y1) = (t.data[c][i - 1], t.data[c][i]);
                    if y0.is_finite() && y1.is_finite() {
                        s += 0.5 * (y0 + y1) * (b - a);
                    }
                }
            }
            if s > 0.0 {
                out.push(ReactionShare { reaction: t.cols[c].trim().to_string(), integrated: s, fraction: 0.0 });
            }
        }
        let tot: f64 = out.iter().map(|r| r.integrated).sum();
        for r in &mut out {
            r.fraction = r.integrated / tot.max(1e-300);
        }
        out.sort_by(|a, b| b.integrated.partial_cmp(&a.integrated).unwrap_or(std::cmp::Ordering::Equal));
        out
    }
}

/// One row of the reaction-rates table.
#[derive(Clone, Debug)]
pub struct RateRow {
    pub formation: bool,
    pub reaction: String,
    pub integrated: f64,
    pub fraction: f64,
    pub peak: f64,
    pub peak_depth: f64,
}

impl Chem {
    /// Every reaction with its share, peak rate and where the peak is, largest first.
    pub fn table(&self, d0: f64, d1: f64) -> Vec<RateRow> {
        let mut out = vec![];
        for formation in [true, false] {
            let Some(t) = (if formation { self.formation.as_ref() } else { self.destruction.as_ref() }) else { continue };
            for s in self.shares(formation, d0, d1) {
                let Some(c) = (1..t.cols.len()).find(|c| t.cols[*c].trim() == s.reaction) else { continue };
                let (mut peak, mut at) = (0.0, f64::NAN);
                for (d, v) in t.data[0].iter().zip(&t.data[c]) {
                    if *d >= d0 && *d <= d1 && *v > peak {
                        peak = *v;
                        at = *d;
                    }
                }
                out.push(RateRow { formation, reaction: s.reaction, integrated: s.integrated, fraction: s.fraction, peak, peak_depth: at });
            }
        }
        out
    }
}

/// Line intensities of a run, enough to place it on a BPT diagram.
#[derive(Clone, Debug)]
pub struct BptRun {
    pub name: String,
    pub source: String,
    pub lines: Vec<(String, f64)>,
    pub visible: bool,
}

impl BptRun {
    pub fn from_results(r: &Results) -> BptRun {
        let name = if r.deck.title.is_empty() { r.prefix.clone() } else { r.deck.title.clone() };
        BptRun { name, source: r.dir.display().to_string(), lines: r.lines.iter().map(|l| (l.label.clone(), l.intensity)).collect(), visible: true }
    }

    /// Read a CLOUDY output file (.out): its table of intrinsic line intensities.
    pub fn from_out(path: &Path) -> Option<BptRun> {
        let text = read(path)?;
        let lines = parse_out_lines(&text);
        if lines.is_empty() {
            return None;
        }
        let stem = path.file_stem()?.to_string_lossy().to_string();
        let parent = path.parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let name = if parent.is_empty() { stem } else { format!("{parent}/{stem}") };
        Some(BptRun { name, source: path.display().to_string(), lines, visible: true })
    }
}

/// The "Intrinsic line intensities" table of a CLOUDY .out file: (label, intensity).
pub fn parse_out_lines(text: &str) -> Vec<(String, f64)> {
    let mut out: Vec<(String, f64)> = vec![];
    let mut inside = false;
    for l in text.lines() {
        if l.contains("Intrinsic line intensities") {
            // keep only the last iteration's table
            out.clear();
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if l.contains("Emergent line intensities") || l.contains("Cloudy ends") {
            inside = false;
            continue;
        }
        let t: Vec<&str> = l.split_whitespace().collect();
        for i in 1..t.len().saturating_sub(2) {
            let w = t[i];
            let is_wl = w.len() > 1 && w.ends_with(['A', 'm', 'c']) && w[..w.len() - 1].parse::<f64>().is_ok();
            if !is_wl {
                continue;
            }
            let (Ok(lg), Ok(_)) = (t[i + 1].parse::<f64>(), t[i + 2].parse::<f64>()) else { continue };
            let stage = t[i - 1].chars().all(|c| c.is_ascii_digit());
            let species = if stage && i >= 2 { format!("{:<2} {}", t[i - 2], t[i - 1]) } else { t[i - 1].to_string() };
            out.push((format!("{species:<4} {w}"), 10f64.powf(lg)));
        }
    }
    out
}

/// Every CLOUDY run found in a folder and its sub-folders (at most `limit` output files).
pub fn scan_runs(dir: &Path, limit: usize) -> Vec<BptRun> {
    let mut found = vec![];
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                if depth < 3 {
                    stack.push((p, depth + 1));
                }
            } else if p.extension().map(|e| e == "out").unwrap_or(false) && found.len() < limit {
                if let Some(r) = BptRun::from_out(&p) {
                    found.push(r);
                }
            }
        }
    }
    found
}

/// "C+,OH=>H+,CO" -> "C⁺ + OH → H⁺ + CO"
pub fn pretty_reaction(r: &str) -> String {
    let side = |s: &str| -> String {
        s.split(',')
            .map(|x| crate::figures::markup_to_unicode(&crate::figures::ion_markup(x.trim())).replace("PHOTON", "γ").replace("CRPHOT", "CR γ").replace("CRP", "CR"))
            .collect::<Vec<_>>()
            .join(" + ")
    };
    match r.split_once("=>") {
        Some((a, b)) => format!("{} → {}", side(a), side(b)),
        None => r.to_string(),
    }
}

fn read(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Depth where `v` first crosses `level` (interpolated in log depth).
pub fn crossing(depth: &[f64], v: &[f64], level: f64, falling: bool) -> Option<f64> {
    for i in 1..depth.len().min(v.len()) {
        let (a, b) = (v[i - 1], v[i]);
        let hit = if falling { a >= level && b < level } else { a <= level && b > level };
        if hit {
            let t = ((level - a) / (b - a)).clamp(0.0, 1.0);
            let (d0, d1) = (depth[i - 1].max(1e-30), depth[i].max(1e-30));
            return Some(10f64.powf(d0.log10() + t * (d1.log10() - d0.log10())));
        }
    }
    None
}

/// Trapezoid integral of y over x.
fn trapz(x: &[f64], y: &[f64]) -> f64 {
    let mut s = 0.0;
    for i in 1..x.len().min(y.len()) {
        let (a, b) = (y[i - 1], y[i]);
        if a.is_finite() && b.is_finite() {
            s += 0.5 * (a + b) * (x[i] - x[i - 1]);
        }
    }
    s
}

impl Results {
    /// Load every recognised output from a run folder.
    pub fn load(dir: &Path, prefix_hint: Option<&str>) -> Result<Results, String> {
        let siblings = runs_in(dir);
        let prefix = prefix_hint.map(|s| s.to_string()).or_else(|| siblings.first().cloned()).or_else(|| detect::loose_run(dir));
        let prefix = prefix.ok_or_else(|| "No CLOUDY run here: InCloud found no input file (.in), no text output (.out) and no file holding a saved overview.".to_string())?;
        let f = |ext: &str| dir.join(format!("{prefix}.{ext}"));
        let mut r = Results { dir: dir.to_path_buf(), prefix: prefix.clone(), siblings, ..Default::default() };

        let out_text = read(&f("out"));
        if let Some((t, _)) = detect::input_of(dir, &prefix, out_text.as_deref()) {
            r.deck = read_deck(&t);
            r.input_text = t;
        }
        r.files = detect::find(dir, &prefix, (!r.input_text.is_empty()).then_some(r.input_text.as_str()), r.siblings.len() <= 1);
        let file = |k: Kind| r.files.get(&k).map(Path::to_path_buf);
        let (ovr_path, con_path, grain_path, het_path, col_path, ems_path) = (file(Kind::Overview), file(Kind::Continuum), file(Kind::Grains), file(Kind::Heating), file(Kind::Cooling), file(Kind::Emissivity));
        let cum_path = file(Kind::Cumulative);
        let element_paths = r.files.elements();
        let chemistry_paths = r.files.chemistry();
        if let Some(t) = out_text {
            r.exited_ok = Some(t.contains("Cloudy exited OK") || t.contains("Cloudy ends: OK") || t.contains("exited OK"));
            r.demo = t.contains("CLOUDY-STUDIO-DEMO") || t.contains("INCLOUD-DEMO");
            // the demo has no real output to report on
            r.info = if r.demo { RunInfo::default() } else { runinfo::parse(&t) };
            r.warnings = r.info.messages.iter().filter(|m| matches!(m.level, Level::Warning | Level::Caution)).map(|m| m.text.clone()).collect();
            r.warnings.extend(r.info.problems.iter().take(40).cloned());
            // a run started elsewhere: its input is repeated at the top of the output
            if r.input_text.is_empty() && !r.info.input.is_empty() {
                r.input_text = r.info.input.join("\n") + "\n";
                r.deck = read_deck(&r.input_text);
            }
        }

        let ov = ovr_path.as_deref().and_then(read).map(|t| parse_table(&t)).unwrap_or_default();
        if ov.rows() == 0 {
            if r.info.present {
                r.info_only = true;
                return Ok(r);
            }
            return Err(format!(
                "Run “{prefix}”: no text output (.out) and no overview were found, so there is nothing to show. InCloud needs at least the output of CLOUDY ({prefix}.out) or a file saved with “save overview \"{prefix}.ovr\" last”. Files in the folder: {}.",
                if r.files.others.is_empty() { "none InCloud recognises".to_string() } else { r.files.others.iter().take(12).cloned().collect::<Vec<_>>().join(", ") }
            ));
        }
        r.depth = ov.data[0].clone();
        let n = r.depth.len();
        r.te = ov.col(&["Te"]).cloned().unwrap_or_else(|| vec![f64::NAN; n]);
        let dens_log = ov.col(&["hden"]).map(|v| density_is_log(v, r.deck.log_hden)).unwrap_or(false);
        r.hden = ov.col(&["hden"]).map(|v| density_linear(v, dens_log)).unwrap_or_else(|| vec![f64::NAN; n]);
        r.eden = ov.col(&["eden"]).map(|v| density_linear(v, dens_log)).unwrap_or_else(|| vec![f64::NAN; n]);
        r.x_hplus = ov.col(&["HII", "H+"]).map(|v| to_linear(v)).unwrap_or_else(|| vec![f64::NAN; n]);
        r.x_h0 = ov.col(&["HI", "H0"]).map(|v| to_linear(v)).unwrap_or_else(|| vec![f64::NAN; n]);
        r.x_h2 = ov.col(&["2H_2/H", "2H2/H", "H2"]).map(|v| to_linear(v)).unwrap_or_else(|| vec![0.0; n]);
        r.co_c = ov.col(&["CO/C"]).map(|v| to_linear(v));
        // linear copy of every overview column for custom plots and data export
        let mut ovl = ov.clone();
        for c in 1..ovl.cols.len() {
            let name = ovl.cols[c].to_ascii_lowercase();
            ovl.data[c] = if name == "hden" || name == "eden" {
                density_linear(&ov.data[c], dens_log)
            } else if name == "te" || name.starts_with("av") || name.starts_with("tau") || name.starts_with("htot") {
                ov.data[c].clone()
            } else {
                to_linear(&ov.data[c])
            };
        }
        r.overview = ovl;

        // column density from hden and depth
        let mut col = Vec::with_capacity(n);
        let mut acc = 0.0;
        for i in 0..n {
            let dr = if i == 0 { r.depth[0] } else { r.depth[i] - r.depth[i - 1] };
            let nh = if i == 0 { r.hden[0] } else { 0.5 * (r.hden[i] + r.hden[i - 1]) };
            if nh.is_finite() {
                acc += nh * dr;
            }
            col.push(acc);
        }
        r.column = col;
        if let Some(av) = ov.col(&["AV(extend)", "AV(point)", "AV"]) {
            r.av = Some(av.clone());
        } else {
            r.av = Some(r.column.iter().map(|c| c / 1.87e21).collect());
            r.av_estimated = true;
        }

        // fronts
        r.fronts.ionisation = crossing(&r.depth, &r.x_hplus, 0.5, true);
        r.fronts.dissociation = crossing(&r.depth, &r.x_h2, 0.5, false);
        if let Some(co) = &r.co_c {
            r.fronts.co = crossing(&r.depth, co, 0.5, false);
        }

        for (sym, path) in element_paths {
            if let Some(t) = read(&path) {
                let raw = parse_table(&t);
                // C25 adds level populations such as "O[1]": keep only atoms, ions and molecules
                let keep: Vec<usize> = (0..raw.cols.len()).filter(|i| *i == 0 || !raw.cols[*i].contains('[')).collect();
                let mut tb = Table {
                    cols: keep.iter().map(|i| raw.cols[*i].clone()).collect(),
                    data: keep.iter().map(|i| raw.data[*i].clone()).collect(),
                };
                if tb.rows() > 0 && tb.cols.len() > 1 {
                    for c in 1..tb.data.len() {
                        tb.data[c] = to_linear(&tb.data[c]);
                    }
                    // densities rather than fractions: normalise each row
                    let rows = tb.rows();
                    for i in 0..rows {
                        let s: f64 = (1..tb.data.len()).map(|c| tb.data[c][i]).filter(|x| x.is_finite()).sum();
                        if s > 1.5 {
                            for c in 1..tb.data.len() {
                                tb.data[c][i] /= s;
                            }
                        }
                    }
                    r.elements.push((sym.to_string(), tb));
                }
            }
        }
        if let Some(t) = con_path.as_deref().and_then(read) {
            let tb = parse_table(&t);
            if tb.rows() > 0 {
                r.continuum = Some(tb);
            }
        }
        if let Some(t) = grain_path.as_deref().and_then(read) {
            let tb = parse_table(&t);
            if tb.rows() > 0 {
                r.grains = Some(tb);
            }
        }
        if let Some(t) = het_path.as_deref().and_then(read) {
            let a = parse_agents(&t, true);
            if !a.depth.is_empty() {
                r.heating = Some(a);
            }
        }
        if let Some(t) = col_path.as_deref().and_then(read) {
            let a = parse_agents(&t, false);
            if !a.depth.is_empty() {
                r.cooling = Some(a);
            }
        }
        for (sp, ff, fd) in chemistry_paths {
            let load = |path: &Option<PathBuf>| -> Option<Table> {
                let t = parse_table(&read(path.as_ref()?)?);
                (t.rows() > 0 && t.cols.len() > 1).then_some(t)
            };
            let c = Chem { species: sp, formation: load(&ff), destruction: load(&fd) };
            if c.formation.is_some() || c.destruction.is_some() {
                r.chemistry.push(c);
            }
        }
        if let Some(t) = ems_path.as_deref().and_then(read) {
            let mut tb = parse_table(&t);
            if tb.rows() > 0 {
                for c in 1..tb.data.len() {
                    tb.data[c] = to_linear(&tb.data[c]);
                }
                r.compute_lines(&tb);
                r.emissivity = Some(tb);
            }
        }
        // a cumulative file already holds the integrals: its last row is the intensity of each
        // line, and its slope the emissivity (it must never be integrated a second time)
        if let Some(t) = cum_path.as_deref().and_then(read) {
            let mut tb = parse_table(&t);
            if tb.rows() > 1 {
                for c in 1..tb.data.len() {
                    tb.data[c] = to_linear(&tb.data[c]);
                }
                let last = tb.rows() - 1;
                let lines: Vec<LineIntensity> = (1..tb.cols.len()).map(|c| LineIntensity { label: tb.cols[c].clone(), intensity: tb.data[c][last].max(0.0), relative: 0.0 }).collect();
                if r.emissivity.is_none() {
                    let depth = tb.data[0].clone();
                    let mut ems = tb.clone();
                    for c in 1..ems.data.len() {
                        let v = &tb.data[c];
                        ems.data[c] = (0..depth.len())
                            .map(|k| {
                                let (a, b) = (k.saturating_sub(1), (k + 1).min(depth.len() - 1));
                                let dd = depth[b] - depth[a];
                                if dd > 0.0 { ((v[b] - v[a]) / dd).max(0.0) } else { 0.0 }
                            })
                            .collect();
                    }
                    r.emissivity = Some(ems);
                }
                r.set_lines(lines);
            }
        }
        Ok(r)
    }

    fn compute_lines(&mut self, ems: &Table) {
        let depth = &ems.data[0];
        let sphere = self.deck.sphere;
        let r0 = self.deck.log_radius.map(|l| 10f64.powf(l)).unwrap_or(0.0);
        let mut out = Vec::new();
        for c in 1..ems.cols.len() {
            let e = &ems.data[c];
            let y: Vec<f64> = if sphere && r0 > 0.0 {
                depth.iter().zip(e).map(|(d, v)| v * ((r0 + d) / r0).powi(2)).collect()
            } else {
                e.clone()
            };
            let i = trapz(depth, &y);
            out.push(LineIntensity { label: ems.cols[c].clone(), intensity: i, relative: 0.0 });
        }
        self.set_lines(out);
    }

    /// Keep these line intensities, each also relative to Hβ (or to the strongest line).
    fn set_lines(&mut self, mut out: Vec<LineIntensity>) {
        let hb = out
            .iter()
            .find(|l| {
                let s = l.label.replace(' ', "").to_ascii_uppercase();
                s.starts_with("H1486") || s.starts_with("HI486")
            })
            .map(|l| (l.intensity, l.label.clone()));
        let (norm, name) = match hb {
            Some((v, _)) if v > 0.0 => (v, "Hβ".to_string()),
            _ => {
                let m = out.iter().map(|l| l.intensity).fold(0.0, f64::max);
                (if m > 0.0 { m } else { 1.0 }, "strongest line".to_string())
            }
        };
        for l in &mut out {
            l.relative = l.intensity / norm;
        }
        self.lines = out;
        self.line_ref = name;
    }

    /// Mean temperature of the ionised gas, weighted by n_e n_H+.
    pub fn mean_te_hii(&self) -> Option<f64> {
        let w: Vec<f64> = (0..self.depth.len())
            .map(|i| self.eden[i] * self.hden[i] * self.x_hplus[i])
            .collect();
        let num = trapz(&self.depth, &w.iter().zip(&self.te).map(|(a, b)| a * b).collect::<Vec<_>>());
        let den = trapz(&self.depth, &w);
        if den > 0.0 && num.is_finite() {
            Some(num / den)
        } else {
            None
        }
    }

    /// A_V at a given depth (interpolated).
    pub fn av_at(&self, d: f64) -> Option<f64> {
        let av = self.av.as_ref()?;
        interp(&self.depth, av, d)
    }

    /// Integrated intensity of a line (matched by species and wavelength).
    pub fn line(&self, label: &str) -> Option<&LineIntensity> {
        self.lines.iter().find(|l| same_line(&l.label, label))
    }

    /// Local emissivity 4πj of a line versus depth.
    pub fn emissivity_of(&self, label: &str) -> Option<(Vec<f64>, Vec<f64>)> {
        let t = self.emissivity.as_ref()?;
        let c = (1..t.cols.len()).find(|c| same_line(&t.cols[*c], label))?;
        Some((t.data[0].clone(), t.data[c].clone()))
    }

    /// Intensity emitted between the illuminated face and each depth (sphere: weighted by r²).
    pub fn cumulative_of(&self, label: &str) -> Option<(Vec<f64>, Vec<f64>)> {
        let (d, e) = self.emissivity_of(label)?;
        let r0 = if self.deck.sphere { self.deck.log_radius.map(|l| 10f64.powf(l)).unwrap_or(0.0) } else { 0.0 };
        let mut acc = 0.0;
        let mut out = Vec::with_capacity(d.len());
        for i in 0..d.len() {
            if i > 0 {
                let w = |k: usize| if r0 > 0.0 { ((r0 + d[k]) / r0).powi(2) } else { 1.0 };
                let (a, b) = (e[i - 1] * w(i - 1), e[i] * w(i));
                if a.is_finite() && b.is_finite() {
                    acc += 0.5 * (a + b) * (d[i] - d[i - 1]);
                }
            }
            out.push(acc);
        }
        Some((d, out))
    }

    /// Every quantity that can go on a custom plot: (group, name).
    pub fn columns(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = vec![];
        for n in ["Depth", "A_V", "N(H)"] {
            v.push(("Axis".into(), n.into()));
        }
        for c in self.overview.cols.iter().skip(1) {
            v.push(("Structure".into(), c.clone()));
        }
        for (el, t) in &self.elements {
            for c in t.cols.iter().skip(1) {
                v.push((format!("Ions {el}"), c.clone()));
            }
        }
        if let Some(t) = &self.emissivity {
            for c in t.cols.iter().skip(1) {
                v.push(("Line emissivity".into(), c.trim().to_string()));
            }
        }
        if let Some(t) = &self.grains {
            for c in t.cols.iter().skip(1) {
                v.push(("Grain temperature".into(), c.trim().to_string()));
            }
        }
        for (g, a) in [("Heating", &self.heating), ("Cooling", &self.cooling)] {
            if let Some(a) = a {
                v.push((g.into(), "Total".into()));
                for (n, _) in &a.agents {
                    v.push((g.into(), n.trim().to_string()));
                }
            }
        }
        v
    }

    /// Values of a quantity versus depth.
    pub fn column(&self, group: &str, name: &str) -> Option<(Vec<f64>, Vec<f64>)> {
        let find = |t: &Table| -> Option<(Vec<f64>, Vec<f64>)> {
            let c = (1..t.cols.len()).find(|c| t.cols[*c].trim() == name.trim())?;
            Some((t.data[0].clone(), t.data[c].clone()))
        };
        match group {
            "Axis" => match name {
                "Depth" => Some((self.depth.clone(), self.depth.clone())),
                "A_V" => self.av.clone().map(|a| (self.depth.clone(), a)),
                "N(H)" => Some((self.depth.clone(), self.column.clone())),
                _ => None,
            },
            "Structure" => find(&self.overview),
            "Line emissivity" => self.emissivity_of(name),
            "Grain temperature" => find(self.grains.as_ref()?),
            "Heating" | "Cooling" => {
                let a = if group == "Heating" { self.heating.as_ref()? } else { self.cooling.as_ref()? };
                if name == "Total" {
                    return Some((a.depth.clone(), a.total.clone()));
                }
                a.agents.iter().find(|(n, _)| n.trim() == name.trim()).map(|(_, v)| (a.depth.clone(), v.clone()))
            }
            g if g.starts_with("Ions ") => {
                let el = &g[5..];
                find(&self.elements.iter().find(|(s, _)| s == el)?.1)
            }
            _ => None,
        }
    }

}

pub fn interp(x: &[f64], y: &[f64], at: f64) -> Option<f64> {
    if x.is_empty() {
        return None;
    }
    if at <= x[0] {
        return y.first().copied();
    }
    for i in 1..x.len().min(y.len()) {
        if at <= x[i] {
            let t = (at - x[i - 1]) / (x[i] - x[i - 1]).max(1e-300);
            return Some(y[i - 1] + t * (y[i] - y[i - 1]));
        }
    }
    y.last().copied()
}

/// Human friendly name for a CLOUDY line label ("O  3 5006.84A" -> "[O III] 5007").
/// Species (first 4 characters), wavelength value and unit of a CLOUDY line label.
pub fn line_key(s: &str) -> (String, f64, char) {
    let s = s.trim();
    let s = s.strip_suffix("air").or_else(|| s.strip_suffix("vacuum")).unwrap_or(s).trim();
    let (sp, rest) = if s.len() > 4 && s.is_char_boundary(4) { s.split_at(4) } else { (s, "") };
    let rest = rest.trim();
    let unit = rest.chars().last().filter(|c| c.is_ascii_alphabetic()).unwrap_or('A');
    let num = rest.trim_end_matches(|c: char| c.is_ascii_alphabetic()).parse::<f64>().unwrap_or(f64::NAN);
    (sp.trim().to_ascii_lowercase(), num, unit.to_ascii_lowercase())
}

/// Two labels name the same line ("Blnd 6300A air" == "Blnd 6300.00A").
pub fn same_line(a: &str, b: &str) -> bool {
    let (x, y) = (line_key(a), line_key(b));
    x.0 == y.0 && x.2 == y.2 && (x.1 - y.1).abs() <= 1e-4 * x.1.abs().max(1.0)
}

pub fn pretty_line(label: &str) -> String {
    for l in crate::model::default_lines() {
        if same_line(l.label, label) {
            return l.name.to_string();
        }
    }
    label.split_whitespace().collect::<Vec<_>>().join(" ")
}
