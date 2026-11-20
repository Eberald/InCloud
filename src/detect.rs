//! Finding the files of a CLOUDY run, whatever they are called: first from the save commands of
//! its input file, then by looking inside every file of the folder.

use std::path::{Path, PathBuf};

/// What a file holds.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Output,
    Overview,
    Continuum,
    Element(String),
    Grains,
    Heating,
    Cooling,
    Emissivity,
    /// "save lines, cumulative": the intensity of each line integrated up to each depth
    Cumulative,
    Formation(String),
    Destruction(String),
}

impl Kind {
    pub fn name(&self) -> String {
        match self {
            Kind::Output => "Text output".into(),
            Kind::Overview => "Overview (structure)".into(),
            Kind::Continuum => "Continuum (SED)".into(),
            Kind::Element(e) => format!("Ionisation of {e}"),
            Kind::Grains => "Grain temperatures".into(),
            Kind::Heating => "Heating".into(),
            Kind::Cooling => "Cooling".into(),
            Kind::Emissivity => "Line emissivities".into(),
            Kind::Cumulative => "Line intensities (cumulative)".into(),
            Kind::Formation(s) => format!("Formation rates of {s}"),
            Kind::Destruction(s) => format!("Destruction rates of {s}"),
        }
    }

    /// What InCloud draws from it.
    pub fn used_for(&self) -> &'static str {
        match self {
            Kind::Output => "Run info, warnings, the strongest lines",
            Kind::Overview => "every plot against depth, the fronts and the regions",
            Kind::Continuum => "the SED plot",
            Kind::Element(_) => "the ionisation plot",
            Kind::Grains => "the grain temperature plot",
            Kind::Heating | Kind::Cooling => "the heating and cooling plot",
            Kind::Emissivity => "line intensities, emissivities, ratios, BPT",
            Kind::Cumulative => "line intensities, ratios, BPT, and where lines form",
            Kind::Formation(_) | Kind::Destruction(_) => "reaction rates and the chemical network",
        }
    }

    /// The command that saves it, for the input file.
    pub fn command(&self, prefix: &str) -> String {
        match self {
            Kind::Output => format!("(what CLOUDY prints: cloudy.exe < {prefix}.in > {prefix}.out)"),
            Kind::Overview => format!("save overview \"{prefix}.ovr\" last"),
            Kind::Continuum => format!("save continuum \"{prefix}.con\" units microns last"),
            Kind::Element(e) => format!("save element {} \"{prefix}.ele_{e}\" last", element_name(e).unwrap_or("oxygen")),
            Kind::Grains => format!("save grain temperature \"{prefix}.gtemp\" last"),
            Kind::Heating => format!("save heating \"{prefix}.het\" last"),
            Kind::Cooling => format!("save cooling \"{prefix}.col\" last"),
            Kind::Emissivity => format!("save lines, emissivity \"{prefix}.ems\" last\n  …one line label per row…\nend of lines"),
            Kind::Cumulative => format!("save lines, cumulative \"{prefix}.lnc\" last\n  …one line label per row…\nend of lines"),
            Kind::Formation(s) => format!("save chemistry rates creation \"{prefix}.chf_{}\" \"{s}\" last", crate::model::species_file(s)),
            Kind::Destruction(s) => format!("save chemistry rates destruction \"{prefix}.chd_{}\" \"{s}\" last", crate::model::species_file(s)),
        }
    }
}

/// How a file was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// named by a save command of the input file
    Command,
    /// recognised from its first line
    Content,
    /// the name InCloud gives it
    Name,
}

