//! The first start: say hello, find CLOUDY, or help to set it or install it.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, RichText};

use crate::app::{ACCENT, AMBER, BAD, OK};
use crate::runner::{self, Install};

/// What the person chose.
pub enum Choice {
    None,
    /// use this executable and data folder
    Use(PathBuf, String),
    /// open the window that installs CLOUDY
    Install,
    /// look around with the demo data
    Demo,
    /// pick a file or folder for the fields below
    PickExe,
    PickData,
}

#[derive(Default)]
pub struct Welcome {
    pub open: bool,
    /// the installation chosen among those found
    chosen: usize,
    pub exe: String,
    pub data: String,
    /// the last update, shown at the top
    pub whats_new: Option<crate::whatsnew::WhatsNew>,
    /// opened by the first start after an update
    pub updated: bool,
    /// the CLOUDY in use, chosen first among those found
    pub prefer: String,
}

/// A plausible CLOUDY executable: a file the computer can run.
pub fn exe_ok(p: &str) -> bool {
    let path = Path::new(p.trim());
    if !path.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return path.metadata().map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false);
    }
    #[allow(unreachable_code)]
    true
}

/// A plausible data folder: CLOUDY's data folder holds many .dat files.
pub fn data_ok(d: &str) -> bool {
    d.split(':').map(str::trim).filter(|x| x.len() > 1).any(|x| {
        let dats = std::fs::read_dir(x).map(|r| r.flatten().filter(|e| e.path().extension().is_some_and(|e| e == "dat")).count()).unwrap_or(0);
        dats >= 5
    })
}

fn check(ui: &mut egui::Ui, ok: bool, good: &str, bad: &str) {
    if ok {
        ui.label(RichText::new(format!("✔ {good}")).color(OK).small());
    } else {
        ui.label(RichText::new(format!("✘ {bad}")).color(BAD).small());
    }
}

