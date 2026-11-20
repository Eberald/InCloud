//! Figure descriptions built from results. The same description is drawn on screen
//! (egui_plot) and exported (SVG -> PNG / PDF).
//!
//! Labels use a tiny markup: `_{...}` subscript, `^{...}` superscript.

use crate::results::{interp, line_key, pretty_line, pretty_reaction, same_line, BptRun, Results};

pub use crate::theme::{pal, role, Rgb, Role, PALETTE_LEN};
use crate::theme;

/// The name, in the legend of the SED, of the saved lines drawn as spikes.
pub const SED_LINES: &str = "Saved lines";

/// The height of a curve at `at`, drawn straight between its points on logarithmic axes.
/// `curve` holds (ln x, ln y) with x rising.
fn curve_at(curve: &[(f64, f64)], at: f64) -> Option<f64> {
    let (first, last) = (curve.first()?, curve.last()?);
    let x = at.ln();
    let k = curve.partition_point(|p| p.0 < x);
    let y = if k == 0 {
        first.1
    } else if k == curve.len() {
        last.1
    } else {
        let (a, b) = (curve[k - 1], curve[k]);
        a.1 + (x - a.0) / (b.0 - a.0).max(1e-300) * (b.1 - a.1)
    };
    Some(y.exp())
}

/// The resolving power ν/Δν of the continuum cells around `at` (curve in ln x, sorted): CLOUDY
/// puts a line of flux F into its cell as νF_ν = F ν/Δν, which is how high it stands on a νF_ν plot.
fn power_at(curve: &[(f64, f64)], at: f64) -> Option<f64> {
    let x = at.ln();
    let k = curve.partition_point(|p| p.0 < x);
    let a = curve.get(k.saturating_sub(2)..(k + 2).min(curve.len()))?;
    if a.len() < 2 {
        return None;
    }
    let step = (a[a.len() - 1].0 - a[0].0).abs() / (a.len() - 1) as f64;
    (step > 0.0).then(|| 1.0 / step)
}

/// Fill the cells without a value, evenly on a logarithmic scale between their neighbours.
fn fill_gaps(y: &mut [f64]) {
    let n = y.len();
    let mut i = 0;
    while i < n {
        if y[i].is_finite() {
            i += 1;
            continue;
        }
        let mut end = i;
        while end < n && !y[end].is_finite() {
            end += 1;
        }
        let left = i.checked_sub(1).map(|k| y[k]);
        let right = y.get(end).copied();
        for k in i..end {
            y[k] = match (left, right) {
                (Some(a), Some(b)) if a > 0.0 && b > 0.0 => {
                    let t = (k + 1 - i) as f64 / (end + 1 - i) as f64;
                    (a.ln() + t * (b.ln() - a.ln())).exp()
                }
                (Some(a), _) => a,
                (_, Some(b)) => b,
                _ => 0.0,
            };
        }
        i = end;
    }
}


#[derive(Clone, Debug)]
pub struct Series {
    pub name: String,
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub color: Rgb,
    pub dashed: bool,
    pub width: f32,
    pub legend: bool,
    /// draw markers instead of (or on top of) a line
    pub markers: bool,
    pub line: bool,
    /// drawn in the text colour of the theme (black on paper, light in dark mode)
    pub ink: bool,
    /// the name written at the top of a spike, only where it fits right beside it
    pub tip: String,
}

/// A word written at a data position (e.g. "Seyfert" on a BPT diagram).
#[derive(Clone, Debug)]
pub struct Note {
    pub x: f64,
    pub y: f64,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct Region {
    pub x0: f64,
    pub x1: f64,
    pub color: Rgb,
    pub label: String,
}

#[derive(Clone, Debug)]
pub struct Mark {
    pub x: f64,
    pub label: String,
    pub color: Rgb,
}

#[derive(Clone, Debug)]
pub struct Bars {
    pub labels: Vec<String>,
    pub values: Vec<f64>,
    pub colors: Vec<Rgb>,
}

#[derive(Clone, Debug)]
pub struct Figure {
    pub key: String,
    pub title: String,
    pub xlabel: String,
    pub ylabel: String,
    pub xlog: bool,
    pub ylog: bool,
    pub series: Vec<Series>,
    pub regions: Vec<Region>,
    pub marks: Vec<Mark>,
    pub bars: Option<Bars>,
    pub y_floor: Option<f64>,
    pub y_ceil: Option<f64>,
    /// fixed axis ranges (data units), e.g. for the BPT diagram
    pub x_range: Option<(f64, f64)>,
    pub y_range: Option<(f64, f64)>,
    pub note: Option<String>,
    pub labels: Vec<Note>,
}

impl Figure {
    pub fn new(key: &str, title: &str, xlabel: &str, ylabel: &str, xlog: bool, ylog: bool) -> Self {
        Figure {
            key: key.into(),
            title: title.into(),
            xlabel: xlabel.into(),
            ylabel: ylabel.into(),
            xlog,
            ylog,
            series: vec![],
            regions: vec![],
            marks: vec![],
            bars: None,
            y_floor: None,
            y_ceil: None,
            x_range: None,
            y_range: None,
            note: None,
            labels: vec![],
        }
    }
    pub fn line(&mut self, name: &str, x: Vec<f64>, y: Vec<f64>, color: Rgb) -> &mut Series {
        self.series.push(Series { name: name.into(), x, y, color, dashed: false, width: 2.0, legend: true, markers: false, line: true, ink: false, tip: String::new() });
        self.series.last_mut().unwrap()
    }
    pub fn points(&mut self, name: &str, x: Vec<f64>, y: Vec<f64>, color: Rgb) -> &mut Series {
        self.series.push(Series { name: name.into(), x, y, color, dashed: false, width: 2.0, legend: true, markers: true, line: false, ink: false, tip: String::new() });
        self.series.last_mut().unwrap()
    }
    pub fn is_empty(&self) -> bool {
        self.bars.as_ref().map(|b| b.values.is_empty()).unwrap_or(true) && self.series.iter().all(|s| s.x.is_empty())
    }
}

/// Horizontal axis of the continuum plot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ContUnit {
    Micron,
    Angstrom,
    Hz,
    Ev,
    Ryd,
}