impl How {
    pub fn text(self) -> &'static str {
        match self {
            How::Command => "named in the input file",
            How::Content => "recognised from its content",
            How::Name => "InCloud's own name",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Found {
    pub kind: Kind,
    pub path: PathBuf,
    pub how: How,
}

/// All the files of one run.
#[derive(Clone, Debug, Default)]
pub struct Files {
    pub found: Vec<Found>,
    /// files of the folder that hold nothing InCloud reads
    pub others: Vec<String>,
}

impl Files {
    pub fn get(&self, k: &Kind) -> Option<&Path> {
        self.found.iter().find(|f| &f.kind == k).map(|f| f.path.as_path())
    }
    pub fn elements(&self) -> Vec<(String, PathBuf)> {
        self.found.iter().filter_map(|f| if let Kind::Element(e) = &f.kind { Some((e.clone(), f.path.clone())) } else { None }).collect()
    }
    /// (species, formation file, destruction file)
    pub fn chemistry(&self) -> Vec<(String, Option<PathBuf>, Option<PathBuf>)> {
        let mut v: Vec<(String, Option<PathBuf>, Option<PathBuf>)> = vec![];
        for f in &self.found {
            let (sp, form) = match &f.kind {
                Kind::Formation(s) => (s, true),
                Kind::Destruction(s) => (s, false),
                _ => continue,
            };
            let i = match v.iter().position(|x| &x.0 == sp) {
                Some(i) => i,
                None => {
                    v.push((sp.clone(), None, None));
                    v.len() - 1
                }
            };
            if form {
                v[i].1 = Some(f.path.clone());
            } else {
                v[i].2 = Some(f.path.clone());
            }
        }
        v
    }
    fn add(&mut self, kind: Kind, path: PathBuf, how: How) {
        if path.is_file() && self.get(&kind).is_none() && !self.found.iter().any(|f| f.path == path) {
            self.found.push(Found { kind, path, how });
        }
    }
}

const ELEMENT_NAMES: [(&str, &str); 30] = [
    ("H", "hydrogen"), ("He", "helium"), ("Li", "lithium"), ("Be", "beryllium"), ("B", "boron"), ("C", "carbon"), ("N", "nitrogen"), ("O", "oxygen"),
    ("F", "fluorine"), ("Ne", "neon"), ("Na", "sodium"), ("Mg", "magnesium"), ("Al", "aluminium"), ("Si", "silicon"), ("P", "phosphorus"), ("S", "sulphur"),
    ("Cl", "chlorine"), ("Ar", "argon"), ("K", "potassium"), ("Ca", "calcium"), ("Sc", "scandium"), ("Ti", "titanium"), ("V", "vanadium"), ("Cr", "chromium"),
    ("Mn", "manganese"), ("Fe", "iron"), ("Co", "cobalt"), ("Ni", "nickel"), ("Cu", "copper"), ("Zn", "zinc"),
];

fn element_name(sym: &str) -> Option<&'static str> {
    ELEMENT_NAMES.iter().find(|e| e.0 == sym).map(|e| e.1)
}

/// The symbol of an element written as CLOUDY reads it (only the first four letters count).
fn element_symbol(word: &str) -> Option<&'static str> {
    let w = word.to_ascii_lowercase();
    if w.len() < 4 {
        return None;
    }
    let head = &w[..4];
    // "sulfur" and "aluminum" are accepted too
    ELEMENT_NAMES.iter().find(|e| e.1.starts_with(head) || (head == "sulf" && e.0 == "S") || (head == "alum" && e.0 == "Al")).map(|e| e.0)
}

/// What a save command saves, and to which file.
fn from_command(line: &str) -> Option<(Kind, String)> {
    let t = line.trim();
    let low = t.to_ascii_lowercase();
    if !(low.starts_with("save") || low.starts_with("punch")) {
        return None;
    }
    let quoted: Vec<&str> = t.split('"').skip(1).step_by(2).collect();
    let file = quoted.first()?.to_string();
    let words = low.split('"').next().unwrap_or("");
    let has = |w: &str| words.split(|c: char| c.is_whitespace() || c == ',').any(|x| x.starts_with(w));
    let kind = if has("overview") || has("over") {
        Kind::Overview
    } else if has("continuum") && !has("emitted") && !has("diffuse") && !has("incident") {
        Kind::Continuum
    } else if has("element") {
        let sym = words.split_whitespace().skip(2).find_map(element_symbol)?;
        Kind::Element(sym.to_string())
    } else if has("grain") && (has("temperature") || has("temp")) {
        Kind::Grains
    } else if has("heating") {
        Kind::Heating
    } else if has("cooling") {
        Kind::Cooling
    } else if has("lines") && has("cumulative") {
        Kind::Cumulative
    } else if has("lines") && has("emissivity") {
        Kind::Emissivity
    } else if has("chemistry") && has("rates") {
        let sp = quoted.get(1)?.to_string();
        if has("destruction") {
            Kind::Destruction(sp)
        } else {
            Kind::Formation(sp)
        }
    } else {
        return None;
    };
    Some((kind, file))
}

