//! Publication-quality rendering of a [`Figure`] to SVG, and conversion to PNG / PDF / CSV.

use std::fmt::Write as _;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use crate::figures::{Figure, Rgb};
use crate::place::Placer;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Png,
    Pdf,
    Svg,
    Csv,
}

impl Format {
    pub const ALL: [Format; 4] = [Format::Pdf, Format::Png, Format::Svg, Format::Csv];
    pub fn ext(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Pdf => "pdf",
            Format::Svg => "svg",
            Format::Csv => "csv",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Format::Png => "PNG (raster)",
            Format::Pdf => "PDF (vector)",
            Format::Svg => "SVG (vector)",
            Format::Csv => "CSV (data)",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Style {
    pub width_in: f64,
    pub height_in: f64,
    pub dpi: f64,
    pub serif: bool,
    pub font_pt: f64,
    pub title: bool,
    /// light grid lines at the major ticks
    pub grid: bool,
}

impl Default for Style {
    fn default() -> Self {
        Style { width_in: 6.0, height_in: 4.2, dpi: 300.0, serif: true, font_pt: 11.0, title: false, grid: false }
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Markup (`_{}` / `^{}`) to SVG tspans.
fn markup_svg(s: &str, fs: f64) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    let mut plain = String::new();
    let mut pending = 0.0f64;
    let flush = |out: &mut String, plain: &mut String, pending: &mut f64| {
        if !plain.is_empty() {
            if *pending != 0.0 {
                let _ = write!(out, "<tspan dy=\"{:.2}\">{}</tspan>", *pending, esc(plain));
                *pending = 0.0;
            } else {
                let _ = write!(out, "<tspan>{}</tspan>", esc(plain));
            }
            plain.clear();
        }
    };
    while let Some(c) = chars.next() {
        if (c == '_' || c == '^') && chars.peek() == Some(&'{') {
            chars.next();
            let mut inner = String::new();
            for d in chars.by_ref() {
                if d == '}' {
                    break;
                }
                inner.push(d);
            }
            flush(&mut out, &mut plain, &mut pending);
            let shift = if c == '^' { -0.38 * fs } else { 0.22 * fs };
            let _ = write!(out, "<tspan dy=\"{:.2}\" font-size=\"{:.2}\">{}</tspan>", shift + pending, fs * 0.7, esc(&inner));
            pending = -shift;
        } else {
            plain.push(c);
        }
    }
    flush(&mut out, &mut plain, &mut pending);
    out
}

/// Approximate rendered width of markup text.
fn text_w(s: &str, fs: f64) -> f64 {
    let mut w = 0.0;
    let mut small = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if (c == '_' || c == '^') && chars.peek() == Some(&'{') {
            chars.next();
            small = true;
            continue;
        }
        if c == '}' && small {
            small = false;
            continue;
        }
        let f = if small { 0.7 } else { 1.0 };
        let cw = if c.is_ascii_uppercase() { 0.66 } else if c == ' ' { 0.3 } else { 0.52 };
        w += cw * f * fs;
    }
    w
}

fn rgb(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

fn tr(v: f64, log: bool) -> Option<f64> {
    if !v.is_finite() {
        return None;
    }
    if log {
        if v > 0.0 {
            Some(v.log10())
        } else {
            None
        }
    } else {
        Some(v)
    }
}

fn nice_step(span: f64, target: f64) -> f64 {
    let raw = (span / target).max(1e-300);
    let p = 10f64.powf(raw.log10().floor());
    let m = raw / p;
    let k = if m < 1.5 { 1.0 } else if m < 3.5 { 2.0 } else if m < 7.5 { 5.0 } else { 10.0 };
    k * p
}

pub struct Tick {
    pub pos: f64,
    pub label: Option<String>,
}

fn fmt_num(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    let a = v.abs();
    if (1e-3..1e5).contains(&a) {
        let s = format!("{:.4}", v);
        let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        if s == "-0" { "0".into() } else { s }
    } else {
        let e = a.log10().floor();
        let m = v / 10f64.powf(e);
        if (m.abs() - 1.0).abs() < 1e-6 {
            format!("10^{{{}}}", e as i64)
        } else {
            format!("{}×10^{{{}}}", fmt_num(m), e as i64)
        }
    }
}

/// Ticks in transformed coordinates. Labels use markup.
pub fn ticks(lo: f64, hi: f64, log: bool, target: f64) -> Vec<Tick> {
    let mut out = vec![];
    if !(hi > lo) {
        return out;
    }
    if log && hi - lo >= 1.0 {
        let span = hi - lo;
        let step = if span > 24.0 { 5.0 } else if span > 12.0 { 2.0 } else { 1.0 };
        let mut e = (lo / step).floor() * step;
        while e <= hi + 1e-9 {
            if e >= lo - 1e-9 {
                out.push(Tick { pos: e, label: Some(format!("10^{{{}}}", e.round() as i64)) });
            }
            if step == 1.0 && span < 9.0 {
                for k in 2..10 {
                    let p = e + (k as f64).log10();
                    if p > lo && p < hi {
                        out.push(Tick { pos: p, label: None });
                    }
                }
            }
            e += step;
        }
        return out;
    }
    if log {
        let (a, b) = (10f64.powf(lo), 10f64.powf(hi));
        let st = nice_step(b - a, target);
        let mut v = (a / st).ceil() * st;
        while v <= b * (1.0 + 1e-9) {
            if v > 0.0 {
                out.push(Tick { pos: v.log10(), label: Some(fmt_num(v)) });
            }
            v += st;
        }
        return out;
    }
    let st = nice_step(hi - lo, target);
    let mut v = (lo / st).ceil() * st;
    let mut guard = 0;
    while v <= hi + st * 1e-6 && guard < 200 {
        let vv = if v.abs() < st * 1e-9 { 0.0 } else { v };
        out.push(Tick { pos: vv, label: Some(fmt_num(vv)) });
        v += st;
        guard += 1;
    }
    out
}

/// Data range in transformed coordinates.
pub fn ranges(f: &Figure) -> Option<((f64, f64), (f64, f64))> {
    let mut xr = (f64::MAX, f64::MIN);
    let mut yr = (f64::MAX, f64::MIN);
    let yfloor = f.y_floor.and_then(|v| tr(v, f.ylog));
    let yceil = f.y_ceil.and_then(|v| tr(v, f.ylog));
    for s in &f.series {
        for (x, y) in s.x.iter().zip(&s.y) {
            if let (Some(tx), Some(ty)) = (tr(*x, f.xlog), tr(*y, f.ylog)) {
                if let Some(fl) = yfloor {
                    if ty < fl {
                        xr.0 = xr.0.min(tx);
                        xr.1 = xr.1.max(tx);
                        continue;
                    }
                }
                xr.0 = xr.0.min(tx);
                xr.1 = xr.1.max(tx);
                yr.0 = yr.0.min(ty);
                yr.1 = yr.1.max(ty);
            }
        }
    }
    if xr.0 > xr.1 || yr.0 > yr.1 {
        return None;
    }
    if let Some(fl) = yfloor {
        yr.0 = yr.0.max(fl);
    }
    if let Some(c) = yceil {
        yr.1 = yr.1.min(c);
    }
    if yr.1 - yr.0 < 1e-9 {
        let d = if yr.0.abs() > 0.0 { yr.0.abs() * 0.1 } else { 1.0 };
        yr = (yr.0 - d, yr.1 + d);
    }
    if xr.1 - xr.0 < 1e-12 {
        xr = (xr.0 - 1.0, xr.1 + 1.0);
    }
    let py = (yr.1 - yr.0) * 0.05;
    let top = if f.regions.is_empty() { py } else { (yr.1 - yr.0) * 0.12 };
    let mut out = ((xr.0, xr.1), (yr.0 - py, yr.1 + top));
    if let Some((a, b)) = f.x_range {
        if let (Some(a), Some(b)) = (tr(a, f.xlog), tr(b, f.xlog)) {
            out.0 = (a, b);
        }
    }
    if let Some((a, b)) = f.y_range {
        if let (Some(a), Some(b)) = (tr(a, f.ylog), tr(b, f.ylog)) {
            out.1 = (a, b);
        }
    }
    Some(out)
}

/// A figure cut to the view on screen. `b` = [xmin, ymin, xmax, ymax] as drawn (log10 on log axes).
pub fn zoomed(f: &Figure, b: [f64; 4]) -> Figure {
    let back = |v: f64, log: bool| if log { 10f64.powf(v) } else { v };
    let mut z = f.clone();
    z.x_range = Some((back(b[0], f.xlog), back(b[2], f.xlog)));
    if let Some(bars) = &mut z.bars {
        // rows sit at y = -index
        let keep: Vec<usize> = (0..bars.values.len()).filter(|i| { let y = -(*i as f64); y >= b[1] && y <= b[3] }).collect();
        if !keep.is_empty() {
            bars.labels = keep.iter().map(|i| bars.labels[*i].clone()).collect();
            bars.values = keep.iter().map(|i| bars.values[*i]).collect();
            bars.colors = keep.iter().map(|i| bars.colors[*i]).collect();
        }
        return z;
    }
    z.y_range = Some((back(b[1], f.ylog), back(b[3], f.ylog)));
    for se in &mut z.series {
        // keep the points in view and their neighbours, so that curves reach the edges
        let n = se.x.len().min(se.y.len());
        let inside: Vec<bool> = (0..n).map(|i| tr(se.x[i], f.xlog).map(|x| x >= b[0] && x <= b[2]).unwrap_or(false)).collect();
        let keep: Vec<usize> = (0..n).filter(|i| inside[*i] || (*i > 0 && inside[*i - 1]) || (*i + 1 < n && inside[*i + 1])).collect();
        se.x = keep.iter().map(|i| se.x[*i]).collect();
        se.y = keep.iter().map(|i| se.y[*i]).collect();
    }
    z
}

/// Where the axes are on the page.
#[derive(Clone, Copy)]
struct Geo {
    ml: f64,
    mt: f64,
    pw: f64,
    ph: f64,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
}

impl Geo {
    fn px(&self, x: f64) -> f64 {
        self.ml + (x - self.x0) / (self.x1 - self.x0) * self.pw
    }
    fn py(&self, y: f64) -> f64 {
        self.mt + self.ph - (y - self.y0) / (self.y1 - self.y0) * self.ph
    }
}

/// A placer that knows every curve, marker and front line of the figure.
fn placer_for(f: &Figure, g: &Geo) -> Placer {
    let mut pl = Placer::new([g.ml, g.mt, g.ml + g.pw, g.mt + g.ph]);
    for se in &f.series {
        let pts: Vec<(f64, f64)> = se
            .x
            .iter()
            .zip(&se.y)
            .map(|(x, y)| match (tr(*x, f.xlog), tr(*y, f.ylog)) {
                (Some(a), Some(b)) => (g.px(a), g.py(b)),
                _ => (f64::NAN, f64::NAN),
            })
            .collect();
        if se.markers {
            let r = 2.0 + se.width as f64;
            for (x, y) in pts.iter().filter(|p| p.0.is_finite()) {
                pl.reserve([x - r, y - r, x + r, y + r]);
            }
        }
        if se.line {
            pl.add_line(pts);
        }
    }
    for m in &f.marks {
        if let Some(a) = tr(m.x, f.xlog).filter(|a| *a >= g.x0 && *a <= g.x1) {
            let x = g.px(a);
            pl.add_line([(x, g.mt), (x, g.mt + g.ph)]);
        }
    }
    pl
}

/// Render a figure as an SVG document (sizes in points).
pub fn to_svg(f: &Figure, st: &Style) -> String {
    if let Some(b) = &f.bars {
        return bars_svg(f, b, st);
    }
    let w = st.width_in * 72.0;
    let mut h = st.height_in * 72.0;
    let fs = st.font_pt;
    let family = family(st);
    let top = if st.title { fs * 2.4 } else { fs * 1.0 };
    let (ml, mr, mut mt, mb) = (fs * 5.4, fs * 1.2, top, fs * 3.6);
    let (pw, ph) = (w - ml - mr, h - mt - mb);
    let Some(((x0, x1), (y0, y1))) = ranges(f) else {
        return format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.1}pt\" height=\"{h:.1}pt\" viewBox=\"0 0 {w:.1} {h:.1}\" font-family=\"{family}\" font-size=\"{fs:.1}\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/><text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\">No data</text></svg>", w / 2.0, h / 2.0);
    };

    // the legend goes in a free corner of the plot; when none is free, in a band above the axes
    let items: Vec<_> = f.series.iter().filter(|s| s.legend && !s.name.is_empty()).collect();
    let lfs = fs * 0.85;
    let row_h = lfs * 1.35;
    let lw = (items.iter().map(|s| text_w(&s.name, lfs)).fold(0.0, f64::max) + 34.0) + 8.0;
    let lh = items.len() as f64 * row_h + 6.0;
    let mut corner: Option<(f64, f64)> = None;
    let mut band: Vec<(f64, f64, usize)> = vec![];
    let band_top = mt;
    if !items.is_empty() {
        let g = Geo { ml, mt, pw, ph, x0, x1, y0, y1 };
        let pl = placer_for(f, &g);
        let corners = [(ml + pw - lw - 6.0, mt + 6.0), (ml + 6.0, mt + 6.0), (ml + pw - lw - 6.0, mt + ph - lh - 6.0), (ml + 6.0, mt + ph - lh - 6.0)];
        corner = corners.into_iter().find(|c| pl.is_free([c.0, c.1, c.0 + lw, c.1 + lh]));
        if corner.is_none() {
            let (mut x, mut y) = (ml, band_top + row_h * 0.6);
            for (i, se) in items.iter().enumerate() {
                let iw = text_w(&se.name, lfs) + 40.0;
                if x + iw > ml + pw && x > ml {
                    x = ml;
                    y += row_h;
                }
                band.push((x, y, i));
                x += iw;
            }
            let extra = y - band_top + row_h * 0.8;
            mt += extra;
            h += extra;
        }
    }
    let g = Geo { ml, mt, pw, ph, x0, x1, y0, y1 };
    let px = |x: f64| g.px(x);
    let py = |y: f64| g.py(y);

    let mut s = String::new();
    let _ = write!(
        s,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.1}pt\" height=\"{h:.1}pt\" viewBox=\"0 0 {w:.1} {h:.1}\" font-family=\"{family}\" font-size=\"{fs:.1}\">\n<rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n"
    );
    if st.title {
        let _ = write!(s, "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-size=\"{:.1}\">{}</text>\n", ml + pw / 2.0, fs * 1.5, fs * 1.1, markup_svg(&f.title, fs * 1.1));
    }
    let _ = write!(s, "<defs><clipPath id=\"pa\"><rect x=\"{ml:.2}\" y=\"{mt:.2}\" width=\"{pw:.2}\" height=\"{ph:.2}\"/></clipPath></defs>\n");

