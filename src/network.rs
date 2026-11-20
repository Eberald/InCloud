//! Chemical networks.
//!
//! With one species: the reactions that form it (left) and destroy it (right).
//! With several species: a map of how they feed each other, an arrow A → B meaning
//! that a share of the destruction of A produces B. Rates are integrated over a chosen zone.

use std::fmt::Write as _;

use eframe::egui::{self, Color32, FontId, Pos2, Stroke};

use crate::export::Style;
use crate::figures::{ion_markup, markup_to_unicode, pal};
use crate::results::{pretty_reaction, ReactionShare, Results};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    Whole,
    Hii,
    Pdr,
    Molecular,
}

impl Zone {
    pub const ALL: [Zone; 4] = [Zone::Whole, Zone::Hii, Zone::Pdr, Zone::Molecular];
    pub fn label(self) -> &'static str {
        match self {
            Zone::Whole => "Whole cloud",
            Zone::Hii => "H II region",
            Zone::Pdr => "PDR",
            Zone::Molecular => "Molecular zone",
        }
    }
    /// Depth range of the zone, if it exists in this model.
    pub fn range(self, r: &Results) -> Option<(f64, f64)> {
        let first = *r.depth.first()?;
        let last = *r.depth.last()?;
        let (i, d) = (r.fronts.ionisation, r.fronts.dissociation);
        match self {
            Zone::Whole => Some((0.0, last)),
            Zone::Hii => Some((0.0, i.unwrap_or(if r.x_hplus.first().copied().unwrap_or(0.0) > 0.5 { last } else { first }))),
            Zone::Pdr => Some((i.unwrap_or(first), d.unwrap_or(last))).filter(|(a, b)| b > a),
            Zone::Molecular => d.map(|d| (d, last)).filter(|(a, b)| b > a),
        }
    }
}

fn sp(s: &str) -> String {
    markup_to_unicode(&ion_markup(s))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn c32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn family(st: &Style) -> &'static str {
    if st.serif {
        "DejaVu Serif, Times New Roman, serif"
    } else {
        "DejaVu Sans, Arial, sans-serif"
    }
}

/// An arrow as a smooth centre line ending where its head begins, plus the head's three corners.
struct Arrow {
    line: Vec<(f32, f32)>,
    head: [(f32, f32); 3],
    width: f32,
}

/// Width of an arrow for a share between 0 and 1.
fn arrow_width(share: f64) -> f32 {
    1.4 + 7.5 * (share as f32).clamp(0.0, 1.0).sqrt()
}

/// Build an arrow along a curve; the shaft stops at the base of the head so nothing pokes through.
fn arrow_along(curve: impl Fn(f32) -> (f32, f32), width: f32) -> Arrow {
    let head_len = 9.0 + width * 1.3;
    let head_half = 5.0 + width * 0.75;
    let n = 40;
    let pts: Vec<(f32, f32)> = (0..=n).map(|i| curve(i as f32 / n as f32)).collect();
    let tip = *pts.last().unwrap();
    // walk back along the curve until the head length is used up
    let mut k = pts.len() - 1;
    while k > 0 {
        let (dx, dy) = (tip.0 - pts[k].0, tip.1 - pts[k].1);
        if (dx * dx + dy * dy).sqrt() >= head_len {
            break;
        }
        k -= 1;
    }
    let base = pts[k];
    let (dx, dy) = (tip.0 - base.0, tip.1 - base.1);
    let l = (dx * dx + dy * dy).sqrt().max(1e-3);
    let (ux, uy) = (dx / l, dy / l);
    Arrow {
        line: pts[..=k].to_vec(),
        head: [tip, (base.0 - uy * head_half, base.1 + ux * head_half), (base.0 + uy * head_half, base.1 - ux * head_half)],
        width,
    }
}