/// The species common to every reaction of a rates file: on the right of all of them (formation)
/// or on the left (destruction).
fn rates_species(cols: &[&str]) -> Option<Kind> {
    let reactions: Vec<(Vec<&str>, Vec<&str>)> = cols
        .iter()
        .filter_map(|c| c.trim().split_once("=>"))
        .map(|(a, b)| (a.split(',').map(str::trim).collect(), b.split(',').map(str::trim).collect()))
        .collect();
    if reactions.is_empty() {
        return None;
    }
    let common = |side: &dyn Fn(&(Vec<&str>, Vec<&str>)) -> Vec<String>| -> Option<String> {
        let first = side(&reactions[0]);
        first.into_iter().find(|s| reactions.iter().all(|r| side(r).contains(s)))
    };
    // a species on both sides of a reaction is a catalyst there: count only true formation or loss
    let products = |r: &(Vec<&str>, Vec<&str>)| r.1.iter().filter(|s| !r.0.contains(s)).map(|s| s.to_string()).collect();
    let reactants = |r: &(Vec<&str>, Vec<&str>)| r.0.iter().filter(|s| !r.1.contains(s)).map(|s| s.to_string()).collect();
    if let Some(s) = common(&products) {
        return Some(Kind::Formation(s));
    }
    common(&reactants).map(Kind::Destruction)
}

/// What a file holds, from its first line.
fn from_content(path: &Path) -> Option<Kind> {
    use std::io::{BufRead, BufReader, Read};
    let f = std::fs::File::open(path).ok()?;
    let mut first = String::new();
    BufReader::new(f).take(64 * 1024).read_line(&mut first).ok()?;
    let head = first.trim_end();
    if !head.starts_with('#') {
        return None;
    }
    let cols: Vec<&str> = head.trim_start_matches('#').split('\t').map(str::trim).collect();
    let low = head.to_ascii_lowercase();
    let c0 = cols.first().map(|c| c.to_ascii_lowercase()).unwrap_or_default();
    if c0.starts_with("cont") && low.contains("incident") && low.contains("trans") {
        return Some(Kind::Continuum);
    }
    if low.contains("heat fracs") {
        return Some(Kind::Heating);
    }
    if low.contains("cool fracs") {
        return Some(Kind::Cooling);
    }
    if !c0.starts_with("depth") {
        return None;
    }
    let rest = &cols[1..];
    if rest.iter().any(|c| c.contains("=>")) {
        return rates_species(rest);
    }
    if rest.iter().any(|c| *c == "Te") && rest.iter().any(|c| *c == "hden") {
        return Some(Kind::Overview);
    }
    if rest.iter().filter(|c| c.starts_with("gra-") || c.starts_with("sil-") || c.starts_with("pah") || c.contains("grn")).count() >= 1 && rest.iter().all(|c| c.contains('-') || c.is_empty()) {
        return Some(Kind::Grains);
    }
    // "O  O+  O+2 …": an element and its ions
    if let (Some(a), Some(b)) = (rest.first(), rest.get(1)) {
        if ELEMENT_NAMES.iter().any(|e| e.0 == *a) && *b == format!("{a}+") {
            return Some(Kind::Element(a.to_string()));
        }
    }
    // line labels such as "O  3 5006.84A"
    let is_label = |c: &&str| {
        let c = c.trim();
        let last = c.split_whitespace().last().unwrap_or("");
        c.len() > 5 && last.len() > 1 && last.ends_with(['A', 'm', 'c']) && last[..last.len() - 1].parse::<f64>().is_ok()
    };
    if !rest.is_empty() && rest.iter().filter(|c| is_label(c)).count() * 2 >= rest.len() {
        return Some(if growing(path) { Kind::Cumulative } else { Kind::Emissivity });
    }
    None
}