    // shaded regions
    s.push_str("<g clip-path=\"url(#pa)\">\n");
    for r in &f.regions {
        let (Some(a), Some(b)) = (tr(r.x0, f.xlog), tr(r.x1, f.xlog)) else { continue };
        let (xa, xb) = (px(a).max(ml), px(b).min(ml + pw));
        if xb > xa {
            let _ = write!(s, "<rect x=\"{xa:.2}\" y=\"{mt:.2}\" width=\"{:.2}\" height=\"{ph:.2}\" fill=\"{}\" fill-opacity=\"{:.3}\"/>\n", xb - xa, rgb(r.color), crate::theme::get().band);
        }
    }
    // light grid at the major ticks
    if st.grid {
        for t in ticks(x0, x1, f.xlog, 6.0).iter().filter(|t| t.label.is_some()) {
            let x = px(t.pos);
            let _ = write!(s, "<line x1=\"{x:.2}\" y1=\"{mt:.2}\" x2=\"{x:.2}\" y2=\"{:.2}\" stroke=\"#d8d8d8\" stroke-width=\"0.5\"/>\n", mt + ph);
        }
        for t in ticks(y0, y1, f.ylog, 5.0).iter().filter(|t| t.label.is_some()) {
            let y = py(t.pos);
            let _ = write!(s, "<line x1=\"{ml:.2}\" y1=\"{y:.2}\" x2=\"{:.2}\" y2=\"{y:.2}\" stroke=\"#d8d8d8\" stroke-width=\"0.5\"/>\n", ml + pw);
        }
    }
    // front markers
    let mut marks: Vec<(f64, &crate::figures::Mark)> = f.marks.iter().filter_map(|m| tr(m.x, f.xlog).filter(|a| *a >= x0 && *a <= x1).map(|a| (px(a), m))).collect();
    marks.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    for (xa, m) in &marks {
        let _ = write!(s, "<line x1=\"{xa:.2}\" y1=\"{mt:.2}\" x2=\"{xa:.2}\" y2=\"{:.2}\" stroke=\"{}\" stroke-width=\"0.9\" stroke-dasharray=\"4 3\"/>\n", mt + ph, rgb(m.color));
    }
    // series
    for se in &f.series {
        let mut d = String::new();
        let mut pen = false;
        let floor = f.y_floor.and_then(|v| tr(v, f.ylog));
        for (x, y) in se.x.iter().zip(&se.y) {
            match (tr(*x, f.xlog), tr(*y, f.ylog)) {
                (Some(a), Some(mut b)) => {
                    if let Some(fl) = floor {
                        b = b.max(fl - (y1 - y0));
                    }
                    let _ = write!(d, "{}{:.2} {:.2} ", if pen { "L" } else { "M" }, px(a), py(b));
                    pen = true;
                }
                _ => pen = false,
            }
        }
        if d.is_empty() {
            continue;
        }
        if se.line {
            let dash = if se.dashed { " stroke-dasharray=\"5 3\"" } else { "" };
            let _ = write!(s, "<path d=\"{d}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{:.2}\" stroke-linejoin=\"round\"{dash}/>\n", rgb(se.color), se.width as f64 * 0.75);
        }
        if se.markers {
            for (x, y) in se.x.iter().zip(&se.y) {
                if let (Some(a), Some(b)) = (tr(*x, f.xlog), tr(*y, f.ylog)) {
                    let _ = write!(s, "<circle cx=\"{:.2}\" cy=\"{:.2}\" r=\"{:.2}\" fill=\"{}\" stroke=\"black\" stroke-width=\"0.5\"/>\n", px(a), py(b), 1.2 + se.width as f64, rgb(se.color));
                }
            }
        }
    }
    s.push_str("</g>\n");