impl Arrow {
    fn paint(&self, p: &egui::Painter, at: &dyn Fn(f32, f32) -> Pos2, scale: f32, col: Color32) {
        let pts: Vec<Pos2> = self.line.iter().map(|(x, y)| at(*x, *y)).collect();
        if pts.len() >= 2 {
            p.add(egui::Shape::line(pts, Stroke::new(self.width * scale, col)));
        }
        let h: Vec<Pos2> = self.head.iter().map(|(x, y)| at(*x, *y)).collect();
        p.add(egui::Shape::convex_polygon(h, col, Stroke::NONE));
    }
    fn svg(&self, s: &mut String, col: &str) {
        let mut d = String::new();
        for (i, (x, y)) in self.line.iter().enumerate() {
            let _ = write!(d, "{}{x:.1} {y:.1} ", if i == 0 { "M" } else { "L" });
        }
        let _ = writeln!(s, "<path d=\"{d}\" fill=\"none\" stroke=\"{col}\" stroke-width=\"{:.2}\" stroke-linejoin=\"round\"/>", self.width);
        let h = self.head;
        let _ = writeln!(s, "<polygon points=\"{:.1},{:.1} {:.1},{:.1} {:.1},{:.1}\" fill=\"{col}\"/>", h[0].0, h[0].1, h[1].0, h[1].1, h[2].0, h[2].1);
    }
}

/// S-shaped connector that leaves and arrives horizontally.
fn s_curve(a: (f32, f32), b: (f32, f32)) -> impl Fn(f32) -> (f32, f32) {
    move |t: f32| {
        let mx = 0.5 * (a.0 + b.0);
        let u = 1.0 - t;
        let x = u * u * u * a.0 + 3.0 * u * u * t * mx + 3.0 * u * t * t * mx + t * t * t * b.0;
        let y = u * u * u * a.1 + 3.0 * u * u * t * a.1 + 3.0 * u * t * t * b.1 + t * t * t * b.1;
        (x, y)
    }
}

// ------------------------------------------------------------------ one species

pub struct Net {
    pub species: String,
    pub zone: String,
    pub form: Vec<ReactionShare>,
    pub dest: Vec<ReactionShare>,
}

pub const TOP: usize = 7;

pub fn build(r: &Results, species: &str, zone: Zone) -> Result<Net, String> {
    let c = r
        .chemistry
        .iter()
        .find(|c| c.species == species)
        .ok_or_else(|| if r.chemistry.is_empty() { "No reaction rates saved for this run (turn on Chemistry before running)".to_string() } else { format!("No rates saved for {species}") })?;
    let (d0, d1) = zone.range(r).ok_or_else(|| format!("This model has no {}", zone.label()))?;
    let form: Vec<ReactionShare> = c.shares(true, d0, d1).into_iter().take(TOP).collect();
    let dest: Vec<ReactionShare> = c.shares(false, d0, d1).into_iter().take(TOP).collect();
    if form.is_empty() && dest.is_empty() {
        return Err("All rates are zero in this zone".into());
    }
    Ok(Net { species: species.to_string(), zone: zone.label().to_string(), form, dest })
}

const W: f32 = 1000.0;
const H: f32 = 560.0;
const BLUE: [u8; 3] = [31, 119, 180];
const ORANGE: [u8; 3] = [221, 90, 30];
const BOX_L: (f32, f32) = (20.0, 350.0);
const BOX_R: (f32, f32) = (650.0, 980.0);
const NODE_HW: f32 = 65.0;
const NODE_HH: f32 = 40.0;
const NODE_Y: f32 = H / 2.0 + 20.0;

struct Item {
    text: String,
    pct: f64,
    y: f32,
    arrow: Arrow,
}