/// Every column only grows with depth, as an integral does: a cumulative file. Emissivities rise
/// and fall across the cloud.
fn growing(path: &Path) -> bool {
    use std::io::{BufRead, BufReader};
    let Ok(f) = std::fs::File::open(path) else { return false };
    let rows: Vec<Vec<f64>> = BufReader::new(f)
        .lines()
        .map_while(Result::ok)
        .filter(|l| !l.starts_with('#'))
        .take(4000)
        .map(|l| l.split('\t').skip(1).map(|v| v.trim().parse::<f64>().unwrap_or(f64::NAN)).collect())
        .collect();
    if rows.len() < 3 {
        return false;
    }
    let cols = rows[0].len();
    let mut rising = 0;
    for c in 0..cols {
        let v: Vec<f64> = rows.iter().filter_map(|r| r.get(c).copied()).filter(|x| x.is_finite()).collect();
        if v.len() < 3 || v.windows(2).any(|w| w[1] < w[0] * (1.0 - 1e-6) - 1e-300) {
            continue;
        }
        if v.last() > v.first() {
            rising += 1;
        }
    }
    // a column that falls anywhere is an emissivity; most columns rising and none falling: cumulative
    let falling = (0..cols).any(|c| {
        let v: Vec<f64> = rows.iter().filter_map(|r| r.get(c).copied()).filter(|x| x.is_finite()).collect();
        v.windows(2).any(|w| w[1] < w[0] * (1.0 - 1e-6) - 1e-300)
    });
    !falling && rising * 2 >= cols
}

/// The input file of a run: `prefix.in`, else the only input file of the folder, else what the
/// text output repeats of it.
pub fn input_of(dir: &Path, prefix: &str, out_text: Option<&str>) -> Option<(String, Option<PathBuf>)> {
    let own = dir.join(format!("{prefix}.in"));
    if let Ok(t) = std::fs::read_to_string(&own) {
        return Some((t, Some(own)));
    }
    let ins: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "in")).collect();
    if let [one] = ins.as_slice() {
        if let Ok(t) = std::fs::read_to_string(one) {
            return Some((t, Some(one.clone())));
        }
    }
    let echoed = crate::runinfo::parse(out_text?).input;
    (!echoed.is_empty()).then(|| (echoed.join("\n") + "\n", None))
}

/// Find every file of the run `prefix` in `dir`.
pub fn find(dir: &Path, prefix: &str, input: Option<&str>, single_run: bool) -> Files {
    let mut files = Files::default();
    let own = |ext: &str| dir.join(format!("{prefix}.{ext}"));
    files.add(Kind::Output, own("out"), How::Name);

    // 1. the save commands of the input file; "set save prefix" goes in front of their names
    if let Some(text) = input {
        let save_prefix = text.lines().find_map(|l| {
            let t = l.trim();
            t.to_ascii_lowercase().starts_with("set save prefix").then(|| t.split('"').nth(1).map(str::to_string)).flatten()
        });
        // the overview saved at the end beats the one saved while running
        let mut saves: Vec<(Kind, String, bool)> = text
            .lines()
            .filter_map(|l| from_command(l).map(|(k, f)| (k, f, l.to_ascii_lowercase().contains(" last"))))
            .collect();
        saves.sort_by_key(|s| !s.2);
        for (kind, file, _) in saves {
            let file = match &save_prefix {
                Some(p) => format!("{p}{file}"),
                None => file,
            };
            files.add(kind, dir.join(&file), How::Command);
        }
    }

    // 2. InCloud's own names
    for (kind, ext) in [(Kind::Overview, "ovr"), (Kind::Continuum, "con"), (Kind::Grains, "gtemp"), (Kind::Heating, "het"), (Kind::Cooling, "col"), (Kind::Emissivity, "ems")] {
        files.add(kind, own(ext), How::Name);
    }

    // 3. every other file, by what it holds: only this run's files when the folder has several
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    names.sort();
    for p in names {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if files.found.iter().any(|f| f.path == p) || name.ends_with(".in") || name.ends_with(".out") || name.starts_with('.') {
            continue;
        }
        if !single_run && !name.starts_with(prefix) {
            continue;
        }
        // the zone-by-zone overview written while running is only for the progress
        if name.ends_with(".prog") {
            continue;
        }
        match from_content(&p) {
            Some(kind) => {
                let how = How::Content;
                if files.get(&kind).is_none() {
                    files.found.push(Found { kind, path: p, how });
                }
            }
            None => files.others.push(name),
        }
    }
    files
}