    // labels: only where they touch neither a curve nor each other
    let mut pl = placer_for(f, &g);
    if let Some((lx, ly)) = corner {
        pl.reserve([lx, ly, lx + lw, ly + lh]);
    }
    let mfs = fs * 0.8;
    for (xa, m) in &marks {
        let tw = text_w(&m.label, mfs);
        let spots = pl.beside_marker(*xa, tw, mfs * 1.2);
        if let Some((cx, cy)) = pl.place(tw, mfs * 1.2, &spots) {
            let _ = write!(s, "<text x=\"{cx:.2}\" y=\"{:.2}\" text-anchor=\"middle\" font-size=\"{mfs:.1}\" fill=\"{}\">{}</text>\n", cy + mfs * 0.35, rgb(m.color), esc(&m.label));
        }
    }
    let rfs = fs * 0.85;
    for r in &f.regions {
        let (Some(a), Some(b)) = (tr(r.x0, f.xlog), tr(r.x1, f.xlog)) else { continue };
        let tw = text_w(&r.label, rfs);
        let spots = pl.inside_band(px(a), px(b), tw, rfs * 1.2);
        if let Some((cx, cy)) = pl.place(tw, rfs * 1.2, &spots) {
            let _ = write!(s, "<text x=\"{cx:.2}\" y=\"{:.2}\" text-anchor=\"middle\" font-size=\"{rfs:.1}\" fill=\"#555555\">{}</text>\n", cy + rfs * 0.35, markup_svg(&r.label, rfs));
        }
    }
    let nfs = fs * 0.9;
    for n in &f.labels {
        if let (Some(a), Some(b)) = (tr(n.x, f.xlog), tr(n.y, f.ylog)) {
            let tw = text_w(&n.text, nfs);
            let spots = pl.around(px(a), py(b), nfs * 1.2);
            if let Some((cx, cy)) = pl.place(tw, nfs * 1.2, &spots) {
                let _ = write!(s, "<text x=\"{cx:.2}\" y=\"{:.2}\" text-anchor=\"middle\" font-size=\"{nfs:.1}\" font-style=\"italic\" fill=\"#555555\">{}</text>\n", cy + nfs * 0.35, esc(&n.text));
            }
        }
    }
    // the names of the spikes, only right beside them
    for se in f.series.iter().filter(|se| !se.tip.is_empty() && crate::theme::get().line_names) {
        if let (Some(x), Some(y)) = (se.x.last(), se.y.last()) {
            if let (Some(a), Some(b)) = (tr(*x, f.xlog), tr(*y, f.ylog)) {
                let (tw, th) = (text_w(&se.tip, nfs), nfs * 1.2);
                let spots = pl.beside_tip(px(a), py(b), tw, th);
                if let Some((cx, cy)) = pl.place(tw, th, &spots) {
                    let c = se.color;
                    let _ = write!(s, "<text x=\"{cx:.2}\" y=\"{:.2}\" text-anchor=\"middle\" font-size=\"{nfs:.1}\" fill=\"#{:02x}{:02x}{:02x}\">{}</text>\n", cy + nfs * 0.35, c[0], c[1], c[2], esc(&se.tip));
                }
            }
        }
    }