/// The rows of one side, with their arrows spread along the side of the central box.
fn rows(list: &[ReactionShare], left: bool) -> Vec<Item> {
    let n = list.len().max(1) as f32;
    let cx = W / 2.0;
    list.iter()
        .enumerate()
        .map(|(i, s)| {
            let y = 70.0 + (H - 100.0) * (i as f32 + 0.5) / n;
            let port = NODE_Y + NODE_HH * 1.5 * ((i as f32 + 0.5) / n - 0.5);
            let width = arrow_width(s.fraction);
            let arrow = if left {
                arrow_along(s_curve((BOX_L.1 + 4.0, y), (cx - NODE_HW - 3.0, port)), width)
            } else {
                arrow_along(s_curve((cx + NODE_HW + 3.0, port), (BOX_R.0 - 4.0, y)), width)
            };
            Item { text: pretty_reaction(&s.reaction), pct: s.fraction * 100.0, y, arrow }
        })
        .collect()
}

/// Draw the network in the space given (scaled to fit).
pub fn draw(ui: &mut egui::Ui, net: &Net, width: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let s = (rect.width() / W).min(rect.height() / H);
    let off = rect.min + egui::vec2((rect.width() - W * s) / 2.0, (rect.height() - H * s) / 2.0);
    let at = |x: f32, y: f32| Pos2::new(off.x + x * s, off.y + y * s);
    let text_col = ui.visuals().text_color();
    let bg = ui.visuals().extreme_bg_color;
    let fs = (14.0 * s).clamp(9.0, 17.0);

    p.text(at(W / 2.0, 22.0), egui::Align2::CENTER_CENTER, format!("Formation and destruction of {} — {}", sp(&net.species), net.zone), FontId::proportional(fs * 1.15), text_col);
    p.text(at(185.0, 50.0), egui::Align2::CENTER_CENTER, "Formation", FontId::proportional(fs), c32(BLUE));
    p.text(at(815.0, 50.0), egui::Align2::CENTER_CENTER, "Destruction", FontId::proportional(fs), c32(ORANGE));

    for (list, left, col) in [(&net.form, true, BLUE), (&net.dest, false, ORANGE)] {
        let col = c32(col);
        for it in rows(list, left) {
            it.arrow.paint(&p, &at, s, col);
            let (x0, x1) = if left { BOX_L } else { BOX_R };
            let b = egui::Rect::from_min_max(at(x0, it.y - 21.0), at(x1, it.y + 21.0));
            p.rect_filled(b, 6.0 * s, bg);
            p.rect_stroke(b, 6.0 * s, Stroke::new(1.5, col), egui::StrokeKind::Inside);
            p.text(b.center() - egui::vec2(0.0, 8.0 * s), egui::Align2::CENTER_CENTER, &it.text, FontId::proportional(fs), text_col);
            p.text(b.center() + egui::vec2(0.0, 11.0 * s), egui::Align2::CENTER_CENTER, format!("{:.1}%", it.pct), FontId::proportional(fs * 0.85), col);
        }
    }
    let center = egui::Rect::from_center_size(at(W / 2.0, NODE_Y), egui::vec2(2.0 * NODE_HW * s, 2.0 * NODE_HH * s));
    p.rect_filled(center, 10.0 * s, c32([245, 225, 120]));
    p.rect_stroke(center, 10.0 * s, Stroke::new(2.0, Color32::from_gray(40)), egui::StrokeKind::Inside);
    p.text(center.center(), egui::Align2::CENTER_CENTER, sp(&net.species), FontId::proportional(fs * 1.7), Color32::BLACK);
}

