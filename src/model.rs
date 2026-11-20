//! Model parameters and generation of the CLOUDY input file.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Geometry {
    Slab,
    Sphere,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sed {
    Blackbody,
    Agn,
    IsmField,
    Custom,
}

impl Sed {
    pub const ALL: [Sed; 4] = [Sed::Blackbody, Sed::Agn, Sed::IsmField, Sed::Custom];
    pub fn label(self) -> &'static str {
        match self {
            Sed::Blackbody => "Blackbody (star)",
            Sed::Agn => "AGN (Mathews & Ferland)",
            Sed::IsmField => "Interstellar radiation field (ISM)",
            Sed::Custom => "Custom SED (from a file or typed in)",
        }
    }
}

/// Unit of the first column of a custom SED.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SedX {
    Ryd,
    Ev,
    Kev,
    Micron,
    Angstrom,
    Hz,
}

impl SedX {
    pub const ALL: [SedX; 6] = [SedX::Micron, SedX::Angstrom, SedX::Ev, SedX::Kev, SedX::Ryd, SedX::Hz];
    pub fn label(self) -> &'static str {
        match self {
            SedX::Ryd => "Energy [Ryd]",
            SedX::Ev => "Energy [eV]",
            SedX::Kev => "Energy [keV]",
            SedX::Micron => "Wavelength [µm]",
            SedX::Angstrom => "Wavelength [Å]",
            SedX::Hz => "Frequency [Hz]",
        }
    }
    fn keyword(self) -> &'static str {
        match self {
            SedX::Ryd => "Ryd",
            SedX::Ev => "eV",
            SedX::Kev => "keV",
            SedX::Micron => "micron",
            SedX::Angstrom => "angstrom",
            SedX::Hz => "Hz",
        }
    }
    /// Photon energy in Rydberg of a value in this unit.
    pub fn to_ryd(self, x: f64) -> f64 {
        match self {
            SedX::Ryd => x,
            SedX::Ev => x / 13.605693,
            SedX::Kev => x * 1000.0 / 13.605693,
            SedX::Micron => 0.0911267 / x,
            SedX::Angstrom => 911.267 / x,
            SedX::Hz => x / 3.289842e15,
        }
    }
}

/// Meaning of the second column of a custom SED.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SedY {
    NuFnu,
    Fnu,
    Flambda,
}

impl SedY {
    pub const ALL: [SedY; 3] = [SedY::NuFnu, SedY::Fnu, SedY::Flambda];
    pub fn label(self) -> &'static str {
        match self {
            SedY::NuFnu => "νFν (or λFλ)",
            SedY::Fnu => "Fν",
            SedY::Flambda => "Fλ",
        }
    }
}

/// A custom incident spectrum: two columns of text plus what they mean.
#[derive(Clone, Debug)]
pub struct CustomSed {
    pub text: String,
    pub x: SedX,
    pub y: SedY,
    pub y_is_log: bool,
    pub extrapolate: bool,
    /// the file it was loaded from, if any
    pub source: String,
}

impl Default for CustomSed {
    fn default() -> Self {
        CustomSed { text: String::new(), x: SedX::Micron, y: SedY::NuFnu, y_is_log: false, extrapolate: true, source: String::new() }
    }
}

pub const SED_FILE: &str = "incloud_custom.sed";

pub const SED_EXAMPLE: &str = "# wavelength [µm]   nuFnu (any scale: only the shape counts)
1000     1.0e-6
10       1.0e-3
1        0.1
0.3      1.0
0.1      0.8
0.03     0.2
0.01     0.01
0.001    1.0e-4
";

impl CustomSed {
    /// The (x, y) points in file units, sorted by x. Explains what is wrong when it can't be used.
    pub fn points(&self) -> Result<Vec<(f64, f64)>, String> {
        let mut pts: Vec<(f64, f64)> = vec![];
        for (i, raw) in self.text.lines().enumerate() {
            let l = raw.split('#').next().unwrap_or("").trim();
            if l.is_empty() {
                continue;
            }
            if l.starts_with("***") {
                break;
            }
            let mut it = l.split(|c: char| c.is_whitespace() || c == ',' || c == ';').filter(|t| !t.is_empty());
            let (Some(a), Some(b)) = (it.next(), it.next()) else {
                return Err(format!("Row {}: two columns are needed, x and y.", i + 1));
            };
            let (Ok(x), Ok(y)) = (a.parse::<f64>(), b.parse::<f64>()) else {
                return Err(format!("Row {}: “{l}” is not two numbers.", i + 1));
            };
            let y = if self.y_is_log { 10f64.powf(y) } else { y };
            if !(x > 0.0) || !x.is_finite() {
                return Err(format!("Row {}: x must be positive.", i + 1));
            }
            if !(y > 0.0) || !y.is_finite() {
                return Err(format!("Row {}: y must be positive (tick “y is log₁₀” for logarithms).", i + 1));
            }
            pts.push((x, y));
        }
        if pts.len() < 2 {
            return Err("Write at least two rows: x and y, separated by a space.".into());
        }
        pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        if pts.windows(2).any(|w| w[0].0 == w[1].0) {
            return Err("Two rows have the same x: each x must appear once.".into());
        }
        Ok(pts)
    }

    /// Photon energy range covered, in Rydberg.
    pub fn energy_range(&self) -> Option<(f64, f64)> {
        let p = self.points().ok()?;
        let (a, b) = (self.x.to_ryd(p[0].0), self.x.to_ryd(p[p.len() - 1].0));
        Some((a.min(b), a.max(b)))
    }

    /// The SED file in the format of CLOUDY's "table SED" command.
    pub fn file_text(&self) -> Result<String, String> {
        let pts = self.points()?;
        let mut s = String::from("# custom SED written by InCloud\n");
        for (i, (x, y)) in pts.iter().enumerate() {
            s.push_str(&format!("{x:.8e} {y:.8e}"));
            if i == 0 {
                s.push_str(&format!(" units {}", self.x.keyword()));
                match self.y {
                    SedY::NuFnu => s.push_str(" nuFnu"),
                    SedY::Flambda => s.push_str(" Flambda"),
                    SedY::Fnu => {}
                }
                if self.extrapolate {
                    s.push_str(" extrapolate");
                }
            }
            s.push('\n');
        }
        s.push_str("*****\n");
        Ok(s)
    }