    // axes box
    let _ = write!(s, "<rect x=\"{ml:.2}\" y=\"{mt:.2}\" width=\"{pw:.2}\" height=\"{ph:.2}\" fill=\"none\" stroke=\"black\" stroke-width=\"0.8\"/>\n");
    // x ticks
    for t in ticks(x0, x1, f.xlog, 6.0) {
        let x = px(t.pos);
        let len = if t.label.is_some() { 5.0 } else { 2.8 };
        let _ = write!(s, "<line x1=\"{x:.2}\" y1=\"{:.2}\" x2=\"{x:.2}\" y2=\"{:.2}\" stroke=\"black\" stroke-width=\"0.6\"/>", mt + ph, mt + ph - len);
        let _ = write!(s, "<line x1=\"{x:.2}\" y1=\"{mt:.2}\" x2=\"{x:.2}\" y2=\"{:.2}\" stroke=\"black\" stroke-width=\"0.6\"/>\n", mt + len);
        if let Some(l) = t.label {
            let _ = write!(s, "<text x=\"{x:.2}\" y=\"{:.2}\" text-anchor=\"middle\">{}</text>\n", mt + ph + fs * 1.35, markup_svg(&l, fs));
        }
    }
    for t in ticks(y0, y1, f.ylog, 5.0) {
        let y = py(t.pos);
        let len = if t.label.is_some() { 5.0 } else { 2.8 };
        let _ = write!(s, "<line x1=\"{ml:.2}\" y1=\"{y:.2}\" x2=\"{:.2}\" y2=\"{y:.2}\" stroke=\"black\" stroke-width=\"0.6\"/>", ml + len);
        let _ = write!(s, "<line x1=\"{:.2}\" y1=\"{y:.2}\" x2=\"{:.2}\" y2=\"{y:.2}\" stroke=\"black\" stroke-width=\"0.6\"/>\n", ml + pw, ml + pw - len);
        if let Some(l) = t.label {
            let _ = write!(s, "<text x=\"{:.2}\" y=\"{:.2}\" text-anchor=\"end\">{}</text>\n", ml - 4.0, y + fs * 0.35, markup_svg(&l, fs));
        }
    }
    // axis labels
    let _ = write!(s, "<text x=\"{:.2}\" y=\"{:.2}\" text-anchor=\"middle\">{}</text>\n", ml + pw / 2.0, h - fs * 0.7, markup_svg(&f.xlabel, fs));
    let _ = write!(s, "<text transform=\"translate({:.2} {:.2}) rotate(-90)\" text-anchor=\"middle\">{}</text>\n", fs * 1.3, mt + ph / 2.0, markup_svg(&f.ylabel, fs));