/// The same drawing as an SVG document, for PNG / PDF / SVG export.
pub fn to_svg(net: &Net, st: &Style) -> String {
    let fs = 14.0;
    let mut s = String::new();
    let _ = write!(s, "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{W}pt\" height=\"{H}pt\" viewBox=\"0 0 {W} {H}\" font-family=\"{}\" font-size=\"{fs}\">\n<rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n", family(st));
    let _ = writeln!(s, "<text x=\"{}\" y=\"26\" text-anchor=\"middle\" font-size=\"{}\">Formation and destruction of {} — {}</text>", W / 2.0, fs * 1.15, esc(&sp(&net.species)), esc(&net.zone));
    let _ = writeln!(s, "<text x=\"185\" y=\"54\" text-anchor=\"middle\" fill=\"{}\">Formation</text>", hex(BLUE));
    let _ = writeln!(s, "<text x=\"815\" y=\"54\" text-anchor=\"middle\" fill=\"{}\">Destruction</text>", hex(ORANGE));
    for (list, left, col) in [(&net.form, true, hex(BLUE)), (&net.dest, false, hex(ORANGE))] {
        for it in rows(list, left) {
            it.arrow.svg(&mut s, &col);
            let (x0, x1) = if left { BOX_L } else { BOX_R };
            let _ = writeln!(s, "<rect x=\"{x0}\" y=\"{:.1}\" width=\"{}\" height=\"42\" rx=\"6\" fill=\"white\" stroke=\"{col}\" stroke-width=\"1.5\"/>", it.y - 21.0, x1 - x0);
            let _ = writeln!(s, "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\">{}</text>", (x0 + x1) / 2.0, it.y - 3.0, esc(&it.text));
            let _ = writeln!(s, "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-size=\"{}\" fill=\"{col}\">{:.1}%</text>", (x0 + x1) / 2.0, it.y + 15.0, fs * 0.85, it.pct);
        }
    }
    let cx = W / 2.0;
    let _ = writeln!(s, "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{}\" height=\"{}\" rx=\"10\" fill=\"#f5e178\" stroke=\"#282828\" stroke-width=\"2\"/>", cx - NODE_HW, NODE_Y - NODE_HH, 2.0 * NODE_HW, 2.0 * NODE_HH);
    let _ = writeln!(s, "<text x=\"{cx}\" y=\"{:.1}\" text-anchor=\"middle\" font-size=\"{}\">{}</text>", NODE_Y + 8.0, fs * 1.7, esc(&sp(&net.species)));
    s.push_str("</svg>\n");
    s
}

pub fn to_csv(net: &Net) -> String {
    let mut s = format!("# {} — {}\nkind,reaction,integrated_rate_cm-2_s-1,fraction\n", net.species, net.zone);
    for (k, list) in [("formation", &net.form), ("destruction", &net.dest)] {
        for r in list.iter() {
            let _ = writeln!(s, "{k},\"{}\",{:.6e},{:.6}", r.reaction, r.integrated, r.fraction);
        }
    }
    s
}

// ------------------------------------------------------------------ several species

/// A → B: the share of the destruction of A that goes through reactions producing B.
pub struct Link {
    pub from: String,
    pub to: String,
    pub share: f64,
    /// (reaction, share of the destruction of A), largest first
    pub reactions: Vec<(String, f64)>,
}

pub struct Map {
    pub species: Vec<String>,
    pub zone: String,
    pub links: Vec<Link>,
    /// species asked for without saved rates
    pub missing: Vec<String>,
}

fn side(s: &str) -> Vec<&str> {
    s.split(',').map(|x| x.trim()).collect()
}

pub fn build_map(r: &Results, species: &[String], zone: Zone) -> Result<Map, String> {
    let (d0, d1) = zone.range(r).ok_or_else(|| format!("This model has no {}", zone.label()))?;
    let mut links: Vec<Link> = vec![];
    let mut have = vec![];
    let mut missing = vec![];
    for a in species {
        let Some(c) = r.chemistry.iter().find(|c| &c.species == a) else {
            missing.push(a.clone());
            continue;
        };
        have.push(a.clone());
        for sh in c.shares(false, d0, d1) {
            let Some((lhs, rhs)) = sh.reaction.split_once("=>") else { continue };
            let (lhs, rhs) = (side(lhs), side(rhs));
            if !lhs.contains(&a.as_str()) {
                continue;
            }
            for b in species {
                if b == a || !rhs.contains(&b.as_str()) {
                    continue;
                }
                match links.iter_mut().find(|l| &l.from == a && &l.to == b) {
                    Some(l) => {
                        l.share += sh.fraction;
                        l.reactions.push((sh.reaction.clone(), sh.fraction));
                    }
                    None => links.push(Link { from: a.clone(), to: b.clone(), share: sh.fraction, reactions: vec![(sh.reaction.clone(), sh.fraction)] }),
                }
            }
        }
    }
    if have.is_empty() {
        return Err("No reaction rates saved for these species".into());
    }
    links.sort_by(|x, y| y.share.partial_cmp(&x.share).unwrap_or(std::cmp::Ordering::Equal));
    Ok(Map { species: species.to_vec(), zone: zone.label().to_string(), links, missing })
}

