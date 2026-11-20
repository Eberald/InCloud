//! Choosing the species of the chemical network: from CLOUDY's own list, or typed by hand.

use std::path::Path;

use eframe::egui::{self, RichText};

use crate::app::AMBER;
use crate::figures::{ion_markup, markup_to_unicode};
use crate::model::NO_REACTIONS;

/// The species of CLOUDY C25 that take part in reactions, used when its data folder can't be read.
const BUILT_IN: &str = "C O Si N S Cl Ne Ar F C+ O+ Si+ N+ S+ Cl+ Ar+ S+2 CH CH+ OH OH+ O2 CO CO+ H2O H2O+ O2+ H3O+ CH2+ CH2 HCO+ CH3+ CH3 CH4 CH4+ CH5+ SiH2+ SiH SiOH+ SiO SiO+ N2 N2+ NO NO+ S2 S2+ OCN OCN+ NH NH+ NH2 NH2+ NH3 NH3+ NH4+ CN CN+ HCN HCN+ HNO HNO+ HS HS+ CS CS+ NO2 NO2+ NS NS+ SO SO+ SiN SiN+ N2O HCS+ OCS OCS+ HNC HCNH+ C2 C2+ CCl ClO HCl+ HCl H2Cl+ CCl+ H2CCl+ ClO+ ArH+ NeH+ H2CO SO2 SiC2 HC3N C2H C2H+ C3 C3+ C3H+ C3H C2H2 C2H2+ C2H3+ N2H+ H H+ H2 H2+ H3+ HeH+ He He+ Li- LiH LiH+ P P+ PN PO PH CP PH+ PN+ PO+ HCP CP+ PH2 HCP+ PH2+ PH3+ PH3 HF+ HF H2F+ CF+ SiS HSiS HSiS+ SiS+ SiC SiC+ SiC2+ SiNC+ SiH2 SiCH2+ SiNC";

/// Species that are not in the data file but have reactions.
const EXTRA: [&str; 3] = ["e-", "H-", "H2*"];

/// The species to offer: those of the installed CLOUDY when its data can be read.
pub fn known(data_path: &str) -> Vec<String> {
    let mut list: Vec<String> = vec![];
    for dir in data_path.split(':').map(str::trim).filter(|d| d.len() > 1) {
        let Ok(text) = std::fs::read_to_string(Path::new(dir).join("chem_species.dat")) else { continue };
        for l in text.lines().filter(|l| !l.starts_with('#')) {
            if let Some(name) = l.split_whitespace().next() {
                list.push(name.to_string());
            }
        }
        break;
    }
    if list.is_empty() {
        list = BUILT_IN.split_whitespace().map(str::to_string).collect();
    }
    list.extend(EXTRA.iter().map(|s| s.to_string()));
    list.retain(|s| !NO_REACTIONS.contains(&s.as_str()));
    list.sort();
    list.dedup();
    list
}

/// A species written with sub- and superscripts.
pub fn pretty(s: &str) -> String {
    markup_to_unicode(&ion_markup(s))
}

/// 0: atoms and atomic ions, 1: molecules, 2: molecular ions.
fn group(s: &str) -> usize {
    let base: &str = s.split(['+', '-', '*']).next().unwrap_or(s);
    let mut c = base.chars();
    let atom = c.next().is_some_and(|f| f.is_alphabetic()) && base.len() <= 2 && c.all(|l| l.is_ascii_lowercase());
    if atom {
        0
    } else if s.contains(['+', '-']) {
        2
    } else {
        1
    }
}

const GROUPS: [&str; 3] = ["Atoms and atomic ions", "Molecules", "Molecular ions"];

/// The label CLOUDY uses for what was typed: "hco+" becomes "HCO+".
fn canonical(typed: &str, known: &[String]) -> String {
    known.iter().find(|k| k.eq_ignore_ascii_case(typed)).cloned().unwrap_or_else(|| typed.to_string())
}

/// The chosen species, a search box and the list. `hide` are species not to offer. True when the choice changed.
pub fn picker(ui: &mut egui::Ui, id: &str, chosen: &mut Vec<String>, known: &[String], query: &mut String, limit: usize, hide: &[String]) -> bool {
    let mut changed = false;
    if !chosen.is_empty() {
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            for (i, s) in chosen.iter().enumerate() {
                if ui.small_button(format!("{}  ✕", pretty(s))).on_hover_text("Remove").clicked() {
                    remove = Some(i);
                }
            }
        });
        if let Some(i) = remove {
            chosen.remove(i);
            changed = true;
        }
    }
    let full = chosen.len() >= limit;
    if full {
        ui.label(RichText::new(format!("{limit} species is the most CLOUDY can save in one run.")).small().color(AMBER));
    }

    let box_id = ui.make_persistent_id((id, "find"));
    let edit = ui.add(egui::TextEdit::singleline(query).id(box_id).hint_text("🔍 Find or type a species, e.g. NH3").desired_width(f32::INFINITY));
    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let typed = query.trim().to_string();
    let usable = !typed.is_empty() && typed.chars().all(|c| c.is_ascii_alphanumeric() || "+-*".contains(c));
    let name = canonical(&typed, known);
    let listed = known.contains(&name);
    let mut add = enter && usable && !full && !chosen.contains(&name);

    if usable && !listed && !chosen.contains(&name) {
        let why = if NO_REACTIONS.contains(&name.as_str()) { format!("{name} has no reactions in CLOUDY's network, so there will be nothing to draw.") } else { format!("{name} is not in CLOUDY's list of species: check how it is written.") };
        ui.label(RichText::new(why).small().color(AMBER));
        add |= ui.add_enabled(!full, egui::Button::new(format!("Add “{name}” anyway"))).clicked();
    }
    if add {
        chosen.push(name);
        query.clear();
        changed = true;
    }

    let q = typed.to_lowercase();
    egui::ScrollArea::vertical().id_salt((id, "list")).max_height(170.0).min_scrolled_height(170.0).auto_shrink([false, true]).show(ui, |ui| {
        let mut any = false;
        for (g, title) in GROUPS.iter().enumerate() {
            let items: Vec<&String> = known.iter().filter(|s| group(s) == g && !hide.contains(s) && s.to_lowercase().contains(&q)).collect();
            if items.is_empty() {
                continue;
            }
            any = true;
            ui.label(RichText::new(*title).small().weak());
            ui.horizontal_wrapped(|ui| {
                for s in items {
                    let mut on = chosen.contains(s);
                    if ui.add_enabled(on || !full, egui::Checkbox::new(&mut on, pretty(s))).changed() {
                        if on {
                            chosen.push(s.clone());
                        } else {
                            chosen.retain(|c| c != s);
                        }
                        changed = true;
                    }
                }
            });
        }
        if !any {
            ui.label(RichText::new("No species with this name.").small().weak());
        }
    });
    changed
}
