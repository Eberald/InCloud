//! The built-in reader for the CLOUDY manuals (Hazy 1, 2 and 3).
//! Pages are rendered on a background thread, so that scrolling never blocks the window.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

use eframe::egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use eframe::egui::{self, Color32, FontId, Modifiers, RichText};
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{render, PixmapSettings, RenderCache, RenderSettings};

use crate::pdffind::{self, Found, Hit, Query, MAX_HITS};

pub const VOLUMES: [(u8, &str, &str); 3] = [(1, "Hazy 1", "Commands"), (2, "Hazy 2", "Output and physics"), (3, "Hazy 3", "Microphysics")];

#[derive(Clone, Debug)]
pub struct TocItem {
    pub level: usize,
    pub title: String,
    /// first page is 1
    pub page: usize,
}

pub struct Manual {
    pub vol: u8,
    pub toc: Vec<TocItem>,
    pub pages: usize,
}

struct Request {
    vol: u8,
    page: usize,
    scale: f32,
}

struct Rendered {
    vol: u8,
    page: usize,
    scale: f32,
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

type Key = (u8, usize, u32);

fn key(vol: u8, page: usize, scale: f32) -> Key {
    (vol, page, (scale * 100.0).round() as u32)
}

pub struct Library {
    pub dir: PathBuf,
    pub manuals: Vec<Manual>,
    tx: Sender<Request>,
    rx: Receiver<Rendered>,
    textures: HashMap<Key, egui::TextureHandle>,
    order: VecDeque<Key>,
    pending: HashSet<Key>,
    /// size of a page in PDF points
    page_size: egui::Vec2,
    find_tx: Option<Sender<Query>>,
    find_rx: Option<Receiver<Found>>,
}

/// What the reader is showing.
pub struct ReaderState {
    pub vol: u8,
    pub search: String,
    pub zoom: f32,
    pub fit_width: bool,
    pub goto: Option<usize>,
    pub current: usize,
    page_box: String,
    /// the words looked for in the text of the pages
    pub find: String,
    find_case: bool,
    find_word: bool,
    find_all: bool,
    pub hits: Vec<Hit>,
    hit: Option<usize>,
    gen: u64,
    sent: Option<(String, bool, bool, u8)>,
    need_first: bool,
    read: usize,
    pub finished: bool,
    focus_find: bool,
    scroll_list: bool,
    prepared: bool,
}

impl Default for ReaderState {
    fn default() -> Self {
        ReaderState {
            vol: 1,
            search: String::new(),
            zoom: 1.0,
            fit_width: true,
            goto: None,
            current: 1,
            page_box: "1".into(),
            find: String::new(),
            find_case: false,
            find_word: false,
            find_all: false,
            hits: vec![],
            hit: None,
            gen: 0,
            sent: None,
            need_first: false,
            read: 0,
            finished: false,
            focus_find: false,
            scroll_list: false,
            prepared: false,
        }
    }
}

/// The folder holding hazy1.pdf, looked for around the CLOUDY executable and data folder.
pub fn find_docs(starts: &[PathBuf]) -> Option<PathBuf> {
    for s in starts {
        let mut p: Option<&Path> = Some(s.as_path());
        for _ in 0..6 {
            let Some(d) = p else { break };
            for c in [d.join("docs"), d.to_path_buf()] {
                if c.join("hazy1.pdf").is_file() {
                    return Some(c);
                }
            }
            p = d.parent();
        }
    }
    None
}

fn worker(dir: PathBuf, rx: Receiver<Request>, tx: Sender<Rendered>, ctx: egui::Context) {
    let mut docs: HashMap<u8, Option<Pdf>> = HashMap::new();
    let settings = InterpreterSettings::default();
    while let Ok(first) = rx.recv() {
        // the newest requests are the pages on screen now
        let mut queue = vec![first];
        while let Ok(r) = rx.try_recv() {
            queue.push(r);
        }
        for r in queue.into_iter().rev() {
            let doc = docs.entry(r.vol).or_insert_with(|| std::fs::read(dir.join(format!("hazy{}.pdf", r.vol))).ok().and_then(|b| Pdf::new(b).ok()));
            let Some(pdf) = doc else { continue };
            let pages = pdf.pages();
            let Some(page) = r.page.checked_sub(1).and_then(|i| pages.get(i)) else { continue };
            let cache = RenderCache::new();
            let px = render(page, &cache, &settings, &RenderSettings::default(), &PixmapSettings { x_scale: r.scale, y_scale: r.scale, bg_color: WHITE });
            let (width, height) = (px.width() as usize, px.height() as usize);
            let rgba = px.data_as_u8_slice().to_vec();
            if tx.send(Rendered { vol: r.vol, page: r.page, scale: r.scale, width, height, rgba }).is_err() {
                return;
            }
            ctx.request_repaint();
        }
    }
}

impl Library {
    /// Read the contents of the manuals found in `dir`.
    pub fn open(dir: &Path, ctx: &egui::Context) -> Option<Library> {
        let mut manuals = vec![];
        for (vol, _, _) in VOLUMES {
            let path = dir.join(format!("hazy{vol}.pdf"));
            let Ok(doc) = lopdf::Document::load(&path) else { continue };
            let pages = doc.get_pages().len();
            let toc = doc.get_toc().map(|t| t.toc.into_iter().map(|e| TocItem { level: e.level, title: clean_title(&e.title), page: e.page }).collect()).unwrap_or_default();
            manuals.push(Manual { vol, toc, pages });
        }
        if manuals.is_empty() {
            return None;
        }
        let (req_tx, req_rx) = channel();
        let (done_tx, done_rx) = channel();
        let (d, c) = (dir.to_path_buf(), ctx.clone());
        std::thread::spawn(move || worker(d, req_rx, done_tx, c));
        Some(Library {
            dir: dir.to_path_buf(),
            manuals,
            tx: req_tx,
            rx: done_rx,
            textures: HashMap::new(),
            order: VecDeque::new(),
            pending: HashSet::new(),
            page_size: egui::vec2(612.0, 792.0),
            find_tx: None,
            find_rx: None,
        })
    }

