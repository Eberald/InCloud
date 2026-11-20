//! Installing a version of CLOUDY: download the official source, unpack it, compile it, check it.
//!
//! When InCloud itself runs inside a Flatpak sandbox, the compiler lives outside it: the commands
//! then go through `flatpak-spawn --host`.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const BASE: &str = "https://data.nublado.org/cloudy_releases";

/// Versions known to exist, with the size of their archive, used until the list is read online.
const KNOWN: [(&str, u64); 4] = [("c25.00", 988_518_032), ("c23.01", 338_434_070), ("c22.02", 332_713_880), ("c17.03", 139_863_662)];

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// "c25.00"
    pub name: String,
    pub bytes: u64,
}

impl Release {
    pub fn url(&self) -> String {
        format!("{BASE}/{}/{}.tar.gz", &self.name[..3], self.name)
    }
    /// "C25.00"
    pub fn label(&self) -> String {
        self.name.to_uppercase()
    }
}

pub fn known() -> Vec<Release> {
    KNOWN.iter().map(|(n, b)| Release { name: n.to_string(), bytes: *b }).collect()
}

/// True for "c25.00".
fn is_release(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 6 && b[0] == b'c' && b[1].is_ascii_digit() && b[2].is_ascii_digit() && b[3] == b'.' && b[4].is_ascii_digit() && b[5].is_ascii_digit()
}

fn links(page: &str) -> Vec<String> {
    page.split("href=\"").skip(1).filter_map(|p| p.split('"').next()).map(str::to_string).collect()
}

/// The versions on the official server, newest first (from C17 on, the ones this program can read).
pub fn fetch_releases() -> Option<Vec<Release>> {
    let index = fetch_text(&format!("{BASE}/"))?;
    let mut dirs: Vec<String> = links(&index)
        .into_iter()
        .filter_map(|l| l.strip_suffix('/').map(str::to_string))
        .filter(|d| d.len() == 3 && d.starts_with('c') && d[1..].parse::<u32>().is_ok_and(|n| n >= 17))
        .collect();
    dirs.sort();
    dirs.reverse();
    let mut out = vec![];
    for d in dirs {
        let Some(page) = fetch_text(&format!("{BASE}/{d}/")) else { continue };
        let mut names: Vec<String> = links(&page).into_iter().filter_map(|l| l.strip_suffix(".tar.gz").map(str::to_string)).filter(|n| is_release(n)).collect();
        names.sort();
        names.reverse();
        for name in names {
            let bytes = KNOWN.iter().find(|k| k.0 == name).map(|k| k.1).or_else(|| remote_size(&Release { name: name.clone(), bytes: 0 }.url())).unwrap_or(0);
            out.push(Release { name, bytes });
        }
    }
    (!out.is_empty()).then_some(out)
}

