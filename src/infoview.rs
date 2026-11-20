//! "Run info": what a finished CLOUDY run reports about itself.

use std::path::PathBuf;

use eframe::egui::{self, Color32, RichText};

use crate::app::{help, ACCENT, AMBER, BAD, OK};
use crate::results::{pretty_line, Results};
use crate::runinfo::{pretty, Item, Level, RunInfo};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Summary,
    Physics,
    Spectrum,
    Messages,
    Iterations,
    Input,
    Files,
    Output,
}

pub struct InfoState {
    page: Page,
    show: [bool; 4],
    table: usize,
    find: String,
    output: Option<(PathBuf, Vec<String>)>,
}

impl Default for InfoState {
    fn default() -> Self {
        InfoState { page: Page::Summary, show: [true; 4], table: 0, find: String::new(), output: None }
    }
}

impl InfoState {
    /// Go to a page by its name.
    pub fn show(&mut self, name: &str) {
        self.page = match name {
            "physics" => Page::Physics,
            "spectrum" => Page::Spectrum,
            "messages" => Page::Messages,
            "iterations" => Page::Iterations,
            "input" => Page::Input,
            "files" => Page::Files,
            "output" => Page::Output,
            _ => Page::Summary,
        };
    }
}

pub enum Action {
    None,
    EditInput,
    OpenRun(String),
}

fn level_color(l: Level) -> Color32 {
    match l {
        Level::Warning => BAD,
        Level::Caution => AMBER,
        Level::Surprise => ACCENT,
        Level::Note => Color32::GRAY,
    }
}

fn panel(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(ui.visuals().faint_bg_color).stroke(ui.visuals().widgets.noninteractive.bg_stroke).corner_radius(10.0).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).strong().size(16.0));
        ui.add_space(4.0);
        add(ui);
    });
    ui.add_space(10.0);
}

/// Name on the left, value on the right.
fn row(ui: &mut egui::Ui, name: &str, value: &str, tip: &str) {
    ui.horizontal(|ui| {
        let r = ui.add(egui::Label::new(RichText::new(name).color(ui.visuals().weak_text_color())).truncate());
        if !tip.is_empty() {
            r.on_hover_text(tip);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add(egui::Label::new(RichText::new(value).strong()).truncate());
        });
    });
}

fn items(ui: &mut egui::Ui, list: &[Item]) {
    for i in list {
        row(ui, &i.name, &i.value, i.tip);
    }
}

/// What the run did, in one sentence.
fn verdict(r: &Results) -> (Color32, String, String) {
    let i = &r.info;
    if r.demo {
        return (AMBER, "Demo data".into(), "These numbers are synthetic: no CLOUDY calculation was run.".into());
    }
    if !i.present {
        return (AMBER, "No text output".into(), "The .out file of this run is missing: only the saved tables are available.".into());
    }
    let mut parts = vec![];
    if !i.stop_reason.is_empty() {
        parts.push(format!("Stopped because: {}", i.stop_reason));
    }
    if let Some(c) = i.converged {
        parts.push(if c { format!("converged in {} iterations", i.iterations.unwrap_or(0)) } else { "NOT converged".to_string() });
    }
    if let Some(z) = i.zones {
        parts.push(format!("{z} zones"));
    }
    if let Some(t) = i.exec_time {
        parts.push(format!("{}", crate::runner::fmt_duration(std::time::Duration::from_secs_f64(t.max(0.0)))));
    }
    let text = parts.join("  ·  ");
    if !i.complete {
        return (BAD, "The run did not finish".into(), "CLOUDY stopped before the end: the last lines it wrote are below.".into());
    }
    let (w, c) = (i.count(Level::Warning), i.count(Level::Caution));
    if w > 0 {
        (BAD, format!("Finished with {w} warning{}", if w == 1 { "" } else { "s" }), text)
    } else if i.converged == Some(false) {
        (AMBER, "Finished without converging".into(), text)
    } else if c > 0 {
        (AMBER, format!("Finished with {c} caution{}", if c == 1 { "" } else { "s" }), text)
    } else {
        (OK, "Finished normally".into(), text)
    }
}

