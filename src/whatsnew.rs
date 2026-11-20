//! What changed since the version that ran before: the new notes of CHANGELOG.md, the new git
//! commits and the files changed by hand. Worked out at the first start after an update, kept
//! in the settings folder and shown in the welcome window (also when it is opened again later).

use std::path::PathBuf;

use crate::updater;

const CHANGELOG: &str = include_str!("../CHANGELOG.md");
const BUILT: &str = env!("INCLOUD_BUILT");
const COMMIT: &str = env!("INCLOUD_COMMIT");

#[derive(Clone, Debug, Default)]
pub struct WhatsNew {
    pub from_version: String,
    pub from_commit: String,
    pub to_version: String,
    pub to_commit: String,
    /// when the update was seen (seconds since 1970)
    pub when: u64,
    /// new notes, by section of the changelog
    pub notes: Vec<(String, Vec<String>)>,
    pub commits: Vec<String>,
    pub files: Vec<String>,
    /// the update was made with Settings → Updates
    pub in_app: bool,
}

impl WhatsNew {
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty() && self.commits.is_empty() && self.files.is_empty()
    }

    pub fn from_text(&self) -> String {
        if self.from_commit.is_empty() { self.from_version.clone() } else { format!("{} ({})", self.from_version, self.from_commit) }
    }

    pub fn to_text(&self) -> String {
        if self.to_commit.is_empty() { self.to_version.clone() } else { format!("{} ({})", self.to_version, self.to_commit) }
    }

    pub fn date(&self) -> String {
        date(self.when)
    }

    fn write(&self) -> String {
        let mut s = format!("from={}\nfrom_commit={}\nto={}\nto_commit={}\nwhen={}\nin_app={}\n", self.from_version, self.from_commit, self.to_version, self.to_commit, self.when, self.in_app);
        for (sec, items) in &self.notes {
            for i in items {
                s.push_str(&format!("note={sec}\t{i}\n"));
            }
        }
        for c in &self.commits {
            s.push_str(&format!("commit={c}\n"));
        }
        for f in &self.files {
            s.push_str(&format!("file={f}\n"));
        }
        s
    }

    fn read(t: &str) -> WhatsNew {
        let mut w = WhatsNew::default();
        for l in t.lines() {
            let Some((k, v)) = l.split_once('=') else { continue };
            match k {
                "from" => w.from_version = v.into(),
                "from_commit" => w.from_commit = v.into(),
                "to" => w.to_version = v.into(),
                "to_commit" => w.to_commit = v.into(),
                "when" => w.when = v.parse().unwrap_or(0),
                "in_app" => w.in_app = v == "true",
                "note" => {
                    let (sec, item) = v.split_once('\t').unwrap_or(("", v));
                    match w.notes.iter_mut().find(|n| n.0 == sec) {
                        Some(n) => n.1.push(item.into()),
                        None => w.notes.push((sec.into(), vec![item.into()])),
                    }
                }
                "commit" => w.commits.push(v.into()),
                "file" => w.files.push(v.into()),
                _ => {}
            }
        }
        w
    }
}

/// Day, month and year of a time in seconds since 1970 (UTC).
fn date(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    // civil from days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    format!("{d} {} {y}", MONTHS[(m - 1) as usize])
}

/// The notes of a changelog: (section, item), in order. Items are the "- " lines; indented
/// lines below an item continue it.
pub fn notes(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    let mut section = String::new();
    let mut open = false;
    for l in text.lines() {
        let t = l.trim();
        if let Some(h) = t.strip_prefix("## ") {
            section = h.trim().to_string();
            open = false;
        } else if let Some(i) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            out.push((section.clone(), i.trim().to_string()));
            open = true;
        } else if open && !t.is_empty() && l.starts_with(char::is_whitespace) {
            if let Some(last) = out.last_mut() {
                last.1.push(' ');
                last.1.push_str(t);
            }
        } else {
            open = false;
        }
    }
    out
}

fn group(items: Vec<(String, String)>) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = vec![];
    for (sec, i) in items {
        match out.iter_mut().find(|n| n.0 == sec) {
            Some(n) => n.1.push(i),
            None => out.push((sec, vec![i])),
        }
    }
    out
}

/// The notes of the newest section of this version's changelog.
pub fn this_version() -> (String, Vec<String>) {
    group(notes(CHANGELOG)).into_iter().next().unwrap_or_default()
}

