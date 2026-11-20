//! Finding words inside the CLOUDY manuals.
//! The text of every page is read once on a background thread, with the place of each letter,
//! so that a match can be listed and marked on the page.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};

use eframe::egui;
use hayro::hayro_interpret::font::{Glyph, GlyphRun};
use hayro::hayro_interpret::hayro_syntax::Pdf;
use hayro::hayro_interpret::kurbo::{Affine, BezPath, Point, Rect};
use hayro::hayro_interpret::util::TransformExt;
use hayro::hayro_interpret::{interpret_page, BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps, InterpreterCache, InterpreterSettings, SoftMask};

/// Marks a word cut in two at the end of a line.
const SOFT: char = '\u{AD}';
/// More matches than anyone can read: the list stops here.
pub const MAX_HITS: usize = 5000;
/// Positions are kept in sixteenths of a point.
const UNIT: f32 = 16.0;

#[derive(Default)]
pub struct PageText {
    chars: Vec<char>,
    boxes: Vec<[u16; 4]>,
}

/// What to look for.
#[derive(Clone)]
pub struct Query {
    pub gen: u64,
    pub text: String,
    pub case: bool,
    pub word: bool,
    pub vols: Vec<u8>,
}

/// One match.
#[derive(Clone)]
pub struct Hit {
    pub vol: u8,
    /// first page is 1
    pub page: usize,
    pub start: usize,
    /// left, top, right, bottom in points from the top left of the page, one per line of text
    pub rects: Vec<[f32; 4]>,
    pub before: String,
    pub matched: String,
    pub after: String,
}

/// News from the reading thread.
pub struct Found {
    pub gen: u64,
    pub replace: bool,
    pub hits: Vec<Hit>,
    pub pages_read: usize,
    pub finished: bool,
}

struct RawGlyph {
    text: String,
    x0: f32,
    x1: f32,
    y: f32,
    size: f32,
}

#[derive(Default)]
struct Collector {
    glyphs: Vec<RawGlyph>,
}

impl<'a> Device<'a> for Collector {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, _: &DrawMode) {
        for g in run.glyphs() {
            let t: Affine = props.transform * g.transform();
            let c = t.as_coeffs();
            let size = ((c[2] * c[2] + c[3] * c[3]).sqrt() * 1000.0) as f32;
            let advance = match &**g {
                Glyph::Outline(o) => o.advance_width(),
                Glyph::Type3(o) => o.advance_width(),
            }
            .unwrap_or(500.0);
            let (start, end) = (t * Point::ZERO, t * Point::new(advance as f64, 0.0));
            let text = match g.as_unicode() {
                Some(hayro::hayro_interpret::hayro_cmap::BfString::Char(c)) => c.to_string(),
                Some(hayro::hayro_interpret::hayro_cmap::BfString::String(s)) => s,
                None => continue,
            };
            self.glyphs.push(RawGlyph { text, x0: start.x as f32, x1: end.x as f32, y: start.y as f32, size });
        }
    }
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

/// Letters that print differently but are searched as the same thing.
fn plain(c: char) -> &'static str {
    match c {
        'ﬀ' => "ff",
        'ﬁ' => "fi",
        'ﬂ' => "fl",
        'ﬃ' => "ffi",
        'ﬄ' => "ffl",
        '‘' | '’' | '`' => "'",
        '“' | '”' => "\"",
        '–' | '—' | '−' | '‐' | '‑' => "-",
        _ => "",
    }
}

fn to_box(x0: f32, y0: f32, x1: f32, y1: f32) -> [u16; 4] {
    [x0, y0, x1, y1].map(|v| (v * UNIT).clamp(0.0, 65535.0) as u16)
}

fn build_page(glyphs: Vec<RawGlyph>) -> PageText {
    let mut p = PageText::default();
    let mut last: Option<(f32, f32, f32)> = None;
    for g in glyphs {
        let (top, bottom) = (g.y - 0.78 * g.size, g.y + 0.24 * g.size);
        if let Some((px1, py, ps)) = last {
            let em = g.size.max(ps);
            let same_line = (g.y - py).abs() < 0.6 * em && g.x0 > px1 - 1.5 * em;
            let gap = if same_line {
                g.x0 - px1 > 0.15 * em
            } else {
                let n = p.chars.len();
                if n >= 2 && p.chars[n - 1] == '-' && p.chars[n - 2].is_alphabetic() {
                    p.chars[n - 1] = SOFT;
                    false
                } else {
                    true
                }
            };
            if gap && p.chars.last().is_some_and(|c| *c != ' ') {
                let (a, b) = if same_line { (px1, g.x0) } else { (g.x0, g.x0) };
                p.chars.push(' ');
                p.boxes.push(to_box(a, top, b, bottom));
            }
        }
        // one box per letter, also for the letters printed joined together
        let letters: Vec<char> = g.text.chars().flat_map(|c| if plain(c).is_empty() { vec![c] } else { plain(c).chars().collect() }).filter(|c| !c.is_control()).collect();
        let w = (g.x1 - g.x0) / letters.len().max(1) as f32;
        for (i, c) in letters.iter().enumerate() {
            let c = if c.is_whitespace() { ' ' } else { *c };
            if c == ' ' && p.chars.last().is_none_or(|l| *l == ' ') {
                continue;
            }
            p.chars.push(c);
            p.boxes.push(to_box(g.x0 + w * i as f32, top, g.x0 + w * (i + 1) as f32, bottom));
        }
        last = Some((g.x1, g.y, g.size));
    }
    p
}