/// The quantities InCloud measures on the saved structure of the cloud.
fn structure(r: &Results) -> Vec<Item> {
    let mut v = vec![];
    let mut add = |name: &str, value: Option<String>, tip: &'static str| {
        if let Some(value) = value {
            v.push(Item { name: name.to_string(), value, tip });
        }
    };
    let cm = |d: f64| format!("{} cm ({} pc)", pretty(d), pretty(d / 3.0857e18));
    add("Hydrogen density", r.deck.log_hden.map(|h| format!("log n(H) = {h:.2}")), "");
    add("Thickness", r.depth.last().map(|d| cm(*d)), "Depth of the last zone");
    add("Column density N(H)", r.column.last().map(|c| format!("{} cm⁻²", pretty(*c))), "");
    add("A_V", r.av.as_ref().and_then(|a| a.iter().rev().find(|v| v.is_finite())).map(|a| format!("{a:.2} mag")), "");
    add("Temperature range", {
        let f: Vec<f64> = r.te.iter().copied().filter(|x| x.is_finite()).collect();
        (!f.is_empty()).then(|| format!("{} – {} K", pretty(f.iter().cloned().fold(f64::MAX, f64::min)), pretty(f.iter().cloned().fold(f64::MIN, f64::max))))
    }, "");
    add("Mean T in the H II region", r.mean_te_hii().map(|t| format!("{} K", pretty(t))), "");
    add("Ionisation front", r.fronts.ionisation.map(cm), "Where H⁺ falls below 50%");
    add("Dissociation front", r.fronts.dissociation.map(cm), "Where 2H₂/H rises above 50%");
    add("C → CO transition", r.fronts.co.map(cm), "");
    add("Zones saved", (!r.depth.is_empty()).then(|| r.depth.len().to_string()), "");
    // what was asked for, against what was reached
    let d = &r.deck;
    let reached_col = r.column.last().copied().filter(|c| *c > 0.0).map(|c| c.log10());
    let reached_av = r.av.as_ref().and_then(|a| a.iter().rev().find(|v| v.is_finite()).copied());
    let reached_thick = r.depth.last().copied().filter(|x| *x > 0.0).map(|x| x.log10());
    let mut asked = |name: &str, want: Option<f64>, got: Option<f64>, fmt: &dyn Fn(f64) -> String, log: bool| {
        if let (Some(w), Some(g)) = (want, got) {
            let short = if log { g < w - 0.05 } else { g < 0.95 * w };
            let value = if short { format!("{} — reached {} only", fmt(w), fmt(g)) } else { format!("{} — reached", fmt(w)) };
            v.push(Item { name: format!("Asked: {name}"), value, tip: "A stopping criterion of the input file and how far the run got" });
        }
    };
    asked("log N(H)", d.stop_log_column, reached_col, &|x| format!("{x:.2}"), true);
    asked("A_V", d.stop_av, reached_av, &|x| format!("{x:.2} mag"), false);
    asked("log thickness", d.stop_log_thickness, reached_thick, &|x| format!("{x:.2}"), true);
    v
}

/// Everything on one page of text.
fn report(r: &Results) -> String {
    let i = &r.info;
    let mut s = format!("Run {} in {}\n", r.prefix, r.dir.display());
    let (_, head, text) = verdict(r);
    s += &format!("{head}. {text}\n\n");
    for it in structure(r) {
        s += &format!("{}: {}\n", it.name, it.value);
    }
    for sec in &i.sections {
        s += &format!("\n{}\n", sec.title);
        for it in &sec.items {
            s += &format!("  {}: {}\n", it.name, it.value);
        }
    }
    for l in Level::ALL {
        let list: Vec<&str> = i.messages.iter().filter(|m| m.level == l).map(|m| m.text.as_str()).collect();
        if !list.is_empty() {
            s += &format!("\n{}\n", l.plural());
            for m in list {
                s += &format!("  {m}\n");
            }
        }
    }
    s + &format!("\nInput file\n{}\n", r.input_text)
}

