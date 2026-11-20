//! Making the CLOUDY manuals (Hazy 1, 2, 3) from the LaTeX sources a release carries in
//! docs/latex, when the PDFs aren't there; and, if TeX is missing, installing a small one
//! (TinyTeX) in the home folder or saying which system packages give it.
//!
//! Nothing is written into the CLOUDY folder: the sources are compiled in a copy of docs/latex
//! next to links to the rest of the release (the manuals read files of its test suite), and the
//! PDFs go to InCloud's own folder.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::installer::command;

const VOLUMES: [&str; 3] = ["hazy1", "hazy2", "hazy3"];

/// Where TinyTeX puts its programs, put in front of PATH for every command.
const PATH_FIX: &str = "export PATH=\"$HOME/.TinyTeX/bin/x86_64-linux:$HOME/.TinyTeX/bin/aarch64-linux:$HOME/Library/TinyTeX/bin/universal-darwin:$HOME/bin:$HOME/.local/bin:$PATH\"";

/// The CLOUDY release (its top folder) whose docs/latex holds the manuals' sources.
pub fn sources(starts: &[PathBuf]) -> Option<PathBuf> {
    for s in starts {
        let mut p: Option<&Path> = Some(s.as_path());
        for _ in 0..6 {
            let Some(d) = p else { break };
            if d.join("docs/latex/hazy1/hazy1.tex").is_file() {
                return Some(d.to_path_buf());
            }
            p = d.parent();
        }
    }
    None
}

/// InCloud's folder for manuals it made.
pub fn home() -> PathBuf {
    let root = crate::installer::root();
    root.parent().map(|p| p.join("manuals")).unwrap_or_else(|| root.join("manuals"))
}

/// Where the PDFs made from one release go.
pub fn built_dir(release: &Path) -> PathBuf {
    let name: String = release.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "cloudy".into()).chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect();
    home().join(name)
}

/// What there is to compile with.
#[derive(Clone, Debug, Default)]
pub struct Tools {
    pub pdflatex: bool,
    pub bibtex: bool,
    pub tlmgr: bool,
    /// installing TinyTeX needs one of these
    pub downloader: bool,
    /// the system's package command for TeX, to show
    pub hint: String,
}

fn sh(script: &str) -> std::process::Command {
    let full = format!("{PATH_FIX}; {script}");
    command("sh", &["-c", full.as_str()], None, &[])
}

fn has(program: &str) -> bool {
    sh(&format!("command -v {program}")).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

pub fn tools() -> Tools {
    let os = sh("cat /etc/os-release 2>/dev/null; uname -s").stdin(Stdio::null()).output().map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase()).unwrap_or_default();
    let id_has = |w: &str| os.lines().any(|l| (l.starts_with("id=") || l.starts_with("id_like=")) && l.contains(w));
    let hint = if os.contains("darwin") {
        "brew install --cask basictex".to_string()
    } else if id_has("debian") || id_has("ubuntu") {
        "sudo apt install texlive-latex-extra texlive-bibtex-extra texlive-fonts-recommended".to_string()
    } else if id_has("fedora") || id_has("rhel") || id_has("centos") {
        "sudo dnf install texlive-scheme-medium".to_string()
    } else if id_has("arch") {
        "sudo pacman -S texlive-basic texlive-latexextra texlive-bibtexextra texlive-fontsrecommended".to_string()
    } else if id_has("suse") {
        "sudo zypper install texlive-scheme-medium".to_string()
    } else {
        "install TeX Live (pdflatex and bibtex) with your system's package manager".to_string()
    };
    Tools { pdflatex: has("pdflatex"), bibtex: has("bibtex"), tlmgr: has("tlmgr"), downloader: has("curl") || has("wget"), hint }
}

// ------------------------------------------------------------------ the jobs

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Running,
    Done,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct View {
    pub what: String,
    pub step: String,
    pub state: State,
    pub log: Vec<String>,
    pub started: Instant,
    /// the folder with the PDFs, when they were made
    pub out: Option<PathBuf>,
}

pub struct Job {
    view: Arc<Mutex<View>>,
    cancel: Arc<AtomicBool>,
}

impl Job {
    fn start(what: &str, work: impl FnOnce(&Arc<Mutex<View>>, &AtomicBool) -> Result<Option<PathBuf>, String> + Send + 'static) -> Job {
        let view = Arc::new(Mutex::new(View { what: what.into(), step: "Starting".into(), state: State::Running, log: vec![], started: Instant::now(), out: None }));
        let cancel = Arc::new(AtomicBool::new(false));
        let (v, c) = (view.clone(), cancel.clone());
        std::thread::spawn(move || {
            let end = work(&v, &c);
            if let Ok(mut g) = v.lock() {
                match end {
                    Ok(out) => {
                        g.out = out;
                        g.state = State::Done;
                    }
                    Err(e) => g.state = State::Failed(e),
                }
            }
        });
        Job { view, cancel }
    }

    /// Compile the manuals of `release`.
    pub fn build(release: PathBuf) -> Job {
        Job::start("Making the manuals", move |v, c| build(&release, v, c).map(Some))
    }

