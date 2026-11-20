//! Abundances as a table: every zone of the cloud, with the mean over the chosen region.

use eframe::egui::{self, Color32, FontId, RichText};

use crate::figures::{markup_to_unicode, Ctx, Plot};
use crate::network::Zone;

/// The plots that show abundances.
pub fn is_abundance(p: Plot) -> bool {
    matches!(p, Plot::Structure | Plot::Molecules | Plot::Ions)
}

/// The columns before the abundances: depth, A_V, N(H) and temperature.
const BASE: usize = 4;

pub struct Table {
    pub cols: Vec<String>,
    pub regions: Vec<&'static str>,
    pub rows: Vec<Vec<f64>>,
    pub mean: Vec<f64>,
}

fn region(ctx: &Ctx, depth: f64) -> &'static str {
    for (z, name) in [(Zone::Hii, "H II"), (Zone::Pdr, "PDR"), (Zone::Molecular, "H₂")] {
        if z.range(ctx.r).is_some_and(|(a, b)| depth >= a && depth <= b) {
            return name;
        }
    }
    "—"
}

pub fn build(ctx: &Ctx, p: Plot, zone: Zone) -> Result<Table, String> {
    let r = ctx.r;
    let (d0, d1) = zone.range(r).ok_or_else(|| format!("This model has no {}", zone.label()))?;
    let f = ctx.figure(p);
    let series: Vec<_> = f.series.iter().filter(|s| !s.name.is_empty() && !s.y.is_empty()).collect();
    if series.is_empty() {
        return Err(f.note.clone().unwrap_or_else(|| "No abundances saved for this run".into()));
    }
    let n = r.depth.len();
    // a table with another number of rows is read at the same fraction of the cloud
    let at = |v: &[f64], i: usize| -> f64 {
        if v.len() == n {
            v[i]
        } else if v.is_empty() || n < 2 {
            f64::NAN
        } else {
            v[i * (v.len() - 1) / (n - 1)]
        }
    };
    let mut cols: Vec<String> = ["Depth [cm]", "A_V [mag]", "N(H) [cm⁻²]", "Tₑ [K]"].iter().map(|s| s.to_string()).collect();
    cols.extend(series.iter().map(|s| markup_to_unicode(&s.name)));
    let (mut rows, mut regions) = (vec![], vec![]);
    for i in 0..n {
        let d = r.depth[i];
        if d < d0 || d > d1 {
            continue;
        }
        let mut row = vec![d, r.av.as_ref().map(|a| at(a, i)).unwrap_or(f64::NAN), at(&r.column, i), at(&r.te, i)];
        row.extend(series.iter().map(|s| at(&s.y, i)));
        rows.push(row);
        regions.push(region(ctx, d));
    }
    if rows.is_empty() {
        return Err(format!("No zone falls in the {}", zone.label()));
    }
    // mean over depth, for the temperature and the abundances
    let mut mean = vec![f64::NAN; cols.len()];
    for (c, m) in mean.iter_mut().enumerate().skip(BASE - 1) {
        let (mut sum, mut width) = (0.0, 0.0);
        for w in rows.windows(2) {
            let dx = w[1][0] - w[0][0];
            if w[0][c].is_finite() && w[1][c].is_finite() && dx > 0.0 {
                sum += 0.5 * (w[0][c] + w[1][c]) * dx;
                width += dx;
            }
        }
        *m = if width > 0.0 { sum / width } else { rows[0][c] };
    }
    Ok(Table { cols, regions, rows, mean })
}

fn cell(col: usize, v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    match col {
        0 | 2 => format!("{v:.3e}"),
        1 => format!("{v:.3}"),
        3 => format!("{v:.1}"),
        _ if v == 0.0 => "0".into(),
        _ if v.abs() >= 1e-3 => format!("{v:.4}"),
        _ => format!("{v:.3e}"),
    }
}

/// The table as text for a spreadsheet.
pub fn csv(t: &Table) -> String {
    let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    let mut s = format!("Region,{}\n", t.cols.iter().map(|c| quote(c)).collect::<Vec<_>>().join(","));
    for (row, region) in t.rows.iter().zip(&t.regions) {
        let vals: Vec<String> = row.iter().map(|v| if v.is_finite() { format!("{v:.6e}") } else { String::new() }).collect();
        s += &format!("{},{}\n", quote(region), vals.join(","));
    }
    s
}

pub fn show(ui: &mut egui::Ui, t: &Table, id: &str, height: f32, width: f32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} zones", t.rows.len())).weak());
        if ui.small_button("Copy table").on_hover_text("Copy as CSV, to paste in a spreadsheet").clicked() {
            ui.ctx().copy_text(csv(t));
        }
    });
    let font = FontId::proportional(13.5);
    let (text, weak) = (ui.visuals().text_color(), ui.visuals().weak_text_color());
    let line = 24.0;
    let first = 64.0;
    let widths: Vec<f32> = t.cols.iter().map(|c| (ui.painter().layout_no_wrap(c.clone(), font.clone(), text).size().x + 22.0).max(88.0)).collect();
    let total = first + widths.iter().sum::<f32>();
    let stripe = ui.visuals().faint_bg_color;
    let accent = crate::app::ACCENT.gamma_multiply(0.18);

    let draw = |ui: &mut egui::Ui, label: &str, cells: &[String], fill: Option<Color32>, col: Color32| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(total, line), egui::Sense::hover());
        if !ui.is_rect_visible(rect) {
            return;
        }
        if let Some(c) = fill {
            ui.painter().rect_filled(rect, 0.0, c);
        }
        ui.painter().text(egui::pos2(rect.left() + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, label, font.clone(), col);
        let mut x = rect.left() + first;
        for (c, w) in cells.iter().zip(&widths) {
            x += w;
            ui.painter().text(egui::pos2(x - 10.0, rect.center().y), egui::Align2::RIGHT_CENTER, c, font.clone(), col);
        }
    };

    let body = (height - 34.0 - 2.0 * line - 16.0).max(line * 3.0);
    egui::ScrollArea::horizontal().id_salt((id, "abund_x")).max_width(width).auto_shrink([false, true]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.set_min_width(total);
        draw(ui, "Region", &t.cols, None, text);
        ui.separator();
        let mean: Vec<String> = t.mean.iter().enumerate().map(|(c, v)| if v.is_finite() { cell(c, *v) } else { String::new() }).collect();
        draw(ui, "Mean", &mean, Some(accent), text);
        egui::ScrollArea::vertical().id_salt((id, "abund_y")).max_height(body).auto_shrink([false, false]).show_rows(ui, line, t.rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for i in range {
                let cells: Vec<String> = t.rows[i].iter().enumerate().map(|(c, v)| cell(c, *v)).collect();
                draw(ui, t.regions[i], &cells, (i % 2 == 1).then_some(stripe), if t.regions[i] == "—" { weak } else { text });
            }
        });
    });
}