fn fetch_text(url: &str) -> Option<String> {
    let out = if have("curl") { Command::new("curl").args(["-sfL", "--max-time", "20", url]).output().ok()? } else { Command::new("wget").args(["-q", "-T", "20", "-O", "-", url]).output().ok()? };
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn remote_size(url: &str) -> Option<u64> {
    let out = Command::new("curl").args(["-sIL", "--max-time", "20", url]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().filter_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").and_then(|v| v.trim().parse().ok())).last()
}

fn have(tool: &str) -> bool {
    Command::new("sh").args(["-c", &format!("command -v {tool}")]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

// ------------------------------------------------------------------ where commands run

/// Inside Flatpak, run on the computer itself, where the compiler is.
pub fn on_host() -> bool {
    static HOST: OnceLock<bool> = OnceLock::new();
    *HOST.get_or_init(|| {
        std::env::var_os("INCLOUD_NO_HOST").is_none()
            && Path::new("/.flatpak-info").exists()
            && Command::new("flatpak-spawn").args(["--host", "true"]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
    })
}

pub(crate) fn command(program: &str, args: &[&str], dir: Option<&Path>, env: &[(&str, String)]) -> Command {
    if on_host() {
        let mut c = Command::new("flatpak-spawn");
        c.arg("--host");
        if let Some(d) = dir {
            c.arg(format!("--directory={}", d.display()));
        }
        for (k, v) in env {
            c.arg(format!("--env={k}={v}"));
        }
        c.arg(program).args(args);
        c
    } else {
        let mut c = Command::new(program);
        c.args(args);
        if let Some(d) = dir {
            c.current_dir(d);
        }
        for (k, v) in env {
            c.env(k, v);
        }
        c
    }
}

/// What the computer has for installing CLOUDY.
#[derive(Clone, Debug, Default)]
pub struct Tools {
    pub download: Option<&'static str>,
    pub missing: Vec<&'static str>,
    /// a command that installs what is missing, for this system
    pub hint: String,
    pub host: bool,
    pub cores: usize,
}

impl Tools {
    pub fn ready(&self) -> bool {
        self.missing.is_empty() && self.download.is_some()
    }
}

pub fn check_tools() -> Tools {
    let list = "curl wget tar make g++ clang++ perl";
    let script = format!("for t in {list}; do command -v $t >/dev/null 2>&1 && echo $t; done; getconf _NPROCESSORS_ONLN 2>/dev/null; cat /etc/os-release 2>/dev/null");
    let out = command("sh", &["-c", &script], None, &[]).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let found: Vec<&str> = out.lines().map(str::trim).collect();
    let has = |t: &str| found.contains(&t);
    let mut missing = vec![];
    for (need, name) in [("tar", "tar"), ("make", "make"), ("perl", "perl")] {
        if !has(need) {
            missing.push(name);
        }
    }
    if !has("g++") && !has("clang++") {
        missing.push("a C++ compiler (g++)");
    }
    let download = if has("curl") { Some("curl") } else if has("wget") { Some("wget") } else { None };
    if download.is_none() {
        missing.push("curl");
    }
    let cores = found.iter().find_map(|l| l.parse::<usize>().ok()).unwrap_or(2);
    let id = |key: &str| found.iter().find_map(|l| l.strip_prefix(key)).map(|v| v.trim_matches('"').to_lowercase()).unwrap_or_default();
    let family = format!("{} {}", id("ID="), id("ID_LIKE="));
    let hint = if cfg!(target_os = "macos") {
        "xcode-select --install".to_string()
    } else if family.contains("debian") || family.contains("ubuntu") {
        "sudo apt install build-essential perl curl".to_string()
    } else if family.contains("fedora") || family.contains("rhel") || family.contains("centos") {
        "sudo dnf install gcc-c++ make perl curl tar".to_string()
    } else if family.contains("arch") {
        "sudo pacman -S base-devel perl curl".to_string()
    } else if family.contains("suse") {
        "sudo zypper install gcc-c++ make perl curl tar".to_string()
    } else {
        "Install a C++ compiler (g++), make, perl and curl with your system's package manager.".to_string()
    };
    Tools { download, missing, hint, host: on_host(), cores }
}

// ------------------------------------------------------------------ where versions go

static ROOT: RwLock<String> = RwLock::new(String::new());

/// Folders where versions can go, best first. Inside a sandbox (Flatpak) ~/.local/share may be
/// read-only, while the app's own data folder ($XDG_DATA_HOME) is writable and the same path
/// outside, where the compiler runs.
fn candidates() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()));
    let mut v = vec![home.join(".local/share/incloud/cloudy")];
    if let Ok(x) = std::env::var("XDG_DATA_HOME") {
        if !x.trim().is_empty() {
            v.push(PathBuf::from(x).join("incloud/cloudy"));
        }
    }
    v.push(home.join("incloud/cloudy"));
    v.dedup();
    v
}

/// True when files can be written in `dir` (made if missing).
pub fn writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".incloud-write-test");
    let ok = std::fs::write(&probe, b"ok").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// The folder versions go to when none was chosen: the first one that can be written.
pub fn default_root() -> PathBuf {
    static ROOT_FOUND: OnceLock<PathBuf> = OnceLock::new();
    ROOT_FOUND
        .get_or_init(|| {
            let list = candidates();
            list.iter().find(|d| !d.display().to_string().contains(char::is_whitespace) && writable(d)).cloned().unwrap_or_else(|| list[0].clone())
        })
        .clone()
}

/// The folder where versions are installed.
pub fn root() -> PathBuf {
    let r = ROOT.read().map(|r| r.clone()).unwrap_or_default();
    if r.trim().is_empty() {
        default_root()
    } else {
        PathBuf::from(r.trim())
    }
}

pub fn set_root(r: &str) {
    if let Ok(mut g) = ROOT.write() {
        *g = r.to_string();
    }
}

/// The executable of a compiled version in `dir` (…/cXX.YY), if there is one.
pub fn exe_in(dir: &Path) -> Option<PathBuf> {
    let src = dir.join("source");
    let main = src.join("cloudy.exe");
    if main.is_file() {
        return Some(main);
    }
    std::fs::read_dir(&src).ok()?.flatten().map(|e| e.path().join("cloudy.exe")).find(|p| p.is_file())
}

/// The versions installed by InCloud: (folder name, executable, data folder).
pub fn installed() -> Vec<(String, PathBuf, String)> {
    let mut out = vec![];
    let mut roots = vec![root()];
    for c in candidates() {
        if !roots.contains(&c) {
            roots.push(c);
        }
    }
    for r in roots {
        for e in std::fs::read_dir(&r).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(exe) = exe_in(&e.path()) {
                out.push((name, exe, e.path().join("data").display().to_string()));
            }
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out
}

/// Free space where `path` is, in bytes.
fn free_bytes(path: &Path) -> Option<u64> {
    let out = Command::new("df").args(["-Pk"]).arg(path).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let kb: u64 = text.lines().nth(1)?.split_whitespace().nth(3)?.parse().ok()?;
    Some(kb * 1024)
}

// ------------------------------------------------------------------ the installation itself

#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    Download,
    Unpack,
    Compile,
    Test,
    Done,
    Failed(String),
    Cancelled,
}

impl Stage {
    /// The step number (1 to 4) while it is running.
    pub fn step(&self) -> usize {
        match self {
            Stage::Download => 1,
            Stage::Unpack => 2,
            Stage::Compile => 3,
            Stage::Test => 4,
            _ => 5,
        }
    }
}

#[derive(Clone, Debug)]
pub struct JobView {
    pub release: Release,
    pub stage: Stage,
    pub fraction: Option<f32>,
    pub detail: String,
    pub log: Vec<String>,
    pub exe: Option<PathBuf>,
    pub data: String,
    pub started: Instant,
    /// the last step reached (1 to 4), to show where it stopped
    pub reached: usize,
}

pub struct Job {
    view: Arc<Mutex<JobView>>,
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Job {
    pub fn start(release: Release, tools: Tools) -> Job {
        let folder = root().join(&release.name);
        let view = Arc::new(Mutex::new(JobView { release: release.clone(), stage: Stage::Download, fraction: None, detail: String::new(), log: vec![], exe: None, data: String::new(), started: Instant::now(), reached: 1 }));
        let cancel = Arc::new(AtomicBool::new(false));
        let (v, c) = (view.clone(), cancel.clone());
        let thread = std::thread::spawn(move || {
            let end = match install(&release, &tools, &folder, &v, &c) {
                Ok((exe, data)) => {
                    if let Ok(mut g) = v.lock() {
                        g.exe = Some(exe);
                        g.data = data;
                    }
                    Stage::Done
                }
                Err(_) if c.load(Ordering::SeqCst) => Stage::Cancelled,
                Err(e) => Stage::Failed(e),
            };
            if let Ok(mut g) = v.lock() {
                g.stage = end;
                g.fraction = None;
            }
        });
        Job { view, cancel, thread: Some(thread) }
    }

    pub fn view(&self) -> JobView {
        self.view.lock().map(|g| g.clone()).unwrap_or_else(|p| p.into_inner().clone())
    }

    pub fn running(&self) -> bool {
        self.view().stage.step() <= 4
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn set(view: &Arc<Mutex<JobView>>, stage: Stage, fraction: Option<f32>, detail: String) {
    if let Ok(mut g) = view.lock() {
        if stage.step() <= 4 {
            g.reached = stage.step();
        }
        g.stage = stage;
        g.fraction = fraction;
        g.detail = detail;
    }
}

fn tail(path: &Path, n: usize) -> Vec<String> {
    let text = std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].iter().map(|l| l.chars().take(240).collect()).collect()
}

/// Run a command to its end with its output in `log`, calling `tick` while it runs.
fn run(mut cmd: Command, log: &Path, stdin: Option<&Path>, cancel: &AtomicBool, limit: Option<Duration>, view: &Arc<Mutex<JobView>>, mut tick: impl FnMut()) -> Result<(), String> {
    let out = File::create(log).map_err(|e| format!("Cannot write {}: {e}", log.display()))?;
    let err = out.try_clone().map_err(|e| e.to_string())?;
    cmd.stdout(Stdio::from(out)).stderr(Stdio::from(err));
    match stdin {
        Some(p) => cmd.stdin(Stdio::from(File::open(p).map_err(|e| e.to_string())?)),
        None => cmd.stdin(Stdio::null()),
    };
    let mut child = cmd.spawn().map_err(|e| format!("Cannot start the command: {e}"))?;
    let t0 = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if let Ok(mut g) = view.lock() {
                g.log = tail(log, 40);
            }
            return if status.success() { Ok(()) } else { Err(format!("The command stopped with an error (exit code {:?}).", status.code())) };
        }
        let late = limit.is_some_and(|l| t0.elapsed() > l);
        if cancel.load(Ordering::SeqCst) || late {
            // politely first, so that make also stops the compilers it started
            let _ = Command::new("kill").args(["-TERM", &child.id().to_string()]).status();
            for _ in 0..30 {
                if child.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            let _ = child.kill();
            let _ = child.wait();
            return Err(if late { "It took too long and was stopped.".into() } else { "Cancelled.".into() });
        }
        tick();
        if let Ok(mut g) = view.lock() {
            g.log = tail(log, 40);
        }
        std::thread::sleep(Duration::from_millis(400));
    }
}

fn count(dir: &Path, ext: &str) -> usize {
    std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.path().extension().is_some_and(|x| x == ext)).count()
}

fn mb(b: u64) -> String {
    format!("{:.0} MB", b as f64 / 1e6)
}

fn install(release: &Release, tools: &Tools, folder: &Path, view: &Arc<Mutex<JobView>>, cancel: &AtomicBool) -> Result<(PathBuf, String), String> {
    if !tools.ready() {
        return Err(format!("Missing on this computer: {}.", tools.missing.join(", ")));
    }
    let root = folder.parent().ok_or("No folder")?.to_path_buf();
    if root.display().to_string().contains(char::is_whitespace) {
        return Err("CLOUDY can't be compiled in a folder whose path has spaces: choose another folder.".into());
    }
    if !writable(&root) {
        return Err(format!("InCloud can't write in {}: choose another folder with “Change folder…”.", root.display()));
    }
    std::fs::create_dir_all(root.join("downloads")).map_err(|e| format!("Cannot create {}: {e}", root.display()))?;
    let already = folder.join("source").join("Makefile").is_file();
    // the archive, the unpacked source and the compiled program take about six times the archive
    if let Some(free) = free_bytes(&root) {
        let need = if already { release.bytes * 2 } else { release.bytes * 6 };
        if release.bytes > 0 && free < need {
            return Err(format!("Not enough free space in {}: {} free, about {} needed.", root.display(), mb(free), mb(need)));
        }
    }
    let archive = root.join("downloads").join(format!("{}.tar.gz", release.name));

    // 1. download
    if !already {
        let complete = release.bytes > 0 && std::fs::metadata(&archive).is_ok_and(|m| m.len() == release.bytes);
        if !complete {
            let part = root.join("downloads").join(format!("{}.tar.gz.part", release.name));
            let url = release.url();
            let p = part.display().to_string();
            let cmd = if tools.download == Some("curl") {
                command("curl", &["-fL", "--retry", "3", "--retry-delay", "3", "-C", "-", "-o", &p, &url], None, &[])
            } else {
                command("wget", &["-c", "-O", &p, &url], None, &[])
            };
            set(view, Stage::Download, Some(0.0), format!("from {BASE}"));
            let total = release.bytes;
            run(cmd, &root.join("downloads").join("download.log"), None, cancel, None, view, || {
                let got = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
                let (f, d) = if total > 0 { (Some((got as f32 / total as f32).min(1.0)), format!("{} of {}", mb(got), mb(total))) } else { (None, mb(got)) };
                set(view, Stage::Download, f, d);
            })?;
            if total > 0 && std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0) != total {
                return Err("The download is incomplete: try again.".into());
            }
            std::fs::rename(&part, &archive).map_err(|e| e.to_string())?;
        }

        // 2. unpack, into a folder of its own, then move into place
        set(view, Stage::Unpack, None, "about a minute".into());
        let tmp = root.join(format!(".unpack-{}", release.name));
        if tmp.exists() {
            std::fs::remove_dir_all(&tmp).map_err(|e| e.to_string())?;
        }
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        let (a, t) = (archive.display().to_string(), tmp.display().to_string());
        let unpacked = run(command("tar", &["-xzf", &a, "-C", &t], None, &[]), &root.join("downloads").join("unpack.log"), None, cancel, None, view, || {});
        if let Err(e) = unpacked {
            let _ = std::fs::remove_dir_all(&tmp);
            // a damaged archive is downloaded again next time
            if !cancel.load(Ordering::SeqCst) {
                let _ = std::fs::remove_file(&archive);
            }
            return Err(format!("The archive could not be unpacked: {e}"));
        }
        let tops: Vec<PathBuf> = std::fs::read_dir(&tmp).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        let top = match tops.as_slice() {
            [one] => one.clone(),
            _ => tmp.clone(),
        };
        if folder.exists() {
            std::fs::remove_dir_all(folder).map_err(|e| format!("Cannot replace {}: {e}", folder.display()))?;
        }
        std::fs::rename(&top, folder).map_err(|e| format!("Cannot move the source into place: {e}"))?;
        let _ = std::fs::remove_dir_all(&tmp);
    }

    // 3. compile
    let src = folder.join("source");
    if !src.join("Makefile").is_file() {
        return Err(format!("No Makefile in {}: this archive is not a CLOUDY source.", src.display()));
    }
    let jobs = tools.cores.saturating_sub(1).max(1).to_string();
    let total = count(&src, "cpp").max(1);
    set(view, Stage::Compile, Some(0.0), format!("on {jobs} cores"));
    let built = run(command("make", &["-j", &jobs], Some(&src), &[]), &folder.join("incloud-build.log"), None, cancel, None, view, || {
        let done = count(&src, "o");
        set(view, Stage::Compile, Some((done as f32 / total as f32).min(0.99)), format!("{done} of {total} files, on {jobs} cores"));
    });
    if let Err(e) = built {
        let full = folder.join("incloud-build.log");
        return Err(format!("{e} The whole compiler output is in {}.", full.display()));
    }
    let exe = exe_in(folder).ok_or("The compilation ended but cloudy.exe was not made.")?;

    // 4. check: CLOUDY's own quick test
    set(view, Stage::Test, None, "CLOUDY's own quick test, under a minute".into());
    let data = folder.join("data").display().to_string();
    let test = folder.join("incloud-test");
    std::fs::create_dir_all(&test).map_err(|e| e.to_string())?;
    std::fs::write(test.join("test.in"), "test\n").map_err(|e| e.to_string())?;
    let out = test.join("test.out");
    let e = exe.display().to_string();
    // the program's exit code says nothing here: it is an error code whenever CLOUDY printed a warning
    let _ = run(command(&e, &[], Some(&test), &[("CLOUDY_DATA_PATH", data.clone())]), &out, Some(&test.join("test.in")), cancel, Some(Duration::from_secs(600)), view, || {});
    if cancel.load(Ordering::SeqCst) {
        return Err("Cancelled.".into());
    }
    let text = std::fs::read(&out).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    if !(text.contains("Cloudy exited OK") || text.contains("Cloudy ends")) {
        return Err(format!("CLOUDY was compiled but its quick test failed: see {}.", out.display()));
    }
    let _ = std::fs::remove_dir_all(&test);
    let _ = std::fs::remove_file(&archive);
    Ok((exe, data))
}

/// Developer check: install one version and print every change of state.
pub fn check(name: &str) {
    let tools = check_tools();
    println!("folder: {} (default {})", root().display(), default_root().display());
    println!("tools: download {:?}, missing {:?}, cores {}, host {}, hint {:?}", tools.download, tools.missing, tools.cores, tools.host, tools.hint);
    let release = known().into_iter().find(|r| r.name == name).unwrap_or(Release { name: name.to_string(), bytes: 0 });
    let job = Job::start(release, tools);
    let mut last = String::new();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let v = job.view();
        let now = format!("{:?} {:?} {}", v.stage, v.fraction.map(|f| (f * 20.0).round() / 20.0), v.detail.split(" of ").next().unwrap_or(""));
        if now != last {
            println!("{:>5.1}s {now}", v.started.elapsed().as_secs_f32());
            last = now;
        }
        if !job.running() {
            println!("end: {:?} exe {:?} data {}", v.stage, v.exe, v.data);
            for l in v.log.iter().rev().take(4).rev() {
                println!("  | {l}");
            }
            break;
        }
    }
}