pub fn show(ui: &mut egui::Ui, r: &Results, st: &mut InfoState) -> Action {
    let mut action = Action::None;
    let i = &r.info;
    ui.add_space(4.0);

    // the verdict
    let (col, head, text) = verdict(r);
    egui::Frame::new().fill(col.gamma_multiply(0.14)).stroke(egui::Stroke::new(1.5, col)).corner_radius(10.0).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(head).strong().size(19.0).color(col));
            help(ui, "runinfo");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if r.siblings.len() > 1 {
                    egui::ComboBox::from_id_salt("run_choice").selected_text(r.prefix.as_str()).width(170.0).truncate().show_ui(ui, |ui| {
                        for name in &r.siblings {
                            if ui.selectable_label(*name == r.prefix, name).clicked() && *name != r.prefix {
                                action = Action::OpenRun(name.clone());
                            }
                        }
                    });
                    ui.label("Run:");
                }
            });
        });
        if !text.is_empty() {
            ui.label(text);
        }
    });
    ui.add_space(8.0);

    // the pages
    ui.horizontal_wrapped(|ui| {
        let n = i.count(Level::Warning) + i.count(Level::Caution) + i.count(Level::Surprise) + i.count(Level::Note);
        let pages = [
            (Page::Summary, "Summary".to_string()),
            (Page::Physics, "Physical conditions".to_string()),
            (Page::Spectrum, "Lines and abundances".to_string()),
            (Page::Messages, format!("Messages ({n})")),
            (Page::Iterations, format!("Iterations ({})", i.passes.len())),
            (Page::Input, "Input file".to_string()),
            (Page::Files, format!("Files ({})", r.files.found.len())),
            (Page::Output, "Full output".to_string()),
        ];
        for (p, label) in pages {
            ui.selectable_value(&mut st.page, p, label);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Copy report").on_hover_text("Copy all of this as text").clicked() {
                ui.ctx().copy_text(report(r));
            }
        });
    });
    ui.separator();

    if st.page == Page::Output {
        output_page(ui, r, st);
        return action;
    }
    egui::ScrollArea::vertical().id_salt(("info_page", st.page as u8)).auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(6.0);
        match st.page {
            Page::Summary => summary_page(ui, r, st, &mut action),
            Page::Physics => physics_page(ui, r),
            Page::Spectrum => spectrum_page(ui, i, st),
            Page::Messages => messages_page(ui, i, st),
            Page::Iterations => iterations_page(ui, i),
            Page::Input => input_page(ui, r, &mut action),
            Page::Files => files_page(ui, r),
            Page::Output => {}
        }
    });
    action
}