/// Developer check: print what is found in a folder.
pub fn check(dir: &Path) {
    for r in list_runs(dir) {
        println!("run {:?} in {} title {:?} finished {:?} files {} plots {}", r.name, r.dir.display(), r.title, r.finished, r.kinds, r.plots);
    }
    let mut runs = crate::results::runs_in(dir);
    if runs.is_empty() {
        runs.extend(loose_run(dir));
    }
    println!("runs: {runs:?}");
    let prefix = runs.first().cloned().unwrap_or_default();
    let out = std::fs::read_to_string(dir.join(format!("{prefix}.out"))).ok();
    let input = input_of(dir, &prefix, out.as_deref());
    let files = find(dir, &prefix, input.as_ref().map(|i| i.0.as_str()), runs.len() <= 1);
    for f in &files.found {
        println!("{:<34} {:<28} {}", f.kind.name(), f.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), f.how.text());
    }
    println!("not used: {:?}", files.others);
}

/// A run whose files have no .in, .out or .ovr name: called after its overview file.
pub fn loose_run(dir: &Path) -> Option<String> {
    let mut names: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    names.sort();
    names.into_iter().find(|p| from_content(p) == Some(Kind::Overview)).and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
}

/// One run found in a folder.
#[derive(Clone, Debug)]
pub struct RunEntry {
    pub dir: PathBuf,
    pub name: String,
    pub title: String,
    pub modified: Option<std::time::SystemTime>,
    /// None when there is no text output
    pub finished: Option<bool>,
    pub kinds: usize,
    pub plots: bool,
}

/// The start and the end of a file, without reading what is in between (outputs can be large).
fn head_tail(path: &Path, head: usize, tail: usize) -> Option<(String, String)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut h = vec![];
    (&mut f).take(head as u64).read_to_end(&mut h).ok()?;
    let mut t = vec![];
    if len > head as u64 {
        f.seek(SeekFrom::Start(len.saturating_sub(tail as u64).max(head as u64))).ok()?;
        f.take(tail as u64).read_to_end(&mut t).ok()?;
    }
    Some((String::from_utf8_lossy(&h).into_owned(), String::from_utf8_lossy(&t).into_owned()))
}

fn title_in(text: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let t = l.trim().trim_matches('*').trim();
        let low = t.to_ascii_lowercase();
        (low.starts_with("title ") || low.starts_with("title\t")).then(|| t[5..].trim().to_string())
    })
}

/// A quick look at one run, for the list: only the input and the ends of the output are read.
fn entry(dir: &Path, name: &str, names: &[(String, Option<std::time::SystemTime>)]) -> RunEntry {
    use std::io::Read;
    let input = std::fs::File::open(dir.join(format!("{name}.in"))).ok().and_then(|f| {
        let mut b = vec![];
        f.take(256 * 1024).read_to_end(&mut b).ok()?;
        Some(String::from_utf8_lossy(&b).into_owned())
    });
    let out = head_tail(&dir.join(format!("{name}.out")), 96 * 1024, 16 * 1024);
    let title = input.as_deref().and_then(title_in).or_else(|| out.as_ref().and_then(|o| title_in(&o.0))).unwrap_or_default();
    let finished = out.as_ref().map(|(h, t)| h.contains("Cloudy ends") || t.contains("Cloudy ends"));
    let dotted = format!("{name}.");
    let mine: Vec<&(String, Option<std::time::SystemTime>)> = names.iter().filter(|n| n.0.starts_with(&dotted) || n.0 == name).collect();
    let saves = input.as_deref().map(|t| t.lines().filter(|l| from_command(l).is_some()).count()).unwrap_or(0);
    let plots = names.iter().any(|n| n.0 == format!("{name}.ovr"))
        || input.as_deref().is_some_and(|t| t.lines().any(|l| matches!(from_command(l), Some((Kind::Overview, _)))))
        || (input.is_none() && out.is_none());
    RunEntry {
        dir: dir.to_path_buf(),
        name: name.to_string(),
        title,
        modified: mine.iter().filter_map(|n| n.1).max(),
        finished,
        kinds: mine.len().max(saves),
        plots,
    }
}

