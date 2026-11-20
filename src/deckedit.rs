//! The editor of the CLOUDY input file.

use std::sync::Arc;

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{self, Color32, FontId, RichText};

use crate::app::{AMBER, BAD, OK};
use crate::model::{check_deck, deck_overview};

/// Commands offered ready to insert (each one checked against CLOUDY C25).
pub const SNIPPETS: [(&str, &str); 9] = [
    ("CMB", "cosmic microwave background"),
    ("magnetic field -5", "log B in gauss"),
    ("turbulence 2 km/s", "microturbulence"),
    ("covering factor 0.5", "fraction of the source covered"),
    ("filling factor 0.5", "clumpy gas"),
    ("set nend 3000", "more zones allowed"),
    ("failures 100", "convergence failures allowed"),
    ("print line faint -2", "print fainter lines"),
    ("database H2", "large H₂ model (slow)"),
];

pub enum Action {
    None,
    Run,
    Open,
    SaveAs,
}

#[derive(Default)]
pub struct DeckEditor {
    pub open: bool,
    /// the text below is what will be run, not the file made from the settings
    pub edited: bool,
    pub text: String,
}

impl DeckEditor {
    /// Start from an existing input file.
    pub fn load(&mut self, text: String) {
        self.text = text;
        self.edited = true;
        self.open = true;
    }

    pub fn window(&mut self, ctx: &egui::Context, generated: &str, outputs: &str, running: bool) -> Action {
        if !self.open {
            return Action::None;
        }
        if !self.edited {
            self.text = generated.to_string();
        }
        let mut action = Action::None;
        let mut open = self.open;
        // always inside the window of the program, however small it is
        let screen = ctx.content_rect();
        let size = egui::vec2((screen.width() - 40.0).clamp(420.0, 880.0), (screen.height() - 90.0).clamp(320.0, 820.0));
        egui::Window::new("Input file").open(&mut open).fixed_size(size).default_pos(screen.center() - size * 0.5).collapsible(false).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                let b = egui::Button::new(RichText::new("▶  Run this file").strong().color(Color32::WHITE)).fill(crate::app::ACCENT);
                if ui.add_enabled(!running, b).clicked() {
                    action = Action::Run;
                }
                if ui.button("Open…").clicked() {
                    action = Action::Open;
                }
                if ui.button("Save as…").clicked() {
                    action = Action::SaveAs;
                }
                if ui.button("Copy").clicked() {
                    ui.ctx().copy_text(self.text.clone());
                }
                ui.separator();
                if self.edited {
                    if ui.button("Back to the settings").on_hover_text("Forget these changes and use the file made from the settings").clicked() {
                        self.edited = false;
                        self.text = generated.to_string();
                    }
                    ui.label(RichText::new("edited by hand").color(AMBER));
                } else {
                    ui.label(RichText::new("made from the settings: type to change it").weak());
                }
            });
            ui.add_space(4.0);

            let notes = check_deck(&self.text);
            let needs_overview = deck_overview(&self.text).is_none();
            let footer = if needs_overview { 40.0 } else { 0.0 } + (notes.len().clamp(1, 5) as f32) * 22.0 + 12.0;
            let height = (size.y - ui.min_rect().height() - footer).max(140.0);

            egui::Frame::new().fill(ui.visuals().extreme_bg_color).stroke(ui.visuals().widgets.noninteractive.bg_stroke).corner_radius(6.0).show(ui, |ui| {
                egui::ScrollArea::both().id_salt("deck_scroll").max_height(height).min_scrolled_height(height).auto_shrink([false, false]).show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        // room for the line numbers, drawn beside each row once the text is laid out
                        ui.add_space(44.0);
                        let weak = ui.visuals().weak_text_color();
                        let dark = ui.visuals().dark_mode;
                        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, _wrap: f32| -> Arc<egui::Galley> {
                            let job = highlight(text.as_str(), dark, ui.visuals().text_color());
                            ui.fonts_mut(|f| f.layout_job(job))
                        };
                        let edit = egui::TextEdit::multiline(&mut self.text)
                            .code_editor()
                            .frame(egui::Frame::NONE)
                            .margin(egui::Margin::symmetric(6, 4))
                            .desired_width(f32::INFINITY)
                            .desired_rows(24)
                            .lock_focus(true)
                            .layouter(&mut layouter);
                        let out = edit.show(ui);
                        if out.response.changed() {
                            self.edited = true;
                        }
                        for (n, row) in out.galley.rows.iter().enumerate() {
                            let at = egui::pos2(out.galley_pos.x - 12.0, out.galley_pos.y + row.rect().center().y);
                            ui.painter().text(at, egui::Align2::RIGHT_CENTER, (n + 1).to_string(), FontId::monospace(12.5), weak);
                        }
                    });
                });
            });

            ui.add_space(4.0);
            if needs_overview {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("⚠ This file saves no overview, so InCloud could show its run info but no plots.").color(AMBER));
                    if ui.button("Add what InCloud needs").on_hover_text("Adds the save commands InCloud reads back").clicked() {
                        let lines: Vec<String> = outputs.lines().map(str::to_string).collect();
                        self.text = crate::model::deck_add_lines(&self.text, &lines);
                        self.edited = true;
                    }
                });
            }
            if notes.is_empty() && !needs_overview {
                ui.label(RichText::new("✓ No mistakes found. CLOUDY has the last word when it reads the file.").small().color(OK));
            }
            for n in notes.iter().take(4) {
                let col = if n.error { BAD } else { AMBER };
                let at = if n.line > 0 { format!("Line {}: ", n.line) } else { String::new() };
                ui.label(RichText::new(format!("{at}{}", n.text)).small().color(col));
            }
            if notes.len() > 4 {
                ui.label(RichText::new(format!("and {} more", notes.len() - 4)).small().weak());
            }
        });
        self.open = open;
        action
    }
}