fn read_volume(path: &Path, mut on_page: impl FnMut(PageText)) {
    let Some(pdf) = std::fs::read(path).ok().and_then(|b| Pdf::new(b).ok()) else { return };
    let pages = pdf.pages();
    let cache = InterpreterCache::new();
    let settings = InterpreterSettings::default();
    let everywhere = Rect::new(-1e5, -1e5, 1e5, 1e5);
    for page in pages.iter() {
        let mut context = Context::new(page.initial_transform(true).to_kurbo(), everywhere, &cache, page.xref(), settings.clone());
        let mut device = Collector::default();
        interpret_page(page, &mut context, &mut device);
        on_page(build_page(device.glyphs));
    }
}

fn fold(c: char, case: bool) -> char {
    if case {
        c
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

/// The words looked for, in the same form as the text of the pages.
fn needle(text: &str, case: bool) -> Vec<char> {
    let mut out: Vec<char> = vec![];
    for c in text.trim().chars() {
        let parts: Vec<char> = if plain(c).is_empty() { vec![c] } else { plain(c).chars().collect() };
        for c in parts {
            if c.is_whitespace() {
                if out.last().is_some_and(|l| *l != ' ') {
                    out.push(' ');
                }
            } else {
                out.push(fold(c, case));
            }
        }
    }
    out
}

fn is_gap(c: char) -> bool {
    c == ' ' || c == '-'
}

/// "ionisation" also finds "ionization", the spelling of the manuals, and the other way round.
fn same_spelling(q: &[char], j: usize, t: char) -> bool {
    let c = q[j];
    ((c == 's' && t == 'z') || (c == 'z' && t == 's')) && j >= 1 && q[j - 1] == 'i' && q.get(j + 1).is_none_or(|n| matches!(n, 'a' | 'e' | 'i'))
}

/// Where a match starting at `i` ends. A space and a hyphen stand for each other, and a word cut
/// at the end of a line counts as whole.
fn match_at(t: &[char], mut i: usize, q: &[char], case: bool) -> Option<usize> {
    let mut j = 0;
    while j < q.len() {
        let c = fold(*t.get(i)?, case);
        if c == SOFT {
            if is_gap(q[j]) {
                j += 1;
            }
            i += 1;
        } else if c == q[j] || (is_gap(c) && is_gap(q[j])) || (!case && same_spelling(q, j, c)) {
            i += 1;
            j += 1;
        } else {
            return None;
        }
    }
    Some(i)
}

/// The text around a match, without the rows of dots of the tables of contents.
fn snippet(chars: &[char]) -> String {
    let mut s: String = chars.iter().filter(|c| **c != SOFT).collect();
    while s.contains(". . ") {
        s = s.replace(". . ", ". ");
    }
    s.replace(" . ", " … ")
}

fn hit_rects(p: &PageText, start: usize, end: usize) -> Vec<[f32; 4]> {
    let mut rects: Vec<[f32; 4]> = vec![];
    for b in &p.boxes[start..end] {
        let r = b.map(|v| v as f32 / UNIT);
        match rects.last_mut() {
            // the same line of text
            Some(l) if r[1] < l[3] - 0.5 * (l[3] - l[1]) && r[3] > l[1] + 0.5 * (l[3] - l[1]) => {
                l[0] = l[0].min(r[0]);
                l[1] = l[1].min(r[1]);
                l[2] = l[2].max(r[2]);
                l[3] = l[3].max(r[3]);
            }
            _ => rects.push(r),
        }
    }
    rects
}

fn find_in_page(p: &PageText, q: &[char], query: &Query, vol: u8, page: usize, room: usize) -> Vec<Hit> {
    let mut hits = vec![];
    if q.is_empty() {
        return hits;
    }
    let t = &p.chars;
    let mut i = 0;
    while i < t.len() && hits.len() < room {
        let c = fold(t[i], query.case);
        if c != q[0] && !(is_gap(c) && is_gap(q[0])) && (query.case || !same_spelling(q, 0, c)) {
            i += 1;
            continue;
        }
        let Some(end) = match_at(t, i, q, query.case) else {
            i += 1;
            continue;
        };
        let whole = !query.word || ((i == 0 || !t[i - 1].is_alphanumeric()) && t.get(end).is_none_or(|c| !c.is_alphanumeric()));
        if !whole {
            i += 1;
            continue;
        }
        // whole words around the match
        let mut from = i.saturating_sub(46);
        if from > 0 {
            while from < i && t[from - 1] != ' ' {
                from += 1;
            }
        }
        let mut to = (end + 90).min(t.len());
        while to > end && to < t.len() && t[to] != ' ' {
            to -= 1;
        }
        hits.push(Hit {
            vol,
            page,
            start: i,
            rects: hit_rects(p, i, end),
            before: format!("{}{}", if from > 0 { "…" } else { "" }, snippet(&t[from..i])),
            matched: snippet(&t[i..end]),
            after: format!("{}{}", snippet(&t[end..to]), if to < t.len() { "…" } else { "" }),
        });
        i = end.max(i + 1);
    }
    hits
}

fn pages_read(q: &Query, index: &HashMap<u8, Vec<PageText>>) -> usize {
    q.vols.iter().map(|v| index.get(v).map(|p| p.len()).unwrap_or(0)).sum()
}

fn search_all(q: &Query, index: &HashMap<u8, Vec<PageText>>) -> Vec<Hit> {
    let n = needle(&q.text, q.case);
    let mut hits = vec![];
    for vol in &q.vols {
        for (i, p) in index.get(vol).map(|v| v.as_slice()).unwrap_or(&[]).iter().enumerate() {
            hits.extend(find_in_page(p, &n, q, *vol, i + 1, MAX_HITS - hits.len()));
        }
    }
    hits
}

/// The reading thread: it reads only the manuals asked for, and answers while it reads.
pub fn worker(dir: PathBuf, rx: Receiver<Query>, tx: Sender<Found>, ctx: egui::Context) {
    let mut index: HashMap<u8, Vec<PageText>> = HashMap::new();
    let mut read: HashSet<u8> = HashSet::new();
    let mut query: Option<Query> = None;
    let mut found = 0usize;
    loop {
        let wanted = query.as_ref().and_then(|q| q.vols.iter().copied().find(|v| !read.contains(v)));
        let Some(vol) = wanted else {
            let Ok(mut q) = rx.recv() else { return };
            while let Ok(newer) = rx.try_recv() {
                q = newer;
            }
            let hits = search_all(&q, &index);
            found = hits.len();
            let finished = q.vols.iter().all(|v| read.contains(v));
            let _ = tx.send(Found { gen: q.gen, replace: true, hits, pages_read: pages_read(&q, &index), finished });
            ctx.request_repaint();
            query = Some(q);
            continue;
        };
        index.insert(vol, vec![]);
        read_volume(&dir.join(format!("hazy{vol}.pdf")), |text| {
            let pages = index.entry(vol).or_default();
            pages.push(text);
            let page = pages.len();
            let mut newest = None;
            while let Ok(q) = rx.try_recv() {
                newest = Some(q);
            }
            if let Some(q) = newest {
                let hits = search_all(&q, &index);
                found = hits.len();
                let _ = tx.send(Found { gen: q.gen, replace: true, hits, pages_read: pages_read(&q, &index), finished: false });
                ctx.request_repaint();
                query = Some(q);
                return;
            }
            let Some(q) = &query else { return };
            if !q.vols.contains(&vol) {
                return;
            }
            let hits = match index.get(&vol).and_then(|v| v.last()) {
                Some(p) => find_in_page(p, &needle(&q.text, q.case), q, vol, page, MAX_HITS - found),
                None => vec![],
            };
            found += hits.len();
            if !hits.is_empty() || page % 6 == 0 {
                let _ = tx.send(Found { gen: q.gen, replace: false, hits, pages_read: pages_read(q, &index), finished: false });
                ctx.request_repaint();
            }
        });
        read.insert(vol);
        if let Some(q) = &query {
            if q.vols.iter().all(|v| read.contains(v)) {
                let _ = tx.send(Found { gen: q.gen, replace: false, hits: vec![], pages_read: pages_read(q, &index), finished: true });
                ctx.request_repaint();
            }
        }
    }
}

/// Developer check: read the manuals and report what some words find.
pub fn check(dir: &Path, words: &[String]) {
    let t0 = std::time::Instant::now();
    let mut index: HashMap<u8, Vec<PageText>> = HashMap::new();
    for vol in [1u8, 2, 3] {
        let t = std::time::Instant::now();
        let mut pages = vec![];
        read_volume(&dir.join(format!("hazy{vol}.pdf")), |p| pages.push(p));
        let chars: usize = pages.iter().map(|p| p.chars.len()).sum();
        println!("hazy{vol}: {} pages, {chars} letters, {:.1} s", pages.len(), t.elapsed().as_secs_f32());
        index.insert(vol, pages);
    }
    println!("all read in {:.1} s", t0.elapsed().as_secs_f32());
    for w in words {
        let (word, whole) = match w.strip_prefix('=') {
            Some(rest) => (rest, true),
            None => (w.as_str(), false),
        };
        let q = Query { gen: 0, text: word.to_string(), case: false, word: whole, vols: vec![1, 2, 3] };
        let t = std::time::Instant::now();
        let hits = search_all(&q, &index);
        println!("\n\"{word}\"{}: {} matches in {} ms", if whole { " (whole word)" } else { "" }, hits.len(), t.elapsed().as_millis());
        for h in hits.iter().take(3) {
            println!("  Hazy {} p.{}: {}[{}]{}", h.vol, h.page, h.before, h.matched, h.after);
        }
    }
}