impl ContUnit {
    pub const ALL: [ContUnit; 5] = [ContUnit::Micron, ContUnit::Angstrom, ContUnit::Hz, ContUnit::Ev, ContUnit::Ryd];
    pub fn label(self) -> &'static str {
        match self {
            ContUnit::Micron => "Wavelength [µm]",
            ContUnit::Angstrom => "Wavelength [Å]",
            ContUnit::Hz => "Frequency [Hz]",
            ContUnit::Ev => "Energy [eV]",
            ContUnit::Ryd => "Energy [Ryd]",
        }
    }
    fn axis(self) -> &'static str {
        match self {
            ContUnit::Micron => "\u{03bb} [\u{00b5}m]",
            ContUnit::Angstrom => "\u{03bb} [\u{00c5}]",
            ContUnit::Hz => "\u{03bd} [Hz]",
            ContUnit::Ev => "E [eV]",
            ContUnit::Ryd => "E [Ryd]",
        }
    }
    /// Convert a wavelength in micron to this unit.
    fn from_micron(self, l: f64) -> f64 {
        match self {
            ContUnit::Micron => l,
            ContUnit::Angstrom => l * 1e4,
            ContUnit::Hz => 2.99792458e14 / l,
            ContUnit::Ev => 1.23984198 / l,
            ContUnit::Ryd => 0.0911267 / l,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XAxis {
    DepthLog,
    DepthLin,
    Av,
    Column,
}

impl XAxis {
    pub const ALL: [XAxis; 4] = [XAxis::DepthLog, XAxis::DepthLin, XAxis::Av, XAxis::Column];
    pub fn label(self) -> &'static str {
        match self {
            XAxis::DepthLog => "Depth (log)",
            XAxis::DepthLin => "Depth (linear)",
            XAxis::Av => "Extinction A_V",
            XAxis::Column => "Column density N(H)",
        }
    }
}

/// A plot the person can show. Custom plots are numbered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Plot {
    Thermal,
    Density,
    Structure,
    Molecules,
    Ions,
    Lines,
    Emissivity,
    Cumulative,
    Ratios,
    Bpt,
    Continuum,
    Grains,
    HeatCool,
    Rates,
    Network,
    Custom(usize),
}

impl Plot {
    pub const BUILTIN: [Plot; 15] = [
        Plot::Thermal,
        Plot::Structure,
        Plot::Density,
        Plot::Molecules,
        Plot::Ions,
        Plot::Lines,
        Plot::Emissivity,
        Plot::Cumulative,
        Plot::Ratios,
        Plot::Bpt,
        Plot::Continuum,
        Plot::Grains,
        Plot::HeatCool,
        Plot::Rates,
        Plot::Network,
    ];
    pub const DEFAULT: [Plot; 4] = [Plot::Thermal, Plot::Structure, Plot::Lines, Plot::Continuum];

