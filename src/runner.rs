//! Running CLOUDY in the background and estimating its progress.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::model::{read_deck, DeckInfo};
use crate::results::parse_table;

#[derive(Clone, Debug)]
pub enum RunEnd {
    Ok,
    Failed(String),
    Cancelled,
}

#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub iteration: u32,
    pub iterations: u32,
    pub zones: usize,
    pub depth: f64,
    pub te: f64,
    pub av: Option<f64>,
    /// 0..1 when an estimate is possible
    pub fraction: Option<f64>,
    /// N(H) so far, cm⁻²
    pub column: f64,
    /// at the last zone: hydrogen and electron density, H⁺ and 2H₂/H fractions
    pub hden: Option<f64>,
    pub eden: Option<f64>,
    pub hplus: Option<f64>,
    pub h2: Option<f64>,
    /// zones computed in the last minute
    pub zones_per_minute: Option<f64>,
}

pub struct Run {
    pub dir: PathBuf,
    pub prefix: String,
    pub temporary: bool,
    pub started: Instant,
    pub finished_after: Option<Duration>,
    child: Arc<Mutex<Option<Child>>>,
    end: Arc<Mutex<Option<RunEnd>>>,
    pub deck: DeckInfo,
    pub progress: Progress,
    last_poll: Option<Instant>,
    t0_log: Option<f64>,
    /// best progress seen in the current iteration, so the bar never goes backwards
    best: (u32, f64),
    /// (seconds, iteration, zones) of the recent readings
    pace: Vec<(f64, u32, usize)>,
}

/// One CLOUDY installation found on this computer.
#[derive(Clone, Debug)]
pub struct Install {
    pub exe: PathBuf,
    /// value for CLOUDY_DATA_PATH (may be a ':' separated search list)
    pub data: String,
    /// where it was found ("~/.zshrc: CLOUDY_EXE", "alias cloudy23 in ~/.bashrc", ...)
    pub origin: String,
    pub version: String,
    pub exists: bool,
}

impl Install {
    pub fn label(&self) -> String {
        let v = if self.version.is_empty() { "CLOUDY".to_string() } else { self.version.clone() };
        let mark = if self.exists { "✔" } else { "✘ not found" };
        format!("{v} — {} {mark}", self.origin)
    }
}

/// "c25.00", "Cloudy23.01" -> "C25.00" / "C23.01"
pub fn version_of(path: &str) -> String {
    let b = path.as_bytes();
    for i in 0..b.len() {
        // two digits, '.', two digits, preceded by 'c'/'C' or "Cloudy"/"cloudy"
        if i + 5 <= b.len() && b[i].is_ascii_digit() && b[i + 1].is_ascii_digit() && b[i + 2] == b'.' && b[i + 3].is_ascii_digit() && b[i + 4].is_ascii_digit() {
            let pre = &path[..i].to_ascii_lowercase();
            if pre.ends_with('c') || pre.ends_with("cloudy") || pre.ends_with("cloudy_") || pre.ends_with("cloudy-") {
                return format!("C{}", &path[i..i + 5]);
            }
        }
    }
    String::new()
}