    // legend
    let swatch = |s: &mut String, se: &crate::figures::Series, x: f64, y: f64| {
        if se.markers && !se.line {
            let _ = write!(s, "<circle cx=\"{:.2}\" cy=\"{y:.2}\" r=\"{:.2}\" fill=\"{}\" stroke=\"black\" stroke-width=\"0.5\"/>", x + 10.0, 1.2 + se.width as f64, rgb(se.color));
        } else {
            let dash = if se.dashed { " stroke-dasharray=\"5 3\"" } else { "" };
            let _ = write!(s, "<line x1=\"{x:.2}\" y1=\"{y:.2}\" x2=\"{:.2}\" y2=\"{y:.2}\" stroke=\"{}\" stroke-width=\"{:.2}\"{dash}/>", x + 20.0, rgb(se.color), se.width as f64 * 0.75);
        }
        let _ = write!(s, "<text x=\"{:.2}\" y=\"{:.2}\" font-size=\"{lfs:.1}\">{}</text>\n", x + 24.0, y + lfs * 0.33, markup_svg(&se.name, lfs));
    };
    if let Some((lx, ly)) = corner {
        let _ = write!(s, "<rect x=\"{lx:.2}\" y=\"{ly:.2}\" width=\"{lw:.2}\" height=\"{lh:.2}\" fill=\"white\" fill-opacity=\"0.85\" stroke=\"#bbbbbb\" stroke-width=\"0.5\"/>\n");
        for (i, se) in items.iter().enumerate() {
            swatch(&mut s, se, lx + 5.0, ly + 3.0 + row_h * (i as f64 + 0.5) + 1.0);
        }
    }
    for (x, y, i) in &band {
        swatch(&mut s, items[*i], *x, *y);
    }
    s.push_str("</svg>\n");
    s
}