/// How a search through folders is going; shared with the window that shows it.
#[derive(Default)]
pub struct Search {
    pub folders: std::sync::atomic::AtomicUsize,
    pub files: std::sync::atomic::AtomicUsize,
    pub runs: std::sync::atomic::AtomicUsize,
    pub current: std::sync::Mutex<String>,
    pub cancel: std::sync::atomic::AtomicBool,
    /// file endings seen, to explain an empty result
    pub endings: std::sync::Mutex<std::collections::BTreeMap<String, usize>>,
    /// stopped at a limit
    pub cut: std::sync::atomic::AtomicBool,
}

impl Search {
    /// The commonest file endings seen, as "out ×12, txt ×4".
    pub fn endings_text(&self) -> String {
        let e = self.endings.lock().map(|g| g.clone()).unwrap_or_default();
        let mut v: Vec<(String, usize)> = e.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v.iter().take(8).map(|(k, n)| format!(".{k} ×{n}")).collect::<Vec<_>>().join(", ")
    }
}

/// Folders looked into below the chosen one.
pub const DEPTH: usize = 4;

/// Every run in a folder and in its sub-folders, newest first.
pub fn list_runs(dir: &Path) -> Vec<RunEntry> {
    list_runs_with(dir, &Search::default())
}

/// The same, telling `search` how far it got; stops when it is cancelled.
pub fn list_runs_with(dir: &Path, search: &Search) -> Vec<RunEntry> {
    use std::sync::atomic::Ordering::Relaxed;
    let mut out = vec![];
    let mut stack = vec![(dir.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        if search.cancel.load(Relaxed) {
            break;
        }
        search.folders.fetch_add(1, Relaxed);
        if let Ok(mut c) = search.current.lock() {
            *c = d.strip_prefix(dir).map(|p| p.display().to_string()).unwrap_or_default();
        }
        // one listing of the folder serves everything below
        let mut names: Vec<(String, Option<std::time::SystemTime>)> = vec![];
        let mut subs: Vec<PathBuf> = vec![];
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with('.') {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() || (ft.is_symlink() && e.path().is_dir()) {
                subs.push(e.path());
            } else {
                if let Some(x) = Path::new(&n).extension() {
                    if let Ok(mut m) = search.endings.lock() {
                        *m.entry(x.to_string_lossy().to_ascii_lowercase()).or_default() += 1;
                    }
                }
                names.push((n, e.metadata().and_then(|m| m.modified()).ok()));
            }
        }
        search.files.fetch_add(names.len(), Relaxed);
        let mut runs: Vec<String> = vec![];
        for (n, _) in &names {
            let p = Path::new(n);
            if p.extension().is_some_and(|x| x == "out" || x == "ovr" || x == "in") {
                if let Some(stem) = p.file_stem().map(|x| x.to_string_lossy().into_owned()) {
                    if !runs.contains(&stem) {
                        runs.push(stem);
                    }
                }
            }
        }
        if runs.is_empty() && names.len() <= 400 {
            runs.extend(loose_run(&d));
        }
        for n in &runs {
            if search.cancel.load(Relaxed) {
                break;
            }
            out.push(entry(&d, n, &names));
            search.runs.fetch_add(1, Relaxed);
        }
        if depth < DEPTH {
            subs.sort();
            for s in subs.into_iter().rev() {
                stack.push((s, depth + 1));
            }
        }
        if out.len() >= 50_000 || search.folders.load(Relaxed) >= 50_000 {
            search.cut.store(true, Relaxed);
            break;
        }
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then(a.name.cmp(&b.name)));
    out
}