/// Expand ~, $HOME, $VAR and ${VAR} using variables already met in the file and the environment.
fn expand(v: &str, vars: &std::collections::HashMap<String, String>) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut s = v.trim().to_string();
    if s.starts_with("~/") || s == "~" {
        s = format!("{home}{}", &s[1..]);
    }
    s = s.replace(":~/", &format!(":{home}/"));
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() {
            let (name, next) = if chars[i + 1] == '{' {
                let close = chars[i + 2..].iter().position(|c| *c == '}').map(|p| p + i + 2);
                match close {
                    Some(c) => (chars[i + 2..c].iter().collect::<String>(), c + 1),
                    None => (String::new(), i + 1),
                }
            } else {
                let mut j = i + 1;
                while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                (chars[i + 1..j].iter().collect::<String>(), j)
            };
            if !name.is_empty() {
                let val = vars.get(&name).cloned().or_else(|| std::env::var(&name).ok()).unwrap_or_default();
                out.push_str(&val);
                i = next;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    // drop a trailing comment outside quotes
    let v = if !v.starts_with('"') && !v.starts_with('\'') { v.split(" #").next().unwrap_or(v).trim() } else { v };
    let v = v.trim_end_matches(';');
    if v.len() >= 2 && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\''))) {
        v[1..v.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

/// The data folder that belongs to an executable (…/cXX.YY/source[/sys_gcc]/cloudy.exe -> …/cXX.YY/data).
pub fn data_for_exe(exe: &Path) -> String {
    let mut dir = exe.parent();
    for _ in 0..4 {
        let Some(d) = dir else { break };
        let cand = d.join("data");
        if cand.is_dir() {
            return cand.display().to_string();
        }
        if d.file_name().map(|n| n == "source").unwrap_or(false) {
            if let Some(root) = d.parent() {
                return root.join("data").display().to_string();
            }
        }
        dir = d.parent();
    }
    String::new()
}

/// Read the shell start-up files (~/.bashrc, ~/.zshrc, ...) for CLOUDY_EXE, CLOUDY_DATA_PATH
/// and aliases that start cloudy.exe. The settings that a new shell would end up with come first.
pub fn shell_installs() -> Vec<Install> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let files = [".bashrc", ".bash_profile", ".profile", ".bash_aliases", ".zshenv", ".zprofile", ".zshrc", ".cshrc", ".tcshrc"];
    let mut exports: Vec<(String, String)> = vec![]; // (file, exe)
    let mut aliases: Vec<(String, String, String)> = vec![]; // (file, alias name, exe)
    let mut data_by_file: Vec<(String, String)> = vec![]; // (file, data path), in order
    for f in files {
        let path = home.join(f);
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let short = format!("~/{f}");
        let mut vars = std::collections::HashMap::new();
        for raw in text.lines() {
            let l = raw.trim();
            if l.starts_with('#') || l.is_empty() {
                continue;
            }
            let l = l.strip_prefix("export ").map(str::trim).unwrap_or(l);
            // name=value  /  setenv name value  /  alias name=value
            let (kind, name, value) = if let Some(rest) = l.strip_prefix("setenv ") {
                let mut it = rest.trim().splitn(2, char::is_whitespace);
                ("var", it.next().unwrap_or("").to_string(), it.next().unwrap_or("").to_string())
            } else if let Some(rest) = l.strip_prefix("alias ") {
                let rest = rest.trim();
                match rest.split_once('=') {
                    Some((n, v)) => ("alias", n.trim().to_string(), v.to_string()),
                    None => match rest.split_once(char::is_whitespace) {
                        Some((n, v)) => ("alias", n.trim().to_string(), v.to_string()),
                        None => continue,
                    },
                }
            } else if let Some((n, v)) = l.split_once('=') {
                if n.is_empty() || !n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    continue;
                }
                ("var", n.to_string(), v.to_string())
            } else {
                continue;
            };
            let value = expand(&unquote(&value), &vars);
            if kind == "var" {
                vars.insert(name.clone(), value.clone());
                match name.as_str() {
                    "CLOUDY_EXE" => exports.push((short.clone(), value)),
                    "CLOUDY_DATA_PATH" => data_by_file.push((short.clone(), value)),
                    _ => {}
                }
            } else if let Some(exe) = value.split_whitespace().find(|t| t.to_ascii_lowercase().contains("cloudy") && (t.ends_with(".exe") || t.ends_with("/cloudy"))) {
                aliases.push((short.clone(), name, exe.to_string()));
            }
        }
    }
    let last_data = data_by_file.last().map(|d| d.1.clone());
    let mut out: Vec<Install> = vec![];
    let push = |out: &mut Vec<Install>, exe: String, data: String, origin: String| {
        let p = PathBuf::from(&exe);
        if out.iter().any(|i| i.exe == p) {
            return;
        }
        let exists = p.is_file();
        out.push(Install { version: version_of(&exe), exe: p, data, origin, exists });
    };
    if let Ok(e) = std::env::var("CLOUDY_EXE") {
        let data = std::env::var("CLOUDY_DATA_PATH").ok().unwrap_or_else(|| data_for_exe(Path::new(&e)));
        push(&mut out, e, data, "CLOUDY_EXE variable".into());
    }
    // the last export wins in a shell, so list the exports newest first
    for (file, exe) in exports.iter().rev() {
        let own = data_for_exe(Path::new(exe));
        // use the exported data path when it points to this installation (or nothing better is known)
        let data = match &last_data {
            Some(d) if own.is_empty() || d.split(':').any(|x| x.trim_end_matches('/') == own.trim_end_matches('/')) => d.clone(),
            _ if !own.is_empty() => own,
            Some(d) => d.clone(),
            None => String::new(),
        };
        push(&mut out, exe.clone(), data, format!("CLOUDY_EXE in {file}"));
    }
    for (file, name, exe) in &aliases {
        let own = data_for_exe(Path::new(exe));
        let data = match &last_data {
            Some(d) if d.split(':').any(|x| x.trim_end_matches('/') == own.trim_end_matches('/')) => d.clone(),
            _ => own,
        };
        push(&mut out, exe.clone(), data, format!("alias {name} in {file}"));
    }
    // installed ones first, keeping the order above
    out.sort_by_key(|i| !i.exists);
    out
}