fn family(st: &Style) -> &'static str {
    if st.serif {
        "DejaVu Serif, Liberation Serif, Times New Roman, serif"
    } else {
        "DejaVu Sans, Liberation Sans, Arial, sans-serif"
    }
}

fn bars_svg(f: &Figure, b: &crate::figures::Bars, st: &Style) -> String {
    let fs = st.font_pt;
    let n = b.values.len().max(1);
    let w = st.width_in * 72.0;
    let row = fs * 1.55;
    let lab_w = b.labels.iter().map(|l| text_w(l, fs)).fold(0.0, f64::max) + 12.0;
    let top = if st.title { fs * 2.4 } else { fs * 1.0 };
    let (ml, mr, mt, mb) = (lab_w.max(60.0), fs * 1.5, top, fs * 3.6);
    let ph = row * n as f64;
    let h = (mt + ph + mb).max(st.height_in * 72.0 * 0.6);
    let pw = w - ml - mr;
    let family = family(st);
    let mut s = String::new();
    let _ = write!(
        s,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.1}pt\" height=\"{h:.1}pt\" viewBox=\"0 0 {w:.1} {h:.1}\" font-family=\"{family}\" font-size=\"{fs:.1}\">\n<rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n"
    );
    if st.title {
        let _ = write!(s, "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-size=\"{:.1}\">{}</text>\n", ml + pw / 2.0, fs * 1.5, fs * 1.1, markup_svg(&f.title, fs * 1.1));
    }
    let vals: Vec<f64> = b.values.iter().filter(|v| **v > 0.0).map(|v| v.log10()).collect();
    if vals.is_empty() {
        s.push_str("</svg>");
        return s;
    }
    let mut lo = (vals.iter().cloned().fold(f64::MAX, f64::min) - 0.3).floor();
    let mut hi = (vals.iter().cloned().fold(f64::MIN, f64::max) + 0.3).ceil();
    if let Some((a, b)) = f.x_range {
        if a > 0.0 && b > a {
            lo = a.log10();
            hi = b.log10();
        }
    }
    let px = |v: f64| ml + (v - lo) / (hi - lo) * pw;
    for t in ticks(lo, hi, true, 6.0) {
        let x = px(t.pos);
        let col = if t.label.is_some() { "#d0d0d0" } else { "#ececec" };
        let _ = write!(s, "<line x1=\"{x:.2}\" y1=\"{mt:.2}\" x2=\"{x:.2}\" y2=\"{:.2}\" stroke=\"{col}\" stroke-width=\"0.5\"/>\n", mt + ph);
        if let Some(l) = t.label {
            let _ = write!(s, "<text x=\"{x:.2}\" y=\"{:.2}\" text-anchor=\"middle\">{}</text>\n", mt + ph + fs * 1.35, markup_svg(&l, fs));
        }
    }
    for (i, (v, l)) in b.values.iter().zip(&b.labels).enumerate() {
        let yc = mt + row * (i as f64 + 0.5);
        if *v > 0.0 {
            let xe = px(v.log10()).clamp(ml, ml + pw);
            let _ = write!(s, "<rect x=\"{ml:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"{}\" fill-opacity=\"0.85\"/>\n", yc - row * 0.36, (xe - ml).max(0.5), row * 0.72, rgb(b.colors.get(i).copied().unwrap_or([0, 114, 178])));
            let txt = if *v >= 0.01 && *v < 1000.0 { format!("{:.3}", v) } else { format!("{:.2e}", v) };
            let tw = text_w(&txt, fs * 0.75);
            if xe + 3.0 + tw <= ml + pw {
                let _ = write!(s, "<text x=\"{:.2}\" y=\"{:.2}\" font-size=\"{:.1}\" fill=\"#333333\">{}</text>\n", xe + 3.0, yc + fs * 0.3, fs * 0.75, txt);
            } else if xe - ml > tw + 6.0 {
                // no room after the bar: write the value inside it
                let _ = write!(s, "<text x=\"{:.2}\" y=\"{:.2}\" text-anchor=\"end\" font-size=\"{:.1}\" fill=\"white\">{}</text>\n", xe - 3.0, yc + fs * 0.3, fs * 0.75, txt);
            }
        }
        let _ = write!(s, "<text x=\"{:.2}\" y=\"{:.2}\" text-anchor=\"end\">{}</text>\n", ml - 5.0, yc + fs * 0.33, esc(l));
    }
    let _ = write!(s, "<rect x=\"{ml:.2}\" y=\"{mt:.2}\" width=\"{pw:.2}\" height=\"{ph:.2}\" fill=\"none\" stroke=\"black\" stroke-width=\"0.8\"/>\n");
    let _ = write!(s, "<text x=\"{:.2}\" y=\"{:.2}\" text-anchor=\"middle\">{}</text>\n", ml + pw / 2.0, mt + ph + fs * 2.9, markup_svg(&f.xlabel, fs));
    s.push_str("</svg>\n");
    s
}

