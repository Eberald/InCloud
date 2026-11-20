//! Updating InCloud from its own source folder: new commits from git, or files changed by hand.
//! The update pulls (when there is something to pull), builds and replaces the program; a restart
//! then starts the new one.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::installer::command;

/// The version as people read it: "1.0" (a third number only for fixes, as "1.0.1").
pub const VERSION: &str = if konst_patch_is_zero() { concat!(env!("CARGO_PKG_VERSION_MAJOR"), ".", env!("CARGO_PKG_VERSION_MINOR")) } else { env!("CARGO_PKG_VERSION") };

const fn konst_patch_is_zero() -> bool {
    let p = env!("CARGO_PKG_VERSION_PATCH").as_bytes();
    p.len() == 1 && p[0] == b'0'
}
const BUILT: &str = env!("INCLOUD_BUILT");
const COMMIT: &str = env!("INCLOUD_COMMIT");

/// When this program was built.
pub fn built() -> Option<SystemTime> {
    BUILT.parse::<u64>().ok().map(|s| UNIX_EPOCH + Duration::from_secs(s))
}

pub fn built_text() -> String {
    let Some(t) = built() else { return "unknown".into() };
    let ago = SystemTime::now().duration_since(t).unwrap_or_default().as_secs();
    let when = match ago {
        0..=89 => "just now".to_string(),
        90..=5399 => format!("{} minutes ago", ago / 60),
        5400..=129_599 => format!("{} hours ago", ago / 3600),
        _ => format!("{} days ago", ago / 86400),
    };
    if COMMIT.is_empty() {
        when
    } else {
        format!("{when}, commit {COMMIT}")
    }
}

/// The folder InCloud was built from, if it is still there.
pub fn source() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cargo = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    cargo.contains("name = \"incloud\"").then_some(dir)
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub source: Option<PathBuf>,
    pub git: bool,
    /// new commits on the remote branch, newest first ("abc123 message")
    pub new_commits: Vec<String>,
    /// files of the source changed after this program was built
    pub changed_files: Vec<String>,
    /// the git check couldn't reach the remote, or similar
    pub note: Option<String>,
}

impl Status {
    pub fn available(&self) -> bool {
        !self.new_commits.is_empty() || !self.changed_files.is_empty()
    }
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let d = dir.display().to_string();
    let mut all = vec!["-C", d.as_str()];
    all.extend_from_slice(args);
    let out = command("git", &all, None, &[("GIT_TERMINAL_PROMPT", "0".into())]).stdin(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The commits from one to another, newest first ("abc123 message").
pub fn log_between(dir: &Path, from: &str, to: &str) -> Vec<String> {
    git(dir, &["log", "--oneline", "--no-decorate", "-n", "60", &format!("{from}..{to}")])
        .map(|l| l.lines().filter(|x| !x.trim().is_empty()).map(str::to_string).collect())
        .unwrap_or_default()
}

/// Source files newer than this build.
fn changed_since_build(dir: &Path) -> Vec<String> {
    let Some(t) = built() else { return vec![] };
    let mut out = vec![];
    let mut stack = vec![dir.join("src"), dir.join("assets")];
    for f in ["Cargo.toml", "Cargo.lock", "build.rs"] {
        let p = dir.join(f);
        if p.metadata().and_then(|m| m.modified()).is_ok_and(|m| m > t + Duration::from_secs(2)) {
            out.push(f.to_string());
        }
    }
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if e.metadata().and_then(|m| m.modified()).is_ok_and(|m| m > t + Duration::from_secs(2)) {
                out.push(p.strip_prefix(dir).unwrap_or(&p).display().to_string());
            }
        }
    }
    out.sort();
    out
}

/// Look for an update; `fetch` also asks the git remote (needs the network).
pub fn check(fetch: bool) -> Status {
    check_in(source(), fetch)
}

/// The same for a given folder.
pub fn check_in(dir: Option<PathBuf>, fetch: bool) -> Status {
    let mut s = Status { source: dir, ..Default::default() };
    let Some(dir) = s.source.clone() else {
        s.note = Some("The folder InCloud was built from is no longer there, so it can't update itself: download the new version and run install.sh again.".into());
        return s;
    };
    s.changed_files = changed_since_build(&dir);
    s.git = git(&dir, &["rev-parse", "--is-inside-work-tree"]).is_some_and(|v| v == "true");
    if s.git {
        if git(&dir, &["rev-parse", "--abbrev-ref", "@{u}"]).is_none() {
            s.note = Some("This git folder follows no remote branch, so there is nothing to pull.".into());
        } else {
            if fetch && git(&dir, &["fetch", "--quiet"]).is_none() {
                s.note = Some("Couldn't reach the git remote (offline?): only what was fetched before is known.".into());
            }
            if let Some(log) = git(&dir, &["log", "--oneline", "--no-decorate", "HEAD..@{u}"]) {
                s.new_commits = log.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect();
            }
        }
    }
    s
}

// ------------------------------------------------------------------ the update itself

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Pull,
    Build,
    Install,
    Done,
    Failed(String),
}

#[derive(Clone, Debug)]
pub struct JobView {
    pub step: Step,
    pub log: Vec<String>,
    pub started: Instant,
    /// the program to start after the update
    pub exe: Option<PathBuf>,
}

pub struct Job {
    view: Arc<Mutex<JobView>>,
    cancel: Arc<AtomicBool>,
}