    /// Read a spectrum file: two numeric columns, recognising CLOUDY's own keywords when present.
    pub fn load_text(&mut self, content: &str, source: &str) -> Result<usize, String> {
        let mut rows = String::new();
        let mut n = 0;
        for raw in content.lines() {
            let l = raw.split('#').next().unwrap_or("").trim();
            if l.starts_with("***") {
                break;
            }
            let toks: Vec<&str> = l.split(|c: char| c.is_whitespace() || c == ',' || c == ';').filter(|t| !t.is_empty()).collect();
            if toks.len() < 2 {
                continue;
            }
            let (Ok(x), Ok(y)) = (toks[0].parse::<f64>(), toks[1].parse::<f64>()) else { continue };
            rows.push_str(&format!("{x:e}  {y:e}\n"));
            n += 1;
            let low: Vec<String> = toks[2..].iter().map(|t| t.to_ascii_lowercase()).collect();
            for (k, t) in low.iter().enumerate() {
                if t.starts_with("unit") {
                    if let Some(u) = low.get(k + 1) {
                        self.x = match u.as_str() {
                            "ev" => SedX::Ev,
                            "kev" => SedX::Kev,
                            "hz" => SedX::Hz,
                            "ryd" | "rydberg" => SedX::Ryd,
                            u if u.starts_with("mic") => SedX::Micron,
                            u if u.starts_with("ang") || u == "a" => SedX::Angstrom,
                            _ => self.x,
                        };
                    }
                }
                if t.starts_with("nufn") {
                    self.y = SedY::NuFnu;
                } else if t.starts_with("flam") {
                    self.y = SedY::Flambda;
                } else if t.starts_with("extr") {
                    self.extrapolate = true;
                }
            }
        }
        if n < 2 {
            return Err("No two-column numeric data found in this file.".into());
        }
        self.text = rows;
        self.source = source.to_string();
        Ok(n)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Abundances {
    Ism,
    HiiRegion,
    Orion,
    Solar,
    PlanetaryNebula,
}

impl Abundances {
    pub const ALL: [Abundances; 5] = [
        Abundances::Ism,
        Abundances::HiiRegion,
        Abundances::Orion,
        Abundances::Solar,
        Abundances::PlanetaryNebula,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Abundances::Ism => "ISM",
            Abundances::HiiRegion => "H II region",
            Abundances::Orion => "Orion",
            Abundances::Solar => "Solar (GASS10)",
            Abundances::PlanetaryNebula => "Planetary nebula",
        }
    }
    fn command(self) -> &'static str {
        match self {
            Abundances::Ism => "abundances ism no grains",
            Abundances::HiiRegion => "abundances H II region no grains",
            Abundances::Orion => "abundances orion no grains",
            Abundances::Solar => "abundances gass",
            Abundances::PlanetaryNebula => "abundances planetary nebula no grains",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grains {
    None,
    Ism,
    Orion,
    IsmPah,
}

impl Grains {
    pub const ALL: [Grains; 4] = [Grains::Ism, Grains::IsmPah, Grains::Orion, Grains::None];
    pub fn label(self) -> &'static str {
        match self {
            Grains::None => "No grains",
            Grains::Ism => "ISM grains (graphite + silicate)",
            Grains::Orion => "Orion grains",
            Grains::IsmPah => "ISM grains + PAH",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DensityLaw {
    Constant,
    ConstantPressure,
}

#[derive(Clone, Debug)]
pub struct StopCriteria {
    pub use_temp: bool,
    pub temp: f64,
    pub use_av: bool,
    pub av: f64,
    pub use_column: bool,
    pub log_column: f64,
    pub use_thickness: bool,
    pub log_thickness: f64,
    pub use_efrac: bool,
    pub log_efrac: f64,
    pub use_zones: bool,
    pub zones: u32,
}

impl Default for StopCriteria {
    fn default() -> Self {
        Self {
            use_temp: true,
            temp: 10.0,
            use_av: true,
            av: 10.0,
            use_column: false,
            log_column: 22.0,
            use_thickness: false,
            log_thickness: 19.0,
            use_efrac: false,
            log_efrac: -2.0,
            use_zones: false,
            zones: 800,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LineChoice {
    pub label: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    pub enabled: bool,
}

pub fn default_lines() -> Vec<LineChoice> {
    let l = |label, name, group, enabled| LineChoice { label, name, group, enabled };
    vec![
        l("H  1 4861.32A", "Hβ", "Optical", true),
        l("H  1 6562.80A", "Hα", "Optical", true),
        l("Blnd 5875.66A", "He I 5876", "Optical", true),
        l("O  3 5006.84A", "[O III] 5007", "Optical", true),
        l("O  3 4958.91A", "[O III] 4959", "Optical", false),
        l("O  3 4363.21A", "[O III] 4363", "Optical", true),
        l("N  2 6583.45A", "[N II] 6584", "Optical", true),
        l("N  2 6548.05A", "[N II] 6548", "Optical", false),
        l("O  2 3726.03A", "[O II] 3726", "Optical", true),
        l("O  2 3728.81A", "[O II] 3729", "Optical", true),
        l("S  2 6716.44A", "[S II] 6716", "Optical", true),
        l("S  2 6730.82A", "[S II] 6731", "Optical", true),
        l("Blnd 6300A", "[O I] 6300", "Optical", true),
        l("Ne 3 3868.76A", "[Ne III] 3869", "Optical", true),
        l("Ar 3 7135.79A", "[Ar III] 7136", "Optical", false),
        l("Ne 2 12.8101m", "[Ne II] 12.8µm", "Infrared", true),
        l("Ne 3 15.5509m", "[Ne III] 15.6µm", "Infrared", true),
        l("S  3 18.7078m", "[S III] 18.7µm", "Infrared", true),
        l("Si 2 34.8046m", "[Si II] 34.8µm", "Infrared", true),
        l("O  3 88.3323m", "[O III] 88µm", "Infrared", true),
        l("O  1 63.1679m", "[O I] 63µm", "Infrared", true),
        l("O  1 145.495m", "[O I] 145µm", "Infrared", true),
        l("C  2 157.636m", "[C II] 158µm", "Infrared", true),
        l("N  2 205.283m", "[N II] 205µm", "Infrared", false),
        l("H2   2.12125m", "H2 1-0 S(1) (slow)", "Molecular", false),
        l("CO   2600.05m", "CO 1-0", "Molecular", true),
    ]
}

/// Labels used by CLOUDY before C23 for the lines whose label changed.
fn legacy_label(modern: &str) -> String {
    match modern {
        "Blnd 5875.66A" => "He 1 5875.64A".into(),
        "Blnd 6300A" => "O  1 6300.30A".into(),
        "N  2 205.283m" => "N  2 205.244m".into(),
        other => other.into(),
    }
}

/// Major version number from a path or version string ("c25.00" -> 25, "C17.03" -> 17).
pub fn major_version(v: &str) -> Option<u32> {
    let v = v.trim_start_matches(['C', 'c']);
    v.split('.').next()?.parse().ok()
}

/// Elements whose ionisation structure is saved.
pub const ELEMENTS: [(&str, &str); 6] = [
    ("H", "hydrogen"),
    ("He", "helium"),
    ("C", "carbon"),
    ("N", "nitrogen"),
    ("O", "oxygen"),
    ("S", "sulphur"),
];

#[derive(Clone, Debug)]
pub struct Params {
    pub name: String,
    pub log_hden: f64,
    pub log_u: f64,
    pub sed: Sed,
    pub bb_temp: f64,
    /// strength of the interstellar field, log of the factor on the local ISM field
    pub ism_log_g: f64,
    pub geometry: Geometry,
    pub log_radius: f64,
    pub abundances: Abundances,
    pub grains: Grains,
    pub log_metals: f64,
    pub cosmic_rays: bool,
    pub density_law: DensityLaw,
    pub iterations: u32,
    /// keep iterating until CLOUDY finds the optical depths converged
    pub converge: bool,
    pub converge_max: u32,
    pub converge_error: f64,
    pub turbulence: f64,
    pub stop: StopCriteria,
    pub lines: Vec<LineChoice>,
    pub custom_lines: String,
    pub extra_commands: String,
    /// CLOUDY C23 and later: line wavelengths need an explicit "air"/"vacuum" keyword
    pub modern_labels: bool,
    /// save formation / destruction reaction rates ("save chemistry rates")
    pub save_chemistry: bool,
    /// species chosen by hand for the chemistry, whatever lines are saved
    pub chem_species: String,
    /// also save the species behind the chosen lines
    pub chem_auto: bool,
    pub custom_sed: CustomSed,
}

/// The tolerance CLOUDY uses for "iterate to convergence" when none is given.
pub const DEFAULT_CONVERGE_ERROR: f64 = 0.2;

/// CLOUDY accepts at most 100 save commands, and each species takes two of them.
pub const MAX_SAVES: usize = 100;
pub const MAX_CHEM_SPECIES: usize = 40;

/// Species of CLOUDY's list that take part in no reaction of its chemical network (checked on C25):
/// their ionisation is solved apart, so asking for their rates gives an empty file.
pub const NO_REACTIONS: [&str; 41] = [
    "Ti", "Ne+", "C+2", "O+2", "Si+2", "N+2", "Cl+2", "Ne+2", "Ar+2", "SiO2", "CH3OH", "Fe", "Fe+", "Mg", "Mg+", "TiH", "TiH+", "TiH2", "TiH2+", "TiO", "TiO+", "TiO2", "TiC", "TiC+", "TiC2",
    "TiC2+", "TiN", "TiN+", "TiS", "TiS+", "TiF+", "HCTi", "HCTi+", "HNTi", "HNTi+", "TiNC", "TiNC+", "HTiS", "TiOH+", "HTiS+", "SiCH2",
];

/// Species known to the CLOUDY chemical network (safe to ask rates for).
pub const NETWORK_SPECIES: [&str; 30] = [
    "H", "H+", "H2", "H2+", "H3+", "He", "He+", "C", "C+", "C+2", "CH", "CH+", "CO", "CO+", "HCO+", "O", "O+", "O+2", "OH", "H2O",
    "O2", "N", "N+", "N+2", "CN", "HCN", "NO", "S", "S+", "Si+",
];

/// The chemical species behind a line label ("O  3 5006.84A" -> "O+2").
pub fn species_of_line(label: &str) -> Option<&'static str> {
    let l = label.trim();
    let sp = if l.len() >= 4 { &l[..4] } else { l };
    let wl = l.get(4..).unwrap_or("").trim();
    Some(match sp {
        "H  1" => "H+",
        "He 1" => "He+",
        "O  3" => "O+2",
        "O  2" => "O+",
        "O  1" => "O",
        "N  2" => "N+",
        "S  2" => "S+",
        "C  2" => "C+",
        "Si 2" => "Si+",
        "H2  " | "H2 " => "H2",
        "CO  " | "CO " => "CO",
        "Blnd" if wl.starts_with("5875") => "He+",
        "Blnd" if wl.starts_with("6300") => "O",
        _ => {
            if l.starts_with("H2") {
                "H2"
            } else if l.starts_with("CO") {
                "CO"
            } else {
                return None;
            }
        }
    })
}

/// Safe file-name fragment for a species ("C+2" -> "Cp2").
pub fn species_file(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '+' => 'p',
            '-' => 'm',
            '*' => 's',
            c if c.is_ascii_alphanumeric() => c,
            _ => '_',
        })
        .collect()
}

impl Default for Params {
    fn default() -> Self {
        Self {
            name: "model".into(),
            log_hden: 2.0,
            log_u: -2.0,
            sed: Sed::Blackbody,
            bb_temp: 40000.0,
            ism_log_g: 0.0,
            geometry: Geometry::Slab,
            log_radius: 18.0,
            abundances: Abundances::Ism,
            grains: Grains::Ism,
            log_metals: 0.0,
            cosmic_rays: true,
            density_law: DensityLaw::Constant,
            iterations: 2,
            converge: false,
            converge_max: 10,
            converge_error: DEFAULT_CONVERGE_ERROR,
            turbulence: 0.0,
            stop: StopCriteria::default(),
            lines: default_lines(),
            custom_lines: String::new(),
            extra_commands: String::new(),
            modern_labels: true,
            save_chemistry: true,
            chem_species: String::new(),
            chem_auto: true,
            custom_sed: CustomSed::default(),
        }
    }
}

impl Params {
    /// Files the run needs beside the input deck: (path relative to the run folder, content).
    pub fn extra_files(&self) -> Result<Vec<(String, String)>, String> {
        if self.sed == Sed::Custom {
            Ok(vec![(format!("SED/{SED_FILE}"), self.custom_sed.file_text()?)])
        } else {
            Ok(vec![])
        }
    }

    /// Species whose reaction rates are saved.
    pub fn chemistry_species(&self) -> Vec<String> {
        if !self.save_chemistry {
            return vec![];
        }
        // the species chosen by hand come first, so that they are never the ones left out
        let mut v = self.chem_list();
        if self.chem_auto {
            // the molecules of the structure plots plus the emitters of the saved lines
            for s in ["H2", "CO", "C+", "OH"] {
                if !v.iter().any(|x| x == s) {
                    v.push(s.to_string());
                }
            }
            for l in self.lines.iter().filter(|l| l.enabled) {
                if let Some(s) = species_of_line(l.label) {
                    if !v.iter().any(|x| x == s) && NETWORK_SPECIES.contains(&s) && !NO_REACTIONS.contains(&s) {
                        v.push(s.to_string());
                    }
                }
            }
        }
        v.truncate(MAX_CHEM_SPECIES);
        v
    }

    /// The species chosen by hand.
    pub fn chem_list(&self) -> Vec<String> {
        let mut v: Vec<String> = vec![];
        for s in self.chem_species.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()) {
            if !v.iter().any(|x| x == s) {
                v.push(s.to_string());
            }
        }
        v
    }

    pub fn set_chem_list(&mut self, list: &[String]) {
        self.chem_species = list.join(", ");
    }

    /// Line labels requested (preset + custom), in order.
    pub fn line_labels(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .lines
            .iter()
            .filter(|l| l.enabled)
            .map(|l| if self.modern_labels { format!("{} air", l.label) } else { legacy_label(l.label) })
            .collect();
        for line in self.custom_lines.lines() {
            let t = line.trim_end();
            if !t.trim().is_empty() && !v.iter().any(|x| x == t) {
                v.push(t.to_string());
            }
        }
        v
    }

    pub fn has_grains(&self) -> bool {
        self.grains != Grains::None
    }

    pub fn file_prefix(&self) -> String {
        let s: String = self
            .name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        if s.is_empty() {
            "model".into()
        } else {
            s
        }
    }

    /// Full CLOUDY input deck.
    pub fn input_file(&self) -> String {
        let mut s = String::new();
        let mut w = |line: String| {
            s.push_str(&line);
            s.push('\n');
        };
        w(format!("title {}", self.name.replace('\n', " ")));
        w("# generated by InCloud".into());
        w("# ---- ionising source".into());
        match self.sed {
            Sed::Blackbody => w(format!("blackbody {:.0}", self.bb_temp.max(1000.0))),
            Sed::Agn => w("AGN 6.00 -1.40 -0.50 -1.0".into()),
            // the ISM field comes with its own brightness: it takes no ionization parameter
            Sed::IsmField => w(if self.ism_log_g.abs() < 1e-6 { "table ism".into() } else { format!("table ism factor {:.4e}", 10f64.powf(self.ism_log_g)) }),
            Sed::Custom => w(format!("table SED \"{SED_FILE}\"")),
        }
        if self.sed != Sed::IsmField {
            w(format!("ionization parameter {:.3}", self.log_u));
        }
        w("# ---- gas".into());
        w(format!("hden {:.3}", self.log_hden));
        if self.density_law == DensityLaw::ConstantPressure {
            w("constant pressure".into());
        }
        w(self.abundances.command().into());
        // below this density CLOUDY C25 stops with an internal error in the quantum heating of
        // the grains (seen at log n = -2; -1.5 works), so it is turned off there
        let q = if self.no_qheat() { " no qheat" } else { "" };
        match self.grains {
            Grains::None => {}
            Grains::Ism => w(format!("grains ism{q}")),
            Grains::Orion => w(format!("grains orion{q}")),
            Grains::IsmPah => {
                w(format!("grains ism{q}"));
                w(format!("grains PAH{q}"));
            }
        }
        if self.log_metals.abs() > 1e-6 {
            w(format!("metals {:.3} log", self.log_metals));
            if self.has_grains() {
                w(format!("grains {:.3} log", self.log_metals));
            }
        }
        if self.cosmic_rays {
            w("cosmic rays background".into());
        }
        if self.turbulence > 0.0 {
            w(format!("turbulence {:.2} km/s", self.turbulence));
        }
        w("# ---- geometry".into());
        match self.geometry {
            Geometry::Slab => w("# open plane-parallel slab (CLOUDY default)".into()),
            Geometry::Sphere => {
                w("sphere".into());
                w(format!("radius {:.3}", self.log_radius));
            }
        }
        w("# ---- stopping criteria".into());
        let st = &self.stop;
        if st.use_temp {
            w(format!("stop temperature {:.1} linear", st.temp));
        } else {
            // otherwise CLOUDY still stops at its own 4000 K, long before the limits chosen here
            w("stop temperature off".into());
        }
        if st.use_av {
            w(format!("stop AV {:.3}", st.av));
        }
        if st.use_column {
            w(format!("stop column density {:.3}", st.log_column));
        }
        if st.use_thickness {
            w(format!("stop thickness {:.3}", st.log_thickness));
        }
        if st.use_efrac {
            w(format!("stop efrac {:.3}", st.log_efrac));
        }
        if st.use_zones {
            w(format!("stop zone {}", st.zones));
        } else if self.deep() {
            // CLOUDY stops at 1400 zones unless told otherwise: A_V = 30 already needs 1374
            w("set nend 5000".into());
        }
        if self.converge {
            if (self.converge_error - DEFAULT_CONVERGE_ERROR).abs() < 1e-6 {
                w(format!("iterate to convergence max={}", self.converge_max.max(2)));
            } else {
                w(format!("iterate to convergence max={}, error={:.3}", self.converge_max.max(2), self.converge_error));
            }
        } else if self.iterations >= 3 {
            w(format!("iterate {}", self.iterations));
        } else if self.iterations == 2 {
            w("iterate".into());
        }
        if !self.extra_commands.trim().is_empty() {
            w("# ---- extra commands".into());
            // CLOUDY stops reading at the first empty line
            for l in self.extra_commands.lines().filter(|l| !l.trim().is_empty()) {
                w(l.to_string());
            }
        }
        s.push_str(&self.output_commands());
        s
    }

    /// The commands that save what InCloud reads back.
    pub fn output_commands(&self) -> String {
        let p = self.file_prefix();
        let mut s = String::new();
        let mut w = |line: String| {
            s.push_str(&line);
            s.push('\n');
        };
        w("# ---- output".into());
        w(format!("save overview \"{p}.ovr\" last"));
        w(format!("save overview \"{p}.prog\""));
        w(format!("save continuum \"{p}.con\" units microns last"));
        for (sym, name) in ELEMENTS {
            w(format!("save element {name} \"{p}.ele_{sym}\" last"));
        }
        if self.has_grains() {
            w(format!("save grain temperature \"{p}.gtemp\" last"));
        }
        w(format!("save heating \"{p}.het\" last"));
        w(format!("save cooling \"{p}.col\" last"));
        for sp in self.chemistry_species() {
            let f = species_file(&sp);
            w(format!("save chemistry rates creation \"{p}.chf_{f}\" \"{sp}\" last"));
            w(format!("save chemistry rates destruction \"{p}.chd_{f}\" \"{sp}\" last"));
        }
        let lines = self.line_labels();
        if lines.iter().any(|l| l.starts_with("H2 ")) {
            // H2 emission lines exist only with the full H2 molecule model
            w("# H2 lines need the large H2 model (slower)".into());
            w("database H2".into());
        }
        if !lines.is_empty() {
            w(format!("save lines, emissivity \"{p}.ems\" last"));
            for l in lines {
                w(l);
            }
            w("end of lines".into());
        }
        s
    }
}

/// Minimal reading of an input deck (used when a run folder is opened).
#[derive(Clone, Debug, Default)]
pub struct DeckInfo {
    pub sphere: bool,
    pub log_radius: Option<f64>,
    pub iterations: u32,
    /// "iterate to convergence": `iterations` is then the most it may take
    pub converge: bool,
    pub stop_temp: Option<f64>,
    /// "stop temperature off": no lowest temperature at all
    pub temp_off: bool,
    pub stop_av: Option<f64>,
    pub stop_log_column: Option<f64>,
    pub stop_log_thickness: Option<f64>,
    pub stop_zones: Option<u32>,
    pub title: String,
    pub log_hden: Option<f64>,
    /// (species, formation file, destruction file) from "save chemistry rates"
    pub chemistry: Vec<(String, Option<String>, Option<String>)>,
}

pub fn read_deck(text: &str) -> DeckInfo {
    let mut d = DeckInfo { iterations: 1, ..Default::default() };
    for raw in text.lines() {
        let l = raw.trim();
        let low = l.to_ascii_lowercase();
        if low.starts_with('c') && (low.len() == 1 || low.as_bytes()[1] == b' ') || low.starts_with('#') {
            continue;
        }
        let num = |s: &str| -> Option<f64> {
            s.split_whitespace().find_map(|t| t.trim_end_matches(',').parse::<f64>().ok())
        };
        if low.starts_with("title") {
            d.title = l[5..].trim().to_string();
        } else if low.starts_with("hden") {
            d.log_hden = num(&low);
        } else if low.starts_with("save chem") && low.contains("rate") {
            // save chemistry rates [creation|destruction] "file" "species"
            let q: Vec<&str> = l.split('"').collect();
            if q.len() >= 4 {
                let (file, sp) = (q[1].to_string(), q[3].to_string());
                let idx = match d.chemistry.iter().position(|c| c.0 == sp) {
                    Some(i) => i,
                    None => {
                        d.chemistry.push((sp.clone(), None, None));
                        d.chemistry.len() - 1
                    }
                };
                if low.contains("destr") {
                    d.chemistry[idx].2 = Some(file);
                } else {
                    d.chemistry[idx].1 = Some(file);
                }
            }
        } else if low.starts_with("sphe") {
            d.sphere = true;
        } else if low.starts_with("radi") {
            d.log_radius = num(&low);
        } else if low.starts_with("iter") {
            d.converge = low.contains("conv");
            let first = low.split(|c: char| !(c.is_ascii_digit() || c == '.')).find_map(|t| t.parse::<f64>().ok());
            d.iterations = if d.converge { first.map(|v| v as u32).unwrap_or(10).max(2) } else { num(&low).map(|v| v as u32).unwrap_or(2).max(2) };
        } else if low.starts_with("stop temp") {
            d.temp_off = low.contains(" off");
            if let Some(v) = num(&low) {
                d.stop_temp = Some(if v <= 10.0 && !low.contains("linear") { 10f64.powf(v) } else { v });
            }
        } else if low.starts_with("stop av") {
            d.stop_av = num(&low);
        } else if low.starts_with("stop colu") {
            d.stop_log_column = num(&low);
        } else if low.starts_with("stop thic") {
            d.stop_log_thickness = num(&low);
        } else if low.starts_with("stop zone") {
            d.stop_zones = num(&low).map(|v| v as u32);
        }
    }
    d
}

/// The lines CLOUDY reads: it stops at the first empty one.
fn deck_read_part(text: &str) -> usize {
    let mut end = 0;
    for l in text.split_inclusive('\n') {
        if l.trim().is_empty() {
            break;
        }
        end += l.len();
    }
    end
}

/// Add commands to an input file, where CLOUDY will still read them.
pub fn deck_add_lines(text: &str, lines: &[String]) -> String {
    let end = deck_read_part(text);
    let mut s = text[..end].to_string();
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    for l in lines {
        s.push_str(l);
        s.push('\n');
    }
    s.push_str(&text[end..]);
    s
}

/// How many save commands an input file has.
pub fn deck_saves(text: &str) -> usize {
    text[..deck_read_part(text)]
        .lines()
        .filter(|l| {
            let low = l.trim_start().to_ascii_lowercase();
            low.starts_with("save") || low.starts_with("punch")
        })
        .count()
}

/// The file name given to `save overview` in an input file.
pub fn deck_overview(text: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let low = l.trim().to_ascii_lowercase();
        if !(low.starts_with("save over") || low.starts_with("punch over")) {
            return None;
        }
        let name = l.split('"').nth(1)?.to_string();
        name.ends_with(".ovr").then_some(name)
    })
}

/// The base name of the files of an input file written by hand.
pub fn deck_prefix(text: &str) -> Option<String> {
    let name = deck_overview(text)?;
    let stem = name.strip_suffix(".ovr")?;
    (!stem.is_empty() && !stem.contains(['/', '\\'])).then(|| stem.to_string())
}

/// Commands CLOUDY understands (from its own command table).
const COMMANDS: [&str; 96] = [
    "ABSOLUTE", "AGE", "AGN", "ABUNDANCES", "APERTURE", "BACKGROUND", "BLACKBODY", "BREMSSTRAHLUNG", "CASE", "CEXTRA", "CHEMISTRY", "CLUMPING", "CMB", "COMPILE",
    "CONSTANT", "CORONAL", "COSMOLOGY", "COSMIC", "COVERING", "CRASH", "CYLINDER", "DARK", "DATABASE", "ATOM", "DIELECTRONIC", "DIFFUSE", "DISTANCE", "DLAW", "DOUBLE",
    "EDEN", "ELEMENTS", "ENERGY", "EXTINGUISH", "FAILURES", "FILLING", "FLUCTUATIONS", "F(NU)", "FORCE", "FUDGE", "GLOBULE", "GRAINS", "PGRAINS", "GRAVITY", "GRID",
    "HDEN", "HELIKE", "HELP", "HEXTRA", "HIGH", "HYDROGEN", "ILLUMINATION", "INIT", "INTENSITY", "INTERPOLATE", "IONIZATION", "ITERATE", "L(NU)", "LASER", "LUMINOSITY",
    "MAGNETIC", "MAP", "METALS", "MONITOR", "NEUTRONS", "NO", "NORMALIZE", "NUF(NU)", "NUL(NU)", "OPTIMIZE", "PHI(H)", "POWER", "PRINT", "PUNCH", "Q(H)", "RATIO",
    "RADIUS", "ROBERTO", "SAVE", "SET", "SPECIAL", "SPECIES", "SPHERE", "STOP", "TABLE", "TAUMIN", "TEST", "TIME", "TITLE", "TLAW", "TOLERANCE", "TRACE", "VLAW",
    "TURBULENCE", "WIND", "XI", "CONTINUE",
];

/// A remark on one line of an input file.
#[derive(Clone, Debug)]
pub struct DeckNote {
    pub line: usize,
    pub error: bool,
    pub text: String,
}

/// What one radiation command gives: a shape, a brightness, or both.
fn radiation(up: &str) -> (usize, usize) {
    let w: Vec<&str> = up.split_whitespace().collect();
    let (first, second) = (w.first().copied().unwrap_or(""), w.get(1).copied().unwrap_or(""));
    // commands that bring their own brightness
    if first.starts_with("CMB") || first.starts_with("BACK") {
        return (0, 0);
    }
    if first.starts_with("TABL") {
        let own = ["ISM", "DRAI", "HM", "KS", "HAAR"].iter().any(|k| second.starts_with(k));
        return if own { (0, 0) } else { (1, 0) };
    }
    if first.starts_with("BLAC") {
        // with these keywords the blackbody sets its own brightness too
        let own = ["LTE", "STEF", "DILU", "ENER", "LUMI", "RADI"].iter().any(|k| w.iter().skip(2).any(|x| x.starts_with(k)));
        return if own { (0, 0) } else { (1, 0) };
    }
    if ["AGN", "POWE", "BREM", "LASE", "INTERP"].iter().any(|k| first.starts_with(k)) {
        return (1, 0);
    }
    if ["IONI", "Q(H)", "PHI(", "LUMI", "INTENS", "NUF(", "NUL(", "F(NU", "L(NU", "ABSO", "XI"].iter().any(|k| first.starts_with(k)) || (first.starts_with("ENER") && second.starts_with("DENS")) {
        return (0, 1);
    }
    (0, 0)
}

/// Mistakes that can be seen without running CLOUDY.
pub fn check_deck(text: &str) -> Vec<DeckNote> {
    let mut notes = vec![];
    let mut block = false;
    let mut has_hden = false;
    let (mut shapes, mut brightness, mut own, mut ratio) = (0usize, 0usize, false, false);
    for (i, raw) in text.lines().enumerate() {
        let l = raw.trim();
        let line = i + 1;
        if l.is_empty() || l.starts_with('#') || l.starts_with('*') || l.starts_with('%') || l.starts_with("//") {
            continue;
        }
        let up = l.to_ascii_uppercase();
        let word = up.split_whitespace().next().unwrap_or("");
        if block {
            block = !word.starts_with("END");
            continue;
        }
        if word == "C" {
            notes.push(DeckNote { line, error: true, text: "Recent versions of CLOUDY don't accept “c” comments: start the line with # instead.".into() });
            continue;
        }
        if l.matches('"').count() % 2 == 1 {
            notes.push(DeckNote { line, error: true, text: "A quotation mark is not closed.".into() });
        }
        if word.starts_with("HDEN") {
            has_hden = true;
        }
        let (s, b) = radiation(&up);
        shapes += s;
        brightness += b;
        own |= s == 0 && b == 0 && (word.starts_with("CMB") || word.starts_with("BACK") || word.starts_with("TABL") || word.starts_with("BLAC"));
        ratio |= word.starts_with("RATI");
        if (word.starts_with("SAVE") || word.starts_with("PUNC")) && up.contains("LINE") && (up.contains("EMIS") || up.contains("INTE") || up.contains("CUMU")) {
            block = true;
        }
        let number = word.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+' || c == '.');
        let head: String = word.chars().take(4).collect();
        let known = COMMANDS.iter().any(|c| c.starts_with(&head) || head.starts_with(*c) || c.chars().take(4).collect::<String>() == head);
        if !number && !word.starts_with("END") && !known {
            notes.push(DeckNote { line, error: false, text: format!("CLOUDY has no command “{}”: check the spelling.", l.split_whitespace().next().unwrap_or("")) });
        }
    }
    let read = deck_read_part(text);
    if !text[read..].trim().is_empty() {
        notes.push(DeckNote { line: text[..read].lines().count() + 1, error: true, text: "CLOUDY stops reading at this empty line: the commands after it are ignored.".into() });
    }
    let saves = deck_saves(text);
    if saves > MAX_SAVES {
        notes.push(DeckNote { line: 0, error: true, text: format!("{saves} save commands: CLOUDY accepts at most {MAX_SAVES}.") });
    }
    if !has_hden && !text.trim().is_empty() {
        notes.push(DeckNote { line: 0, error: false, text: "No “hden” line: CLOUDY needs the hydrogen density.".into() });
    }
    if !ratio && shapes != brightness {
        let text = if brightness > shapes {
            format!("{brightness} brightness commands (ionization parameter, Q(H), luminosity…) but {shapes} radiation shape that needs one: CLOUDY stops at once. The ISM field, the CMB and “background” bring their own brightness.")
        } else {
            format!("{shapes} radiation shapes but {brightness} brightness commands: each shape needs one (ionization parameter, Q(H), luminosity…), or CLOUDY stops at once.")
        };
        notes.push(DeckNote { line: 0, error: true, text });
    }
    if shapes == 0 && !own && !text.trim().is_empty() {
        notes.push(DeckNote { line: 0, error: true, text: "No radiation strikes the cloud: CLOUDY needs at least one source (a blackbody, a table, the CMB…).".into() });
    }
    notes
}

/// Something about the chosen values that CLOUDY will reject (`error`) or that may go wrong.
#[derive(Clone, Debug)]
pub struct Problem {
    pub error: bool,
    pub text: String,
    /// the wiki page that explains it
    pub topic: &'static str,
}

/// Above this total intensity of the incident light (log, erg cm⁻² s⁻¹) the grains at the face pass
/// their sublimation temperature and CLOUDY ends with warnings (checked on C25: 6.2 gives cautions,
/// 6.3 warnings, whatever the density and the kind of grains).
pub const GRAINS_SUBLIMATE: f64 = 6.25;
pub const GRAINS_NEAR: f64 = 6.0;
/// Above this the gas is heated beyond 10¹⁰ K and CLOUDY aborts (18.56 works, 18.81 aborts).
pub const GAS_TOO_HOT: f64 = 18.65;

const LIGHT: f64 = 2.99792458e10;

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Energy per hydrogen-ionising photon (erg) of a blackbody at `t` K.
fn blackbody_energy_per_photon(t: f64) -> f64 {
    let kt = 1.380649e-16 * t;
    let x0 = 13.598 * 1.602177e-12 / kt;
    // ∫_{x0}^∞ x²/(eˣ−1) dx as a series
    let mut tail = 0.0;
    for k in 1..200 {
        let k = k as f64;
        let term = (-k * x0).exp() * (x0 * x0 / k + 2.0 * x0 / (k * k) + 2.0 / (k * k * k));
        tail += term;
        if term < 1e-30 {
            break;
        }
    }
    kt * (std::f64::consts::PI.powi(4) / 15.0) / tail.max(1e-300)
}

impl CustomSed {
    /// Energy per hydrogen-ionising photon (erg) of this spectrum, from its points.
    fn energy_per_photon(&self) -> Option<f64> {
        let pts = self.points().ok()?;
        // (ln ν, ln νFν), whatever the units of the two columns
        let mut v: Vec<(f64, f64)> = pts
            .iter()
            .map(|(x, y)| {
                let nu = self.x.to_ryd(*x) * 3.2898419e15;
                let nufnu = match self.y {
                    SedY::NuFnu => *y,
                    SedY::Fnu => y * nu,
                    SedY::Flambda => y * LIGHT / nu,
                };
                (nu.ln(), nufnu.max(1e-300).ln())
            })
            .collect();
        v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let nu0 = (13.598 * 1.602177e-12 / 6.62607e-27_f64).ln();
        let h = 6.62607e-27;
        let (mut energy, mut photons) = (0.0, 0.0);
        for w in v.windows(2) {
            let ((a0, b0), (a1, b1)) = (w[0], w[1]);
            let steps = 24;
            for s in 0..steps {
                let t0 = s as f64 / steps as f64;
                let t1 = (s + 1) as f64 / steps as f64;
                let (l0, l1) = (a0 + t0 * (a1 - a0), a0 + t1 * (a1 - a0));
                let (f0, f1) = ((b0 + t0 * (b1 - b0)).exp(), (b0 + t1 * (b1 - b0)).exp());
                let d = l1 - l0;
                energy += 0.5 * (f0 + f1) * d;
                if l0 >= nu0 {
                    photons += 0.5 * (f0 / (h * l0.exp()) + f1 / (h * l1.exp())) * d;
                }
            }
        }
        (photons > 0.0).then(|| energy / photons)
    }
}

impl Params {
    /// Grains without quantum heating, which CLOUDY C25 can't do at very low density.
    pub fn no_qheat(&self) -> bool {
        self.has_grains() && self.log_hden < -1.75
    }

    /// A cloud deep enough to need more than CLOUDY's 1400 zones.
    pub fn deep(&self) -> bool {
        let st = &self.stop;
        let other_stop_first = st.use_temp && st.temp > 1000.0;
        !other_stop_first && ((st.use_av && st.av > 25.0) || (st.use_column && st.log_column > 22.5) || (st.use_thickness && st.log_thickness + self.log_hden > 22.5))
    }

    /// The total intensity of the light at the illuminated face (log, erg cm⁻² s⁻¹), as CLOUDY prints it.
    pub fn log_intensity(&self) -> Option<f64> {
        let per_photon = match self.sed {
            Sed::IsmField => return Some(-1.5558 + self.ism_log_g),
            Sed::Blackbody => blackbody_energy_per_photon(self.bb_temp),
            // the built-in AGN continuum, measured on C25
            Sed::Agn => 10f64.powf(-9.966),
            Sed::Custom => self.custom_sed.energy_per_photon()?,
        };
        Some(self.log_u + self.log_hden + LIGHT.log10() + per_photon.log10())
    }

    /// Checks of the values chosen in the panel, before running CLOUDY.
    pub fn problems(&self) -> Vec<Problem> {
        let mut v = vec![];
        let mut add = |error: bool, topic: &'static str, text: String| v.push(Problem { error, text, topic });
        let st = &self.stop;
        match self.sed {
            Sed::Custom => match self.custom_sed.points() {
                Err(e) => add(true, "customsed", format!("The custom spectrum can't be used: {e}")),
                Ok(_) => {
                    if self.custom_sed.energy_range().is_some_and(|(_, hi)| hi < 1.0) {
                        add(true, "customsed", "The custom spectrum stops below 1 Ryd (912 Å): it has no photons that ionise hydrogen, and log U counts exactly those. Extend it to shorter wavelengths.".into());
                    }
                }
            },
            Sed::Blackbody if self.bb_temp < BB_MIN_SAFE => add(
                true,
                "blackbody",
                format!("A {:.0} K star gives almost no photons that ionise hydrogen: to reach log U = {:.2} its total light would have to be enormous, and CLOUDY stops when the gas goes above 10¹⁰ K. Use T★ ≥ {:.0} K, or a lower log U.", self.bb_temp, self.log_u, BB_MIN_SAFE),
            ),
            Sed::IsmField => add(false, "ismfield", "The ISM field has no photons that ionise hydrogen: the cloud has no H II region and no ionisation front, only a PDR.".into()),
            _ => {}
        }
        if let Some(li) = self.log_intensity() {
            let how = if self.sed == Sed::IsmField { "lower log G/G_ISM" } else { "lower log U or n(H) (the light grows with both)" };
            if li > GAS_TOO_HOT {
                add(true, "ionu", format!("The light striking the cloud is far too intense (log I = {li:.2} erg cm⁻² s⁻¹): the gas would go above 10¹⁰ K, the limit of CLOUDY, which then stops. {}{}.", capitalised(how), if self.sed == Sed::Blackbody && self.bb_temp < 20000.0 { ", or use a hotter star" } else { "" }));
            } else if self.has_grains() && li > GRAINS_SUBLIMATE {
                add(false, "grains", format!("The light is intense enough (log I = {li:.1}) to heat the grains above their sublimation temperature: CLOUDY ends with warnings and its results with grains are not valid there. To keep the grains, {how}; or choose No grains."));
            } else if self.has_grains() && li > GRAINS_NEAR {
                add(false, "grains", format!("The grains come close to their sublimation temperature (log I = {li:.1}): CLOUDY will add cautions."));
            }
        }
        if !(st.use_temp || st.use_av || st.use_column || st.use_thickness || st.use_efrac || st.use_zones) {
            add(false, "stop", "No stopping criterion is ticked: only CLOUDY's limit of 1400 zones ends the run, with a warning. Tick at least one.".into());
        }
        if st.use_av && self.grains == Grains::None {
            add(false, "stop_av", "A_V grows only with dust: without grains the A_V limit is never reached, so another limit (or CLOUDY's 1400 zones) ends the run.".into());
        }
        if st.use_temp {
            let hot = if self.sed == Sed::IsmField { 1000.0 } else { 6000.0 };
            if st.temp >= hot {
                add(false, "stop_temp", format!("The run stops as soon as the gas is cooler than {:.0} K: {}", st.temp, if self.sed == Sed::IsmField { "lit only by the ISM field it is cooler than that almost from the start." } else { "the ionised gas is near 10⁴ K, so it may stop after a few zones." }));
            }
        }
        if st.use_zones && st.zones < 5 {
            add(false, "stop_zone", format!("Only {} zones: the results say little about the structure of the cloud.", st.zones));
        }
        if self.geometry == Geometry::Sphere && st.use_thickness && st.log_thickness > self.log_radius + 1.0 {
            add(false, "radius", "The thickness is much larger than the inner radius: the sphere is almost filled, which is fine, but check that this is what you want.".into());
        }
        if self.log_metals > 1.0 {
            add(false, "metals", "More than 10 times solar metals: CLOUDY may have trouble converging.".into());
        }
        if self.log_hden > 8.0 {
            add(false, "hden", "Very dense gas: the run will be slow.".into());
        }
        if self.line_labels().iter().any(|l| l.starts_with("H2 ")) {
            add(false, "lines", "The H₂ 2.12 µm line turns on the large H₂ model: the run is much slower.".into());
        }
        if self.no_qheat() {
            add(false, "grains", "At this very low density CLOUDY C25 stops with an internal error in the quantum heating of the grains, so InCloud turns it off (“no qheat”): the grain temperatures are then equilibrium values.".into());
        }
        if self.deep() {
            add(false, "stop", "A very deep cloud: CLOUDY would stop at its limit of 1400 zones, so InCloud raises it (“set nend 5000”). The run is long.".into());
        }
        if self.converge && self.converge_error < 0.05 && self.converge_max > 5 {
            add(false, "converge", format!("A very small tolerance ({:.3}) can take up to {} iterations, each about as long as a whole run.", self.converge_error, self.converge_max));
        }
        if self.converge && self.converge_max < 3 {
            add(false, "converge", "Iterating to convergence with at most 2 iterations: there is no room to converge.".into());
        }
        // what the input file itself shows, e.g. your own commands
        for n in check_deck(&self.input_file()) {
            let at = if n.line > 0 { format!("Input file, line {}: ", n.line) } else { String::new() };
            add(n.error, "input", format!("{at}{}", n.text));
        }
        v
    }
}

/// Below this, a blackbody with an ionization parameter heats the gas beyond CLOUDY's limit
/// (checked on C25 with log U from -4 to 0).
/// (checked on C25: 3000 K fails, 5000 K works, with log U from -4 to 0).
pub const BB_MIN_SAFE: f64 = 4000.0;
