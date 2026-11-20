//! What a CLOUDY run says about itself: everything useful that its text output (.out) reports.

use crate::results::parse_out_lines;

/// The four kinds of comment CLOUDY prints at the end of each iteration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Warning,
    Caution,
    Surprise,
    Note,
}

impl Level {
    pub const ALL: [Level; 4] = [Level::Warning, Level::Caution, Level::Surprise, Level::Note];
    pub fn plural(self) -> &'static str {
        match self {
            Level::Warning => "Warnings",
            Level::Caution => "Cautions",
            Level::Surprise => "Surprises",
            Level::Note => "Notes",
        }
    }
    pub fn singular(self) -> &'static str {
        match self {
            Level::Warning => "warning",
            Level::Caution => "caution",
            Level::Surprise => "surprise",
            Level::Note => "note",
        }
    }
    pub fn meaning(self) -> &'static str {
        match self {
            Level::Warning => "Something went wrong: the results should not be trusted until this is understood.",
            Level::Caution => "The calculation may be less accurate here: worth checking.",
            Level::Surprise => "Something unusual happened. It is not an error, but it is worth knowing.",
            Level::Note => "Remarks on the physics of this model.",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Message {
    pub level: Level,
    pub text: String,
}

/// One quantity to show.
#[derive(Clone, Debug)]
pub struct Item {
    pub name: String,
    pub value: String,
    pub tip: &'static str,
}

#[derive(Clone, Debug)]
pub struct Section {
    pub title: &'static str,
    pub items: Vec<Item>,
}

/// One pass of the calculation.
#[derive(Clone, Debug, Default)]
pub struct IterationInfo {
    pub number: u32,
    pub stop: String,
    pub not_converged: Option<String>,
    pub log_heat: Option<f64>,
    pub error: Option<f64>,
    pub te_low: Option<f64>,
    pub te_high: Option<f64>,
    pub exec: Option<f64>,
}

/// A table by element and ion stage (column densities, mean ionisation, mean temperature).
#[derive(Clone, Debug, Default)]
pub struct IonTable {
    pub title: String,
    pub rows: Vec<(String, Vec<f64>)>,
}

#[derive(Clone, Debug, Default)]
pub struct GrainBin {
    pub name: String,
    pub temp: f64,
    pub potential: f64,
    pub dust_gas: f64,
}

#[derive(Clone, Debug, Default)]
pub struct RunInfo {
    pub present: bool,
    pub version: String,
    pub build: String,
    pub seed: String,
    pub title: String,
    pub model: String,
    pub complete: bool,
    pub exited_ok: Option<bool>,
    pub zones: Option<u32>,
    pub iterations: Option<u32>,
    pub exec_time: Option<f64>,
    pub threads: String,
    pub failures: String,
    pub stop_reason: String,
    pub iteration: Option<(u32, u32)>,
    pub converge_asked: bool,
    pub converged: Option<bool>,
    pub messages: Vec<Message>,
    pub problems: Vec<String>,
    pub input: Vec<String>,
    pub sections: Vec<Section>,
    pub gas: Vec<(String, f64)>,
    pub dust: Vec<(String, f64)>,
    pub tables: Vec<IonTable>,
    pub molecules: Vec<(String, f64)>,
    pub grains: Vec<GrainBin>,
    pub passes: Vec<IterationInfo>,
    pub stats: Vec<String>,
    /// strongest lines: label, log intensity, intensity relative to H-beta
    pub strongest: Vec<(String, f64, Option<f64>)>,
    /// the last lines of an output that stops before the end
    pub tail: Vec<String>,
}

impl RunInfo {
    pub fn count(&self, level: Level) -> usize {
        self.messages.iter().filter(|m| m.level == level).count()
    }
}

fn number_at_start(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let mut e = 0;
    if e < b.len() && (b[e] == b'-' || b[e] == b'+') {
        e += 1;
    }
    let digits = e;
    while e < b.len() && (b[e].is_ascii_digit() || b[e] == b'.') {
        e += 1;
    }
    if e == digits {
        return None;
    }
    if e + 1 < b.len() && (b[e] == b'e' || b[e] == b'E') {
        let mut x = e + 1;
        if x < b.len() && (b[x] == b'-' || b[x] == b'+') {
            x += 1;
        }
        let start = x;
        while x < b.len() && b[x].is_ascii_digit() {
            x += 1;
        }
        if x > start {
            e = x;
        }
    }
    s[..e].parse().ok()
}

/// The number written just after `key` on a line.
fn after(line: &str, key: &str) -> Option<f64> {
    let i = line.find(key)?;
    number_at_start(line[i + key.len()..].trim_start_matches(|c: char| c == ':' || c == '=' || c.is_whitespace()))
}

/// The same, looking only past the first appearance of `from`.
fn after_from(line: &str, from: &str, key: &str) -> Option<f64> {
    let i = line.find(from)?;
    after(&line[i + from.len()..], key)
}

fn superscript(n: i32) -> String {
    n.to_string()
        .chars()
        .map(|c| match c {
            '-' => '⁻',
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            _ => '⁹',
        })
        .collect()
}

/// A number the way it would be written in a paper.
pub fn pretty(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    let a = v.abs();
    if a == 0.0 {
        "0".into()
    } else if (1000.0..100000.0).contains(&a) {
        format!("{v:.0}")
    } else if (100.0..1000.0).contains(&a) {
        format!("{v:.1}")
    } else if (1.0..100.0).contains(&a) {
        format!("{v:.2}")
    } else if (0.01..1.0).contains(&a) {
        format!("{v:.3}")
    } else {
        let e = a.log10().floor() as i32;
        format!("{:.2} × 10{}", v / 10f64.powi(e), superscript(e))
    }
}

struct Builder {
    items: Vec<Item>,
}

impl Builder {
    fn new() -> Self {
        Builder { items: vec![] }
    }
    fn add(&mut self, name: &str, v: Option<f64>, unit: &str, tip: &'static str) {
        if let Some(v) = v {
            let value = if unit.is_empty() { pretty(v) } else { format!("{} {unit}", pretty(v)) };
            self.items.push(Item { name: name.to_string(), value, tip });
        }
    }
    /// A logarithm, written with a fixed number of decimals.
    fn log(&mut self, name: &str, v: Option<f64>, unit: &str, tip: &'static str) {
        if let Some(v) = v {
            let value = if unit.is_empty() { format!("{v:.3}") } else { format!("{v:.3} {unit}") };
            self.items.push(Item { name: name.to_string(), value, tip });
        }
    }
    /// A whole number.
    fn count(&mut self, name: &str, v: Option<f64>) {
        if let Some(v) = v {
            self.items.push(Item { name: name.to_string(), value: format!("{v:.0}"), tip: "" });
        }
    }
    fn finish(self, title: &'static str, out: &mut Vec<Section>) {
        if !self.items.is_empty() {
            out.push(Section { title, items: self.items });
        }
    }
}

/// Why the calculation stopped, in words.
fn stop_text(raw: &str) -> String {
    let r = raw.trim().trim_end_matches('.');
    match r {
        "NZONE reached" => "the limit on the number of zones was reached".into(),
        "A_V reached" => "the requested A_V was reached".into(),
        "lowest Te reached" => "the lowest temperature was reached".into(),
        "highest Te reached" => "the highest temperature was reached".into(),
        "H column dens reached" => "the requested column density was reached".into(),
        "outer radius reached" => "the requested thickness was reached".into(),
        "low electron fraction" => "the electron fraction fell below the limit".into(),
        "lowest EDEN reached" => "the electron density fell below the limit".into(),
        "large H_2/H fraction" => "the H₂ fraction limit was reached".into(),
        "optical depth reached" => "the requested optical depth was reached".into(),
        "of radiation pressure" => "radiation pressure became too large".into(),
        "DRAD small- set DRMIN" | "DR small rel to thick" => "the zones became too thin".into(),
        other => other.to_string(),
    }
}

/// True for the label of a real emission line, not of a band or a continuum.
fn is_emission_line(label: &str) -> bool {
    let mut parts = label.rsplitn(2, ' ');
    let (wave, kind) = (parts.next().unwrap_or(""), parts.next().unwrap_or("").trim());
    if wave.ends_with('c') {
        return false;
    }
    if ["Blnd", "CO", "H2", "OH", "H2O", "HD", "CN", "CS", "SiO", "HCO+", "HCN", "NH3"].contains(&kind) {
        return true;
    }
    // an element and its ion stage: "H  1", "Ne 3", "Fe10"
    let letters: String = kind.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let spaced = kind[letters.len()..].starts_with(' ');
    let stage = kind[letters.len()..].trim();
    (spaced || letters.len() == 2) && (1..=2).contains(&letters.len()) && letters.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && !stage.is_empty() && stage.chars().all(|c| c.is_ascii_digit())
}

/// Pairs such as "He: -1.0088" of the composition tables.
fn composition(line: &str, out: &mut Vec<(String, f64)>) {
    let mut rest = line;
    while let Some(i) = rest.find(':') {
        let name: String = rest[..i].chars().rev().take(2).collect::<Vec<_>>().into_iter().rev().collect();
        let name = name.trim().to_string();
        let tail = rest[i + 1..].trim_start();
        if let (false, Some(v)) = (name.is_empty(), number_at_start(tail)) {
            out.push((name, v));
        }
        rest = &rest[i + 1..];
    }
}

/// Read the text output of a run.
pub fn parse(text: &str) -> RunInfo {
    let lines: Vec<&str> = text.lines().collect();
    let mut inf = RunInfo { present: text.contains("Cloudy"), ..Default::default() };
    if !inf.present {
        return inf;
    }
    let (mut in_box, mut box_done, mut zone_seen, mut in_comments) = (false, false, false, false);
    let (mut gas_on, mut dust_on, mut stats_on) = (false, false, false);
    let mut start_zone = "";
    let mut incident: Vec<&str> = vec![];
    let mut last: std::collections::HashMap<&'static str, usize> = Default::default();
    const MARKS: [&str; 15] = ["IONIZE PARMET", "ENERGY BUDGET", "Col(Heff)", "Emiss Measure", "He/Ha:", "Temps(21 cm)", "N(H0/Tspin)", "<a>:", "Mean Jeans", "Hatom level", "ConvrgError", "Continuity(%)", "Peimbert", "Dust to gas ratio", "Contin Optical Depths"];
    let mut averages: Vec<(String, Vec<String>, Vec<f64>)> = vec![];
    let mut avg_names: Vec<String> = vec![];
    let mut table: Option<IonTable> = None;
    let mut tables: Vec<IonTable> = vec![];
    let mut mol_on = false;
    let mut grain_names: Vec<String> = vec![];
    let (mut g_t, mut g_p, mut g_d): (Vec<f64>, Vec<f64>, Vec<f64>) = (vec![], vec![], vec![]);

    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        if inf.version.is_empty() && t.starts_with("Cloudy ") && !t.contains("ends") {
            inf.version = t.to_string();
        }
        if let Some(s) = t.strip_prefix("PRNG seed:") {
            inf.seed = s.trim().to_string();
        }
        // the input, as CLOUDY read it
        if !box_done && t.starts_with("***") {
            if in_box {
                box_done = true;
            } else {
                in_box = true;
                inf.build = t.trim_matches('*').trim().to_string();
            }
            continue;
        }
        if in_box && !box_done {
            let inner = t.trim_start_matches('*').trim_end_matches('*').trim();
            if !inner.is_empty() && !inner.starts_with("#<<<") {
                if inner.to_ascii_lowercase().starts_with("title") {
                    inf.title = inner[5..].trim().to_string();
                }
                inf.input.push(inner.to_string());
            }
            continue;
        }
        if !zone_seen {
            if t.starts_with("####") {
                zone_seen = true;
                start_zone = t;
            } else if ["I(nu>1ryd)", "phi(1.0-1.8)", "I(gam ray)", "U(1.0----)"].iter().any(|k| t.starts_with(k)) {
                incident.push(t);
            }
            if t.starts_with("Gas Phase Chemical Composition") {
                gas_on = true;
                continue;
            }
            if t.starts_with("Grain Chemical Composition") {
                gas_on = false;
                dust_on = true;
                continue;
            }
            if t.is_empty() || t.starts_with("Sum Atoms") {
                gas_on = false;
                dust_on = false;
            }
            if gas_on {
                composition(l, &mut inf.gas);
            }
            if dust_on {
                composition(l, &mut inf.dust);
            }
        }
        for m in MARKS {
            if t.starts_with(m) {
                last.insert(m, i);
            }
        }
        if t.starts_with("PROBLEM") || t.starts_with("DISASTER") || t.contains(" ABORT") || t.starts_with("Unrecognized command") {
            if inf.problems.len() < 100 {
                inf.problems.push(t.to_string());
            }
        }

        // the comments that end each iteration
        if let (false, Some(p)) = (in_comments, t.find("Calculation stopped because ")) {
            let rest = &t[p + "Calculation stopped because ".len()..];
            let (reason, count) = match rest.find(" Iteration ") {
                Some(k) => (&rest[..k], &rest[k + " Iteration ".len()..]),
                None => (rest, ""),
            };
            let mut nums = count.split(" of ");
            let number = nums.next().and_then(|v| v.trim().parse().ok()).unwrap_or(inf.passes.len() as u32 + 1);
            let planned = nums.next().and_then(|v| v.split(|c: char| !c.is_ascii_digit()).next()).and_then(|v| v.parse().ok()).unwrap_or(number);
            let reason = stop_text(reason);
            inf.stop_reason = reason.clone();
            inf.iteration = Some((number, planned));
            inf.passes.push(IterationInfo { number, stop: reason, ..Default::default() });
            inf.messages.clear();
            if t.starts_with("W-") {
                inf.messages.push(Message { level: Level::Warning, text: t[2..].to_string() });
            }
            in_comments = true;
            continue;
        }
        if in_comments {
            if t.is_empty() {
                in_comments = false;
            } else if let Some(why) = t.strip_prefix("Iteration not converged because ") {
                let why = why.trim_end_matches('.');
                if why != "Converged!" {
                    if let Some(p) = inf.passes.last_mut() {
                        p.not_converged = Some(why.to_string());
                    }
                }
            } else if let Some(s) = t.strip_prefix("W-") {
                inf.messages.push(Message { level: Level::Warning, text: s.trim().to_string() });
            } else if let Some(s) = t.strip_prefix("C-") {
                inf.messages.push(Message { level: Level::Caution, text: s.trim().to_string() });
            } else if let Some(s) = t.strip_prefix('!') {
                inf.messages.push(Message { level: Level::Surprise, text: s.trim().to_string() });
            } else {
                inf.messages.push(Message { level: Level::Note, text: t.to_string() });
            }
            continue;
        }

        if t.starts_with("Emission Line Spectrum.") {
            let s = t.trim_start_matches("Emission Line Spectrum.").trim();
            let s = s.split(" Iteration ").next().unwrap_or(s);
            inf.model = s.split_whitespace().collect::<Vec<_>>().join(" ");
        }
        if let Some(p) = inf.passes.last_mut() {
            if t.starts_with("ENERGY BUDGET") {
                p.log_heat = after(t, "Heat:");
                p.error = after(t, "Error:");
            } else if t.starts_with("Col(Heff)") {
                p.te_low = after(t, "Te-low:");
                p.te_high = after(t, "Te-hi:");
            } else if t.starts_with("Hatom level") {
                p.exec = after(t, "ExecTime");
            }
        }

        // averaged temperatures and densities
        if t.starts_with("Te ") && t.contains("Te(Ne)") {
            let spaced = t.replace("Te(", " Te(").replace("Ne(", " Ne(").replace("N(H)", " N(H)");
            avg_names = spaced.split_whitespace().map(|s| s.to_string()).collect();
        } else if !avg_names.is_empty() && (t.starts_with("Radius:") || t.starts_with("Area:") || t.starts_with("Volume:")) {
            let (name, rest) = t.split_once(':').unwrap_or((t, ""));
            let v: Vec<f64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            averages.retain(|a| a.0 != name);
            averages.push((name.to_string(), avg_names.clone(), v));
        }

        // grains, by size bin
        if t.starts_with("Average Grain Properties") {
            grain_names.clear();
            g_t.clear();
            g_p.clear();
            g_d.clear();
        } else if t.contains("-ism") || t.contains("-orn") || t.contains("pah") && t.contains('*') {
            let names: Vec<&str> = t.split_whitespace().filter(|x| *x != "*").collect();
            if names.len() > 1 && names.iter().all(|n| n.contains('-')) {
                grain_names.extend(names.iter().map(|s| s.to_string()));
            }
        } else if let Some(s) = t.strip_prefix("<Tgr>:") {
            g_t.extend(s.split_whitespace().filter_map(|x| x.parse::<f64>().ok()));
        } else if let Some(s) = t.strip_prefix("<Pot>:") {
            g_p.extend(s.split_whitespace().filter_map(|x| x.parse::<f64>().ok()));
        } else if let Some(s) = t.strip_prefix("<D/G>:") {
            g_d.extend(s.split_whitespace().filter_map(|x| x.parse::<f64>().ok()));
        }

        // molecules
        if t.starts_with("====") && t.contains("compounds") {
            if t.contains("Log10 column densities") {
                inf.molecules.clear();
            }
            mol_on = true;
            continue;
        }
        if mol_on {
            if t.is_empty() {
                mol_on = false;
            } else {
                let mut rest = t;
                while let Some(k) = rest.find(':') {
                    let name = rest[..k].trim().to_string();
                    let tail = rest[k + 1..].trim_start();
                    let Some(v) = number_at_start(tail) else { break };
                    inf.molecules.push((name, v));
                    let skip = tail.find(|c: char| c.is_whitespace()).unwrap_or(tail.len());
                    rest = tail[skip..].trim_start();
                }
            }
        }

        // tables by element and ion
        if l.starts_with(" Hydrogen") && l.contains("Log10") {
            if let Some(t) = table.take() {
                tables.push(t);
            }
            let title = l[l.find("Log10").unwrap_or(0)..].trim().trim_start_matches("Log10").trim().to_string();
            table = Some(IonTable { title, rows: vec![] });
        }
        if let Some(tb) = table.as_mut() {
            if t.is_empty() || t.starts_with("Exc state") || t.starts_with('1') {
                tables.push(table.take().unwrap_or_default());
            } else if !l.starts_with('-') && l.len() > 11 && l.is_char_boundary(11) {
                let name = l[..11].trim().to_string();
                let body = l[11..].split("(H2)").next().unwrap_or("");
                let body: Vec<char> = body.chars().collect();
                let vals: Vec<f64> = body.chunks(7).filter_map(|c| c.iter().collect::<String>().trim().parse().ok()).collect();
                if !name.is_empty() && !vals.is_empty() {
                    tb.rows.push((name, vals));
                }
            }
        }

        if t.contains("Convergence statistics") {
            stats_on = true;
            continue;
        }
        if stats_on {
            if t.starts_with("-----") {
                stats_on = false;
            } else if !t.is_empty() {
                inf.stats.push(t.split_whitespace().collect::<Vec<_>>().join(" "));
            }
        }

        // the last words of the run
        if let Some(s) = t.strip_prefix("Cloudy ends:") {
            inf.complete = true;
            let mut parts = s.split(',');
            inf.zones = parts.next().and_then(|v| v.split_whitespace().next()).and_then(|v| v.parse().ok());
            inf.iterations = parts.next().and_then(|v| v.split_whitespace().next()).and_then(|v| v.trim_end_matches('.').parse().ok());
            inf.exec_time = after(s, "ExecTime(s)");
            if let (Some(a), Some(b)) = (s.find('('), s.find(')')) {
                if a < b {
                    inf.threads = s[a + 1..b].to_string();
                }
            }
            if let Some(k) = s.find("Failures:") {
                inf.failures = s[k + 9..].split('.').next().unwrap_or("").trim().to_string();
            }
        }
        if t.contains("Cloudy exited OK") {
            inf.exited_ok = Some(true);
        } else if t.contains("something went wrong") {
            inf.exited_ok = Some(false);
        }
    }
    if let Some(t) = table.take() {
        tables.push(t);
    }
    inf.tables = tables.into_iter().filter(|t| !t.rows.is_empty()).collect();
    inf.molecules.retain(|m| m.1 > -29.0);
    inf.molecules.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    for (i, name) in grain_names.iter().enumerate() {
        if let (Some(t), Some(p), Some(d)) = (g_t.get(i), g_p.get(i), g_d.get(i)) {
            inf.grains.push(GrainBin { name: name.clone(), temp: *t, potential: *p, dust_gas: *d });
        }
    }
    inf.converge_asked = inf.input.iter().any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("iter") && l.contains("conv")
    });
    if inf.converge_asked && inf.complete {
        inf.converged = Some(!inf.messages.iter().any(|m| m.text.contains("did not converge")));
    }
    let set_temp = inf.input.iter().any(|l| l.to_ascii_lowercase().starts_with("stop temp"));
    if inf.stop_reason == "the lowest temperature was reached" && !set_temp {
        inf.stop_reason = "the gas cooled below 4000 K, CLOUDY's own limit (no lowest temperature was set; tick “Lowest temperature” with a low value to go deeper)".into();
    }
    if !inf.complete {
        let mut tail: Vec<String> = lines.iter().rev().filter(|l| !l.trim().is_empty()).take(14).map(|l| l.trim_end().to_string()).collect();
        tail.reverse();
        inf.tail = tail;
    }

    // strongest emission lines of the last iteration
    let found = parse_out_lines(text);
    let hbeta = found.iter().find(|(l, _)| l.starts_with("H  1") && l.contains("4861")).map(|x| x.1);
    let mut strong: Vec<(String, f64)> = vec![];
    for (label, v) in found {
        if !is_emission_line(&label) || !(v > 0.0) {
            continue;
        }
        if !strong.iter().any(|s| s.0 == label) {
            strong.push((label, v));
        }
    }
    strong.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    inf.strongest = strong.into_iter().take(30).map(|(l, v)| (l, v.log10(), hbeta.map(|h| v / h))).collect();

    let line = |k: &str| last.get(k).map(|i| lines[*i]).unwrap_or("");
    let next = |k: &str| last.get(k).and_then(|i| lines.get(*i + 1)).copied().unwrap_or("");
    let inc = |k: &str| incident.iter().find_map(|l| after(l, k));
    let mut out = vec![];

    let (ion, bud, heff, em, misc, t21, t21b, mean, jeans, hatom, conv, cont, peim, dust, tau) = (
        line("IONIZE PARMET"),
        line("ENERGY BUDGET"),
        line("Col(Heff)"),
        line("Emiss Measure"),
        line("He/Ha:"),
        line("Temps(21 cm)"),
        line("N(H0/Tspin)"),
        line("<a>:"),
        line("Mean Jeans"),
        line("Hatom level"),
        line("ConvrgError"),
        line("Continuity(%)"),
        line("Peimbert"),
        line("Dust to gas ratio"),
        line("Contin Optical Depths"),
    );

    let mut b = Builder::new();
    b.log("log U (above 1 Ryd)", after(ion, "U(1-)"), "", "Ionisation parameter: ionising photons per hydrogen atom at the illuminated face");
    b.log("log U (above 4 Ryd)", after(ion, "U(4-):"), "", "The same for photons able to ionise He⁺");
    b.add("Ionising photon flux Φ(H)", inc("Ion pht flx:"), "cm⁻² s⁻¹", "Hydrogen-ionising photons striking the cloud");
    b.log("log Q(ion)", after(ion, "Q(ion):"), "", "Hydrogen-ionising photons");
    b.log("log L(ion)", after(ion, "L(ion)"), "", "Luminosity or intensity of the ionising radiation");
    b.log("log Q(low)", after(ion, "Q(low):"), "", "Photons below 1 Ryd");
    b.log("log L(low)", after(ion, "L(low)"), "", "Luminosity or intensity below 1 Ryd");
    b.log("log total intensity", inc("Total inten:"), "erg cm⁻² s⁻¹", "Incident radiation, all energies");
    b.add("Mean photon energy", inc("Average nu:"), "Ryd", "");
    b.add("Energy-density temperature", inc("T(En-Den):"), "K", "");
    b.add("Compton temperature", inc("T(Comp):"), "K", "Temperature the gas would reach by Compton scattering alone");
    b.add("G₀ (Tielens & Hollenbach)", after(heff, "G0TH85:"), "", "Far-ultraviolet field in Habing units");
    b.add("G₀ (Draine & Bertoldi)", after(heff, "G0DB96:"), "", "Far-ultraviolet field, other definition");
    b.finish("Incident radiation", &mut out);

    let mut b = Builder::new();
    b.add("Hydrogen density at the face", after(start_zone, "Hden:"), "cm⁻³", "");
    b.add("Electron density at the face", after(start_zone, "Ne:"), "cm⁻³", "");
    b.add("Temperature at the face", after(start_zone, "Te:"), "K", "");
    b.add("Lowest temperature", after(heff, "Te-low:"), "K", "");
    b.add("Highest temperature", after(heff, "Te-hi:"), "K", "");
    b.add("Mean temperature", after(mean, "<Tden>:"), "K", "Weighted by density");
    b.add("Mean mass density", after(mean, "<dens>:"), "g cm⁻³", "");
    b.add("Mean molecular weight", after(mean, "<MolWgt>"), "", "");
    b.add("Mass column", after(misc, "MassTot"), "g cm⁻²", "");
    b.add("Effective column density", after(heff, "Col(Heff):"), "cm⁻²", "");
    b.add("Sound travel time", after(heff, "snd travl time"), "s", "Time for sound to cross the cloud");
    b.add("Mean radiative acceleration", after(mean, "<a>:"), "cm s⁻²", "");
    b.finish("Gas", &mut out);

    let mut b = Builder::new();
    b.log("log heating", after(bud, "Heat:"), "erg cm⁻² s⁻¹", "Total heating, integrated over the cloud");
    b.log("log cooling", after(bud, "Coolg:"), "erg cm⁻² s⁻¹", "Total cooling, integrated over the cloud");
    b.add("Heating − cooling error", after(bud, "Error:"), "%", "Small values mean that energy is conserved");
    b.log("log recombination lines", after(bud, "Rec Lin:"), "", "");
    b.add("Largest P(rad)/P(total)", after(bud, "P(rad/tot)max"), "", "Radiation pressure relative to total pressure");
    b.add("Emission measure n(e)n(p)dl", after(em, "n(e)n(p) dl"), "cm⁻⁵", "");
    b.add("n(e)n(He⁺)dl", after(em, "n(e)n(He+)dl"), "cm⁻⁵", "");
    b.add("n(e)n(He²⁺)dl", after(em, "n(e)n(He++) dl"), "cm⁻⁵", "");
    b.finish("Energy budget", &mut out);

    let mut b = Builder::new();
    for (weight, names, vals) in &averages {
        for (n, v) in names.iter().zip(vals) {
            let unit = if n.starts_with("Te") { "K" } else { "cm⁻³" };
            let label = if averages.len() > 1 { format!("{n} ({})", weight.to_lowercase()) } else { n.clone() };
            b.add(&label, Some(*v), unit, "Mean value over the cloud; the brackets give the weighting");
        }
    }
    b.add("T(O III) from the line ratio", after(peim, "T(OIIIr)"), "K", "Temperature from the [O III] line ratio");
    b.add("T(Balmer jump)", after(peim, "T(Bac)"), "K", "");
    b.add("t² (H structure)", after(peim, "t2(Hstrc)"), "", "Temperature fluctuations (Peimbert)");
    b.add("t² (O²⁺ structure)", after(peim, "t2(O3str)"), "", "Temperature fluctuations (Peimbert)");
    b.finish("Mean temperatures and densities", &mut out);

    let mut b = Builder::new();
    b.add("Dust-to-gas mass ratio", after(dust, "(by mass):"), "", "");
    b.add("A_V / N(H), point source", after(dust, "A(V)/N(H)(pnt):"), "mag cm²", "");
    b.add("A_V / N(H), extended source", after_from(dust, "A(V)/N(H)(pnt)", "(ext):"), "mag cm²", "");
    b.add("R_V", after(dust, ", R:"), "", "Ratio of total to selective extinction");
    b.add("A_V, extended source", after(dust, "AV(ext):"), "mag", "");
    b.add("A_V, point source", after_from(dust, "AV(ext)", "(pnt):"), "mag", "");
    b.finish("Dust", &mut out);

    let tau2 = next("Contin Optical Depths");
    let mut b = Builder::new();
    b.add("τ at 1 Ryd (Lyman limit)", after(tau2, "1r:"), "", "");
    b.add("τ at 1.8 Ryd", after(tau2, "1.8:"), "", "");
    b.add("τ at 4 Ryd", after(tau2, " 4.:"), "", "");
    b.add("τ at Lyman α", after(tau2, "La:"), "", "");
    b.add("τ Balmer continuum", after(tau2, "Ba:"), "", "");
    b.add("τ Paschen continuum", after(tau2, "Pa:"), "", "");
    b.add("τ Compton", after(tau, "COMP:"), "", "");
    b.add("τ H⁻", after(tau, "H-:"), "", "");
    b.finish("Continuum optical depths", &mut out);

    let mut b = Builder::new();
    b.add("Mean Jeans length", after(jeans, "l(cm)"), "cm", "");
    b.add("Mean Jeans mass", after(jeans, "M(sun)"), "M☉", "");
    b.add("Smallest Jeans length", after_from(jeans, "smallest:", "len(cm):"), "cm", "");
    b.add("Smallest Jeans mass", after_from(jeans, "smallest:", "M(sun):"), "M☉", "");
    b.add("T from 21 cm / Lyman α", after(t21, "T(21cm/Ly a)"), "K", "");
    b.add("⟨T⟩ weighted by n(H)/T(kin)", after(t21, "T(<nH/Tkin>)"), "K", "");
    b.add("⟨T⟩ weighted by n(H)/T(spin)", after(t21, "T(<nH/Tspin>)"), "K", "");
    b.add("21 cm brightness temperature", after(t21, "TB21cm:"), "K", "");
    b.add("N(H⁰)/T(spin)", after(t21b, "N(H0/Tspin)"), "cm⁻² K⁻¹", "");
    b.finish("Stability and 21 cm", &mut out);

    let mut b = Builder::new();
    b.add("Mean error, electron density", after(conv, "<eden>"), "%", "");
    b.add("Largest error, electron density", after(conv, "MaxEden"), "%", "");
    b.add("Mean error, heating − cooling", after(conv, "<H-C>"), "%", "");
    b.add("Largest error, heating − cooling", after(conv, "Max(H-C)"), "%", "");
    b.add("Mean error, pressure", after(conv, "<Press>"), "%", "");
    b.add("Largest error, pressure", after(conv, "MaxPrs er"), "%", "");
    b.add("Largest zone-to-zone change of T", after(cont, "chng Te"), "%", "");
    b.add("Largest change of electron density", after(cont, "elec den"), "%", "");
    b.add("Solver iterations per zone", after(misc, "itr/zn:"), "", "");
    b.count("Levels of the H atom", after(hatom, "Hatom level"));
    b.count("Levels of He", after(hatom, "He tot level:"));
    b.finish("Numerical accuracy", &mut out);

    inf.sections = out;
    inf
}

/// What the stop messages of CLOUDY mean, in words, with what to change.
pub fn explain(problem: &str) -> Option<&'static str> {
    let p = problem.to_ascii_lowercase();
    [
        ("same number of continuum shapes and luminosities", "Each radiation shape needs exactly one brightness command (ionization parameter, Q(H), luminosity…). Some sources, such as the ISM field, the CMB or “background”, already bring their own and take none."),
        ("no incident radiation field", "No radiation strikes the cloud: add a source, a blackbody or a table, or at least the CMB."),
        ("not have enough information", "Some essential command is missing, usually the radiation source or the hydrogen density."),
        ("kinetic temperature", "The gas became hotter than CLOUDY can follow. This usually comes from a cool star with a high log U, whose total light is then enormous: raise T★ or lower log U."),
        ("unrecognized command", "A command of the input file is not one CLOUDY knows: check its spelling in the input file."),
        ("too many continua", "Too many radiation sources in one model."),
    ]
    .iter()
    .find(|(k, _)| p.contains(k))
    .map(|(_, v)| *v)
}