    /// Install TinyTeX and what the manuals need.
    pub fn install_tex() -> Job {
        Job::start("Installing TeX (TinyTeX)", |v, c| install_tex(v, c).map(|_| None))
    }

    pub fn view(&self) -> View {
        self.view.lock().map(|g| g.clone()).unwrap_or_else(|p| p.into_inner().clone())
    }

    pub fn running(&self) -> bool {
        self.view().state == State::Running
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn step(v: &Arc<Mutex<View>>, s: &str) {
    if let Ok(mut g) = v.lock() {
        g.step = s.to_string();
        g.log.push(format!("— {s}"));
    }
}

fn log(v: &Arc<Mutex<View>>, line: &str) {
    if let Ok(mut g) = v.lock() {
        g.log.push(line.chars().take(240).collect());
        let n = g.log.len();
        if n > 300 {
            g.log.drain(..n - 300);
        }
    }
}

/// Run a shell line in `dir`, its output to the log; Ok(true) when it ended well.
fn run(script: &str, dir: &Path, v: &Arc<Mutex<View>>, cancel: &AtomicBool) -> Result<bool, String> {
    use std::io::{BufRead, BufReader};
    if cancel.load(Ordering::SeqCst) {
        return Err("Stopped.".into());
    }
    let full = format!("{PATH_FIX}; cd \"{}\" && {script}", dir.display());
    let mut child = command("sh", &["-c", full.as_str()], None, &[]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| format!("Cannot start the command: {e}"))?;
    let err = child.stderr.take();
    let v2 = v.clone();
    let reader = std::thread::spawn(move || {
        if let Some(e) = err {
            for l in BufReader::new(e).lines().map_while(Result::ok) {
                log(&v2, &l);
            }
        }
    });
    if let Some(o) = child.stdout.take() {
        for l in BufReader::new(o).lines().map_while(Result::ok) {
            // TeX says a lot: keep the lines that matter
            if l.starts_with('!') || l.contains("Error") || l.starts_with("Output written") || l.contains("installed") || l.contains("install:") || l.starts_with("tlmgr") || l.contains("Downloading") || l.contains("TinyTeX") {
                log(v, &l);
            }
            if cancel.load(Ordering::SeqCst) {
                let _ = child.kill();
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let _ = reader.join();
    if cancel.load(Ordering::SeqCst) {
        return Err("Stopped.".into());
    }
    Ok(status.success())
}

/// Files TeX looked for and didn't find, from a log.
fn missing_files(log: &str) -> Vec<String> {
    let mut out = vec![];
    for l in log.lines() {
        if let Some(rest) = l.split("! LaTeX Error: File `").nth(1) {
            if let Some(f) = rest.split('\'').next() {
                out.push(f.to_string());
            }
        }
        if let Some(rest) = l.strip_prefix("! Font ") {
            if let Some(f) = rest.split('=').nth(1).and_then(|x| x.split_whitespace().next()) {
                out.push(format!("{f}.tfm"));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// With TinyTeX: install the package holding a missing file. False when none was found.
fn fetch_package(file: &str, dir: &Path, v: &Arc<Mutex<View>>, cancel: &AtomicBool) -> Result<bool, String> {
    let name = Path::new(file).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if name.is_empty() || name.starts_with("doc_") || file.contains("..") {
        return Ok(false);
    }
    let out = sh(&format!("tlmgr search --global --file \"/{name}\" 2>/dev/null")).current_dir(dir).stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let pkg = text.lines().find(|l| l.ends_with(':') && !l.starts_with(' ') && !l.starts_with("tlmgr")).map(|l| l.trim_end_matches(':').trim().to_string());
    let Some(pkg) = pkg else { return Ok(false) };
    step(v, &format!("Installing the TeX package {pkg} (for {name})"));
    run(&format!("tlmgr install {pkg}"), dir, v, cancel)
}

fn build(release: &Path, v: &Arc<Mutex<View>>, cancel: &AtomicBool) -> Result<PathBuf, String> {
    let tools = tools();
    if !tools.pdflatex {
        return Err("pdflatex isn't installed: install TeX first.".into());
    }
    // 1. a copy of docs/latex beside links to the rest of the release
    step(v, "Copying the LaTeX sources");
    let work = home().join(format!("build-{}", built_dir(release).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
    let latex = work.join("docs").join("latex");
    std::fs::create_dir_all(work.join("docs")).map_err(|e| format!("Cannot make {}: {e}", work.display()))?;
    copy_tree(&release.join("docs").join("latex"), &latex).map_err(|e| format!("Cannot copy the sources: {e}"))?;
    for e in std::fs::read_dir(release).map_err(|e| e.to_string())?.flatten() {
        let n = e.file_name();
        if n == "docs" {
            continue;
        }
        let link = work.join(&n);
        if !link.exists() {
            #[cfg(unix)]
            let _ = std::os::unix::fs::symlink(e.path(), &link);
        }
    }
    // 2. pdflatex, bibtex, pdflatex until the references settle, for each volume
    for vol in VOLUMES {
        let dir = latex.join(vol);
        let n = vol.trim_start_matches("hazy");
        step(v, &format!("Hazy {n}: first pass"));
        let mut tries = 0;
        loop {
            run(&format!("pdflatex -interaction=nonstopmode {vol}.tex"), &dir, v, cancel)?;
            let logtext = std::fs::read_to_string(dir.join(format!("{vol}.log"))).unwrap_or_default();
            let missing = missing_files(&logtext);
            if missing.is_empty() {
                break;
            }
            if !tools.tlmgr {
                if dir.join(format!("{vol}.pdf")).is_file() {
                    break;
                }
                return Err(format!("TeX misses {} for Hazy {n}. Install more of TeX ({}) and try again.", missing.join(", "), tools.hint));
            }
            let mut got = false;
            for f in &missing {
                got |= fetch_package(f, &dir, v, cancel)?;
            }
            tries += 1;
            if !got || tries > 12 {
                if dir.join(format!("{vol}.pdf")).is_file() {
                    break;
                }
                return Err(format!("TeX misses {} for Hazy {n}, and no package with it was found.", missing.join(", ")));
            }
        }
        if tools.bibtex {
            step(v, &format!("Hazy {n}: references"));
            run(&format!("bibtex {vol}"), &dir, v, cancel)?;
        }
        for k in 0..4 {
            step(v, &format!("Hazy {n}: pass {}", k + 2));
            run(&format!("pdflatex -interaction=nonstopmode {vol}.tex"), &dir, v, cancel)?;
            let logtext = std::fs::read_to_string(dir.join(format!("{vol}.log"))).unwrap_or_default();
            if !(logtext.contains("Rerun to get cross-references right") || logtext.contains("Rerun to get citations correct") || logtext.contains("Rerun to get")) {
                break;
            }
        }
        if !dir.join(format!("{vol}.pdf")).is_file() {
            return Err(format!("Hazy {n} wasn't made: see {}", dir.join(format!("{vol}.log")).display()));
        }
    }
    // the volumes point into each other
    step(v, "Links between Hazy 1 and Hazy 2");
    for vol in ["hazy1", "hazy2"] {
        run(&format!("pdflatex -interaction=nonstopmode {vol}.tex"), &latex.join(vol), v, cancel)?;
    }
    // 3. the PDFs to InCloud's folder
    step(v, "Keeping the PDFs");
    let out = built_dir(release);
    std::fs::create_dir_all(&out).map_err(|e| format!("Cannot make {}: {e}", out.display()))?;
    for vol in VOLUMES {
        std::fs::copy(latex.join(vol).join(format!("{vol}.pdf")), out.join(format!("{vol}.pdf"))).map_err(|e| format!("Cannot keep {vol}.pdf: {e}"))?;
    }
    log(v, &format!("The manuals are in {}", out.display()));
    if !tools.bibtex {
        log(v, "bibtex was missing: the references of the manuals show as “?”.");
    }
    Ok(out)
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        let ft = e.file_type()?;
        if ft.is_symlink() {
            // links between the volumes (hazy1/hazy2.pdf → ../hazy2/hazy2.pdf) stay links
            if dst.symlink_metadata().is_err() {
                #[cfg(unix)]
                std::os::unix::fs::symlink(std::fs::read_link(&src)?, &dst)?;
            }
        } else if ft.is_dir() {
            copy_tree(&src, &dst)?;
        } else if !dst.exists() {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// The packages the manuals use, so that most are there before the first pass.
const NEEDED: &str = "bibtex natbib framed moreverb pdfpages eso-pic tocbibind psnfss times symbol zapfding helvetic courier ncntrsbk url hyperref oberdiek graphics tools";

fn install_tex(v: &Arc<Mutex<View>>, cancel: &AtomicBool) -> Result<(), String> {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    if !tools().tlmgr {
        step(v, "Downloading and installing TinyTeX (about 100 MB, in your home folder)");
        let ok = run("(curl -fsSL https://yihui.org/tinytex/install-bin-unix.sh || wget -qO- https://yihui.org/tinytex/install-bin-unix.sh) | sh", &home, v, cancel)?;
        if !ok || !tools().tlmgr {
            return Err("TinyTeX couldn't be installed (no network, or no curl/wget?). The system packages are another way.".into());
        }
    }
    step(v, "Installing the TeX packages the manuals use");
    if !run(&format!("tlmgr install {NEEDED}"), &home, v, cancel)? {
        log(v, "Some packages weren't installed now; the build installs what it misses.");
    }
    Ok(())
}

/// Developer check: make the manuals of a release and print the steps.
pub fn check(release: &Path) {
    let t = tools();
    println!("pdflatex {} bibtex {} tlmgr {} downloader {} hint: {}", t.pdflatex, t.bibtex, t.tlmgr, t.downloader, t.hint);
    let job = Job::build(release.to_path_buf());
    let mut last = String::new();
    while job.running() {
        std::thread::sleep(std::time::Duration::from_millis(400));
        let s = job.view().step;
        if s != last {
            println!("{:>5.1}s {s}", job.view().started.elapsed().as_secs_f32());
            last = s;
        }
    }
    let v = job.view();
    println!("{:?} out {:?}", v.state, v.out);
}