    pub fn manual(&self, vol: u8) -> Option<&Manual> {
        self.manuals.iter().find(|m| m.vol == vol)
    }

    /// The page of the section whose title best matches `query`.
    pub fn find(&self, vol: u8, query: &str) -> Option<&TocItem> {
        let q = query.to_lowercase();
        let m = self.manual(vol)?;
        m.toc.iter().find(|t| t.title.to_lowercase() == q).or_else(|| m.toc.iter().find(|t| t.title.to_lowercase().starts_with(&q))).or_else(|| m.toc.iter().find(|t| t.title.to_lowercase().contains(&q)))
    }

    fn collect(&mut self, ctx: &egui::Context) {
        while let Ok(r) = self.rx.try_recv() {
            let k = key(r.vol, r.page, r.scale);
            self.pending.remove(&k);
            self.page_size = egui::vec2(r.width as f32 / r.scale, r.height as f32 / r.scale);
            let img = egui::ColorImage::from_rgba_premultiplied([r.width, r.height], &r.rgba);
            let tex = ctx.load_texture(format!("hazy{}-{}", r.vol, r.page), img, egui::TextureOptions::LINEAR);
            self.textures.insert(k, tex);
            self.order.push_back(k);
            while self.order.len() > 16 {
                if let Some(old) = self.order.pop_front() {
                    self.textures.remove(&old);
                }
            }
        }
    }

    fn page_texture(&mut self, vol: u8, page: usize, scale: f32) -> Option<egui::TextureId> {
        let k = key(vol, page, scale);
        if let Some(t) = self.textures.get(&k) {
            return Some(t.id());
        }
        if self.pending.insert(k) {
            let _ = self.tx.send(Request { vol, page, scale });
        }
        None
    }

    fn send_query(&mut self, q: Query, ctx: &egui::Context) {
        if self.find_tx.is_none() {
            let (query_tx, query_rx) = channel();
            let (found_tx, found_rx) = channel();
            let (d, c) = (self.dir.clone(), ctx.clone());
            std::thread::spawn(move || pdffind::worker(d, query_rx, found_tx, c));
            self.find_tx = Some(query_tx);
            self.find_rx = Some(found_rx);
        }
        if let Some(tx) = &self.find_tx {
            let _ = tx.send(q);
        }
    }