/// The whole changelog, grouped by section.
pub fn all_notes() -> Vec<(String, Vec<String>)> {
    group(notes(CHANGELOG))
}

fn dir() -> PathBuf {
    crate::app::settings_path().parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("."))
}

fn read(name: &str) -> Option<String> {
    std::fs::read_to_string(dir().join(name)).ok()
}

fn write(name: &str, text: &str) {
    let _ = std::fs::create_dir_all(dir());
    let _ = std::fs::write(dir().join(name), text);
}

/// The last update seen, if any.
pub fn stored() -> Option<WhatsNew> {
    read("whatsnew.txt").map(|t| WhatsNew::read(&t)).filter(|w| !w.to_version.is_empty())
}

/// Called by the update itself, before it builds: what the old program was and which files
/// were changed by hand.
pub fn before_update(files: &[String]) {
    write("update_pending.txt", &format!("version={}\ncommit={}\nfiles={}\n", updater::VERSION, COMMIT, files.join("\t")));
}

/// At every start: did the program change since it last ran? Returns the update to show now,
/// if there is one worth showing. `known` says InCloud ran here before (its settings exist):
/// then a missing record means an older InCloud, which kept none.
pub fn on_start(known: bool) -> Option<WhatsNew> {
    let last = read("last_run.txt");
    let record = || {
        write("last_run.txt", &format!("built={BUILT}\nversion={}\ncommit={COMMIT}\n", updater::VERSION));
        write("changelog_seen.md", CHANGELOG);
    };
    let Some(last) = last else {
        record();
        if !known {
            // the first start of all: nothing to compare with
            return None;
        }
        // an InCloud from before these notes: everything in them is new
        let w = WhatsNew {
            from_version: "an earlier version".into(),
            to_version: updater::VERSION.into(),
            to_commit: COMMIT.into(),
            when: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
            notes: vec![this_version()],
            ..Default::default()
        };
        write("whatsnew.txt", &w.write());
        return Some(w);
    };
    let field = |k: &str| last.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).unwrap_or("").to_string();
    if field("built") == BUILT {
        return None;
    }
    let pending = read("update_pending.txt");
    let _ = std::fs::remove_file(dir().join("update_pending.txt"));
    let pfield = |k: &str| pending.as_deref().and_then(|p| p.lines().find_map(|l| l.strip_prefix(&format!("{k}=")).map(str::to_string)));

    let mut w = WhatsNew {
        from_version: field("version"),
        from_commit: field("commit"),
        to_version: updater::VERSION.into(),
        to_commit: COMMIT.into(),
        when: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        in_app: pending.is_some(),
        ..Default::default()
    };
    // notes added to the changelog since the one seen last
    let seen: Vec<String> = read("changelog_seen.md").map(|t| notes(&t).into_iter().map(|n| n.1).collect()).unwrap_or_default();
    w.notes = group(notes(CHANGELOG).into_iter().filter(|n| !seen.contains(&n.1)).collect());
    // new commits
    if !w.from_commit.is_empty() && !w.to_commit.is_empty() && w.from_commit != w.to_commit {
        if let Some(src) = updater::source() {
            w.commits = updater::log_between(&src, &w.from_commit, &w.to_commit);
        }
    }
    w.files = pfield("files").map(|f| f.split('\t').filter(|x| !x.is_empty()).map(str::to_string).collect()).unwrap_or_default();
    record();
    if w.is_empty() && !w.in_app {
        // built again without notes or commits (a build by hand): nothing to tell
        return None;
    }
    write("whatsnew.txt", &w.write());
    Some(w)
}

/// For pictures of the window: an update as it would look.
pub fn sample() -> WhatsNew {
    let (sec, items) = this_version();
    WhatsNew {
        from_version: updater::VERSION.into(),
        from_commit: "1a2b3c4".into(),
        to_version: updater::VERSION.into(),
        to_commit: "5d6e7f8".into(),
        when: BUILT.parse().unwrap_or(0),
        notes: vec![(sec, items.into_iter().take(4).collect())],
        commits: vec!["5d6e7f8 Open Settings at the Updates card".into(), "9a0b1c2 Find the files of a run by their content".into()],
        files: vec![],
        in_app: true,
    }
}