/// Every installation found: shell start-up files first, then PATH and the usual folders.
pub fn find_installs() -> Vec<Install> {
    let mut out = shell_installs();
    for (_, exe, data) in crate::installer::installed() {
        if !out.iter().any(|i| i.exe == exe) {
            let version = version_of(&exe.display().to_string());
            out.push(Install { version, exe, data, origin: "installed by InCloud".into(), exists: true });
        }
    }
    let mut extra: Vec<PathBuf> = vec![];
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            for n in ["cloudy.exe", "cloudy"] {
                extra.push(dir.join(n));
            }
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    for base in [home.clone(), format!("{home}/Software"), format!("{home}/software"), format!("{home}/codes"), "/opt".into(), "/usr/local".into()] {
        for sub in ["cloudy", "Cloudy", "c25.00", "c25.01", "c23.01", "c23.00", "c22.02", "c17.03", "c17.02"] {
            extra.push(PathBuf::from(format!("{base}/{sub}/source/cloudy.exe")));
        }
    }
    if let Ok(rd) = std::fs::read_dir(&home) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with('c') && n.len() >= 3 && n.as_bytes()[1].is_ascii_digit() {
                extra.push(e.path().join("source/cloudy.exe"));
            }
        }
    }
    for p in extra {
        if p.is_file() && !out.iter().any(|i| i.exe == p) {
            let s = p.display().to_string();
            out.push(Install { version: version_of(&s), data: data_for_exe(&p), origin: "usual folders".into(), exists: true, exe: p });
        }
    }
    out.sort_by_key(|i| !i.exists);
    out
}

/// The installation to use by default (the first one that exists).
pub fn detect_cloudy() -> Option<Install> {
    find_installs().into_iter().find(|i| i.exists)
}