const MAP_W: f32 = 1000.0;
const MAP_H: f32 = 620.0;
const GRAPH_W: f32 = 640.0;
const MAP_NODE_HW: f32 = 46.0;
const MAP_NODE_HH: f32 = 24.0;
/// links lighter than this are left out of the drawing (they stay in the table)
const MIN_SHARE: f64 = 0.001;

fn node_pos(i: usize, n: usize) -> (f32, f32) {
    let (cx, cy) = (GRAPH_W / 2.0, MAP_H / 2.0 + 16.0);
    let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / n.max(1) as f32;
    (cx + (GRAPH_W / 2.0 - 75.0) * a.cos(), cy + (MAP_H / 2.0 - 70.0) * a.sin())
}

/// How far along direction (ux, uy) the edge of a node box lies.
fn box_exit(ux: f32, uy: f32) -> f32 {
    let tx = if ux.abs() > 1e-4 { MAP_NODE_HW / ux.abs() } else { f32::MAX };
    let ty = if uy.abs() > 1e-4 { MAP_NODE_HH / uy.abs() } else { f32::MAX };
    tx.min(ty)
}

/// The arrow of one link and the position of its percentage.
fn link_arrow(map: &Map, l: &Link) -> Option<(Arrow, (f32, f32))> {
    let n = map.species.len();
    let i = map.species.iter().position(|s| s == &l.from)?;
    let j = map.species.iter().position(|s| s == &l.to)?;
    let (a, b) = (node_pos(i, n), node_pos(j, n));
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let d = (dx * dx + dy * dy).sqrt().max(1.0);
    let (ux, uy) = (dx / d, dy / d);
    // bend to one side, so that A → B and B → A run apart
    let bend = 0.16 * d;
    let ctrl = (0.5 * (a.0 + b.0) - uy * bend, 0.5 * (a.1 + b.1) + ux * bend);
    let dir = |from: (f32, f32), to: (f32, f32)| {
        let (x, y) = (to.0 - from.0, to.1 - from.1);
        let l = (x * x + y * y).sqrt().max(1e-3);
        (x / l, y / l)
    };
    let (sx, sy) = dir(a, ctrl);
    let (ex, ey) = dir(b, ctrl);
    let p0 = (a.0 + sx * (box_exit(sx, sy) + 4.0), a.1 + sy * (box_exit(sx, sy) + 4.0));
    let p1 = (b.0 + ex * (box_exit(ex, ey) + 5.0), b.1 + ey * (box_exit(ex, ey) + 5.0));
    let curve = move |t: f32| {
        let u = 1.0 - t;
        (u * u * p0.0 + 2.0 * u * t * ctrl.0 + t * t * p1.0, u * u * p0.1 + 2.0 * u * t * ctrl.1 + t * t * p1.1)
    };
    Some((arrow_along(curve, arrow_width(l.share)), curve(0.42)))
}

fn node_color(i: usize) -> [u8; 3] {
    pal([0, 1, 2, 3, 4, 5, 7, 8, 9][i % 9])
}