fn fontdb() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        for d in ["/usr/share/fonts", "/run/host/fonts", "/run/host/user-fonts"] {
            if Path::new(d).exists() {
                db.load_fonts_dir(d);
            }
        }
        // always have something to draw text with
        db.load_font_data(egui_default_font().to_vec());
        db.load_font_data(include_bytes!("../assets/fallback-serif.ttf").to_vec());
        db.set_serif_family("DejaVu Serif");
        db.set_sans_serif_family("DejaVu Sans");
        Arc::new(db)
    })
    .clone()
}

fn egui_default_font() -> &'static [u8] {
    include_bytes!("../assets/fallback.ttf")
}

fn tree(svg: &str) -> Result<usvg::Tree, String> {
    let opt = usvg::Options { fontdb: fontdb(), ..Default::default() };
    usvg::Tree::from_str(svg, &opt).map_err(|e| e.to_string())
}

/// A saved figure on the background chosen in Settings, when it is asked for there.
pub fn paper_svg(svg: String) -> String {
    let t = crate::theme::get();
    let (Some(c), true) = (t.plot_bg(), t.export_bg) else { return svg };
    let light = 0.299 * c[0] as f64 + 0.587 * c[1] as f64 + 0.114 * c[2] as f64 > 140.0;
    let bg = rgb(c);
    let mut s = svg.replace("fill=\"white\"", &format!("fill=\"{bg}\""));
    if !light {
        // light ink on dark paper
        s = s.replace("stroke=\"black\"", "stroke=\"#e6e6e6\"").replace("fill=\"black\"", "fill=\"#e6e6e6\"").replace("fill=\"#555555\"", "fill=\"#bdbdbd\"").replace("stroke=\"#d8d8d8\"", "stroke=\"#4a4a4a\"").replace("stroke=\"#bbbbbb\"", "stroke=\"#6a6a6a\"");
        if let Some(i) = s.find("<svg ") {
            if let Some(j) = s[i..].find('>') {
                s.insert_str(i + j, " fill=\"#e6e6e6\"");
            }
        }
    }
    s
}

