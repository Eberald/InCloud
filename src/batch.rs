//! Grids of models run in parallel: from one input file with placeholders and the ranges of its
//! parameters, write every input file and one or more shell scripts that run CLOUDY on them,
//! a few at a time, here or on a server (plain bash or SLURM).

use std::path::{Path, PathBuf};

/// How the values of a parameter are given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spacing {
    /// start, stop and a step, written as they are (e.g. log U from -4 to 0 by 0.5)
    Step,
    /// start and stop as plain numbers, a step in dex between them (e.g. 1e3 to 1e5 by 0.5 dex)
    Dex,
    /// a list of values
    List,
}

impl Spacing {
    pub const ALL: [Spacing; 3] = [Spacing::Step, Spacing::Dex, Spacing::List];
    pub fn label(self) -> &'static str {
        match self {
            Spacing::Step => "range, step",
            Spacing::Dex => "range, step in dex",
            Spacing::List => "list",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Param {
    /// the name used in placeholders: {U}
    pub key: String,
    /// the command written in the input file, with the placeholder, e.g. "ionization parameter {U}"
    pub command: String,
    /// lines of the input file that this command replaces (their first words, lower case)
    pub replaces: Vec<String>,
    pub spacing: Spacing,
    pub start: f64,
    pub stop: f64,
    pub step: f64,
    pub list: String,
    pub decimals: usize,
}

impl Param {
    /// The values, in order; `None` with the reason when they can't be made.
    pub fn values(&self) -> Result<Vec<f64>, String> {
        let v: Vec<f64> = match self.spacing {
            Spacing::List => {
                let mut v = vec![];
                for w in self.list.split(|c: char| c == ',' || c == ';' || c.is_whitespace()).filter(|w| !w.is_empty()) {
                    v.push(w.parse::<f64>().map_err(|_| format!("“{w}” is not a number"))?);
                }
                v
            }
            Spacing::Step => {
                if self.step == 0.0 || !self.step.is_finite() {
                    return Err("the step can't be 0".into());
                }
                let n = ((self.stop - self.start) / self.step + 1e-9).floor();
                if n < 0.0 {
                    return Err("the step goes away from the end value".into());
                }
                if n > 100_000.0 {
                    return Err("more than 100 000 values".into());
                }
                (0..=n as usize).map(|k| self.start + k as f64 * self.step).collect()
            }
            Spacing::Dex => {
                if self.start <= 0.0 || self.stop <= 0.0 {
                    return Err("a range in dex needs values above 0".into());
                }
                if self.step <= 0.0 || !self.step.is_finite() {
                    return Err("the step in dex must be above 0".into());
                }
                let (a, b) = (self.start.log10(), self.stop.log10());
                let s = if b >= a { self.step } else { -self.step };
                let n = ((b - a) / s + 1e-9).floor();
                if n > 100_000.0 {
                    return Err("more than 100 000 values".into());
                }
                (0..=n as usize).map(|k| 10f64.powf(a + k as f64 * s)).collect()
            }
        };
        if v.is_empty() {
            return Err("no values".into());
        }
        Ok(v)
    }

    pub fn format(&self, v: f64) -> String {
        if self.spacing == Spacing::Dex && (v.abs() >= 1e5 || (v != 0.0 && v.abs() < 1e-3)) {
            return format!("{:.*e}", self.decimals.max(1), v);
        }
        let s = format!("{:.*}", self.decimals, v);
        // no "-0.00"
        if s.trim_start_matches('-').chars().all(|c| c == '0' || c == '.') {
            s.trim_start_matches('-').to_string()
        } else {
            s
        }
    }
}

/// Parameters offered ready-made: (label, key, command, lines it replaces, spacing, start, stop, step, decimals).
pub const PRESETS: [(&str, &str, &str, &[&str], Spacing, f64, f64, f64, usize); 11] = [
    ("Ionisation parameter, log U", "U", "ionization parameter {U}", &["ionization parameter", "ionisation parameter", "ionization", "ionisation"], Spacing::Step, -4.0, 0.0, 0.5, 2),
    ("Hydrogen density, log n(H)", "hden", "hden {hden}", &["hden"], Spacing::Step, 0.0, 6.0, 1.0, 2),
    ("Metallicity, log Z/Z☉ (gas and grains)", "Z", "metals and grains {Z} log", &["metals", "grains"], Spacing::Step, -1.0, 0.5, 0.5, 2),
    ("Metallicity, linear Z/Z☉ (gas and grains)", "Z", "metals and grains linear {Z}", &["metals"], Spacing::Step, 0.15, 1.05, 0.1, 2),
    ("Stop at column density, log N(H)", "NH", "stop column density {NH}", &["stop column density", "stop column"], Spacing::Step, 21.0, 23.0, 1.0, 2),
    ("Stop at visual extinction, A_V", "AV", "stop AV {AV}", &["stop av"], Spacing::Dex, 1.0, 100.0, 0.5, 2),
    ("Blackbody temperature, T* [K]", "Tstar", "blackbody {Tstar}", &["blackbody"], Spacing::Step, 30000.0, 50000.0, 10000.0, 0),
    ("Interstellar field strength, G (× Habing)", "G", "table ism factor {G}", &["table ism"], Spacing::Dex, 1.0, 1e4, 1.0, 3),
    ("Turbulence [km/s]", "vturb", "turbulence {vturb} km/s", &["turbulence"], Spacing::Step, 0.0, 5.0, 1.5, 1),
    ("Inner radius, log r [cm]", "r", "radius {r}", &["radius"], Spacing::Step, 17.0, 20.0, 1.0, 2),
    ("Cosmic-ray rate, log (× background)", "CR", "cosmic rays background {CR} log", &["cosmic rays", "cosmic ray"], Spacing::Step, -1.0, 1.0, 1.0, 2),
];

pub fn preset(k: usize) -> Param {
    let (_, key, command, replaces, spacing, start, stop, step, decimals) = PRESETS[k];
    Param { key: key.into(), command: command.into(), replaces: replaces.iter().map(|s| s.to_string()).collect(), spacing, start, stop, step, list: String::new(), decimals }
}

/// A parameter written by hand: a command with a placeholder.
pub fn free(n: usize) -> Param {
    let key = format!("p{n}");
    Param { command: format!("# your command with {{{key}}}"), key, replaces: vec![], spacing: Spacing::List, start: 0.0, stop: 1.0, step: 0.5, list: "0, 1".into(), decimals: 2 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheduler {
    /// plain bash: start it with `bash batch_1.sh` (or nohup)
    Bash,
    /// a SLURM cluster: `sbatch batch_1.sh`
    Slurm,
}

#[derive(Clone, Debug)]
pub struct Batch {
    pub template: String,
    pub params: Vec<Param>,
    pub name_pattern: String,
    /// CLOUDY on the computer that runs the scripts
    pub exe: String,
    pub data_path: String,
    /// where the models run (on that computer); empty: the folder the files are written to
    pub run_dir: String,
    /// where InCloud writes the files, on this computer
    pub out_dir: String,
    pub parallel: usize,
    pub scripts: usize,
    /// `cloudy.exe -r name` (else `cloudy.exe < name.in > name.out`)
    pub use_r: bool,
    /// models whose output already ends well are not run again
    pub skip_done: bool,
    /// the input files are written by the scripts themselves: only the scripts need copying
    pub embed: bool,
    pub scheduler: Scheduler,
    pub slurm_partition: String,
    pub slurm_account: String,
    pub slurm_time: String,
    pub slurm_mem: String,
    pub slurm_job: String,
    /// more header lines, written as they are
    pub header: String,
    /// files the template needs beside it (a custom spectrum): (relative path, content)
    pub extra_files: Vec<(String, String)>,
}

impl Default for Batch {
    fn default() -> Self {
        Batch {
            template: String::new(),
            params: vec![preset(0), preset(1)],
            name_pattern: "model_U{U}_hden{hden}".into(),
            exe: String::new(),
            data_path: String::new(),
            run_dir: String::new(),
            out_dir: String::new(),
            parallel: 4,
            scripts: 1,
            use_r: true,
            skip_done: true,
            embed: false,
            scheduler: Scheduler::Bash,
            slurm_partition: String::new(),
            slurm_account: String::new(),
            slurm_time: "08:00:00".into(),
            slurm_mem: String::new(),
            slurm_job: "cloudy".into(),
            header: String::new(),
            extra_files: vec![],
        }
    }
}

/// One model of the grid.
#[derive(Clone, Debug)]
pub struct Model {
    pub name: String,
    pub values: Vec<String>,
    pub input: String,
}

/// Put every `{key}` of `map` into `text`.
fn fill(text: &str, map: &[(String, String)]) -> String {
    let mut s = text.to_string();
    for (k, v) in map {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

/// Placeholders `{word}` left in a text.
pub fn placeholders(text: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = text;
    while let Some(a) = rest.find('{') {
        let after = &rest[a + 1..];
        match after.find('}') {
            Some(b) if b > 0 && after[..b].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
                let k = after[..b].to_string();
                if !out.contains(&k) {
                    out.push(k);
                }
                rest = &after[b + 1..];
            }
            _ => rest = after,
        }
    }
    out
}

/// A name that is safe as a file name.
fn safe(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.+".contains(c) { c } else { '_' }).collect()
}

/// Quote for bash.
fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

impl Batch {
    /// A template from InCloud's own input file: the title and the file names follow each model's
    /// name, the progress file is left out, and each parameter's line is replaced by its placeholder.
    pub fn template_from(&mut self, deck: &str, file_prefix: &str) {
        let mut out = String::new();
        for l in deck.lines() {
            let t = l.trim();
            let low = t.to_ascii_lowercase();
            if low.starts_with("title") {
                out.push_str("title {name}\n");
                continue;
            }
            if t.contains(".prog\"") {
                continue;
            }
            out.push_str(&l.replace(&format!("\"{file_prefix}."), "\"{name}."));
            out.push('\n');
        }
        self.template = out;
        for i in 0..self.params.len() {
            self.place(i);
        }
    }

    /// A template from an input file written by hand: its save prefix (or its title) follows each
    /// model's name, and the parameters go where their commands are.
    pub fn adopt(&mut self, text: &str) {
        let mut out = String::new();
        for l in text.lines() {
            let low = l.trim().to_ascii_lowercase();
            if low.starts_with("set save prefix") {
                out.push_str("set save prefix \"{name}\"\n");
            } else if low.starts_with("title") {
                out.push_str("title {name}\n");
            } else {
                out.push_str(l);
                out.push('\n');
            }
        }
        self.template = out;
        for i in 0..self.params.len() {
            self.place(i);
        }
    }

    /// Put parameter `i` into the template: replace the line it stands for, or add it before
    /// the output commands.
    pub fn place(&mut self, i: usize) {
        let p = self.params[i].clone();
        let tag = format!("{{{}}}", p.key);
        if self.template.contains(&tag) {
            return;
        }
        let mut lines: Vec<String> = self.template.lines().map(str::to_string).collect();
        let hit = lines.iter().position(|l| {
            let low = l.trim().to_ascii_lowercase();
            !low.starts_with('#') && p.replaces.iter().any(|r| low.starts_with(r.as_str()))
        });
        match hit {
            Some(k) => lines[k] = p.command.clone(),
            None => {
                let at = lines.iter().position(|l| l.trim_start().to_ascii_lowercase().starts_with("# ---- output") || l.trim_start().to_ascii_lowercase().starts_with("save")).unwrap_or(lines.len());
                lines.insert(at, p.command.clone());
            }
        }
        self.template = lines.join("\n") + "\n";
    }

    /// Every model of the grid, in order (the first parameter changes slowest).
    pub fn models(&self) -> Result<Vec<Model>, String> {
        let mut lists: Vec<Vec<f64>> = vec![];
        let mut total: f64 = 1.0;
        for p in &self.params {
            let v = p.values().map_err(|e| format!("{{{}}}: {e}", p.key))?;
            total *= v.len() as f64;
            lists.push(v);
        }
        if total > 200_000.0 {
            return Err(format!("{total:.0} models: that is more than 200 000. Use fewer values."));
        }
        let mut out = vec![];
        let mut idx = vec![0usize; lists.len()];
        loop {
            let values: Vec<String> = self.params.iter().zip(&idx).zip(&lists).map(|((p, k), l)| p.format(l[*k])).collect();
            let mut map: Vec<(String, String)> = self.params.iter().map(|p| p.key.clone()).zip(values.iter().cloned()).collect();
            let name = safe(&fill(&self.name_pattern, &map));
            map.push(("name".into(), name.clone()));
            out.push(Model { name, values, input: fill(&self.template, &map) });
            // the next combination
            let mut d = lists.len();
            loop {
                if d == 0 {
                    return Ok(out);
                }
                d -= 1;
                idx[d] += 1;
                if idx[d] < lists[d].len() {
                    break;
                }
                idx[d] = 0;
            }
        }
    }

    /// What should be looked at before writing: (is it an error, text).
    pub fn problems(&self, models: &[Model]) -> Vec<(bool, String)> {
        let mut out = vec![];
        let keys: Vec<&str> = self.params.iter().map(|p| p.key.as_str()).collect();
        for (i, k) in keys.iter().enumerate() {
            if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                out.push((true, format!("Parameter {}: its name must be letters, digits or _ (it is used as {{name}}).", i + 1)));
            } else if keys[..i].contains(k) {
                out.push((true, format!("Two parameters are called {{{k}}}.")));
            }
        }
        let used = placeholders(&self.template);
        for k in &keys {
            if !used.iter().any(|u| u == k) {
                out.push((true, format!("{{{k}}} is not in the input file, so every model would be the same there. Put it in a command, e.g. “{}”.", self.params.iter().find(|p| p.key == *k).map(|p| p.command.as_str()).unwrap_or(""))));
            }
        }
        for u in &used {
            if u != "name" && !keys.contains(&u.as_str()) {
                out.push((true, format!("The input file has {{{u}}}, but no parameter is called {u}.")));
            }
        }
        let in_name = placeholders(&self.name_pattern);
        for k in &keys {
            if !in_name.iter().any(|u| u == k) && self.params.iter().find(|p| p.key == *k).is_some_and(|p| p.values().map(|v| v.len() > 1).unwrap_or(false)) {
                out.push((true, format!("{{{k}}} is not in the model names: models that differ only by it would get the same name.")));
            }
        }
        if !self.template.to_ascii_lowercase().lines().any(|l| l.trim_start().starts_with("save") || l.trim_start().starts_with("punch")) {
            out.push((false, "The input file saves nothing: only the text output (.out) will be written.".into()));
        } else if !self.template.contains("{name}") && !self.template.to_ascii_lowercase().contains("set save prefix") {
            out.push((true, "The save commands don't use {name}: every model would write over the same files. Write file names as \"{name}.ovr\", or add: set save prefix \"{name}\".".into()));
        }
        // files the models read: they must be where CLOUDY looks for them
        for l in self.template.lines() {
            let low = l.trim().to_ascii_lowercase();
            if (low.starts_with("table sed") || low.starts_with("table star") || low.starts_with("init")) && l.contains('"') {
                if let Some(f) = l.split('"').nth(1) {
                    if !self.extra_files.iter().any(|e| e.0.ends_with(f)) {
                        out.push((false, format!("The models read “{f}”: put it in the run folder (or in CLOUDY's data folder) on the computer that runs them.")));
                    }
                }
            }
        }
        let mut names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
        names.sort_unstable();
        if let Some(w) = names.windows(2).find(|w| w[0] == w[1]) {
            out.push((true, format!("Two models are both called “{}”: add the parameters, or more decimals, to the name.", w[0])));
        }
        if let Some(m) = models.first() {
            for n in crate::model::check_deck(&m.input).into_iter().filter(|n| n.error) {
                out.push((true, format!("Input file, line {}: {}", n.line, n.text)));
            }
        }
        if self.exe.trim().is_empty() {
            out.push((true, "Give the path of cloudy.exe on the computer that runs the models.".into()));
        }
        if self.out_dir.trim().is_empty() {
            out.push((true, "Choose the folder where InCloud writes the files.".into()));
        }
        out
    }

    fn run_dir_or_out(&self) -> String {
        if self.run_dir.trim().is_empty() {
            self.out_dir.trim().to_string()
        } else {
            self.run_dir.trim().to_string()
        }
    }

    /// The models of script `k` (0-based) of `n`: as even shares as can be.
    pub fn share(total: usize, n: usize, k: usize) -> std::ops::Range<usize> {
        let n = n.max(1);
        let base = total / n;
        let extra = total % n;
        let start = k * base + k.min(extra);
        let len = base + usize::from(k < extra);
        start..(start + len).min(total)
    }

    /// One shell script.
    pub fn script(&self, models: &[Model], k: usize, n: usize) -> String {
        let mut s = String::new();
        let range = Self::share(models.len(), n, k);
        let mine = &models[range.clone()];
        let par = self.parallel.max(1);
        s.push_str("#!/bin/bash\n");
        if self.scheduler == Scheduler::Slurm {
            let job = if n > 1 { format!("{}_{}", self.slurm_job.trim(), k + 1) } else { self.slurm_job.trim().to_string() };
            s.push_str(&format!("#SBATCH --job-name={}\n", if job.is_empty() { "cloudy".into() } else { job }));
            if !self.slurm_partition.trim().is_empty() {
                s.push_str(&format!("#SBATCH --partition={}\n", self.slurm_partition.trim()));
            }
            if !self.slurm_account.trim().is_empty() {
                s.push_str(&format!("#SBATCH --account={}\n", self.slurm_account.trim()));
            }
            if !self.slurm_time.trim().is_empty() {
                s.push_str(&format!("#SBATCH --time={}\n", self.slurm_time.trim()));
            }
            s.push_str("#SBATCH --nodes=1\n");
            s.push_str(&format!("#SBATCH --ntasks-per-node={par}\n"));
            if !self.slurm_mem.trim().is_empty() {
                s.push_str(&format!("#SBATCH --mem={}\n", self.slurm_mem.trim()));
            }
            s.push_str(&format!("#SBATCH --output=batch_{}_%j.log\n", k + 1));
        }
        for l in self.header.lines().filter(|l| !l.trim().is_empty()) {
            s.push_str(l.trim_end());
            s.push('\n');
        }
        s.push_str(&format!(
            "#\n# Written by InCloud: script {} of {}, {} CLOUDY models (numbers {} to {} of {}), {} at a time.\n",
            k + 1,
            n,
            mine.len(),
            range.start + 1,
            range.end,
            models.len(),
            par
        ));
        let how = if self.scheduler == Scheduler::Slurm { format!("sbatch batch_{}.sh", k + 1) } else { format!("nohup bash batch_{}.sh > batch_{}.log 2>&1 &", k + 1, k + 1) };
        s.push_str(&format!("# Start it with:  {how}\n"));
        s.push_str("# Run again, it skips the models that already ended well.\n\n");
        s.push_str(&format!("CLOUDY={}\n", q(self.exe.trim())));
        if !self.data_path.trim().is_empty() {
            s.push_str(&format!("export CLOUDY_DATA_PATH={}\n", q(self.data_path.trim())));
        }
        s.push_str(&format!("RUN_DIR={}\n", q(&self.run_dir_or_out())));
        s.push_str(&format!("PARALLEL={par}\n\n"));
        s.push_str("cd \"$RUN_DIR\" || { echo \"Cannot enter $RUN_DIR\"; exit 1; }\n");
        s.push_str("[ -x \"$CLOUDY\" ] || { echo \"CLOUDY not found or not executable: $CLOUDY\"; exit 1; }\n\n");
        if self.embed {
            s.push_str("# the input files (written only when missing)\n");
            for m in mine {
                s.push_str(&format!("[ -f {0}.in ] || cat > {0}.in <<'INCLOUD_END_OF_INPUT'\n", q(&m.name)));
                s.push_str(&m.input);
                if !m.input.ends_with('\n') {
                    s.push('\n');
                }
                s.push_str("INCLOUD_END_OF_INPUT\n");
            }
            s.push('\n');
        }
        s.push_str("MODELS=(\n");
        for m in mine {
            s.push_str(&format!("  {}\n", q(&m.name)));
        }
        s.push_str(")\n\n");
        s.push_str("run_one() {\n");
        s.push_str("  local m=\"$1\"\n");
        if self.skip_done {
            s.push_str("  if [ -f \"$m.out\" ] && grep -q \"Cloudy ends\" \"$m.out\"; then\n    echo \"  $m: already done, skipped\"\n    return 0\n  fi\n");
        }
        if self.use_r {
            s.push_str("  \"$CLOUDY\" -r \"$m\" > /dev/null 2>> \"$m.err\"\n");
        } else {
            s.push_str("  \"$CLOUDY\" < \"$m.in\" > \"$m.out\" 2>> \"$m.err\"\n");
        }
        s.push_str("  local code=$?\n");
        s.push_str("  [ -s \"$m.err\" ] || rm -f \"$m.err\"\n");
        s.push_str("  if [ $code -eq 0 ]; then echo \"  $m: finished\"; else echo \"  $m: CLOUDY stopped with code $code\"; fi\n");
        s.push_str("}\n\n");
        s.push_str("total=${#MODELS[@]}\nn=0\n");
        s.push_str("echo \"$(date '+%F %T')  $total models, $PARALLEL at a time, in $RUN_DIR\"\n");
        s.push_str("for m in \"${MODELS[@]}\"; do\n");
        s.push_str("  # wait for a free place\n");
        s.push_str("  while [ \"$(jobs -rp | wc -l)\" -ge \"$PARALLEL\" ]; do wait -n; done\n");
        s.push_str("  n=$((n + 1))\n");
        s.push_str("  echo \"$(date '+%F %T')  [$n/$total] $m\"\n");
        s.push_str("  run_one \"$m\" &\n");
        s.push_str("done\n");
        s.push_str("wait\n");
        s.push_str("echo \"$(date '+%F %T')  all $total models done\"\n");
        s
    }

    /// The script that starts all the others.
    pub fn starter(&self, n: usize) -> String {
        let mut s = String::from("#!/bin/bash\n# Written by InCloud: starts every batch script.\ncd \"$(dirname \"$0\")\" || exit 1\n");
        for k in 1..=n {
            if self.scheduler == Scheduler::Slurm {
                s.push_str(&format!("sbatch batch_{k}.sh\n"));
            } else {
                s.push_str(&format!("nohup bash batch_{k}.sh > batch_{k}.log 2>&1 &\necho \"batch_{k}.sh started (log: batch_{k}.log)\"\n"));
            }
        }
        s
    }

    /// A table of the models and their parameters, to read the results back.
    pub fn table(&self, models: &[Model]) -> String {
        let mut s = String::from("name");
        for p in &self.params {
            s.push(',');
            s.push_str(&p.key);
        }
        s.push('\n');
        for m in models {
            s.push_str(&m.name);
            for v in &m.values {
                s.push(',');
                s.push_str(v);
            }
            s.push('\n');
        }
        s
    }

    /// Write the input files, the scripts and the table. Returns what was written.
    pub fn write(&self) -> Result<String, String> {
        let models = self.models()?;
        if models.is_empty() {
            return Err("No models to write.".into());
        }
        let dir = PathBuf::from(self.out_dir.trim());
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot make {}: {e}", dir.display()))?;
        let n = self.scripts.clamp(1, models.len());
        let w = |name: &str, text: &str| -> Result<(), String> { std::fs::write(dir.join(name), text).map_err(|e| format!("Cannot write {}: {e}", dir.join(name).display())) };
        if !self.embed {
            for m in &models {
                w(&format!("{}.in", m.name), &m.input)?;
            }
        }
        for (rel, text) in &self.extra_files {
            let p = dir.join(rel);
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            std::fs::write(&p, text).map_err(|e| format!("Cannot write {}: {e}", p.display()))?;
        }
        for k in 0..n {
            let name = format!("batch_{}.sh", k + 1);
            w(&name, &self.script(&models, k, n))?;
            executable(&dir.join(&name));
        }
        w("start_all.sh", &self.starter(n))?;
        executable(&dir.join("start_all.sh"));
        w("models.csv", &self.table(&models))?;
        let per: Vec<String> = (0..n).map(|k| Self::share(models.len(), n, k).len().to_string()).collect();
        Ok(format!(
            "{} models written to {}: {}{} script{} ({} models each), start_all.sh and models.csv.",
            models.len(),
            dir.display(),
            if self.embed { String::new() } else { format!("{} input files, ", models.len()) },
            n,
            if n == 1 { "" } else { "s" },
            per.join(" + ")
        ))
    }
}

fn executable(p: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = p.metadata() {
            let mut perm = m.permissions();
            perm.set_mode(perm.mode() | 0o755);
            let _ = std::fs::set_permissions(p, perm);
        }
    }
    #[cfg(not(unix))]
    let _ = p;
}

/// Developer check: a small grid written to `dir`.
pub fn check(dir: &Path, exe: &str) {
    let mut b = Batch { out_dir: dir.display().to_string(), exe: exe.into(), parallel: 3, scripts: 2, ..Default::default() };
    b.params.push(preset(3));
    b.params[2].start = 0.15;
    b.params[2].stop = 0.35;
    b.name_pattern = "pdr_U{U}_hden{hden}_Z{Z}".into();
    let mut p = crate::model::Params::default();
    p.name = "model".into();
    b.template_from(&p.input_file(), &p.file_prefix());
    if let Some(t) = std::env::var("BATCH_TEMPLATE").ok().and_then(|f| std::fs::read_to_string(f).ok()) {
        b.adopt(&t);
    }
    let models = b.models().unwrap_or_default();
    println!("{} models; first {:?}", models.len(), models.first().map(|m| &m.name));
    for (err, t) in b.problems(&models) {
        println!("{} {t}", if err { "ERROR" } else { "note" });
    }
    match b.write() {
        Ok(s) => println!("{s}"),
        Err(e) => println!("failed: {e}"),
    }
}