/// Rows of the table of connections beside the map.
fn link_rows(map: &Map) -> Vec<(String, String, f64)> {
    map.links
        .iter()
        .take(16)
        .map(|l| {
            let top = l.reactions.iter().max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)).map(|r| pretty_reaction(&r.0)).unwrap_or_default();
            (format!("{} → {}", sp(&l.from), sp(&l.to)), top, l.share * 100.0)
        })
        .collect()
}

pub fn draw_map(ui: &mut egui::Ui, map: &Map, width: f32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let s = (rect.width() / MAP_W).min(rect.height() / MAP_H);
    let off = rect.min + egui::vec2((rect.width() - MAP_W * s) / 2.0, (rect.height() - MAP_H * s) / 2.0);
    let at = |x: f32, y: f32| Pos2::new(off.x + x * s, off.y + y * s);
    let text_col = ui.visuals().text_color();
    let weak = ui.visuals().weak_text_color();
    let bg = ui.visuals().extreme_bg_color;
    let fs = (14.0 * s).clamp(9.0, 16.0);
    p.text(at(GRAPH_W / 2.0, 16.0), egui::Align2::CENTER_CENTER, format!("How the species feed each other — {}", map.zone), FontId::proportional(fs * 1.1), text_col);

    let n = map.species.len();
    let mut tags = vec![];
    for l in map.links.iter().filter(|l| l.share >= MIN_SHARE) {
        let Some((arrow, mid)) = link_arrow(map, l) else { continue };
        let i = map.species.iter().position(|x| x == &l.from).unwrap_or(0);
        arrow.paint(&p, &at, s, c32(node_color(i)).gamma_multiply(0.9));
        tags.push((mid, format!("{:.0}%", (l.share * 100.0).max(1.0)), c32(node_color(i))));
    }
    for (mid, t, col) in tags {
        let g = p.layout_no_wrap(t, FontId::proportional(fs * 0.85), col);
        let r = egui::Rect::from_center_size(at(mid.0, mid.1), g.size() + egui::vec2(8.0, 3.0));
        p.rect_filled(r, 4.0, bg);
        p.galley(r.center() - g.size() / 2.0, g, col);
    }
    for (i, name) in map.species.iter().enumerate() {
        let (x, y) = node_pos(i, n);
        let b = egui::Rect::from_center_size(at(x, y), egui::vec2(2.0 * MAP_NODE_HW * s, 2.0 * MAP_NODE_HH * s));
        let has = !map.missing.contains(name);
        p.rect_filled(b, 9.0 * s, if has { c32([245, 225, 120]) } else { Color32::from_gray(150) });
        p.rect_stroke(b, 9.0 * s, Stroke::new(2.5, c32(node_color(i))), egui::StrokeKind::Inside);
        p.text(b.center(), egui::Align2::CENTER_CENTER, sp(name), FontId::proportional(fs * 1.25), Color32::BLACK);
    }

    // table of connections
    let tx = GRAPH_W + 14.0;
    p.text(at(tx, 16.0), egui::Align2::LEFT_CENTER, "Connections (share of the destruction of the first species)", FontId::proportional(fs * 0.8), weak);
    let rows = link_rows(map);
    if rows.is_empty() {
        p.text(at(tx, 60.0), egui::Align2::LEFT_CENTER, "No direct reaction links these species here.", FontId::proportional(fs * 0.95), text_col);
    }
    let row_h = ((MAP_H - 50.0) / rows.len().max(1) as f32).min(44.0);
    for (k, (pair, top, pct)) in rows.iter().enumerate() {
        let y = 44.0 + row_h * k as f32;
        p.text(at(tx, y), egui::Align2::LEFT_CENTER, pair, FontId::proportional(fs), text_col);
        p.text(at(MAP_W - 6.0, y), egui::Align2::RIGHT_CENTER, fmt_pct(*pct), FontId::proportional(fs), text_col);
        p.text(at(tx, y + 17.0), egui::Align2::LEFT_CENTER, top, FontId::proportional(fs * 0.78), weak);
    }
}