    /// Ask for the words in the box when they change and take in what has been found.
    /// Gives the match to show first, as soon as it is known.
    fn update_find(&mut self, st: &mut ReaderState, ctx: &egui::Context) -> Option<usize> {
        let text = st.find.trim().to_string();
        let key = (text.clone(), st.find_case, st.find_word, if st.find_all { 0 } else { st.vol });
        if st.sent.as_ref() != Some(&key) {
            st.sent = Some(key);
            st.gen += 1;
            st.hits.clear();
            st.hit = None;
            st.read = 0;
            st.finished = false;
            st.need_first = text.chars().count() >= 2;
            if st.need_first {
                let vols = if st.find_all { self.manuals.iter().map(|m| m.vol).collect() } else { vec![st.vol] };
                self.send_query(Query { gen: st.gen, text, case: st.find_case, word: st.find_word, vols }, ctx);
            }
        }
        let selected = st.hit.and_then(|i| st.hits.get(i)).map(|h| (h.vol, h.page, h.start));
        let mut changed = false;
        if let Some(rx) = &self.find_rx {
            while let Ok(f) = rx.try_recv() {
                if f.gen != st.gen {
                    continue;
                }
                if f.replace {
                    st.hits.clear();
                }
                changed |= f.replace || !f.hits.is_empty();
                st.hits.extend(f.hits);
                st.read = f.pages_read;
                st.finished |= f.finished;
            }
        }
        if changed {
            st.hits.sort_by_key(|h| (h.vol, h.page, h.start));
            st.hits.truncate(MAX_HITS);
            st.hit = selected.and_then(|k| st.hits.iter().position(|h| (h.vol, h.page, h.start) == k));
        }
        if st.need_first && !st.hits.is_empty() {
            let here = (st.vol, st.current);
            let first = st.hits.iter().position(|h| (h.vol, h.page) >= here).or(if st.finished { Some(0) } else { None });
            if first.is_some() {
                st.need_first = false;
            }
            return first;
        }
        None
    }