pub fn svg_to_png(svg: &str, dpi: f64) -> Result<Vec<u8>, String> {
    let t = tree(svg)?;
    let scale = (dpi / 72.0) as f32;
    let size = t.size();
    let (w, h) = ((size.width() * scale).ceil() as u32, (size.height() * scale).ceil() as u32);
    let mut pix = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1)).ok_or("image too large")?;
    pix.fill(resvg::tiny_skia::Color::WHITE);
    resvg::render(&t, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pix.as_mut());
    pix.encode_png().map_err(|e| e.to_string())
}

pub fn svg_to_pdf(svg: &str) -> Result<Vec<u8>, String> {
    let t = tree(svg)?;
    svg2pdf::to_pdf(&t, svg2pdf::ConversionOptions::default(), svg2pdf::PageOptions::default()).map_err(|e| format!("{e:?}"))
}

pub fn to_csv(f: &Figure) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# {}", f.title);
    if let Some(b) = &f.bars {
        let _ = writeln!(s, "line,{}", f.xlabel.replace(',', ";"));
        for (l, v) in b.labels.iter().zip(&b.values) {
            let _ = writeln!(s, "\"{}\",{:.6e}", l, v);
        }
        return s;
    }
    // the spikes of the saved lines are in the table of line intensities
    let series: Vec<_> = f.series.iter().filter(|x| !x.name.is_empty() && x.name != crate::figures::SED_LINES).collect();
    let mut head = vec![];
    for se in &series {
        head.push(format!("\"{} x: {}\"", se.name, f.xlabel));
        head.push(format!("\"{} y: {}\"", se.name, f.ylabel));
    }
    let _ = writeln!(s, "{}", head.join(","));
    let n = series.iter().map(|x| x.x.len()).max().unwrap_or(0);
    for i in 0..n {
        let mut row = vec![];
        for se in &series {
            match (se.x.get(i), se.y.get(i)) {
                (Some(x), Some(y)) => {
                    row.push(format!("{:.6e}", x));
                    row.push(format!("{:.6e}", y));
                }
                _ => {
                    row.push(String::new());
                    row.push(String::new());
                }
            }
        }
        let _ = writeln!(s, "{}", row.join(","));
    }
    s
}

/// Write one figure to `path` (extension decided by `fmt`). Returns the written path.
pub fn export(f: &Figure, fmt: Format, st: &Style, dir: &Path, stem: &str) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    let path = dir.join(format!("{stem}.{}", fmt.ext()));
    let bytes: Vec<u8> = match fmt {
        Format::Svg => paper_svg(to_svg(f, st)).into_bytes(),
        Format::Png => svg_to_png(&paper_svg(to_svg(f, st)), st.dpi)?,
        Format::Pdf => svg_to_pdf(&paper_svg(to_svg(f, st)))?,
        Format::Csv => to_csv(f).into_bytes(),
    };
    std::fs::write(&path, bytes).map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
    Ok(path)
}