impl Job {
    pub fn start(pull: bool) -> Job {
        let view = Arc::new(Mutex::new(JobView { step: Step::Pull, log: vec![], started: Instant::now(), exe: None }));
        let cancel = Arc::new(AtomicBool::new(false));
        let (v, c) = (view.clone(), cancel.clone());
        std::thread::spawn(move || {
            let end = match update(pull, &v, &c) {
                Ok(exe) => {
                    if let Ok(mut g) = v.lock() {
                        g.exe = Some(exe);
                    }
                    Step::Done
                }
                Err(e) => Step::Failed(e),
            };
            if let Ok(mut g) = v.lock() {
                g.step = end;
            }
        });
        Job { view, cancel }
    }

    pub fn view(&self) -> JobView {
        self.view.lock().map(|g| g.clone()).unwrap_or_else(|p| p.into_inner().clone())
    }

    pub fn running(&self) -> bool {
        matches!(self.view().step, Step::Pull | Step::Build | Step::Install)
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

fn set(v: &Arc<Mutex<JobView>>, step: Step) {
    if let Ok(mut g) = v.lock() {
        g.step = step;
    }
}

fn log(v: &Arc<Mutex<JobView>>, line: &str) {
    if let Ok(mut g) = v.lock() {
        g.log.push(line.chars().take(240).collect());
        let n = g.log.len();
        if n > 200 {
            g.log.drain(..n - 200);
        }
    }
}

/// Run a command, passing its output to the log as it comes.
fn run(mut cmd: Command, v: &Arc<Mutex<JobView>>, cancel: &AtomicBool) -> Result<(), String> {
    use std::io::{BufRead, BufReader};
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("Cannot start the command: {e}"))?;
    let (out, err) = (child.stdout.take(), child.stderr.take());
    let v2 = v.clone();
    let reader = std::thread::spawn(move || {
        if let Some(e) = err {
            for l in BufReader::new(e).lines().map_while(Result::ok) {
                log(&v2, &l);
            }
        }
    });
    if let Some(o) = out {
        for l in BufReader::new(o).lines().map_while(Result::ok) {
            log(v, &l);
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
    if status.success() {
        Ok(())
    } else {
        Err(format!("The command stopped with an error (exit code {:?}): see its output below.", status.code()))
    }
}

/// The cargo that built this program, or the usual places.
fn cargo() -> (String, Vec<(&'static str, String)>) {
    let mut env = vec![];
    if let Some(h) = option_env!("CARGO_HOME") {
        env.push(("CARGO_HOME", h.to_string()));
    }
    if let Some(h) = option_env!("RUSTUP_HOME") {
        env.push(("RUSTUP_HOME", h.to_string()));
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let found = [option_env!("CARGO").map(str::to_string), Some(format!("{home}/.cargo/bin/cargo"))]
        .into_iter()
        .flatten()
        .find(|p| Path::new(p).is_file())
        .unwrap_or_else(|| "cargo".into());
    (found, env)
}

fn update(pull: bool, v: &Arc<Mutex<JobView>>, cancel: &AtomicBool) -> Result<PathBuf, String> {
    let dir = source().ok_or("The folder InCloud was built from is no longer there.")?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let me = me.canonicalize().unwrap_or(me);

    // what the program was before, for "What's new" at the next start
    crate::whatsnew::before_update(&changed_since_build(&dir));

    // 1. new commits
    if pull {
        set(v, Step::Pull);
        let d = dir.display().to_string();
        log(v, "$ git pull --ff-only");
        run(command("git", &["-C", &d, "pull", "--ff-only"], None, &[("GIT_TERMINAL_PROMPT", "0".into())]), v, cancel)
            .map_err(|e| format!("{e} If you changed files yourself, git can't merge them by itself.") )?;
    }

    // 2. build: the same kind of build as the program running now
    set(v, Step::Build);
    let target = dir.join("target");
    let dev = me.starts_with(target.join("debug"));
    let (cargo, env) = cargo();
    let mut cmd = Command::new(&cargo);
    cmd.current_dir(&dir).arg("build");
    if !dev {
        cmd.arg("--release");
    }
    for (k, val) in env {
        cmd.env(k, val);
    }
    log(v, &format!("$ cargo build{}", if dev { "" } else { " --release" }));
    run(cmd, v, cancel)?;
    let built = target.join(if dev { "debug" } else { "release" }).join(if cfg!(windows) { "incloud.exe" } else { "incloud" });
    if !built.is_file() {
        return Err(format!("The build ended but {} is missing.", built.display()));
    }

    // 3. put the new program where the running one is
    set(v, Step::Install);
    let built = built.canonicalize().unwrap_or(built);
    if built != me {
        let next = me.with_extension("new");
        std::fs::copy(&built, &next).map_err(|e| format!("Cannot write {}: {e}", next.display()))?;
        // a running program can be replaced by renaming over it
        std::fs::rename(&next, &me).map_err(|e| format!("Cannot replace {}: {e}", me.display()))?;
        log(v, &format!("installed {}", me.display()));
    }
    Ok(me)
}

/// Start the program again; the caller then closes this one.
pub fn restart(exe: &Path) -> Result<(), String> {
    Command::new(exe).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Developer check: run an update and print every step.
pub fn run_check(pull: bool) {
    let job = Job::start(pull);
    let mut last = String::new();
    loop {
        std::thread::sleep(Duration::from_millis(300));
        let v = job.view();
        let now = format!("{:?}", v.step);
        if now != last {
            println!("{:>5.1}s {now}", v.started.elapsed().as_secs_f32());
            last = now;
        }
        if !job.running() {
            for l in v.log.iter().rev().take(3).rev() {
                println!("  | {l}");
            }
            println!("exe {:?}", v.exe);
            break;
        }
    }
}