/// Colours for the commands, the comments, the file names and the numbers of an input file.
fn highlight(text: &str, dark: bool, plain: Color32) -> LayoutJob {
    let font = FontId::monospace(14.0);
    let (command, comment, quoted, number) = if dark {
        (Color32::from_rgb(120, 175, 255), Color32::from_rgb(125, 135, 150), Color32::from_rgb(130, 200, 140), Color32::from_rgb(230, 180, 110))
    } else {
        (Color32::from_rgb(25, 85, 180), Color32::from_rgb(120, 125, 135), Color32::from_rgb(30, 125, 65), Color32::from_rgb(165, 95, 15))
    };
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let mut add = |s: &str, c: Color32| job.append(s, 0.0, TextFormat::simple(font.clone(), c));
    for line in text.split_inclusive('\n') {
        let body = line.trim_start();
        if body.starts_with('#') || body.starts_with('*') || body.starts_with('%') || body.starts_with("//") {
            add(line, comment);
            continue;
        }
        let mut first = true;
        let mut rest = line;
        while !rest.is_empty() {
            let c = rest.chars().next().unwrap_or(' ');
            if c.is_whitespace() {
                let n = rest.find(|x: char| !x.is_whitespace()).unwrap_or(rest.len());
                add(&rest[..n], plain);
                rest = &rest[n..];
            } else if c == '"' {
                let n = rest[1..].find('"').map(|k| k + 2).unwrap_or(rest.len());
                add(&rest[..n], quoted);
                rest = &rest[n..];
                first = false;
            } else {
                let n = rest.find(char::is_whitespace).unwrap_or(rest.len());
                let word = &rest[..n];
                let numeric = word.trim_end_matches(',').parse::<f64>().is_ok();
                add(word, if first { command } else if numeric { number } else { plain });
                rest = &rest[n..];
                first = false;
            }
        }
    }
    job
}