    /// The list of matches, in place of the contents.
    fn results_ui(&self, ui: &mut egui::Ui, st: &mut ReaderState) -> Option<usize> {
        let mut pick = None;
        let n = st.hits.len();
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let word = if n == 1 { "match" } else { "matches" };
            let more = if n >= MAX_HITS { "+" } else { "" };
            ui.label(RichText::new(format!("{n}{more} {word}")).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(14.0);
                if ui.small_button("✕").on_hover_text("Close the search (Esc)").clicked() {
                    st.find.clear();
                }
            });
        });
        ui.add_space(2.0);
        let row = 66.0;
        let step = row + ui.spacing().item_spacing.y;
        let mut area = egui::ScrollArea::vertical().id_salt(("hazy_hits", st.gen)).auto_shrink([false, false]);
        if std::mem::take(&mut st.scroll_list) {
            if let Some(i) = st.hit {
                area = area.vertical_scroll_offset((i as f32 * step - 120.0).max(0.0));
            }
        }
        let text_col = ui.visuals().text_color();
        let weak = ui.visuals().weak_text_color();
        let current = st.hit;
        let hits = &st.hits;
        area.show_rows(ui, row, n, |ui, range| {
            for i in range {
                let h = &hits[i];
                let width = (ui.available_width() - 14.0).max(80.0);
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, row), egui::Sense::click());
                let painter = ui.painter_at(rect);
                if current == Some(i) {
                    painter.rect_filled(rect, 6.0, ui.visuals().selection.bg_fill.gamma_multiply(0.35));
                } else if resp.hovered() {
                    painter.rect_filled(rect, 6.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                let section = self.manual(h.vol).and_then(|m| m.toc.iter().rev().find(|t| t.page <= h.page)).map(|t| t.title.as_str()).unwrap_or("");
                let place = if section.is_empty() { format!("Hazy {} · p. {}", h.vol, h.page) } else { format!("Hazy {} · p. {} · {section}", h.vol, h.page) };
                let mut head = LayoutJob::single_section(place, TextFormat::simple(FontId::proportional(12.5), weak));
                head.wrap.max_width = width - 16.0;
                head.wrap.max_rows = 1;
                let head = painter.layout_job(head);
                painter.galley(rect.min + egui::vec2(8.0, 5.0), head, weak);
                let plain = TextFormat::simple(FontId::proportional(14.0), text_col);
                let mark = TextFormat { background: Color32::from_rgb(255, 205, 60), color: Color32::BLACK, ..TextFormat::simple(FontId::proportional(14.0), Color32::BLACK) };
                let mut job = LayoutJob::default();
                job.append(&h.before, 0.0, plain.clone());
                job.append(&h.matched, 0.0, mark);
                job.append(&h.after, 0.0, plain);
                job.wrap.max_width = width - 16.0;
                job.wrap.max_rows = 2;
                let body = painter.layout_job(job);
                painter.galley(rect.min + egui::vec2(8.0, 24.0), body, text_col);
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    pick = Some(i);
                }
            }
        });
        pick
    }

    /// The reader: contents on the left, the pages on the right.
    pub fn ui(&mut self, ui: &mut egui::Ui, st: &mut ReaderState) {
        let ctx = ui.ctx().clone();
        self.collect(&ctx);
        if self.manual(st.vol).is_none() {
            st.vol = self.manuals[0].vol;
        }
        let mut jump: Option<usize> = st.goto.take();
        // where on the page to bring into view, for a match
        let mut jump_at: Option<[f32; 2]> = None;
        let mut pick: Option<usize> = None;
        let mut step = 0i32;

        // keys of the search
        let find_id = ui.scope_id().with("hazy_find");
        let in_box = ctx.memory(|m| m.has_focus(find_id));
        ui.input_mut(|i| {
            if i.consume_key(Modifiers::COMMAND, egui::Key::F) {
                st.focus_find = true;
            }
            if i.consume_key(Modifiers::SHIFT, egui::Key::F3) || (in_box && i.consume_key(Modifiers::SHIFT, egui::Key::Enter)) {
                step = -1;
            } else if i.consume_key(Modifiers::NONE, egui::Key::F3) || (in_box && i.consume_key(Modifiers::NONE, egui::Key::Enter)) {
                step = 1;
            }
            if !st.find.is_empty() && i.key_pressed(egui::Key::Escape) {
                st.find.clear();
            }
        });
        let searching = st.find.trim().chars().count() >= 2;

        egui::Panel::left("hazy_contents").default_size(300.0).size_range(200.0..=460.0).show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for (vol, name, what) in VOLUMES {
                    if self.manual(vol).is_some() {
                        let sel = st.vol == vol;
                        if ui.selectable_label(sel, name).on_hover_text(what).clicked() && !sel {
                            st.vol = vol;
                            jump = Some(1);
                        }
                    }
                }
            });
            if searching {
                pick = self.results_ui(ui, st);
                return;
            }
            ui.add(egui::TextEdit::singleline(&mut st.search).hint_text("Filter the contents…").desired_width(f32::INFINITY));
            ui.add_space(2.0);
            let q = st.search.to_lowercase();
            let current = st.current;
            let items: Vec<TocItem> = self.manual(st.vol).map(|m| m.toc.iter().filter(|t| if q.is_empty() { t.level <= 2 } else { t.title.to_lowercase().contains(&q) }).cloned().collect()).unwrap_or_default();
            egui::ScrollArea::vertical().id_salt(("hazy_toc", st.vol)).auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(ui.available_width() - 16.0);
                for (i, t) in items.iter().enumerate() {
                    let here = t.page <= current && items.get(i + 1).map(|n| n.page > current).unwrap_or(true);
                    ui.horizontal(|ui| {
                        ui.add_space(if q.is_empty() { (t.level.saturating_sub(1) * 12) as f32 } else { 0.0 });
                        ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                            let txt = if t.level == 1 { RichText::new(&t.title).strong() } else { RichText::new(&t.title) };
                            let b = egui::Button::selectable(here, txt).right_text(RichText::new(t.page.to_string()).small().weak()).wrap_mode(egui::TextWrapMode::Truncate);
                            if ui.add(b).on_hover_text(&t.title).clicked() {
                                jump = Some(t.page);
                            }
                        });
                    });
                }
                if items.is_empty() {
                    ui.label(RichText::new("Nothing found.").weak());
                }
            });
        });

        let pages = self.manual(st.vol).map(|m| m.pages).unwrap_or(1).max(1);
        // room between the contents list and the pages
        let mut rect = ui.available_rect_before_wrap();
        rect.min.x += 20.0;
        rect.max.x -= 8.0;
        rect.min.y += 4.0;
        let mut page_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
        let ui = &mut page_ui;
        // toolbar
        ui.horizontal_wrapped(|ui| {
            if ui.button("◀").on_hover_text("Previous page").clicked() {
                jump = Some(st.current.saturating_sub(1).max(1));
            }
            let r = ui.add(egui::TextEdit::singleline(&mut st.page_box).desired_width(52.0).align(egui::Align2::CENTER_CENTER));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                if let Ok(p) = st.page_box.trim().parse::<usize>() {
                    jump = Some(p.clamp(1, pages));
                }
            }
            ui.label(format!("of {pages}"));
            if ui.button("▶").on_hover_text("Next page").clicked() {
                jump = Some((st.current + 1).min(pages));
            }
            ui.separator();
            if ui.button("−").on_hover_text("Smaller").clicked() {
                st.zoom = (st.zoom / 1.2).max(0.4);
                st.fit_width = false;
            }
            ui.label(format!("{:.0}%", st.zoom * 100.0));
            if ui.button("+").on_hover_text("Larger").clicked() {
                st.zoom = (st.zoom * 1.2).min(4.0);
                st.fit_width = false;
            }
            ui.toggle_value(&mut st.fit_width, "Fit width");
            ui.separator();
            ui.label(RichText::new(format!("hazy{}.pdf", st.vol)).small().weak()).on_hover_text(self.dir.display().to_string());
        });
        // find in the text of the manual
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            let out = egui::TextEdit::singleline(&mut st.find).id(find_id).hint_text("🔍 Find in the manual…  (Ctrl+F)").desired_width(270.0).return_key(None).show(ui);
            if std::mem::take(&mut st.focus_find) {
                out.response.request_focus();
                let mut state = out.state.clone();
                state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(st.find.chars().count()))));
                state.store(ui.ctx(), find_id);
            }
            let ready = !st.hits.is_empty();
            if ui.add_enabled(ready, egui::Button::new("◀")).on_hover_text("Previous match (Shift+Enter)").clicked() {
                step = -1;
            }
            if ui.add_enabled(ready, egui::Button::new("▶")).on_hover_text("Next match (Enter)").clicked() {
                step = 1;
            }
            ui.toggle_value(&mut st.find_case, "Aa").on_hover_text("Match upper and lower case");
            ui.toggle_value(&mut st.find_word, "Whole word").on_hover_text("Only the whole word, not a part of a longer one");
            ui.toggle_value(&mut st.find_all, "All manuals").on_hover_text("Look in Hazy 1, 2 and 3 together");
            if searching {
                let total: usize = self.manuals.iter().filter(|m| st.find_all || m.vol == st.vol).map(|m| m.pages).sum();
                let n = st.hits.len();
                let more = if n >= MAX_HITS { "+" } else { "" };
                if !st.finished {
                    ui.spinner();
                    let share = (100 * st.read / total.max(1)).min(100);
                    let done = if n > 0 { format!("{n}{more} so far · ") } else { String::new() };
                    ui.label(RichText::new(format!("{done}reading the pages, {share}%")).small().weak());
                } else if n == 0 {
                    ui.label(RichText::new("No matches").color(Color32::from_rgb(214, 90, 70)));
                } else {
                    ui.label(format!("{} of {n}{more}", st.hit.map(|i| i + 1).unwrap_or(0)));
                }
            }
            // the pages are read as soon as the box is used, so that the first search is quicker
            if out.response.has_focus() && !st.prepared {
                st.prepared = true;
                self.send_query(Query { gen: st.gen, text: String::new(), case: false, word: false, vols: vec![st.vol] }, ui.ctx());
            }
        });
        ui.separator();

        let first = self.update_find(st, ui.ctx());
        let n = st.hits.len();
        if n > 0 && step != 0 {
            pick = Some(match st.hit {
                Some(i) => (i as i64 + step as i64).rem_euclid(n as i64) as usize,
                None if step > 0 => 0,
                None => n - 1,
            });
        }
        if let Some(h) = pick.or(first).and_then(|i| st.hits.get(i).map(|h| (i, h.vol, h.page, h.rects.first().copied()))) {
            st.hit = Some(h.0);
            st.vol = h.1;
            st.scroll_list = true;
            jump = Some(h.2);
            jump_at = h.3.map(|r| [r[0], r[1]]);
        }
        let pages = self.manual(st.vol).map(|m| m.pages).unwrap_or(1).max(1);

        // pages
        let avail = ui.available_width() - 24.0;
        if st.fit_width {
            st.zoom = (avail / self.page_size.x).clamp(0.4, 4.0);
        }
        let size = self.page_size * st.zoom;
        let gap = 10.0;
        let row = size.y + gap;
        let ppp = ui.ctx().pixels_per_point();
        // whole steps of a quarter keep the text sharp without re-rendering at every pixel of zoom
        let scale = ((st.zoom * ppp * 4.0).ceil() / 4.0).clamp(0.5, 6.0);
        let mut area = egui::ScrollArea::both().id_salt(("hazy_pages", st.vol)).auto_shrink([false, false]);
        if let Some(p) = jump {
            let inside = jump_at.map(|a| (a[1] * st.zoom - 140.0).max(0.0)).unwrap_or(0.0);
            area = area.vertical_scroll_offset((p.clamp(1, pages) - 1) as f32 * row + inside);
        }
        if let Some(a) = jump_at {
            if size.x > avail {
                area = area.horizontal_scroll_offset((a[0] * st.zoom - avail * 0.4).max(0.0));
            }
        }
        let marks = st.find.trim().chars().count() >= 2;
        let vol = st.vol;
        let bg = ui.visuals().extreme_bg_color;
        ui.spacing_mut().item_spacing.y = gap;
        let out = area.show_rows(ui, size.y, pages, |ui, range| {
            for i in range.clone() {
                let page = i + 1;
                ui.vertical_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    match self.page_texture(vol, page, scale) {
                        Some(id) => {
                            ui.painter().image(id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                            // the matches on this page, the one being looked at stands out
                            if marks {
                                let from = st.hits.partition_point(|h| (h.vol, h.page) < (vol, page));
                                for (j, h) in st.hits.iter().enumerate().skip(from).take_while(|(_, h)| (h.vol, h.page) == (vol, page)) {
                                    let now = st.hit == Some(j);
                                    for r in &h.rects {
                                        let k = st.zoom;
                                        let area = egui::Rect::from_min_max(rect.min + egui::vec2(r[0] * k, r[1] * k), rect.min + egui::vec2(r[2] * k, r[3] * k)).expand(1.0);
                                        let fill = if now { Color32::from_rgba_unmultiplied(255, 140, 0, 120) } else { Color32::from_rgba_unmultiplied(255, 215, 0, 95) };
                                        ui.painter().rect_filled(area, 2.0, fill);
                                        if now {
                                            ui.painter().rect_stroke(area, 2.0, egui::Stroke::new(1.5, Color32::from_rgb(214, 90, 0)), egui::StrokeKind::Outside);
                                        }
                                    }
                                }
                            }
                        }
                        None => {
                            ui.painter().rect_filled(rect, 0.0, bg);
                            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, format!("page {page}…"), egui::FontId::proportional(16.0), Color32::GRAY);
                        }
                    }
                });
            }
            // have the next page ready
            if range.end < pages {
                let _ = self.page_texture(vol, range.end + 1, scale);
            }
        });
        let now = ((out.state.clamped_offset().y + row * 0.4) / row).floor() as usize + 1;
        let now = now.clamp(1, pages);
        if now != st.current || jump.is_some() {
            st.current = jump.map(|p| p.clamp(1, pages)).unwrap_or(now);
            st.page_box = st.current.to_string();
        }
        if !self.pending.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }
    }
}

/// LaTeX quotes and spacing in section titles, made readable.
fn clean_title(t: &str) -> String {
    t.replace("``", "“").replace("''", "”").replace('\u{2013}', "-").split_whitespace().collect::<Vec<_>>().join(" ")
}