fn fmt_pct(p: f64) -> String {
    if p >= 1.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.2}%")
    }
}

pub fn map_to_svg(map: &Map, st: &Style) -> String {
    let fs = 14.0;
    let mut s = String::new();
    let _ = write!(s, "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{MAP_W}pt\" height=\"{MAP_H}pt\" viewBox=\"0 0 {MAP_W} {MAP_H}\" font-family=\"{}\" font-size=\"{fs}\">\n<rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n", family(st));
    let _ = writeln!(s, "<text x=\"{}\" y=\"22\" text-anchor=\"middle\" font-size=\"{}\">How the species feed each other — {}</text>", GRAPH_W / 2.0, fs * 1.1, esc(&map.zone));
    let n = map.species.len();
    let mut tags = String::new();
    for l in map.links.iter().filter(|l| l.share >= MIN_SHARE) {
        let Some((arrow, mid)) = link_arrow(map, l) else { continue };
        let i = map.species.iter().position(|x| x == &l.from).unwrap_or(0);
        let col = hex(node_color(i));
        arrow.svg(&mut s, &col);
        let t = format!("{:.0}%", (l.share * 100.0).max(1.0));
        let w = t.len() as f32 * fs * 0.55 + 6.0;
        let _ = writeln!(tags, "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{w:.1}\" height=\"16\" rx=\"3\" fill=\"white\"/>", mid.0 - w / 2.0, mid.1 - 8.0);
        let _ = writeln!(tags, "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-size=\"{}\" fill=\"{col}\">{t}</text>", mid.0, mid.1 + 4.0, fs * 0.85);
    }
    s.push_str(&tags);
    for (i, name) in map.species.iter().enumerate() {
        let (x, y) = node_pos(i, n);
        let _ = writeln!(s, "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{}\" height=\"{}\" rx=\"9\" fill=\"#f5e178\" stroke=\"{}\" stroke-width=\"2.5\"/>", x - MAP_NODE_HW, y - MAP_NODE_HH, 2.0 * MAP_NODE_HW, 2.0 * MAP_NODE_HH, hex(node_color(i)));
        let _ = writeln!(s, "<text x=\"{x:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-size=\"{}\">{}</text>", y + 6.0, fs * 1.25, esc(&sp(name)));
    }
    let tx = GRAPH_W + 14.0;
    let _ = writeln!(s, "<text x=\"{tx}\" y=\"20\" font-size=\"{}\" fill=\"#555555\">Connections (share of the destruction of the first species)</text>", fs * 0.72);
    let rows = link_rows(map);
    let row_h = ((MAP_H - 50.0) / rows.len().max(1) as f32).min(44.0);
    for (k, (pair, top, pct)) in rows.iter().enumerate() {
        let y = 48.0 + row_h * k as f32;
        let _ = writeln!(s, "<text x=\"{tx}\" y=\"{y:.1}\">{}</text>", esc(pair));
        let _ = writeln!(s, "<text x=\"{:.1}\" y=\"{y:.1}\" text-anchor=\"end\">{}</text>", MAP_W - 6.0, fmt_pct(*pct));
        let _ = writeln!(s, "<text x=\"{tx}\" y=\"{:.1}\" font-size=\"{}\" fill=\"#555555\">{}</text>", y + 16.0, fs * 0.75, esc(top));
    }
    s.push_str("</svg>\n");
    s
}

pub fn map_to_csv(map: &Map) -> String {
    let mut s = format!("# links between {} — {}\nfrom,to,share_of_destruction_of_from,reaction,reaction_share\n", map.species.join(" "), map.zone);
    for l in &map.links {
        for (r, f) in &l.reactions {
            let _ = writeln!(s, "\"{}\",\"{}\",{:.6},\"{}\",{:.6}", l.from, l.to, l.share, r, f);
        }
    }
    s
}