fn summary_page(ui: &mut egui::Ui, r: &Results, st: &mut InfoState, action: &mut Action) {
    let i = &r.info;
    let two = ui.available_width() > 760.0;
    let left = |ui: &mut egui::Ui, st: &mut InfoState| {
        panel(ui, "The run", |ui| {
            let mut put = |name: &str, v: String| {
                if !v.is_empty() {
                    row(ui, name, &v, "");
                }
            };
            put("Title", i.title.clone());
            put("Version", i.version.clone());
            put("Model", i.model.clone());
            put("Stopped because", i.stop_reason.clone());
            if let Some((a, b)) = i.iteration {
                put("Iteration", if i.complete { a.to_string() } else { format!("{a} of {b}") });
            }
            put("Iterate to convergence", match (i.converge_asked, i.converged) {
                (true, Some(true)) => "converged".into(),
                (true, Some(false)) => "not converged".into(),
                (true, None) => "unknown".into(),
                _ => "not requested".into(),
            });
            put("Zones", i.zones.map(|z| z.to_string()).unwrap_or_default());
            put("Computing time", i.exec_time.map(|t| crate::runner::fmt_duration(std::time::Duration::from_secs_f64(t.max(0.0)))).unwrap_or_default());
            put("Threads", i.threads.clone());
            put("Convergence failures", i.failures.clone());
            put("Random seed", i.seed.clone());
            put("Folder", r.dir.display().to_string());
        });
        if !i.tail.is_empty() {
            panel(ui, "The last lines CLOUDY wrote", |ui| {
                for l in &i.tail {
                    ui.label(RichText::new(l).monospace().small());
                }
            });
        }
        let serious: Vec<_> = i.messages.iter().filter(|m| matches!(m.level, Level::Warning | Level::Caution)).collect();
        if !serious.is_empty() || !i.problems.is_empty() {
            panel(ui, "To check", |ui| {
                for m in serious.iter().take(8) {
                    ui.label(RichText::new(format!("{}: {}", m.level.singular(), m.text)).color(level_color(m.level)));
                }
                for p in i.problems.iter().take(4) {
                    ui.label(RichText::new(p).small().color(BAD));
                    if let Some(why) = crate::runinfo::explain(p) {
                        ui.label(RichText::new(format!("→ {why}")).strong());
                    }
                }
                if ui.link("All the messages").clicked() {
                    st.page = Page::Messages;
                }
            });
        }
    };
    let right = |ui: &mut egui::Ui, action: &mut Action| {
        let list = structure(r);
        if !list.is_empty() {
            panel(ui, "The cloud", |ui| items(ui, &list));
        }
        if let Some(sec) = i.sections.iter().find(|s| s.title == "Incident radiation") {
            panel(ui, sec.title, |ui| items(ui, &sec.items[..sec.items.len().min(6)]));
        }
        panel(ui, "Run it again", |ui| {
            ui.label(RichText::new("Start from the input file of this run and change what you need.").small().weak());
            if ui.add_enabled(!r.input_text.is_empty(), egui::Button::new(RichText::new("✎ Edit this input file").strong())).clicked() {
                *action = Action::EditInput;
            }
        });
    };
    if two {
        ui.columns(2, |c| {
            left(&mut c[0], st);
            right(&mut c[1], action);
        });
    } else {
        left(ui, st);
        right(ui, action);
    }
}