impl Welcome {
    /// What's new since the previous version (or, before any update, in this version).
    fn whats_new_ui(&mut self, ui: &mut egui::Ui) {
        use crate::whatsnew;
        let stroke = if self.updated { ACCENT } else { ui.visuals().widgets.noninteractive.bg_stroke.color };
        egui::Frame::new().fill(ACCENT.gamma_multiply(if self.updated { 0.10 } else { 0.04 })).stroke(egui::Stroke::new(1.0, stroke)).corner_radius(10.0).inner_margin(14).show(ui, |ui| {
            ui.set_width(ui.available_width());
            match &self.whats_new {
                Some(w) => {
                    let title = if self.updated { "✨ InCloud was updated: what's new" } else { "✨ What's new since the previous version" };
                    ui.label(RichText::new(title).strong().size(17.0).color(ACCENT));
                    ui.label(RichText::new(format!("From {} to {}, updated on {}{}.", w.from_text(), w.to_text(), w.date(), if w.in_app { " from Settings → Updates" } else { "" })).small().weak());
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().id_salt("whats_new_list").max_height(230.0).auto_shrink([false, true]).show(ui, |ui| {
                    for (sec, items) in &w.notes {
                        if !sec.is_empty() {
                            ui.label(RichText::new(sec).strong());
                        }
                        for i in items {
                            ui.label(format!("•  {i}"));
                        }
                        ui.add_space(2.0);
                    }
                    if !w.commits.is_empty() {
                        ui.label(RichText::new(format!("{} new change{} on git", w.commits.len(), if w.commits.len() == 1 { "" } else { "s" })).strong());
                        for c in w.commits.iter().take(20) {
                            let (id, msg) = c.split_once(' ').unwrap_or((c.as_str(), ""));
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(id).monospace().small().weak());
                                ui.label(msg);
                            });
                        }
                        if w.commits.len() > 20 {
                            ui.label(RichText::new(format!("… and {} more", w.commits.len() - 20)).small().weak());
                        }
                    }
                    if !w.files.is_empty() {
                        ui.label(RichText::new(format!("Files changed in the source folder: {}", w.files.iter().take(10).cloned().collect::<Vec<_>>().join(", "))).small().weak());
                    }
                    });
                    if w.is_empty() {
                        ui.label(RichText::new("InCloud was built again, but this update came with no notes.").weak());
                    }
                }
                None => {
                    let (sec, items) = whatsnew::this_version();
                    ui.label(RichText::new("✨ What's new in this version").strong().size(17.0).color(ACCENT));
                    if !sec.is_empty() {
                        ui.label(RichText::new(sec).small().weak());
                    }
                    ui.add_space(2.0);
                    for i in items.iter().take(6) {
                        ui.label(format!("•  {i}"));
                    }
                    ui.label(RichText::new("After each update this box lists what changed since the version you had.").small().weak());
                }
            }
            egui::CollapsingHeader::new("All the notes").id_salt("all_notes").show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("all_notes_list").max_height(200.0).show(ui, |ui| {
                for (sec, items) in whatsnew::all_notes() {
                    ui.label(RichText::new(sec).strong());
                    for i in items {
                        ui.label(RichText::new(format!("•  {i}")).small());
                    }
                }
                });
            });
        });
        ui.add_space(10.0);
    }

    pub fn show(&mut self, ctx: &egui::Context, installs: &[Install], logo: Option<&egui::TextureHandle>) -> Choice {
        if !self.open {
            return Choice::None;
        }
        let found: Vec<&Install> = installs.iter().filter(|i| i.exists).collect();
        let mut choice = Choice::None;
        let screen = ctx.content_rect();
        let width = (screen.width() - 80.0).clamp(420.0, 720.0);
        egui::Modal::new(egui::Id::NULL.with("incloud_welcome")).show(ctx, |ui| {
            ui.set_width(width);
            // the modal keeps the size of its first frame: ask for the room a long page needs
            let tall = (screen.height() * 0.8).max(300.0);
            egui::ScrollArea::vertical().max_height(tall).min_scrolled_height(tall).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(t) = logo {
                        ui.add(egui::Image::new(egui::load::SizedTexture::new(t.id(), egui::vec2(64.0, 64.0))));
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Welcome to InCloud").size(26.0).strong().color(ACCENT));
                        ui.label("Set up, run and look at CLOUDY photoionisation and PDR models, with publication-ready plots.");
                    });
                });
                ui.add_space(12.0);
                self.whats_new_ui(ui);

                if !found.is_empty() {
                    if !self.prefer.is_empty() {
                        if let Some(k) = found.iter().position(|i| i.exe.display().to_string() == self.prefer) {
                            self.chosen = k;
                        }
                        self.prefer.clear();
                    }
                    self.chosen = self.chosen.min(found.len() - 1);
                    egui::Frame::new().fill(OK.gamma_multiply(0.12)).stroke(egui::Stroke::new(1.0, OK)).corner_radius(10.0).inner_margin(14).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let n = found.len();
                        ui.label(RichText::new(if n == 1 { "✔ CLOUDY was found on this computer".to_string() } else { format!("✔ {n} installations of CLOUDY were found") }).strong().size(17.0).color(OK));
                        ui.add_space(4.0);
                        for (k, i) in found.iter().enumerate() {
                            let name = format!("{}  —  {}", if i.version.is_empty() { "CLOUDY" } else { i.version.as_str() }, i.origin);
                            if n > 1 {
                                ui.radio_value(&mut self.chosen, k, name);
                            } else {
                                ui.label(RichText::new(name).strong());
                            }
                            if k == self.chosen {
                                ui.label(RichText::new(format!("Program: {}", i.exe.display())).small().weak());
                                ui.label(RichText::new(format!("Data: {}", if i.data.is_empty() { "—" } else { i.data.as_str() })).small().weak());
                            }
                        }
                        ui.add_space(8.0);
                        let b = egui::Button::new(RichText::new(if self.updated { "Continue" } else { "Start" }).size(17.0).strong().color(Color32::WHITE)).fill(ACCENT).min_size(egui::vec2(160.0, 38.0));
                        if ui.add(b).clicked() {
                            let i = found[self.chosen];
                            let data = if i.data.is_empty() { runner::data_for_exe(&i.exe) } else { i.data.clone() };
                            choice = Choice::Use(i.exe.clone(), data);
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(RichText::new("You can change version, or install another one, from the CLOUDY label at the top right. This window can be opened again from Settings.").small().weak());
                    return;
                }

                egui::Frame::new().fill(AMBER.gamma_multiply(0.14)).stroke(egui::Stroke::new(1.0, AMBER)).corner_radius(10.0).inner_margin(12).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("CLOUDY was not found on this computer").strong().size(17.0).color(AMBER));
                    ui.label("InCloud looked at CLOUDY_EXE and CLOUDY_DATA_PATH, at the aliases in your shell start-up files (~/.bashrc, ~/.zshrc…) and in the usual folders. Choose one of these:");
                });
                ui.add_space(10.0);

                // 1. it is already there
                egui::Frame::new().fill(ui.visuals().faint_bg_color).stroke(ui.visuals().widgets.noninteractive.bg_stroke).corner_radius(10.0).inner_margin(14).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("1.  I already have CLOUDY").strong().size(16.0));
                    ui.label(RichText::new("Choose the program “cloudy.exe”: it is in the “source” folder of your CLOUDY, for example …/c25.00/source/cloudy.exe. The data folder is next to it, …/c25.00/data, and is filled in for you.").small());
                    ui.add_space(4.0);
                    ui.label("Program");
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("📁").on_hover_text("Choose cloudy.exe").clicked() {
                                choice = Choice::PickExe;
                            }
                            if ui.add(egui::TextEdit::singleline(&mut self.exe).hint_text("…/c25.00/source/cloudy.exe").desired_width(ui.available_width())).changed() && exe_ok(&self.exe) && self.data.trim().is_empty() {
                                self.data = runner::data_for_exe(Path::new(self.exe.trim()));
                            }
                        });
                    });
                    ui.label("Data folder (CLOUDY_DATA_PATH)");
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("📁").on_hover_text("Choose the data folder").clicked() {
                                choice = Choice::PickData;
                            }
                            ui.add(egui::TextEdit::singleline(&mut self.data).hint_text("…/c25.00/data").desired_width(ui.available_width()));
                        });
                    });
                    let (e, d) = (exe_ok(&self.exe), data_ok(&self.data));
                    if !self.exe.trim().is_empty() {
                        check(ui, e, "This is a program that can run.", "No program at this path (or it can't be run): look for source/cloudy.exe.");
                    }
                    if !self.data.trim().is_empty() {
                        check(ui, d, "This looks like CLOUDY's data folder.", "This folder doesn't hold CLOUDY's data files (*.dat): look for the “data” folder.");
                    }
                    ui.add_space(4.0);
                    if ui.add_enabled(e && d, egui::Button::new(RichText::new("Use this CLOUDY").strong())).clicked() {
                        choice = Choice::Use(PathBuf::from(self.exe.trim()), self.data.trim().to_string());
                    }
                });
                ui.add_space(8.0);

                // 2. install it
                egui::Frame::new().fill(ui.visuals().faint_bg_color).stroke(ui.visuals().widgets.noninteractive.bg_stroke).corner_radius(10.0).inner_margin(14).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("2.  Install it for me").strong().size(16.0));
                    ui.label(RichText::new("InCloud downloads the official CLOUDY from nublado.org and compiles it on this computer. It needs a C++ compiler, make and perl (InCloud tells you how to get them), and takes from a few minutes to half an hour. You can look around meanwhile.").small());
                    ui.add_space(4.0);
                    if ui.button(RichText::new("⬇ Install CLOUDY…").strong()).clicked() {
                        choice = Choice::Install;
                    }
                });
                ui.add_space(8.0);

                // 3. later
                egui::Frame::new().fill(ui.visuals().faint_bg_color).stroke(ui.visuals().widgets.noninteractive.bg_stroke).corner_radius(10.0).inner_margin(14).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("3.  Not now").strong().size(16.0));
                    ui.label(RichText::new("Explore InCloud with demo data. Runs stay off until CLOUDY is set: the CLOUDY label at the top right turns red and opens the setup when clicked.").small());
                    ui.add_space(4.0);
                    if ui.button("Explore the demo").clicked() {
                        choice = Choice::Demo;
                    }
                });
            });
        });
        choice
    }
}