    pub fn label(self, customs: &[CustomSpec]) -> String {
        match self {
            Plot::Thermal => "Thermal structure".into(),
            Plot::Density => "Density".into(),
            Plot::Structure => "H II / PDR / fronts".into(),
            Plot::Molecules => "Molecular fractions".into(),
            Plot::Ions => "Ionisation (element)".into(),
            Plot::Lines => "Line intensities".into(),
            Plot::Emissivity => "Line emissivity".into(),
            Plot::Cumulative => "Where lines form".into(),
            Plot::Ratios => "Line ratios".into(),
            Plot::Bpt => "BPT diagram".into(),
            Plot::Continuum => "Continuum (SED)".into(),
            Plot::Grains => "Grain temperature".into(),
            Plot::HeatCool => "Heating / cooling".into(),
            Plot::Rates => "Reaction rates".into(),
            Plot::Network => "Chemical network".into(),
            Plot::Custom(i) => customs.get(i).map(|c| if c.title.is_empty() { format!("Custom plot {}", i + 1) } else { c.title.clone() }).unwrap_or_else(|| "Custom".into()),
        }
    }
    /// The wiki page about this plot.
    pub fn wiki_key(self) -> &'static str {
        match self {
            Plot::Thermal => "p_thermal",
            Plot::Density => "p_density",
            Plot::Structure => "p_structure",
            Plot::Molecules => "p_molecules",
            Plot::Ions => "p_ions",
            Plot::Lines => "p_lines",
            Plot::Emissivity => "p_emissivity",
            Plot::Cumulative => "p_cumulative",
            Plot::Ratios => "p_ratios",
            Plot::Bpt => "p_bpt",
            Plot::Continuum => "p_continuum",
            Plot::Grains => "p_grains",
            Plot::HeatCool => "p_heatcool",
            Plot::Rates => "p_rates",
            Plot::Network => "p_network",
            Plot::Custom(_) => "p_custom",
        }
    }
    /// Plots drawn against depth, A_V or column density.
    pub fn uses_depth(self) -> bool {
        matches!(
            self,
            Plot::Thermal | Plot::Density | Plot::Structure | Plot::Molecules | Plot::Ions | Plot::Emissivity | Plot::Cumulative | Plot::Ratios | Plot::Grains | Plot::HeatCool | Plot::Rates
        )
    }
    pub fn key(self) -> String {
        match self {
            Plot::Custom(i) => format!("custom{}", i + 1),
            other => format!("{other:?}").to_ascii_lowercase(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RatioSpec {
    pub num: String,
    pub den: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CustomSpec {
    pub title: String,
    /// (group, name)
    pub x: (String, String),
    pub ys: Vec<(String, String)>,
    pub xlog: bool,
    pub ylog: bool,
}

impl Default for CustomSpec {
    fn default() -> Self {
        CustomSpec {
            title: String::new(),
            x: ("Axis".into(), "Depth".into()),
            ys: vec![("Structure".into(), "Te".into())],
            xlog: true,
            ylog: true,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BptKind {
    Nii,
    Sii,
    Oi,
}

impl BptKind {
    pub const ALL: [BptKind; 3] = [BptKind::Nii, BptKind::Sii, BptKind::Oi];
    pub fn label(self) -> &'static str {
        match self {
            BptKind::Nii => "[N II]/Hα (classic BPT)",
            BptKind::Sii => "[S II]/Hα",
            BptKind::Oi => "[O I]/Hα",
        }
    }
}

pub const HB: &str = "H  1 4861.32A";
pub const HA: &str = "H  1 6562.80A";
pub const O3: &str = "O  3 5006.84A";
pub const N2: &str = "N  2 6583.45A";
pub const S2A: &str = "S  2 6716.44A";
pub const S2B: &str = "S  2 6730.82A";
pub const O1: &str = "Blnd 6300A";
pub const O1_OLD: &str = "O  1 6300.30A";

pub fn default_ratios() -> Vec<RatioSpec> {
    vec![
        RatioSpec { num: O3.into(), den: HB.into() },
        RatioSpec { num: N2.into(), den: HA.into() },
        RatioSpec { num: S2A.into(), den: S2B.into() },
        RatioSpec { num: "O  3 4363.21A".into(), den: O3.into() },
    ]
}

/// The point of one model on the BPT diagram: (x, y) = log ratios.
pub fn bpt_point(r: &Results, kind: BptKind) -> Option<(f64, f64)> {
    bpt_from(&|l: &str| r.line(l).map(|x| x.intensity), kind)
}

/// The point of a run known only by its line intensities.
pub fn bpt_point_run(r: &BptRun, kind: BptKind) -> Option<(f64, f64)> {
    bpt_from(&|l: &str| r.lines.iter().find(|(k, _)| same_line(k, l)).map(|(_, v)| *v), kind)
}

fn bpt_from(get: &dyn Fn(&str) -> Option<f64>, kind: BptKind) -> Option<(f64, f64)> {
    let i = |l: &str| get(l).filter(|v| *v > 0.0);
    let y = (i(O3)? / i(HB)?).log10();
    let x = match kind {
        BptKind::Nii => (i(N2)? / i(HA)?).log10(),
        BptKind::Sii => ((i(S2A)? + i(S2B)?) / i(HA)?).log10(),
        BptKind::Oi => (i(O1).or_else(|| i(O1_OLD))? / i(HA)?).log10(),
    };
    Some((x, y))
}

pub struct Ctx<'a> {
    pub r: &'a Results,
    pub xaxis: XAxis,
    pub element: &'a str,
    pub species: &'a str,
    pub show_regions: bool,
    pub ratios: &'a [RatioSpec],
    pub customs: &'a [CustomSpec],
    pub bpt: BptKind,
    /// other runs shown on the BPT diagram
    pub others: &'a [BptRun],
    pub cont_unit: ContUnit,
    /// draw the saved lines on the SED
    pub sed_lines: bool,
}

impl<'a> Ctx<'a> {
    fn x_of_depth(&self, d: f64) -> Option<f64> {
        match self.xaxis {
            XAxis::DepthLog | XAxis::DepthLin => Some(d),
            XAxis::Av => self.r.av_at(d),
            XAxis::Column => interp(&self.r.depth, &self.r.column, d),
        }
    }
    fn xs(&self, depth: &[f64]) -> Vec<f64> {
        depth.iter().map(|d| self.x_of_depth(*d).unwrap_or(f64::NAN)).collect()
    }
    fn xlabel(&self) -> &'static str {
        match self.xaxis {
            XAxis::DepthLog | XAxis::DepthLin => "Depth [cm]",
            XAxis::Av => {
                if self.r.av_estimated {
                    "A_{V} [mag] (from N_{H})"
                } else {
                    "A_{V} [mag]"
                }
            }
            XAxis::Column => "N(H) [cm^{-2}]",
        }
    }
    fn xlog(&self) -> bool {
        matches!(self.xaxis, XAxis::DepthLog | XAxis::Column)
    }
    fn depth_figure(&self, key: &str, title: &str, ylabel: &str, ylog: bool) -> Figure {
        let mut f = Figure::new(key, title, self.xlabel(), ylabel, self.xlog(), ylog);
        if self.show_regions {
            self.add_regions(&mut f);
        }
        f
    }
    fn add_regions(&self, f: &mut Figure) {
        let r = self.r;
        let (Some(first), Some(last)) = (r.depth.first(), r.depth.last()) else { return };
        let (first, last) = (*first, *last);
        let fr = &r.fronts;
        let x = |d: f64| self.x_of_depth(d);
        let mut push = |a: f64, b: f64, color: Rgb, label: &str| {
            if let (Some(xa), Some(xb)) = (x(a), x(b)) {
                if xb > xa {
                    f.regions.push(Region { x0: xa, x1: xb, color, label: label.into() });
                }
            }
        };
        match (fr.ionisation, fr.dissociation) {
            (Some(i), Some(dd)) => {
                push(first, i, role(Role::RegionHii), "H II");
                push(i, dd, role(Role::RegionPdr), "PDR");
                push(dd, last, role(Role::RegionMol), "H_{2}");
            }
            (Some(i), None) => {
                push(first, i, role(Role::RegionHii), "H II");
                push(i, last, role(Role::RegionPdr), "PDR");
            }
            (None, _) => {
                if r.x_hplus.first().copied().unwrap_or(0.0) > 0.5 {
                    push(first, last, role(Role::RegionHii), "H II");
                } else {
                    push(first, last, role(Role::RegionPdr), "PDR");
                }
            }
        }
        if let Some(i) = fr.ionisation.and_then(x) {
            f.marks.push(Mark { x: i, label: "IF".into(), color: role(Role::FrontIf) });
        }
        if let Some(d) = fr.dissociation.and_then(x) {
            f.marks.push(Mark { x: d, label: "DF".into(), color: role(Role::FrontDf) });
        }
        if let Some(c) = fr.co.and_then(x) {
            f.marks.push(Mark { x: c, label: "C/CO".into(), color: role(Role::FrontCo) });
        }
    }

    pub fn figure(&self, p: Plot) -> Figure {
        let mut f = match p {
            Plot::Thermal => self.thermal(),
            Plot::Density => self.density(),
            Plot::Structure => self.structure(),
            Plot::Molecules => self.molecules(),
            Plot::Ions => self.ions(),
            Plot::Lines => self.lines(),
            Plot::Emissivity => self.emissivity(),
            Plot::Cumulative => self.cumulative(),
            Plot::Ratios => self.ratios(),
            Plot::Bpt => self.bpt(),
            Plot::Continuum => self.continuum(),
            Plot::Grains => self.grains(),
            Plot::HeatCool => self.heatcool(),
            Plot::Rates => self.rates(),
            Plot::Network => Figure::new("network", "Chemical network", "", "", false, false),
            Plot::Custom(i) => self.custom(i),
        };
        f.key = p.key();
        let thick = theme::get().width;
        for s in &mut f.series {
            s.width *= thick;
        }
        f
    }

    fn thermal(&self) -> Figure {
        let r = self.r;
        let mut f = self.depth_figure("temperature", "Thermal structure", "T_{e} [K]", true);
        let s = f.line("T_{e}", self.xs(&r.depth), r.te.clone(), role(Role::Temperature));
        s.width = 2.6;
        s.legend = false;
        f
    }

    fn density(&self) -> Figure {
        let r = self.r;
        let mut f = self.depth_figure("density", "Density structure", "n [cm^{-3}]", true);
        let x = self.xs(&r.depth);
        f.line("n_{H}", x.clone(), r.hden.clone(), pal(0));
        f.line("n_{e}", x.clone(), r.eden.clone(), pal(1)).dashed = true;
        let nh2: Vec<f64> = r.hden.iter().zip(&r.x_h2).map(|(n, x)| 0.5 * n * x).collect();
        if nh2.iter().any(|v| *v > 0.0) {
            f.line("n(H_{2})", x, nh2, pal(2));
        }
        f.y_floor = Some(r.hden.iter().cloned().fold(f64::MAX, f64::min) * 1e-5);
        f
    }

    fn structure(&self) -> Figure {
        let r = self.r;
        let mut f = self.depth_figure("structure", "Ionisation and dissociation structure", "Fraction", true);
        let x = self.xs(&r.depth);
        f.line("H^{+}", x.clone(), r.x_hplus.clone(), role(Role::HPlus)).width = 2.4;
        f.line("H^{0}", x.clone(), r.x_h0.clone(), role(Role::H0)).width = 2.4;
        f.line("2H_{2}/H", x.clone(), r.x_h2.clone(), role(Role::H2)).width = 2.4;
        if let Some(co) = &r.co_c {
            f.line("CO/C", x.clone(), co.clone(), role(Role::CoC)).dashed = true;
        }
        if let Some((_, he)) = r.elements.iter().find(|(s, _)| s == "He") {
            if let Some(i) = he.find(&["He+"]) {
                f.line("He^{+}", self.xs(&he.data[0]), he.data[i].clone(), role(Role::HePlus)).dashed = true;
            }
        }
        f.y_floor = Some(1e-4);
        f.y_ceil = Some(1.5);
        f
    }

    fn molecules(&self) -> Figure {
        let r = self.r;
        let mut f = self.depth_figure("molecules", "Molecular fractions", "Fraction", true);
        let x = self.xs(&r.overview.data.first().cloned().unwrap_or_default());
        let mut k = 0;
        for name in ["2H_2/H", "CO/C", "H2O/O"] {
            if let Some(c) = r.overview.find(&[name]) {
                let label = match name {
                    "2H_2/H" => "2H_{2}/H",
                    "H2O/O" => "H_{2}O/O",
                    n => n,
                };
                f.line(label, x.clone(), r.overview.data[c].clone(), pal(k)).width = 2.4;
                k += 1;
            }
        }
        f.y_floor = Some(1e-8);
        f.y_ceil = Some(2.0);
        f
    }

    fn ions(&self) -> Figure {
        let r = self.r;
        let el = self.element;
        let mut f = self.depth_figure("ions", &format!("Ionisation structure of {el}"), "Fraction", true);
        if let Some((_, t)) = r.elements.iter().find(|(s, _)| s == el) {
            let x = self.xs(&t.data[0]);
            let mut k = 0;
            for c in 1..t.cols.len() {
                let maxv = t.data[c].iter().cloned().filter(|v| v.is_finite()).fold(0.0, f64::max);
                if maxv < 1e-3 {
                    continue;
                }
                let s = f.line(&ion_markup(&t.cols[c]), x.clone(), t.data[c].clone(), pal(k % PALETTE_LEN));
                s.width = 2.3;
                s.dashed = k >= 6;
                k += 1;
            }
        } else {
            f.note = Some(format!("No file for element {el}"));
        }
        f.y_floor = Some(1e-4);
        f.y_ceil = Some(1.5);
        f
    }

    fn lines(&self) -> Figure {
        let r = self.r;
        let reference = if r.line_ref == "Hβ" { "H\u{03b2}".to_string() } else { "strongest line".into() };
        let mut f = Figure::new("lines", "Emission-line intensities", &format!("I / I({reference})"), "", true, false);
        let (mut labels, mut values, mut colors) = (vec![], vec![], vec![]);
        for l in &r.lines {
            if l.relative > 0.0 && l.relative.is_finite() {
                labels.push(pretty_line(&l.label));
                values.push(l.relative);
                let wl = crate::results::line_key(&l.label);
                let c = if wl.2 == 'm' {
                    if wl.0.starts_with("h2") || wl.0.starts_with("co") { pal(2) } else { pal(1) }
                } else {
                    pal(0)
                };
                colors.push(c);
            }
        }
        if labels.is_empty() {
            f.note = Some("No lines saved, or all are zero".into());
        }
        f.bars = Some(crate::figures::Bars { labels, values, colors });
        f
    }

    fn emissivity(&self) -> Figure {
        let r = self.r;
        let mut f = self.depth_figure("emissivity", "Line emissivity (normalised to peak)", "4\u{03c0}j / max", true);
        if let Some(t) = &r.emissivity {
            let x = self.xs(&t.data[0]);
            for c in 1..t.cols.len() {
                let m = t.data[c].iter().cloned().filter(|v| v.is_finite()).fold(0.0, f64::max);
                if m <= 0.0 {
                    continue;
                }
                let k = c - 1;
                let s = f.line(&pretty_line(&t.cols[c]), x.clone(), t.data[c].iter().map(|v| v / m).collect(), pal(k % PALETTE_LEN));
                s.width = 1.8;
                s.dashed = k >= PALETTE_LEN;
            }
        }
        f.y_floor = Some(1e-4);
        f.y_ceil = Some(2.0);
        f
    }

    fn cumulative(&self) -> Figure {
        let r = self.r;
        let mut f = self.depth_figure("cumulative", "Where the lines form (cumulative fraction of emission)", "Fraction of total intensity", false);
        if let Some(t) = &r.emissivity {
            for c in 1..t.cols.len() {
                if let Some((d, cum)) = r.cumulative_of(&t.cols[c]) {
                    let tot = cum.last().copied().unwrap_or(0.0);
                    if tot <= 0.0 {
                        continue;
                    }
                    let k = c - 1;
                    let s = f.line(&pretty_line(&t.cols[c]), self.xs(&d), cum.iter().map(|v| v / tot).collect(), pal(k % PALETTE_LEN));
                    s.width = 1.8;
                    s.dashed = k >= PALETTE_LEN;
                }
            }
        }
        f.y_floor = Some(0.0);
        f.y_ceil = Some(1.05);
        f
    }

    fn ratios(&self) -> Figure {
        let r = self.r;
        let mut f = self.depth_figure("ratios", "Line ratios (solid: local, dashed: integrated to depth)", "Ratio", true);
        let mut notes = vec![];
        for (k, spec) in self.ratios.iter().enumerate() {
            let name = format!("{}/{}", pretty_line(&spec.num), pretty_line(&spec.den));
            let color = pal(k % PALETTE_LEN);
            let (Some((d, en)), Some((_, ed))) = (r.emissivity_of(&spec.num), r.emissivity_of(&spec.den)) else {
                notes.push(format!("{name}: line not saved"));
                continue;
            };
            let total = match (r.line(&spec.num), r.line(&spec.den)) {
                (Some(a), Some(b)) if b.intensity > 0.0 => Some(a.intensity / b.intensity),
                _ => None,
            };
            let label = match total {
                Some(t) => format!("{name} = {}", fmt_val(t)),
                None => name.clone(),
            };
            let local: Vec<f64> = en.iter().zip(&ed).map(|(a, b)| if *b > 0.0 { a / b } else { f64::NAN }).collect();
            let x = self.xs(&d);
            f.line(&label, x.clone(), local, color).width = 2.2;
            if let (Some((_, cn)), Some((_, cd))) = (r.cumulative_of(&spec.num), r.cumulative_of(&spec.den)) {
                let cum: Vec<f64> = cn.iter().zip(&cd).map(|(a, b)| if *b > 0.0 { a / b } else { f64::NAN }).collect();
                let s = f.line("", x, cum, color);
                s.dashed = true;
                s.legend = false;
                s.width = 1.6;
            }
        }
        if self.ratios.is_empty() {
            notes.push("Add ratios in the plot settings".into());
        }
        if !notes.is_empty() {
            f.note = Some(notes.join(" · "));
        }
        f
    }

    fn bpt(&self) -> Figure {
        // demarcation lines: y = a / (x - b) + c, and the Seyfert / LINER division y = m x + q
        type Curve = (&'static str, f64, f64, f64, bool);
        struct Set {
            xl: &'static str,
            xr: (f64, f64),
            curves: Vec<Curve>,
            split: (&'static str, f64, f64),
            words: Vec<(f64, f64, &'static str)>,
        }
        let set = match self.bpt {
            BptKind::Nii => Set {
                xl: "log([N II] 6584 / H\u{03b1})",
                xr: (-2.0, 1.0),
                curves: vec![("Kewley+01 (maximum starburst)", 0.61, 0.47, 1.19, false), ("Kauffmann+03 (pure star formation)", 0.61, 0.05, 1.30, true)],
                split: ("Schawinski+07 (Seyfert / LINER)", 1.05, 0.45),
                words: vec![(-1.35, -0.75, "Star-forming"), (-0.12, -1.15, "Comp."), (-0.55, 1.2, "Seyfert"), (0.55, 0.05, "LINER")],
            },
            BptKind::Sii => Set {
                xl: "log([S II] 6716+6731 / H\u{03b1})",
                xr: (-1.5, 1.0),
                curves: vec![("Kewley+01 (maximum starburst)", 0.72, 0.32, 1.30, false)],
                split: ("Kewley+06 (Seyfert / LINER)", 1.89, 0.76),
                words: vec![(-0.95, -0.75, "Star-forming"), (-0.55, 1.25, "Seyfert"), (0.55, 0.05, "LINER")],
            },
            BptKind::Oi => Set {
                xl: "log([O I] 6300 / H\u{03b1})",
                xr: (-2.5, 0.5),
                curves: vec![("Kewley+01 (maximum starburst)", 0.73, -0.59, 1.33, false)],
                split: ("Kewley+06 (Seyfert / LINER)", 1.18, 1.30),
                words: vec![(-1.9, -0.75, "Star-forming"), (-1.25, 1.25, "Seyfert"), (0.0, -0.35, "LINER")],
            },
        };
        let (ymin, ymax) = (-1.5, 1.5);
        let mut f = Figure::new("bpt", "BPT diagram", set.xl, "log([O III] 5007 / H\u{03b2})", false, false);
        f.x_range = Some(set.xr);
        f.y_range = Some((ymin, ymax));
        for (name, a, b, c, dashed) in &set.curves {
            let xend = (*b - 1e-3).min(set.xr.1);
            let (mut xs, mut ys) = (vec![], vec![]);
            for i in 0..400 {
                let x = set.xr.0 + (xend - set.xr.0) * i as f64 / 399.0;
                let y = a / (x - b) + c;
                if y >= ymin {
                    xs.push(x);
                    ys.push(y.min(ymax));
                }
            }
            let s = f.line(name, xs, ys, [20, 20, 20]);
            s.ink = true;
            s.dashed = *dashed;
            s.width = 1.8;
        }
        // the Seyfert / LINER line starts on the maximum-starburst curve
        {
            let (a, b, c) = (set.curves[0].1, set.curves[0].2, set.curves[0].3);
            let (name, m, q) = set.split;
            let gap = |x: f64| m * x + q - (a / (x - b) + c);
            let (mut lo, mut hi) = (set.xr.0, b - 1e-4);
            for _ in 0..80 {
                let mid = 0.5 * (lo + hi);
                if gap(mid) > 0.0 {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            let x0 = 0.5 * (lo + hi);
            let x1 = ((ymax - q) / m).min(set.xr.1);
            let s = f.line(name, vec![x0, x1], vec![m * x0 + q, m * x1 + q], [20, 20, 20]);
            s.ink = true;
            s.dashed = true;
            s.width = 1.4;
        }
        for (x, y, w) in &set.words {
            f.labels.push(crate::figures::Note { x: *x, y: *y, text: (*w).to_string() });
        }
        // other runs chosen for this diagram
        let shown: Vec<(&BptRun, (f64, f64))> = self.others.iter().filter(|o| o.visible).filter_map(|o| bpt_point_run(o, self.bpt).map(|p| (o, p))).collect();
        if shown.len() <= 8 {
            for (k, (o, (x, y))) in shown.iter().enumerate() {
                let c = [pal(0), pal(2), pal(3), pal(4), pal(5), pal(7), pal(8), pal(9)][k % 8];
                f.points(&o.name, vec![*x], vec![*y], c);
            }
        } else {
            let (ox, oy): (Vec<f64>, Vec<f64>) = shown.iter().map(|(_, p)| *p).unzip();
            f.points(&format!("Other runs ({})", ox.len()), ox, oy, pal(0));
        }
        // track of this model: ratios integrated up to each depth
        let r = self.r;
        let num_x: Vec<&str> = match self.bpt {
            BptKind::Nii => vec![N2],
            BptKind::Sii => vec![S2A, S2B],
            BptKind::Oi => vec![O1, O1_OLD],
        };
        let cum = |l: &str| r.cumulative_of(l);
        if let (Some((_, ch_b)), Some((_, ch_a)), Some((_, co3))) = (cum(HB), cum(HA), cum(O3)) {
            let mut nx: Option<Vec<f64>> = None;
            for l in &num_x {
                if let Some((_, c)) = cum(l) {
                    nx = Some(match nx {
                        Some(prev) => prev.iter().zip(&c).map(|(a, b)| a + b).collect(),
                        None => c,
                    });
                    if self.bpt == BptKind::Oi {
                        break;
                    }
                }
            }
            if let Some(nx) = nx {
                let n = ch_b.len().min(ch_a.len()).min(co3.len()).min(nx.len());
                let (mut tx, mut ty) = (vec![], vec![]);
                for i in 0..n {
                    if ch_b[i] > 0.0 && ch_a[i] > 0.0 && co3[i] > 0.0 && nx[i] > 0.0 {
                        tx.push((nx[i] / ch_a[i]).log10());
                        ty.push((co3[i] / ch_b[i]).log10());
                    }
                }
                if tx.len() > 1 {
                    let s = f.line("Integrated to depth", tx, ty, pal(5));
                    s.width = 1.4;
                }
            }
        }
        match bpt_point(r, self.bpt) {
            Some((x, y)) => {
                let s = f.points(&format!("This model ({}, {})", fmt_val(x), fmt_val(y)), vec![x], vec![y], pal(1));
                s.width = 3.0;
            }
            None => f.note = Some("Needs Hβ, [O III] 5007, Hα and the x-axis line(s) among the saved lines".into()),
        }
        f
    }

    fn continuum(&self) -> Figure {
        let r = self.r;
        let mut f = Figure::new("continuum", "Spectral energy distribution", self.cont_unit.axis(), "\u{03bd}F_{\u{03bd}} [erg cm^{-2} s^{-1}]", true, true);
        if let Some(t) = &r.continuum {
            let x: Vec<f64> = t.data[0].iter().map(|l| self.cont_unit.from_micron(*l)).collect();
            let pick = [
                (&["incident", "inci"][..], "Incident", role(Role::Incident), true),
                (&["trans"][..], "Transmitted", role(Role::Transmitted), false),
                (&["DiffOut", "diffuse"][..], "Diffuse (outward)", role(Role::Diffuse), false),
                (&["total"][..], "Total", role(Role::Total), false),
            ];
            let mut maxv: f64 = 0.0;
            // CLOUDY folds its lines into these curves: with the lines drawn apart, only the continuum stays
            let (reflected, outward) = (t.find(&["reflin"]), t.find(&["outlin"]));
            let mut base: Option<Vec<f64>> = None;
            for (names, label, color, dashed) in pick {
                if let Some(i) = t.find(names) {
                    if i == 0 {
                        continue;
                    }
                    let mut y = t.data[i].clone();
                    let parts: Vec<usize> = match label {
                        "Total" if self.sed_lines => [reflected, outward].into_iter().flatten().collect(),
                        "Diffuse (outward)" if self.sed_lines => outward.into_iter().collect(),
                        _ => vec![],
                    };
                    if !parts.is_empty() {
                        for (k, v) in y.iter_mut().enumerate() {
                            let line: f64 = parts.iter().map(|c| t.data[*c][k]).filter(|l| l.is_finite()).sum();
                            // a cell that is mostly line says little about the continuum under it
                            *v = if line > 0.2 * *v { f64::NAN } else { *v - line };
                        }
                        fill_gaps(&mut y);
                    }
                    if label == "Total" {
                        base = Some(y.clone());
                    }
                    maxv = maxv.max(y.iter().cloned().filter(|v| v.is_finite()).fold(0.0, f64::max));
                    let name = if parts.is_empty() { label.to_string() } else { format!("{} (lines removed)", label.split(' ').next().unwrap_or(label)) };
                    let s = f.line(&name, x.clone(), y, color);
                    s.dashed = dashed;
                    s.width = if label == "Total" { 2.0 } else { 1.5 };
                }
            }
            if maxv > 0.0 {
                f.y_floor = Some(maxv * 1e-7);
                f.y_ceil = Some(maxv * 5.0);
            }
            if let (true, Some(floor)) = (self.sed_lines, f.y_floor) {
                let lo = x.iter().cloned().fold(f64::MAX, f64::min);
                let hi = x.iter().cloned().fold(f64::MIN, f64::max);
                let mut lines: Vec<(f64, f64, String)> = r
                    .lines
                    .iter()
                    .filter_map(|l| {
                        let (_, w, unit) = line_key(&l.label);
                        let micron = match unit {
                            'm' => w,
                            'c' => w * 1e4,
                            _ => w * 1e-4,
                        };
                        let at = self.cont_unit.from_micron(micron);
                        (micron > 0.0 && at >= lo && at <= hi && l.intensity > 0.0).then(|| (at, l.intensity, pretty_line(&l.label)))
                    })
                    .collect();
                // the brightest first: they get their names when there is no room for all
                lines.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                let saved = lines.len();
                let mut shown = 0;
                if let Some(total) = &base {
                    // the total spectrum exactly as it is drawn, so that every line stands on the curve
                    let mut curve: Vec<(f64, f64)> = x.iter().zip(total).filter(|(a, b)| **a > 0.0 && **b > 0.0 && b.is_finite()).map(|(a, b)| (a.ln(), b.max(floor).ln())).collect();
                    curve.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                    let least = theme::get().line_min;
                    for (at, v, name) in lines {
                        let Some(under) = curve_at(&curve, at) else { continue };
                        // the flux spread over one continuum cell, as CLOUDY adds it to its total
                        let v = v * power_at(&curve, at).unwrap_or(1.0);
                        if v < least * under {
                            continue;
                        }
                        let s = f.line(SED_LINES, vec![at, at], vec![under, under + v], role(Role::SavedLines));
                        s.width = 1.4;
                        s.legend = shown == 0;
                        s.tip = name;
                        shown += 1;
                    }
                }
                let faint = if saved > shown { format!(" {shown} of {saved} rise above the continuum.") } else { String::new() };
                f.note = Some(format!("CLOUDY's own lines are taken out of the total and diffuse spectra; only the saved lines are drawn, as spikes as high as CLOUDY makes them in its own total spectrum (line flux × ν/Δν of the continuum cell).{faint}"));
            }
        }
        f
    }

    fn grains(&self) -> Figure {
        let r = self.r;
        let mut f = self.depth_figure("grains", "Grain temperature", "T_{gr} [K]", true);
        if let Some(t) = &r.grains {
            let x = self.xs(&t.data[0]);
            let mut groups: Vec<(String, Vec<usize>)> = vec![];
            for c in 1..t.cols.len() {
                let name = t.cols[c].trim().to_string();
                let key: String = name.split(|ch: char| ch.is_ascii_digit()).next().unwrap_or("").trim_end_matches(['-', '_', ' ']).to_string();
                let key = if key.is_empty() { name.clone() } else { key };
                match groups.iter_mut().find(|g| g.0 == key) {
                    Some(g) => g.1.push(c),
                    None => groups.push((key, vec![c])),
                }
            }
            for (k, (gname, cols)) in groups.iter().enumerate() {
                let color = pal(k % PALETTE_LEN);
                for c in cols {
                    let s = f.line("", x.clone(), t.data[*c].clone(), color);
                    s.width = 0.8;
                    s.legend = false;
                }
                let n = t.rows();
                let mean: Vec<f64> = (0..n).map(|i| cols.iter().map(|c| t.data[*c][i]).sum::<f64>() / cols.len() as f64).collect();
                f.line(&format!("{} (mean)", material_name(gname)), x.clone(), mean, color).width = 2.6;
            }
        }
        f
    }

    fn heatcool(&self) -> Figure {
        let r = self.r;
        let mut f = self.depth_figure("heating_cooling", "Heating and cooling", "Rate [erg cm^{-3} s^{-1}]", true);
        let mut maxv: f64 = 0.0;
        if let Some(h) = &r.heating {
            let x = self.xs(&h.depth);
            maxv = maxv.max(h.total.iter().cloned().fold(0.0, f64::max));
            f.line("Total heating", x.clone(), h.total.clone(), pal(6)).width = 2.6;
            for (k, (name, v)) in h.agents.iter().take(4).enumerate() {
                f.line(&format!("H: {}", name.trim()), x.clone(), v.clone(), pal((k + 1) % PALETTE_LEN)).width = 1.6;
            }
        }
        if let Some(c) = &r.cooling {
            let x = self.xs(&c.depth);
            for (k, (name, v)) in c.agents.iter().take(4).enumerate() {
                let s = f.line(&format!("C: {}", name.trim()), x.clone(), v.clone(), pal((k + 5) % PALETTE_LEN));
                s.width = 1.6;
                s.dashed = true;
            }
        }
        if maxv > 0.0 {
            f.y_floor = Some(maxv * 1e-6);
        }
        f
    }

    fn rates(&self) -> Figure {
        let r = self.r;
        let sp = self.species;
        let mut f = self.depth_figure("rates", &format!("Reaction rates of {}", markup_to_unicode(&ion_markup(sp))), "Rate [cm^{-3} s^{-1}]", true);
        let Some(c) = r.chemistry.iter().find(|c| c.species == sp) else {
            f.note = Some(if r.chemistry.is_empty() { "No reaction rates saved for this run (turn on Chemistry before running)".into() } else { format!("No rates saved for {sp}") });
            return f;
        };
        let (d0, d1) = (r.depth.first().copied().unwrap_or(0.0), r.depth.last().copied().unwrap_or(0.0));
        let mut maxv: f64 = 0.0;
        for (formation, dashed, offset) in [(true, false, 0usize), (false, true, 5usize)] {
            let t = if formation { c.formation.as_ref() } else { c.destruction.as_ref() };
            let Some(t) = t else { continue };
            let x = self.xs(&t.data[0]);
            for (k, sh) in c.shares(formation, d0, d1).iter().take(5).enumerate() {
                if let Some(col) = (1..t.cols.len()).find(|i| t.cols[*i].trim() == sh.reaction) {
                    maxv = maxv.max(t.data[col].iter().cloned().fold(0.0, f64::max));
                    let name = format!("{} {} ({:.0}%)", if formation { "+" } else { "−" }, pretty_reaction(&sh.reaction), sh.fraction * 100.0);
                    let s = f.line(&name, x.clone(), t.data[col].clone(), pal((k + offset) % PALETTE_LEN));
                    s.dashed = dashed;
                    s.width = 1.8;
                }
            }
        }
        if maxv > 0.0 {
            f.y_floor = Some(maxv * 1e-6);
        }
        f
    }

    fn custom(&self, i: usize) -> Figure {
        let r = self.r;
        let Some(spec) = self.customs.get(i) else { return Figure::new("custom", "Custom plot", "", "", false, false) };
        let title = if spec.title.is_empty() { format!("Custom plot {}", i + 1) } else { spec.title.clone() };
        let axis_x = spec.x.0 == "Axis";
        let xlabel = if axis_x {
            match spec.x.1.as_str() {
                "A_V" => "A_{V} [mag]".to_string(),
                "N(H)" => "N(H) [cm^{-2}]".to_string(),
                _ => "Depth [cm]".to_string(),
            }
        } else {
            format!("{} ({})", spec.x.1, spec.x.0)
        };
        let ylabel = if spec.ys.len() == 1 { format!("{} ({})", spec.ys[0].1, spec.ys[0].0) } else { "Value".into() };
        let mut f = Figure::new("custom", &title, &xlabel, &ylabel, spec.xlog, spec.ylog);
        let Some((dx, xv)) = r.column(&spec.x.0, &spec.x.1) else {
            f.note = Some(format!("{} / {} is not in this run", spec.x.0, spec.x.1));
            return f;
        };
        let mut missing = vec![];
        for (k, (g, n)) in spec.ys.iter().enumerate() {
            match r.column(g, n) {
                Some((dy, yv)) => {
                    let xs: Vec<f64> = dy.iter().map(|d| interp(&dx, &xv, *d).unwrap_or(f64::NAN)).collect();
                    let name = if g.starts_with("Line") { pretty_line(n) } else if g.starts_with("Ions") { ion_markup(n) } else { n.clone() };
                    f.line(&name, xs, yv, pal(k % PALETTE_LEN)).width = 2.2;
                }
                None => missing.push(format!("{g} / {n}")),
            }
        }
        if !missing.is_empty() {
            f.note = Some(format!("Not in this run: {}", missing.join(", ")));
        }
        f
    }
}

fn fmt_val(v: f64) -> String {
    if v.abs() >= 0.01 && v.abs() < 1000.0 {
        format!("{v:.3}")
    } else {
        format!("{v:.2e}")
    }
}

fn material_name(g: &str) -> String {
    let l = g.to_ascii_lowercase();
    if l.starts_with("gra") {
        "Graphite".into()
    } else if l.starts_with("sil") {
        "Silicate".into()
    } else if l.starts_with("pah") {
        "PAH".into()
    } else {
        g.to_string()
    }
}

/// "O+2" -> "O^{2+}", "H+" -> "H^{+}", "H2" -> "H_{2}"
pub fn ion_markup(name: &str) -> String {
    let n = name.trim();
    if n == "e-" || n == "PHOTON" || n == "CRPHOT" || n == "CRP" {
        return n.to_string();
    }
    let (body, charge) = match n.find(['+', '-']) {
        Some(pos) if pos > 0 => (&n[..pos], &n[pos..]),
        _ => (n, ""),
    };
    let mut s = String::new();
    let mut prev_alpha = false;
    for ch in body.chars() {
        if ch.is_ascii_digit() && prev_alpha {
            s.push_str(&format!("_{{{ch}}}"));
        } else {
            s.push(ch);
        }
        prev_alpha = ch.is_ascii_alphabetic() || (ch.is_ascii_digit() && prev_alpha);
    }
    if !charge.is_empty() {
        let sign = &charge[..1];
        let num = charge[1..].trim();
        s.push_str(&format!("^{{{num}{sign}}}"));
    }
    s
}

/// Markup -> plain text with Unicode sub/superscripts where possible (for on-screen labels).
pub fn markup_to_unicode(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if (c == '_' || c == '^') && chars.peek() == Some(&'{') {
            chars.next();
            let mut inner = String::new();
            for d in chars.by_ref() {
                if d == '}' {
                    break;
                }
                inner.push(d);
            }
            let map = |ch: char| -> Option<char> {
                if c == '^' {
                    Some(match ch {
                        '0' => '⁰', '1' => '¹', '2' => '²', '3' => '³', '4' => '⁴', '5' => '⁵',
                        '6' => '⁶', '7' => '⁷', '8' => '⁸', '9' => '⁹', '+' => '⁺', '-' => '⁻',
                        _ => return None,
                    })
                } else {
                    Some(match ch {
                        '0' => '₀', '1' => '₁', '2' => '₂', '3' => '₃', '4' => '₄', '5' => '₅',
                        '6' => '₆', '7' => '₇', '8' => '₈', '9' => '₉', '+' => '₊', '-' => '₋',
                        'e' => 'ₑ', 'o' => 'ₒ', 'x' => 'ₓ', 'h' => 'ₕ', 'k' => 'ₖ', 'l' => 'ₗ',
                        'm' => 'ₘ', 'n' => 'ₙ', 'p' => 'ₚ', 's' => 'ₛ', 't' => 'ₜ', 'a' => 'ₐ',
                        _ => return None,
                    })
                }
            };
            let mapped: Option<String> = inner.chars().map(map).collect();
            match mapped {
                Some(m) => out.push_str(&m),
                None => {
                    // no Unicode form (e.g. a subscript V): keep the marker so "A_V" stays readable
                    out.push(c);
                    out.push_str(&inner);
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}