fn physics_page(ui: &mut egui::Ui, r: &Results) {
    let i = &r.info;
    if i.sections.is_empty() {
        ui.label(RichText::new("The output of this run has no final summary.").weak());
        return;
    }
    let cols = if ui.available_width() > 1000.0 { 2 } else { 1 };
    ui.columns(cols, |c| {
        // fill the shorter column each time, so that the two end at about the same height
        let mut heights = vec![0usize; cols];
        for s in &i.sections {
            let k = (0..cols).min_by_key(|k| heights[*k]).unwrap_or(0);
            heights[k] += s.items.len() + 3;
            panel(&mut c[k], s.title, |ui| items(ui, &s.items));
        }
        if !i.grains.is_empty() {
            let k = (0..cols).min_by_key(|k| heights[*k]).unwrap_or(0);
            panel(&mut c[k], "Grains, by size bin", |ui| {
                egui::Grid::new("grains").striped(true).num_columns(4).spacing([18.0, 3.0]).show(ui, |ui| {
                    for h in ["Bin", "⟨T⟩ [K]", "Potential [V]", "Dust/gas"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for g in &i.grains {
                        ui.label(&g.name);
                        ui.label(format!("{:.1}", g.temp));
                        ui.label(format!("{:.2}", g.potential));
                        ui.label(pretty(g.dust_gas));
                        ui.end_row();
                    }
                });
            });
        }
    });
}

fn spectrum_page(ui: &mut egui::Ui, i: &RunInfo, st: &mut InfoState) {
    let cols = if ui.available_width() > 1000.0 { 2 } else { 1 };
    ui.columns(cols, |c| {
        panel(&mut c[0], "Strongest emission lines", |ui| {
            if i.strongest.is_empty() {
                ui.label(RichText::new("No line table in this output.").weak());
            }
            egui::Grid::new("strong").striped(true).num_columns(3).spacing([24.0, 3.0]).show(ui, |ui| {
                for h in ["Line", "log I", "I / Hβ"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for (label, lg, rel) in &i.strongest {
                    ui.label(pretty_line(label)).on_hover_text(label);
                    ui.label(format!("{lg:.3}"));
                    ui.label(rel.map(pretty).unwrap_or_else(|| "—".into()));
                    ui.end_row();
                }
            });
        });
        let k = cols - 1;
        if !i.gas.is_empty() {
            panel(&mut c[k], "Gas-phase abundances, log (X/H)", |ui| {
                egui::Grid::new("gas_ab").num_columns(6).spacing([14.0, 3.0]).show(ui, |ui| {
                    for (n, (el, v)) in i.gas.iter().enumerate() {
                        ui.label(RichText::new(el).strong());
                        ui.label(format!("{v:.2}"));
                        if n % 3 == 2 {
                            ui.end_row();
                        }
                    }
                });
                if !i.dust.is_empty() {
                    ui.add_space(6.0);
                    let list: Vec<String> = i.dust.iter().map(|(e, v)| format!("{e} {v:.2}")).collect();
                    ui.label(RichText::new(format!("In grains: {}", list.join("  ·  "))).small().weak());
                }
            });
        }
        if !i.molecules.is_empty() {
            panel(&mut c[k], "Molecules, log N [cm⁻²]", |ui| {
                ui.label(RichText::new("The most abundant ones.").small().weak());
                egui::Grid::new("mols").num_columns(6).spacing([14.0, 3.0]).show(ui, |ui| {
                    for (n, (m, v)) in i.molecules.iter().take(24).enumerate() {
                        ui.label(RichText::new(crate::figures::markup_to_unicode(&crate::figures::ion_markup(m))).strong());
                        ui.label(format!("{v:.2}"));
                        if n % 3 == 2 {
                            ui.end_row();
                        }
                    }
                });
            });
        }
    });
    if !i.tables.is_empty() {
        st.table = st.table.min(i.tables.len() - 1);
        panel(ui, "Ions of each element", |ui| {
            egui::ComboBox::from_id_salt("ion_table").selected_text(i.tables[st.table].title.as_str()).width(380.0).show_ui(ui, |ui| {
                for (k, t) in i.tables.iter().enumerate() {
                    ui.selectable_value(&mut st.table, k, t.title.as_str());
                }
            });
            ui.add_space(4.0);
            const STAGE: [&str; 8] = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII"];
            egui::ScrollArea::horizontal().id_salt("ion_scroll").show(ui, |ui| {
                egui::Grid::new("ions").striped(true).num_columns(9).spacing([16.0, 2.0]).show(ui, |ui| {
                    ui.label("");
                    for s in STAGE {
                        ui.label(RichText::new(s).strong());
                    }
                    ui.end_row();
                    for (el, vals) in &i.tables[st.table].rows {
                        ui.label(RichText::new(el).strong());
                        for k in 0..STAGE.len() {
                            match vals.get(k) {
                                Some(v) if *v > -29.0 => ui.label(format!("{v:.2}")),
                                _ => ui.label(""),
                            };
                        }
                        ui.end_row();
                    }
                });
            });
            ui.label(RichText::new("Hydrogen: the third column is H₂.").small().weak());
        });
    }
}

fn messages_page(ui: &mut egui::Ui, i: &RunInfo, st: &mut InfoState) {
    if i.messages.is_empty() && i.problems.is_empty() {
        ui.label(RichText::new("CLOUDY left no messages in this output.").weak());
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for (k, l) in Level::ALL.iter().enumerate() {
            ui.toggle_value(&mut st.show[k], format!("{} ({})", l.plural(), i.count(*l))).on_hover_text(l.meaning());
        }
        help(ui, "warnings");
    });
    ui.label(RichText::new("From the last iteration of the run.").small().weak());
    ui.add_space(6.0);
    for (k, l) in Level::ALL.iter().enumerate() {
        let list: Vec<_> = i.messages.iter().filter(|m| m.level == *l).collect();
        if !st.show[k] || list.is_empty() {
            continue;
        }
        panel(ui, l.plural(), |ui| {
            ui.label(RichText::new(l.meaning()).small().weak());
            ui.add_space(4.0);
            for m in list {
                ui.horizontal_top(|ui| {
                    ui.label(RichText::new("●").color(level_color(*l)));
                    ui.add(egui::Label::new(m.text.as_str()).wrap());
                });
            }
        });
    }
    if !i.problems.is_empty() {
        panel(ui, "Problems reported during the run", |ui| {
            for p in &i.problems {
                ui.label(RichText::new(p).monospace().small().color(BAD));
            }
        });
    }
}

fn iterations_page(ui: &mut egui::Ui, i: &RunInfo) {
    if i.passes.is_empty() {
        ui.label(RichText::new("No iteration was completed.").weak());
        return;
    }
    panel(ui, "Each iteration", |ui| {
        egui::ScrollArea::horizontal().id_salt("iter_scroll").show(ui, |ui| {
            egui::Grid::new("iters").striped(true).num_columns(7).spacing([20.0, 5.0]).show(ui, |ui| {
                for h in ["#", "Stopped because", "log heating", "Energy error", "T low – high [K]", "Time so far", "Convergence"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                let n = i.passes.len();
                for (k, p) in i.passes.iter().enumerate() {
                    ui.label(p.number.to_string());
                    ui.label(&p.stop);
                    ui.label(p.log_heat.map(|v| format!("{v:.3}")).unwrap_or_else(|| "—".into()));
                    ui.label(p.error.map(|v| format!("{v:.1} %")).unwrap_or_else(|| "—".into()));
                    ui.label(match (p.te_low, p.te_high) {
                        (Some(a), Some(b)) => format!("{} – {}", pretty(a), pretty(b)),
                        _ => "—".into(),
                    });
                    ui.label(p.exec.map(|v| format!("{v:.1} s")).unwrap_or_else(|| "—".into()));
                    match &p.not_converged {
                        Some(w) => ui.label(RichText::new(format!("not yet: {w}")).color(AMBER)),
                        None if k + 1 == n && i.converged == Some(true) => ui.label(RichText::new("converged").color(OK)),
                        None if k + 1 == n && i.converged == Some(false) => ui.label(RichText::new("not converged").color(BAD)),
                        None => ui.label("—"),
                    };
                    ui.end_row();
                }
            });
        });
    });
    if !i.stats.is_empty() {
        panel(ui, "Solver statistics", |ui| {
            for s in &i.stats {
                ui.label(RichText::new(s).small());
            }
        });
    }
}

/// Which file holds what, how it was found, and what is missing.
fn files_page(ui: &mut egui::Ui, r: &Results) {
    use crate::detect::{How, Kind};
    panel(ui, "Files found", |ui| {
        ui.label(RichText::new(format!("In {}", r.dir.display())).small().weak());
        ui.label(RichText::new("InCloud reads the save commands of the input file to find each file, and recognises the others from what they hold, whatever their names.").small().weak());
        ui.add_space(4.0);
        egui::Grid::new("files_found").striped(true).num_columns(4).spacing([18.0, 4.0]).show(ui, |ui| {
            for h in ["What", "File", "Found by", "Used for"] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for f in &r.files.found {
                ui.label(f.kind.name());
                ui.label(RichText::new(f.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()).monospace());
                ui.label(RichText::new(f.how.text()).weak().color(if f.how == How::Content { AMBER } else { ui.visuals().weak_text_color() }));
                ui.label(RichText::new(f.kind.used_for()).small().weak());
                ui.end_row();
            }
        });
        if !r.files.others.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("Not used: {}", r.files.others.join(", "))).small().weak());
        }
    });
    // what is missing, with the command that saves it
    let prefix = r.prefix.as_str();
    let mut missing: Vec<Kind> = [Kind::Output, Kind::Overview, Kind::Continuum, Kind::Emissivity, Kind::Heating, Kind::Cooling, Kind::Grains].into_iter().filter(|k| r.files.get(k).is_none() && !(*k == Kind::Emissivity && r.files.get(&Kind::Cumulative).is_some())).collect();
    if r.files.elements().is_empty() {
        missing.push(Kind::Element("O".into()));
    }
    if r.files.chemistry().is_empty() {
        missing.push(Kind::Formation("CO".into()));
        missing.push(Kind::Destruction("CO".into()));
    }
    if missing.is_empty() {
        panel(ui, "Nothing missing", |ui| {
            ui.label(RichText::new("✔ Every kind of file InCloud can use was found.").color(OK));
        });
        return;
    }
    panel(ui, "Missing, and how to save it", |ui| {
        ui.label(RichText::new("To have these too, add the commands below to the input file (they can be pasted as they are) and run the model again. File names are free: InCloud finds the files from these commands.").small().weak());
        ui.add_space(4.0);
        let mut all = String::new();
        for k in &missing {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(k.name()).strong());
                ui.label(RichText::new(format!("— for {}", k.used_for())).small().weak());
            });
            let cmd = k.command(prefix);
            ui.label(RichText::new(&cmd).monospace().small());
            ui.add_space(2.0);
            if !cmd.starts_with('(') {
                all.push_str(&cmd);
                all.push('\n');
            }
        }
        if !all.is_empty() && ui.button("Copy the commands").clicked() {
            ui.ctx().copy_text(all);
        }
    });
}