impl Run {
    pub fn start(
        exe: &Path,
        data_path: &str,
        dir: PathBuf,
        prefix: &str,
        input: &str,
        extra_files: &[(String, String)],
        temporary: bool,
    ) -> Result<Run, String> {
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create folder {}: {e}", dir.display()))?;
        for (rel, content) in extra_files {
            let p = dir.join(rel);
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d).map_err(|e| format!("Cannot create folder {}: {e}", d.display()))?;
            }
            std::fs::write(&p, content).map_err(|e| format!("Cannot write {}: {e}", p.display()))?;
        }
        let in_path = dir.join(format!("{prefix}.in"));
        std::fs::write(&in_path, input).map_err(|e| format!("Cannot write {}: {e}", in_path.display()))?;
        let stdin = std::fs::File::open(&in_path).map_err(|e| e.to_string())?;
        let stdout = std::fs::File::create(dir.join(format!("{prefix}.out"))).map_err(|e| e.to_string())?;
        let mut cmd = Command::new(exe);
        cmd.current_dir(&dir).stdin(Stdio::from(stdin)).stdout(Stdio::from(stdout)).stderr(Stdio::piped());
        let data = data_path.trim();
        if !data.is_empty() {
            // files written beside the input (a custom SED) are only found if "." is searched
            let has_dot = data.split(':').any(|p| p.trim() == ".");
            if !extra_files.is_empty() && !has_dot {
                cmd.env("CLOUDY_DATA_PATH", format!(".:{data}"));
            } else {
                cmd.env("CLOUDY_DATA_PATH", data);
            }
        }
        let child = cmd.spawn().map_err(|e| format!("Cannot start CLOUDY ({}): {e}", exe.display()))?;
        let child = Arc::new(Mutex::new(Some(child)));
        let end = Arc::new(Mutex::new(None));
        {
            let child = child.clone();
            let end = end.clone();
            let out_path = dir.join(format!("{prefix}.out"));
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(200));
                let mut guard = child.lock().unwrap();
                let Some(c) = guard.as_mut() else {
                    return; // cancelled
                };
                match c.try_wait() {
                    Ok(Some(status)) => {
                        let mut err = String::new();
                        if let Some(mut s) = c.stderr.take() {
                            use std::io::Read;
                            let _ = s.read_to_string(&mut err);
                        }
                        let out = std::fs::read(&out_path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
                        // CLOUDY returns an error code when it printed warnings, even after a complete run
                        let ok_text = out.contains("exited OK") || out.contains("ends: OK") || out.contains("Cloudy ends:");
                        let r = if ok_text || (status.success() && !out.contains("ABORT")) {
                            RunEnd::Ok
                        } else {
                            let tail: Vec<&str> = out.lines().rev().take(12).collect();
                            let tail: Vec<&str> = tail.into_iter().rev().collect();
                            RunEnd::Failed(format!(
                                "CLOUDY stopped with exit code {:?}.\n{}\n{}",
                                status.code(),
                                err.trim(),
                                tail.join("\n")
                            ))
                        };
                        *end.lock().unwrap() = Some(r);
                        *guard = None;
                        return;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        *end.lock().unwrap() = Some(RunEnd::Failed(e.to_string()));
                        *guard = None;
                        return;
                    }
                }
            });
        }
        let deck = read_deck(input);
        Ok(Run {
            dir,
            prefix: prefix.to_string(),
            temporary,
            started: Instant::now(),
            finished_after: None,
            child,
            end,
            progress: Progress { iterations: deck.iterations.max(1), iteration: 1, ..Default::default() },
            deck,
            last_poll: None,
            t0_log: None,
            best: (1, 0.0),
            pace: vec![],
        })
    }

    pub fn cancel(&mut self) {
        if let Some(mut c) = self.child.lock().unwrap().take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        *self.end.lock().unwrap() = Some(RunEnd::Cancelled);
    }

    pub fn ended(&self) -> Option<RunEnd> {
        self.end.lock().unwrap().clone()
    }

    pub fn elapsed(&self) -> Duration {
        self.finished_after.unwrap_or_else(|| self.started.elapsed())
    }

    /// Re-read the zone-by-zone overview to estimate progress (throttled).
    pub fn poll(&mut self) {
        if let Some(t) = self.last_poll {
            if t.elapsed() < Duration::from_millis(700) {
                return;
            }
        }
        self.last_poll = Some(Instant::now());
        let path = self.dir.join(format!("{}.prog", self.prefix));
        let Ok(bytes) = std::fs::read(&path) else { return };
        let text = String::from_utf8_lossy(&bytes);
        // count iterations: a depth smaller than the previous one starts a new iteration
        let mut iteration = 1u32;
        let mut prev = f64::NAN;
        for line in text.lines() {
            if line.starts_with('#') {
                continue;
            }
            if let Some(d) = line.split('\t').next().and_then(|s| s.trim().parse::<f64>().ok()) {
                if prev.is_finite() && d < prev * 0.999 {
                    iteration += 1;
                }
                prev = d;
            }
        }
        let t = parse_table(&text);
        let n = t.rows();
        if n == 0 {
            return;
        }
        let depth = t.data[0][n - 1];
        let te = t.col(&["Te"]).map(|c| c[n - 1]).unwrap_or(f64::NAN);
        let av = t.col(&["AV(extend)", "AV(point)"]).map(|c| c[n - 1]);
        if self.t0_log.is_none() {
            if let Some(c) = t.col(&["Te"]) {
                self.t0_log = c.first().map(|v| v.max(1.0).log10());
            }
        }
        // column density
        let mut col = 0.0;
        let (mut hden, mut eden) = (None, None);
        if let Some(h) = t.col(&["hden"]) {
            let is_log = crate::results::density_is_log(h, self.deck.log_hden);
            let lin = |v: f64| if is_log { 10f64.powf(v) } else { v };
            hden = h.last().map(|v| lin(*v));
            eden = t.col(&["eden"]).and_then(|e| e.last()).map(|v| lin(*v));
            for i in 0..n {
                let dr = if i == 0 { t.data[0][0] } else { t.data[0][i] - t.data[0][i - 1] };
                let nh = if is_log { 10f64.powf(h[i]) } else { h[i] };
                col += nh * dr.max(0.0);
            }
        }
        let d = &self.deck;
        let mut fr: Vec<f64> = vec![];
        if let Some(z) = d.stop_zones {
            fr.push(n as f64 / z.max(1) as f64);
        }
        if let (Some(a), Some(stop)) = (av, d.stop_av) {
            if stop > 0.0 {
                fr.push(a / stop);
            }
        }
        if let Some(lt) = d.stop_log_thickness {
            let d0 = t.data[0][0].max(1.0).log10();
            if lt > d0 {
                fr.push((depth.max(1.0).log10() - d0) / (lt - d0));
            }
        }
        if let Some(lc) = d.stop_log_column {
            if col > 0.0 {
                let c0 = (lc - 4.0).max(0.0);
                fr.push((col.log10() - c0) / (lc - c0));
            }
        }
        if let (Some(ts), Some(t0)) = (d.stop_temp, self.t0_log) {
            if te.is_finite() && te > 0.0 && t0 > ts.max(1.0).log10() {
                fr.push(((t0 - te.log10()) / (t0 - ts.max(1.0).log10())).clamp(0.0, 1.0) * 0.9);
            }
        }
        let mut within = fr.iter().cloned().filter(|x| x.is_finite()).fold(f64::NAN, f64::max);
        if within.is_finite() {
            if self.best.0 == iteration {
                within = within.max(self.best.1);
            }
            self.best = (iteration, within);
        }
        // when iterating to convergence the number of passes isn't known: usually a couple more
        let iters = if self.deck.converge { (iteration + 1).max(3).min(self.deck.iterations.max(iteration)) } else { self.progress.iterations.max(iteration) };
        let fraction = if within.is_finite() {
            Some(((iteration - 1) as f64 + within.clamp(0.0, 0.99)) / iters as f64)
        } else {
            None
        };
        let last = |names: &[&str]| t.col(names).map(|c| crate::results::to_linear(c)).and_then(|c| c.last().copied()).filter(|v| v.is_finite());
        let (hplus, h2) = (last(&["HII", "H+"]), last(&["2H_2/H", "2H2/H", "H2"]));
        // the pace of the last minute: zones of this iteration against the clock
        let now = self.started.elapsed().as_secs_f64();
        self.pace.push((now, iteration, n));
        self.pace.retain(|p| now - p.0 <= 60.0 && p.1 == iteration);
        let zones_per_minute = match (self.pace.first(), self.pace.last()) {
            (Some(a), Some(b)) if b.0 - a.0 > 5.0 => Some((b.2 as f64 - a.2 as f64) / (b.0 - a.0) * 60.0),
            _ => None,
        };
        self.progress = Progress { iteration, iterations: iters, zones: n, depth, te, av, fraction, column: col, hden, eden, hplus, h2, zones_per_minute };
    }

    /// Estimated remaining time, when it can be guessed.
    pub fn remaining(&self) -> Option<Duration> {
        let f = self.progress.fraction?;
        let el = self.started.elapsed().as_secs_f64();
        if f < 0.03 || el < 2.0 {
            return None;
        }
        Some(Duration::from_secs_f64((el / f - el).max(0.0)))
    }
}

pub fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}h {:02}m {:02}s", s / 3600, (s / 60) % 60, s % 60)
    } else if s >= 60 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{:.1}s", d.as_secs_f64())
    }
}

/// A fresh folder under the system temporary directory.
pub fn temp_run_dir() -> PathBuf {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    std::env::temp_dir().join(format!("incloud-{}-{}", std::process::id(), stamp))
}

pub fn timestamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    // civil date from unix time (UTC)
    let days = secs.div_euclid(86400);
    let sod = secs.rem_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}{:02}{:02}-{:02}{:02}{:02}", y, m, d, sod / 3600, (sod / 60) % 60, sod % 60)
}

pub fn copy_dir(from: &Path, to: &Path) -> Result<usize, String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    let mut n = 0;
    for e in std::fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
        let p = e.path();
        if p.is_file() {
            std::fs::copy(&p, to.join(e.file_name())).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    Ok(n)
}