fn input_page(ui: &mut egui::Ui, r: &Results, action: &mut Action) {
    ui.horizontal(|ui| {
        if ui.add_enabled(!r.input_text.is_empty(), egui::Button::new(RichText::new("✎ Edit and run again").strong())).clicked() {
            *action = Action::EditInput;
        }
        if ui.button("Copy").clicked() {
            ui.ctx().copy_text(r.input_text.clone());
        }
    });
    ui.add_space(6.0);
    let mut text = r.input_text.as_str();
    ui.add(egui::TextEdit::multiline(&mut text).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
}

fn output_page(ui: &mut egui::Ui, r: &Results, st: &mut InfoState) {
    let path = r.dir.join(format!("{}.out", r.prefix));
    if st.output.as_ref().map(|o| o.0 != path).unwrap_or(true) {
        let text = std::fs::read(&path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        st.output = Some((path.clone(), text.lines().map(|l| l.to_string()).collect()));
    }
    let lines = st.output.as_ref().map(|o| &o.1).unwrap();
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut st.find).hint_text("🔍 Show only the lines containing…").desired_width(320.0));
        ui.label(RichText::new(path.display().to_string()).small().weak());
    });
    ui.add_space(4.0);
    let q = st.find.to_lowercase();
    let shown: Vec<usize> = if q.is_empty() { (0..lines.len()).collect() } else { lines.iter().enumerate().filter(|(_, l)| l.to_lowercase().contains(&q)).map(|(k, _)| k).collect() };
    if shown.is_empty() {
        ui.label(RichText::new(if lines.is_empty() { "This run has no text output." } else { "No line contains this." }).weak());
        return;
    }
    let h = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::both().id_salt("out_text").auto_shrink([false, false]).show_rows(ui, h, shown.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for k in range {
            let n = shown[k];
            ui.add(egui::Label::new(RichText::new(format!("{:>6}  {}", n + 1, lines[n])).monospace()).extend());
        }
    });
}
