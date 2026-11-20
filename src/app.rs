//! The InCloud window: parameters on the left (in three tabs), progress and plots on the right.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, FontId, RichText, TextStyle};
use egui_file_dialog::FileDialog;
use egui_plot::{
    Bar, BarChart, CoordinatesFormatter, Corner, GridMark, HoverPosition, Line, LineStyle, MarkerShape, Plot as EPlot, PlotPoint, PlotPoints, Points, Span, VLine,
};

use crate::data::{self, DataSet};
use crate::export::{self, Format, Style};
use crate::figures::{default_ratios, ion_markup, markup_to_unicode, BptKind, ContUnit, Ctx, CustomSpec, Figure, Plot, RatioSpec, XAxis};
use crate::model::{Abundances, DensityLaw, Geometry, Grains, Params, Sed, SedX, SedY, ELEMENTS, SED_EXAMPLE};
use crate::network::{self, Zone};
use crate::pdfview::{self, Library, ReaderState};
use crate::place::Placer;
use crate::results::{pretty_line, pretty_reaction, scan_runs, BptRun, Results};
use crate::runner::{self, fmt_duration, Run, RunEnd};
use crate::wiki;

pub(crate) const ACCENT: Color32 = Color32::from_rgb(38, 110, 210);
const ACCENT2: Color32 = Color32::from_rgb(120, 90, 200);
pub(crate) const OK: Color32 = Color32::from_rgb(30, 150, 80);
pub(crate) const BAD: Color32 = Color32::from_rgb(210, 60, 60);
pub(crate) const AMBER: Color32 = Color32::from_rgb(205, 135, 20);

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Dashboard,
    One(Plot),
    Info,
}

/// The three sections of the program.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Results,
    Batch,
    Wiki,
    Manuals,
    Settings,
}

/// What is remembered about a plot on screen.
#[derive(Clone, Default)]
pub struct PlotView {
    sig: String,
    /// xmin, ymin, xmax, ymax as drawn (log10 on log axes)
    bounds: Option<[f64; 4]>,
    zoomed: bool,
    reset: bool,
    /// legend entries switched off
    hidden: Vec<String>,
    /// the views left behind by zooming or moving, newest last
    history: Vec<[f64; 4]>,
    /// the view when no mouse button was held
    settled: Option<[f64; 4]>,
    /// frames in which a change of view still comes from the mouse
    armed: u8,
    /// a view to go back to
    restore: Option<[f64; 4]>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LeftTab {
    Model,
    Output,
    Run,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pick {
    None,
    CloudyExe,
    DataPath,
    OutDir,
    ExportDir,
    OpenRun,
    SaveRun,
    AddRuns,
    SedFile,
    HazyDir,
    OpenDeck,
    SaveDeck,
    InstallRoot,
    WelcomeExe,
    WelcomeData,
    BatchOut,
    BatchTemplate,
}

/// Which plots are shown and how the extra ones are defined. Editable before and after a run.
pub struct PlotConfig {
    pub selected: Vec<Plot>,
    pub ratios: Vec<RatioSpec>,
    pub customs: Vec<CustomSpec>,
    pub bpt: BptKind,
}

impl Default for PlotConfig {
    fn default() -> Self {
        PlotConfig { selected: Plot::DEFAULT.to_vec(), ratios: default_ratios(), customs: vec![], bpt: BptKind::Nii }
    }
}

pub struct App {
    params: Params,
    cloudy_exe: String,
    data_path: String,
    installs: Vec<runner::Install>,
    use_tmp: bool,
    out_dir: String,

    run: Option<Run>,
    results: Option<Results>,
    /// other runs placed on the BPT diagram
    bpt_runs: Vec<BptRun>,
    results_temporary: bool,
    message: Option<(String, bool)>,
    error_detail: Option<String>,

    left_tab: LeftTab,
    view: View,
    info: crate::infoview::InfoState,
    editor: crate::deckedit::DeckEditor,
    plots: PlotConfig,
    xaxis: XAxis,
    element: String,
    species: String,
    /// species drawn in the chemical network (more than one gives the connection map)
    net_species: Vec<String>,
    rates_table: bool,
    /// abundances as a table instead of a plot
    abund_table: bool,
    /// every species CLOUDY can be asked about, and the data folder they were read from
    species_known: Vec<String>,
    species_path: String,
    chem_query: String,
    net_query: String,
    /// species to add to the network with a new run
    net_more: Vec<String>,
    /// species to show in the network when the new run ends
    net_pending: Vec<String>,
    cont_unit: ContUnit,
    /// the saved lines drawn on the SED
    sed_lines: bool,
    /// the plot shown beside the settings
    preview: Plot,
    /// the size of the interface
    ui_zoom: f32,
    /// a setting changed and has not been written to disk yet
    settings_dirty: bool,
    /// the window that chooses and installs versions of CLOUDY
    versions_open: bool,
    /// updates of InCloud itself
    update_status: Arc<std::sync::Mutex<Option<crate::updater::Status>>>,
    update_checking: Arc<std::sync::atomic::AtomicBool>,
    update_job: Option<crate::updater::Job>,
    update_at_start: bool,
    /// runs of an opened folder, to choose from
    run_choice: Vec<crate::detect::RunEntry>,
    run_choice_dir: PathBuf,
    run_choice_filter: String,
    /// a folder being searched for runs
    run_search: Option<RunSearch>,
    /// Settings should scroll to the Updates card (set by "Update available"), and since when
    goto_updates: Option<(bool, Instant)>,
    /// grids of models and their scripts
    batch: crate::batch::Batch,
    batch_tab: u8,
    batch_script: usize,
    /// the first start
    welcome: crate::welcome::Welcome,
    welcomed: bool,
    job: Option<crate::installer::Job>,
    tools: Option<crate::installer::Tools>,
    releases: Arc<std::sync::Mutex<Vec<crate::installer::Release>>>,
    releases_asked: bool,
    install_root: String,
    zone: Zone,
    show_regions: bool,
    show_grid: bool,
    left_open: bool,
    mode: Mode,
    wiki_topic: String,
    wiki_search: String,
    library: Option<Library>,
    library_tried: bool,
    reader: ReaderState,
    plots_open: bool,
    export_open: bool,
    runs_open: bool,
    net_open: bool,
    export_zoom: bool,
    views: HashMap<String, PlotView>,
    plot_gen: u64,

    export_fmt: Format,
    export_dir: String,
    file_prefix: String,
    style: Style,
    data_sets: Vec<DataSet>,
    data_tsv: bool,

    dark: bool,
    dialog: FileDialog,
    pick: Pick,

    temp_dirs: Vec<PathBuf>,
    screenshot: Option<PathBuf>,
    frame: u64,
    shot_sent: bool,
    logo: Option<egui::TextureHandle>,
}

/// A search for runs going on in the background.
struct RunSearch {
    dir: PathBuf,
    search: Arc<crate::detect::Search>,
    result: Arc<std::sync::Mutex<Option<Vec<crate::detect::RunEntry>>>>,
    started: Instant,
}

pub(crate) fn settings_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".config"));
    base.join("incloud").join("settings.txt")
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<PathBuf>, open_dir: Option<PathBuf>) -> Self {
        let mut app = App {
            params: Params::default(),
            cloudy_exe: String::new(),
            data_path: String::new(),
            installs: runner::find_installs(),
            use_tmp: true,
            out_dir: home().join("incloud_runs").display().to_string(),
            run: None,
            results: None,
            bpt_runs: vec![],
            results_temporary: false,
            message: None,
            error_detail: None,
            left_tab: LeftTab::Model,
            view: View::Dashboard,
            info: Default::default(),
            editor: Default::default(),
            plots: PlotConfig::default(),
            xaxis: XAxis::DepthLog,
            element: "O".into(),
            species: "CO".into(),
            net_species: vec![],
            rates_table: false,
            abund_table: false,
            species_known: vec![],
            species_path: String::new(),
            chem_query: String::new(),
            net_query: String::new(),
            net_more: vec![],
            net_pending: vec![],
            cont_unit: ContUnit::Micron,
            sed_lines: false,
            preview: Plot::Continuum,
            ui_zoom: 1.0,
            settings_dirty: false,
            versions_open: false,
            welcome: Default::default(),
            batch: Default::default(),
            batch_tab: 0,
            batch_script: 1,
            welcomed: false,
            run_choice: vec![],
            run_choice_dir: PathBuf::new(),
            run_choice_filter: String::new(),
            run_search: None,
            goto_updates: None,
            update_status: Arc::new(std::sync::Mutex::new(None)),
            update_checking: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            update_job: None,
            update_at_start: true,
            job: None,
            tools: None,
            releases: Arc::new(std::sync::Mutex::new(crate::installer::known())),
            releases_asked: false,
            install_root: String::new(),
            zone: Zone::Whole,
            show_regions: true,
            show_grid: false,
            left_open: true,
            mode: Mode::Results,
            wiki_topic: "overview".into(),
            wiki_search: String::new(),
            library: None,
            library_tried: false,
            reader: ReaderState::default(),
            plots_open: false,
            export_open: false,
            runs_open: false,
            net_open: false,
            export_zoom: true,
            views: HashMap::new(),
            plot_gen: 0,
            export_fmt: Format::Pdf,
            export_dir: home().join("incloud_plots").display().to_string(),
            file_prefix: String::new(),
            style: Style::default(),
            data_sets: vec![DataSet::Summary, DataSet::Lines, DataSet::Structure],
            data_tsv: false,
            dark: false,
            dialog: FileDialog::new(),
            pick: Pick::None,
            temp_dirs: vec![],
            screenshot: screenshot.clone(),
            frame: 0,
            shot_sent: false,
            logo: None,
        };
        let px = crate::logo::rgba(128, false);
        app.logo = Some(cc.egui_ctx.load_texture("incloud-logo", egui::ColorImage::from_rgba_unmultiplied([128, 128], &px), egui::TextureOptions::LINEAR));
        app.load_settings();
        // a saved executable that no longer exists is replaced by what the shell start-up files say
        if !Path::new(app.cloudy_exe.trim()).is_file() {
            if let Some(i) = app.installs.iter().find(|i| i.exists).cloned() {
                app.cloudy_exe = i.exe.display().to_string();
                app.data_path = i.data.clone();
            }
        }
        if app.data_path.trim().is_empty() && !app.cloudy_exe.trim().is_empty() {
            app.data_path = app
                .installs
                .iter()
                .find(|i| i.exe == Path::new(app.cloudy_exe.trim()))
                .map(|i| i.data.clone())
                .unwrap_or_else(|| runner::data_for_exe(Path::new(app.cloudy_exe.trim())));
        }
        if screenshot.is_some() {
            // switches used only to check the look in pictures
            app.dark = std::env::var("INCLOUD_DARK").is_ok();
            if std::env::var("INCLOUD_ALL_PLOTS").is_ok() {
                app.plots.selected = Plot::BUILTIN.to_vec();
            }
            if let Ok(v) = std::env::var("INCLOUD_ONE") {
                let p = match v.as_str() {
                    "bpt" => Plot::Bpt,
                    "network" => Plot::Network,
                    "rates" => Plot::Rates,
                    "ions" => Plot::Ions,
                    "structure" => Plot::Structure,
                    "molecules" => Plot::Molecules,
                    _ => Plot::Continuum,
                };
                app.plots.selected = vec![p];
                app.view = View::One(p);
            }
            if let Ok(v) = std::env::var("INCLOUD_NET") {
                app.net_species = v.split(',').map(|s| s.to_string()).collect();
            }
            app.rates_table = std::env::var("INCLOUD_TABLE").is_ok();
            app.abund_table = std::env::var("INCLOUD_ABUND").is_ok();
            app.sed_lines = std::env::var("INCLOUD_SEDLINES").is_ok();
            app.net_open = std::env::var("INCLOUD_NETWIN").is_ok();
            if let Ok(v) = std::env::var("INCLOUD_CHEM") {
                app.params.chem_species = v;
            }
            app.left_open = std::env::var("INCLOUD_HIDE_LEFT").is_err();
            app.show_grid = std::env::var("INCLOUD_GRID").is_ok();
            match std::env::var("INCLOUD_MODE").as_deref() {
                Ok("wiki") => app.mode = Mode::Wiki,
                Ok("settings") => app.mode = Mode::Settings,
                Ok("batch") => app.mode = Mode::Batch,
                Ok("batch-slurm") => {
                    app.mode = Mode::Batch;
                    app.batch.scheduler = crate::batch::Scheduler::Slurm;
                    app.batch.scripts = 2;
                    app.batch.slurm_partition = "long1".into();
                    app.batch.slurm_account = "astro".into();
                }
                Ok("updates") => {
                    app.mode = Mode::Settings;
                    app.goto_updates = Some((true, Instant::now()));
                }
                Ok("manual") => {
                    app.mode = Mode::Manuals;
                    app.reader.goto = Some(131);
                }
                _ => {}
            }
            if let Ok(k) = std::env::var("INCLOUD_WIKI") {
                app.wiki_topic = k;
            }
            if std::env::var("INCLOUD_TAB_OUTPUT").is_ok() {
                app.left_tab = LeftTab::Output;
            }
            if let Ok(w) = std::env::var("INCLOUD_FIND") {
                app.reader.find = w;
            }
            if let Ok(t) = std::env::var("INCLOUD_INFO") {
                app.info.show(&t);
                app.view = View::Info;
            }
            if std::env::var("INCLOUD_EDITOR").is_ok() {
                app.editor.open = true;
            }
            if std::env::var("INCLOUD_CONVERGE").is_ok() {
                app.params.converge = true;
            }
            if let Ok(f) = std::env::var("INCLOUD_FONTS") {
                let mut look = crate::theme::get();
                let v: Vec<f32> = f.split(',').filter_map(|x| x.parse().ok()).collect();
                if v.len() == 4 {
                    look.ui_font = v[0] as u8;
                    look.ui_text = v[1];
                    look.plot_font = v[2] as u8;
                    look.plot_text = v[3];
                }
                crate::theme::set(look);
            }
            if let Some(m) = std::env::var("INCLOUD_PLOTBG").ok().and_then(|v| v.parse::<u8>().ok()) {
                let mut look = crate::theme::get();
                look.bg_mode = m;
                look.bg = [250, 246, 235];
                look.export_bg = true;
                crate::theme::set(look);
            }
            if let Ok(r) = std::env::var("INCLOUD_ROOT") {
                app.install_root = r.clone();
                crate::installer::set_root(&r);
            }
            if std::env::var("INCLOUD_VERSIONS").is_ok() {
                app.versions_open = true;
            }
            if let Ok(name) = std::env::var("INCLOUD_INSTALL") {
                app.versions_open = true;
                let r = crate::installer::known().into_iter().find(|r| r.name == name).unwrap_or(crate::installer::Release { name, bytes: 0 });
                app.job = Some(crate::installer::Job::start(r, crate::installer::check_tools()));
            }
            match std::env::var("INCLOUD_CASE").as_deref() {
                Ok("ism") => app.params.sed = Sed::IsmField,
                Ok("cold") => app.params.bb_temp = 3000.0,
                Ok("error") => app.params.extra_commands = "blackbody 20000".into(),
                Ok("hot") => {
                    app.params.log_hden = 9.0;
                    app.params.log_u = 0.5;
                    app.params.stop.av = 200.0;
                    app.params.log_hden = -2.0;
                }
                Ok("dense") => {
                    app.params.log_hden = 8.0;
                    app.params.bb_temp = 5000.0;
                    app.params.log_u = 1.0;
                }
                Ok("warn") => {
                    app.params.grains = crate::model::Grains::None;
                    app.params.log_metals = 1.2;
                }
                _ => {}
            }
            if std::env::var("INCLOUD_TAB_RUN").is_ok() {
                app.left_tab = LeftTab::Run;
            }
            if std::env::var("INCLOUD_SED").is_ok() {
                app.params.sed = Sed::Custom;
                app.params.custom_sed.text = SED_EXAMPLE.to_string();
            }
            if std::env::var("INCLOUD_AV").is_ok() {
                app.xaxis = XAxis::Av;
                app.view = View::One(Plot::Thermal);
            }
        }
        apply_style(&cc.egui_ctx, app.dark);
        if app.screenshot.is_none() && app.ui_zoom != 1.0 {
            cc.egui_ctx.set_zoom_factor(app.ui_zoom);
        }
        // pictures of the first start, with or without CLOUDY
        if app.screenshot.is_some() {
            if std::env::var("INCLOUD_NO_CLOUDY").is_ok() {
                app.cloudy_exe.clear();
                app.data_path.clear();
                app.installs.retain(|_| false);
            }
            app.welcomed = std::env::var("INCLOUD_WELCOME").is_err();
        }
        app.welcome.open = !app.welcomed;
        // the first start after an update: say what's new
        if app.screenshot.is_none() {
            if let Some(w) = crate::whatsnew::on_start(app.welcomed) {
                app.welcome.open = true;
                app.welcome.updated = true;
                app.welcome.whats_new = Some(w);
            } else {
                app.welcome.whats_new = crate::whatsnew::stored();
            }
        } else if std::env::var("INCLOUD_WHATSNEW").is_ok() {
            app.welcome.open = true;
            app.welcome.updated = true;
            app.welcome.whats_new = Some(crate::whatsnew::sample());
        }
        app.welcome.prefer = app.cloudy_exe.clone();
        if app.update_at_start && app.screenshot.is_none() {
            app.check_updates(&cc.egui_ctx, true);
        }
        if app.screenshot.is_some() && std::env::var("INCLOUD_UPDATE").is_ok() {
            app.check_updates(&cc.egui_ctx, false);
        }
        if app.screenshot.is_some() && std::env::var("INCLOUD_START").is_ok() {
            app.start_run();
        }
        if let Some(d) = open_dir {
            app.open_run(&d);
            // pictures of one plot of a run that is already finished
            if let (true, Ok(_), Some(p)) = (screenshot.is_some(), std::env::var("INCLOUD_ONE"), app.plots.selected.first().copied()) {
                app.view = View::One(p);
            }
            if screenshot.is_some() && std::env::var("INCLOUD_DASH").is_ok() {
                app.view = View::Dashboard;
            }
        } else if screenshot.is_some() {
            app.load_demo();
        }
        if screenshot.is_some() && std::env::var("INCLOUD_QUIET").is_ok() {
            app.message = None;
        }
        app
    }

    fn load_settings(&mut self) {
        let Ok(t) = std::fs::read_to_string(settings_path()) else { return };
        let mut look = crate::theme::get();
        for l in t.lines() {
            let Some((k, v)) = l.split_once('=') else { continue };
            let v = v.trim().to_string();
            look.read(k.trim(), &v);
            let number = v.parse::<f64>().ok();
            match k.trim() {
                "show_regions" => self.show_regions = v == "true",
                "welcomed" => self.welcomed = v == "true",
                "update_at_start" => self.update_at_start = v == "true",
                "cloudy_root" => {
                    self.install_root = v.clone();
                    crate::installer::set_root(&v);
                }
                "sed_lines" => self.sed_lines = v == "true",
                "xaxis" => self.xaxis = number.and_then(|i| XAxis::ALL.get(i as usize).copied()).unwrap_or(self.xaxis),
                "cont_unit" => self.cont_unit = number.and_then(|i| ContUnit::ALL.get(i as usize).copied()).unwrap_or(self.cont_unit),
                "export_format" => self.export_fmt = number.and_then(|i| Format::ALL.get(i as usize).copied()).unwrap_or(self.export_fmt),
                "export_width" => self.style.width_in = number.unwrap_or(self.style.width_in).clamp(3.0, 12.0),
                "export_height" => self.style.height_in = number.unwrap_or(self.style.height_in).clamp(2.0, 10.0),
                "export_font" => self.style.font_pt = number.unwrap_or(self.style.font_pt).clamp(7.0, 16.0),
                "export_dpi" => self.style.dpi = number.unwrap_or(self.style.dpi).clamp(72.0, 600.0),
                "export_serif" => self.style.serif = v == "true",
                "export_title" => self.style.title = v == "true",
                "ui_zoom" => self.ui_zoom = number.unwrap_or(1.0).clamp(0.7, 1.6) as f32,
                "cloudy_exe" if !v.is_empty() => self.cloudy_exe = v,
                "data_path" => self.data_path = v,
                "out_dir" if !v.is_empty() => self.out_dir = v,
                "export_dir" if !v.is_empty() => self.export_dir = v,
                "use_tmp" => self.use_tmp = v == "true",
                "dark" => self.dark = v == "true",
                "plot_grid" => self.show_grid = v == "true",
                _ => {}
            }
        }
        crate::theme::set(look);
    }

    fn save_settings(&self) {
        // pictures taken to check the program never touch the real settings
        if self.screenshot.is_some() && std::env::var("INCLOUD_SAVE").is_err() {
            return;
        }
        let p = settings_path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(
            p,
            format!(
                "cloudy_exe={}\ndata_path={}\nout_dir={}\nexport_dir={}\nuse_tmp={}\ndark={}\nplot_grid={}\nshow_regions={}\nsed_lines={}\nxaxis={}\ncont_unit={}\nexport_format={}\nexport_width={}\nexport_height={}\nexport_font={}\nexport_dpi={}\nexport_serif={}\nexport_title={}\nui_zoom={}\ncloudy_root={}\nwelcomed={}\nupdate_at_start={}\n{}",
                self.cloudy_exe,
                self.data_path,
                self.out_dir,
                self.export_dir,
                self.use_tmp,
                self.dark,
                self.show_grid,
                self.show_regions,
                self.sed_lines,
                XAxis::ALL.iter().position(|x| *x == self.xaxis).unwrap_or(0),
                ContUnit::ALL.iter().position(|x| *x == self.cont_unit).unwrap_or(0),
                Format::ALL.iter().position(|x| *x == self.export_fmt).unwrap_or(0),
                self.style.width_in,
                self.style.height_in,
                self.style.font_pt,
                self.style.dpi,
                self.style.serif,
                self.style.title,
                self.ui_zoom,
                self.install_root,
                self.welcomed,
                self.update_at_start,
                crate::theme::get().to_text()
            ),
        );
    }

    fn info(&mut self, s: impl Into<String>) {
        self.message = Some((s.into(), true));
        self.error_detail = None;
    }
    fn fail(&mut self, s: impl Into<String>, detail: Option<String>) {
        self.message = Some((s.into(), false));
        self.error_detail = detail;
    }

    /// Show new results; the previous ones stay available for comparison (BPT diagram).
    fn set_results(&mut self, r: Results, temporary: bool) {
        if let Some(old) = self.results.take() {
            if !old.lines.is_empty() {
                self.bpt_runs.push(BptRun::from_results(&old));
            }
        }
        if !r.chemistry.iter().any(|c| c.species == self.species) {
            if let Some(c) = r.chemistry.first() {
                self.species = c.species.clone();
            }
        }
        self.net_species.retain(|s| r.chemistry.iter().any(|c| &c.species == s));
        for s in std::mem::take(&mut self.net_pending) {
            if r.chemistry.iter().any(|c| c.species == s) && !self.net_species.contains(&s) {
                self.net_species.push(s);
            }
        }
        if self.net_species.is_empty() {
            self.net_species.push(self.species.clone());
        }
        self.results = Some(r);
        self.plot_gen += 1;
        self.results_temporary = temporary;
    }

    fn load_demo(&mut self) {
        let dir = runner::temp_run_dir();
        match crate::demo::write_demo(&self.params, &dir) {
            Ok(prefix) => {
                self.temp_dirs.push(dir.clone());
                match Results::load(&dir, Some(&prefix)) {
                    Ok(r) => {
                        self.set_results(r, true);
                        self.info("Demo loaded: SYNTHETIC data to try the plots, not a CLOUDY calculation.");
                    }
                    Err(e) => self.fail(format!("Demo error: {e}"), None),
                }
            }
            Err(e) => self.fail(format!("Demo error: {e}"), None),
        }
    }

    /// Open a folder: one run opens at once; with several the person chooses which. Big folders
    /// are searched in the background, with a window that shows how far it got.
    fn open_run(&mut self, dir: &Path) {
        if self.screenshot.is_some() && std::env::var("INCLOUD_SEARCH_SHOW").is_err() {
            let runs = crate::detect::list_runs(dir);
            self.runs_found(dir, runs, None);
            return;
        }
        if let Some(old) = &self.run_search {
            old.search.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let search = Arc::new(crate::detect::Search::default());
        let result = Arc::new(std::sync::Mutex::new(None));
        let (s2, r2, d2) = (search.clone(), result.clone(), dir.to_path_buf());
        std::thread::spawn(move || {
            let runs = crate::detect::list_runs_with(&d2, &s2);
            if let Ok(mut g) = r2.lock() {
                *g = Some(runs);
            }
        });
        self.run_choice.clear();
        self.run_search = Some(RunSearch { dir: dir.to_path_buf(), search, result, started: Instant::now() });
    }

    /// The window shown while a folder is searched; it hands the runs on when the search ends.
    fn run_search_ui(&mut self, ctx: &egui::Context) {
        use std::sync::atomic::Ordering::Relaxed;
        let Some(rs) = &self.run_search else { return };
        // pictures of the window keep it open
        let hold = self.screenshot.is_some() && std::env::var("INCLOUD_SEARCH_HOLD").is_ok();
        let done = if hold { None } else { rs.result.lock().ok().and_then(|mut g| g.take()) };
        if let Some(runs) = done {
            let rs = self.run_search.take().expect("search");
            if !rs.search.cancel.load(Relaxed) {
                self.runs_found(&rs.dir, runs, Some(&rs.search));
            }
            return;
        }
        // a quick search shows nothing at all
        if rs.started.elapsed() < Duration::from_millis(350) && self.screenshot.is_none() {
            ctx.request_repaint_after(Duration::from_millis(60));
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(120));
        let (folders, files, runs) = (rs.search.folders.load(Relaxed), rs.search.files.load(Relaxed), rs.search.runs.load(Relaxed));
        let current = rs.search.current.lock().map(|c| c.clone()).unwrap_or_default();
        let (dir, secs) = (rs.dir.clone(), rs.started.elapsed().as_secs_f32());
        let mut stop = false;
        egui::Window::new("Looking for runs").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).default_width(560.0).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(26.0));
                ui.vertical(|ui| {
                    ui.label(RichText::new("Looking through the folder for CLOUDY runs…").strong().size(16.0));
                    ui.add(egui::Label::new(RichText::new(dir.display().to_string()).small().weak()).wrap());
                });
            });
            ui.add_space(8.0);
            egui::Grid::new("search_progress").num_columns(2).spacing([18.0, 4.0]).show(ui, |ui| {
                ui.label("Runs found");
                ui.label(RichText::new(runs.to_string()).strong().color(OK));
                ui.end_row();
                ui.label("Folders looked at");
                ui.label(folders.to_string());
                ui.end_row();
                ui.label("Files seen");
                ui.label(files.to_string());
                ui.end_row();
                ui.label("Time");
                ui.label(format!("{secs:.0} s"));
                ui.end_row();
                ui.label("Now in");
                ui.add(egui::Label::new(RichText::new(if current.is_empty() { ".".to_string() } else { current }).monospace().small()).truncate());
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.label(RichText::new(format!("Sub-folders are looked into up to {} levels down. Big folders on a network disk can take a while; InCloud stays usable meanwhile.", crate::detect::DEPTH)).small().weak());
            ui.add_space(4.0);
            if ui.button("Stop").on_hover_text("Stop looking; nothing is opened").clicked() {
                stop = true;
            }
        });
        if stop {
            if let Some(rs) = self.run_search.take() {
                rs.search.cancel.store(true, Relaxed);
            }
            self.info("Search stopped.");
        }
    }

    fn runs_found(&mut self, dir: &Path, runs: Vec<crate::detect::RunEntry>, search: Option<&crate::detect::Search>) {
        use std::sync::atomic::Ordering::Relaxed;
        let looked = search.map(|s| {
            let endings = s.endings_text();
            format!(
                "\n\nInCloud looked at {} folders (up to {} levels down) and {} files.{}",
                s.folders.load(Relaxed),
                crate::detect::DEPTH,
                s.files.load(Relaxed),
                if endings.is_empty() { String::new() } else { format!(" Their endings: {endings}.") }
            )
        });
        match runs.len() {
            0 => self.fail(
                format!("No CLOUDY run found in {} (nor in its sub-folders). InCloud needs the text output of CLOUDY (.out), its input file (.in) or a saved overview, and recognises the other saved files from what they hold.", dir.display()),
                Some(format!("A run that InCloud can show fully has, in one folder:\n  name.in    the input file\n  name.out   what CLOUDY prints (cloudy.exe < name.in > name.out)\n  and the files of its save commands, for example\n    save overview \"name.ovr\" last\n    save continuum \"name.con\" units microns last\n    save element oxygen \"name.ele_O\" last\n    save lines, emissivity \"name.ems\" last … end of lines\nFile names are free: InCloud reads them from the save commands.{}", looked.unwrap_or_default())),
            ),
            1 => {
                let r = &runs[0];
                let (d, n) = (r.dir.clone(), r.name.clone());
                self.open_named_run(&d, Some(&n));
            }
            _ => {
                self.run_choice = runs;
                self.run_choice_dir = dir.to_path_buf();
                self.run_choice_filter.clear();
                if search.is_some_and(|s| s.cut.load(Relaxed)) {
                    self.info("The folder is very large: the list stops at the first 50 000 runs or folders.");
                }
            }
        }
    }

    /// The window that lists the runs of a folder.
    fn run_choice_ui(&mut self, ctx: &egui::Context) {
        if self.run_choice.is_empty() {
            return;
        }
        let mut chosen: Option<(PathBuf, String)> = None;
        let mut open = true;
        let screen = ctx.content_rect();
        let base = self.run_choice_dir.clone();
        egui::Window::new("Several runs in this folder").open(&mut open).collapsible(false).default_size([820.0, 640.0]).max_height(screen.height() - 80.0).show(ctx, |ui| {
            egui::Frame::new().fill(AMBER.gamma_multiply(0.14)).stroke(egui::Stroke::new(1.0, AMBER)).corner_radius(8.0).inner_margin(10).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(format!("⚠ {} different CLOUDY runs are in {}.", self.run_choice.len(), base.display())).strong().color(AMBER));
                ui.label(RichText::new("They are told apart by the names of their input and output files. Choose the one to open; “Open a run…” on the same folder shows this list again.").small());
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("🔍");
                ui.add(egui::TextEdit::singleline(&mut self.run_choice_filter).hint_text("Filter by name, title or folder").desired_width(320.0));
                if !self.run_choice_filter.is_empty() && ui.small_button("✕").clicked() {
                    self.run_choice_filter.clear();
                }
            });
            let words: Vec<String> = self.run_choice_filter.to_lowercase().split_whitespace().map(str::to_string).collect();
            let shown: Vec<&crate::detect::RunEntry> = self
                .run_choice
                .iter()
                .filter(|r| {
                    let hay = format!("{} {} {}", r.name, r.title, r.dir.strip_prefix(&base).map(|p| p.display().to_string()).unwrap_or_default()).to_lowercase();
                    words.iter().all(|w| hay.contains(w.as_str()))
                })
                .collect();
            const MOST: usize = 300;
            let count = shown.len();
            ui.label(RichText::new(if count > MOST { format!("{count} runs match; the newest {MOST} are listed, type to narrow them down.") } else { format!("{count} runs listed, newest first.") }).small().weak());
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(screen.height() - 300.0).show(ui, |ui| {
                egui::Grid::new("run_choice").striped(true).num_columns(6).spacing([16.0, 6.0]).show(ui, |ui| {
                    for h in ["", "Run", "Title", "Folder", "Ended", "Saved"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for r in shown.iter().take(MOST) {
                        if ui.button("Open").clicked() {
                            chosen = Some((r.dir.clone(), r.name.clone()));
                        }
                        ui.label(RichText::new(&r.name).strong());
                        let title: String = if r.title.is_empty() { "—".to_string() } else if r.title.chars().count() > 60 { r.title.chars().take(58).collect::<String>() + "…" } else { r.title.clone() };
                        ui.label(title);
                        let rel = r.dir.strip_prefix(&base).map(|p| p.display().to_string()).unwrap_or_default();
                        ui.label(RichText::new(if rel.is_empty() { ".".into() } else { rel }).monospace().small());
                        match r.finished {
                            Some(true) => ui.label(RichText::new("✔ finished").color(OK)),
                            Some(false) => ui.label(RichText::new("✘ stopped early").color(BAD)),
                            None => ui.label(RichText::new("no output").weak()),
                        };
                        let what = if r.plots { format!("{} files, plots", r.kinds) } else { format!("{} files, no plots", r.kinds) };
                        ui.label(RichText::new(what).small().weak());
                        ui.end_row();
                    }
                });
            });
        });
        if let Some((d, n)) = chosen {
            self.run_choice.clear();
            self.open_named_run(&d, Some(&n));
        } else if !open {
            self.run_choice.clear();
        }
    }

    /// Open a finished run and show what it reports about itself.
    fn open_named_run(&mut self, dir: &Path, name: Option<&str>) {
        match Results::load(dir, name) {
            Ok(r) => {
                let name = r.prefix.clone();
                let more = r.siblings.len();
                let found = r.files.found.len();
                let guessed = r.files.found.iter().filter(|f| f.how == crate::detect::How::Content).count();
                let only_text = r.info_only;
                if !r.elements.iter().any(|e| e.0 == self.element) {
                    if let Some(e) = r.elements.first() {
                        self.element = e.0.clone();
                    }
                }
                self.set_results(r, false);
                self.view = View::Info;
                if only_text || guessed > 0 {
                    self.info.show("files");
                }
                let others = if more > 1 { format!(" ({more} runs in this folder)") } else { String::new() };
                let how = if guessed > 0 { format!(", {guessed} recognised from their content") } else { String::new() };
                self.info(format!("Run “{name}” opened from {}{others}: {found} files found{how}. See Run info → Files.", dir.display()));
            }
            Err(e) => self.fail(format!("Cannot open the run: {e}"), None),
        }
    }

    /// Compute the open run again, saving the rates of more species.
    fn rerun_with_species(&mut self) {
        let more = std::mem::take(&mut self.net_more);
        let Some(r) = self.results.as_ref() else { return };
        if more.is_empty() {
            return;
        }
        let same = !self.editor.edited && r.input_text.trim() == self.params.input_file().trim();
        let (deck, prefix) = (r.input_text.clone(), r.prefix.clone());
        let mut list = self.params.chem_list();
        for s in &more {
            if !list.contains(s) {
                list.push(s.clone());
            }
        }
        self.params.set_chem_list(&list);
        self.params.save_chemistry = true;
        // a run made elsewhere keeps its own input file, with the new commands added
        if !same {
            let lines: Vec<String> = more
                .iter()
                .flat_map(|sp| {
                    let f = crate::model::species_file(sp);
                    [format!("save chemistry rates creation \"{prefix}.chf_{f}\" \"{sp}\" last"), format!("save chemistry rates destruction \"{prefix}.chd_{f}\" \"{sp}\" last")]
                })
                .collect();
            self.editor.text = crate::model::deck_add_lines(&deck, &lines);
            self.editor.edited = true;
        }
        self.net_pending = more;
        self.net_open = false;
        self.start_run();
    }

    /// What should be fixed, or checked, before running.
    fn preflight(&self) -> Vec<crate::model::Problem> {
        if self.editor.edited {
            crate::model::check_deck(&self.editor.text)
                .into_iter()
                .map(|n| crate::model::Problem { error: n.error, text: if n.line > 0 { format!("Input file, line {}: {}", n.line, n.text) } else { n.text }, topic: "input" })
                .collect()
        } else {
            self.params.problems()
        }
    }

    /// The input file that will be run: the edited one, or the one made from the settings.
    fn deck_text(&self) -> String {
        if self.editor.edited {
            self.editor.text.clone()
        } else {
            self.params.input_file()
        }
    }

    fn start_run(&mut self) {
        let exe = PathBuf::from(self.cloudy_exe.trim());
        if self.cloudy_exe.trim().is_empty() || !exe.is_file() {
            self.fail("CLOUDY not found. Set the path of cloudy.exe in the “Run” tab, or try the demo.", None);
            self.left_tab = LeftTab::Run;
            return;
        }
        let deck = self.deck_text();
        // a file edited by hand needs the custom spectrum only if it still uses it
        let needs_sed = !self.editor.edited || deck.contains(crate::model::SED_FILE);
        let extra = match if needs_sed { self.params.extra_files() } else { Ok(vec![]) } {
            Ok(x) => x,
            Err(e) => {
                self.fail(format!("The custom spectrum cannot be used: {e}"), None);
                self.left_tab = LeftTab::Model;
                return;
            }
        };
        let prefix = if self.editor.edited { crate::model::deck_prefix(&deck).unwrap_or_else(|| self.params.file_prefix()) } else { self.params.file_prefix() };
        let dir = if self.use_tmp { runner::temp_run_dir() } else { PathBuf::from(self.out_dir.trim()).join(format!("{prefix}_{}", runner::timestamp())) };
        match Run::start(&exe, &self.data_path, dir.clone(), &prefix, &deck, &extra, self.use_tmp) {
            Ok(r) => {
                if self.use_tmp {
                    self.temp_dirs.push(dir);
                }
                self.run = Some(r);
                self.message = None;
                self.error_detail = None;
                self.save_settings();
            }
            Err(e) => self.fail(e, None),
        }
    }

    fn poll_run(&mut self, ctx: &egui::Context) {
        let Some(run) = self.run.as_mut() else { return };
        run.poll();
        ctx.request_repaint_after(Duration::from_millis(250));
        let Some(end) = run.ended() else { return };
        run.finished_after = Some(run.started.elapsed());
        let run = self.run.take().unwrap();
        let took = fmt_duration(run.elapsed());
        match end {
            RunEnd::Ok => match Results::load(&run.dir, Some(&run.prefix)) {
                Ok(r) => {
                    let (warn, caution) = (r.info.count(crate::runinfo::Level::Warning), r.info.count(crate::runinfo::Level::Caution));
                    let only_text = r.info_only;
                    self.set_results(r, run.temporary);
                    if only_text {
                        self.view = View::Info;
                    }
                    let where_ = if run.temporary { "in the temporary folder (deleted when InCloud closes)".to_string() } else { format!("in {}", run.dir.display()) };
                    let w = if warn + caution > 0 { format!(" — {warn} warnings, {caution} cautions: see Run info") } else { String::new() };
                    self.info(format!("Run finished in {took}, saved {where_}{w}."));
                }
                Err(e) => self.fail(format!("Run finished ({took}) but its results cannot be read: {e}"), None),
            },
            RunEnd::Failed(msg) => {
                // what CLOUDY wrote before stopping usually says why
                if let Ok(r) = Results::load(&run.dir, Some(&run.prefix)) {
                    self.set_results(r, run.temporary);
                    self.view = View::Info;
                }
                let why = self.results.as_ref().and_then(|r| r.info.problems.iter().find_map(|p| crate::runinfo::explain(p)));
                match why {
                    Some(w) => self.fail(format!("The CLOUDY run failed (after {took}). {w}"), Some(msg)),
                    None => self.fail(format!("The CLOUDY run failed (after {took}): see Run info."), Some(msg)),
                }
            }
            RunEnd::Cancelled => self.info("Run cancelled."),
        }
    }

    /// Lines to choose from for ratios: (label, readable name).
    fn line_options(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = vec![];
        let mut add = |label: &str| {
            let l = label.trim().trim_end_matches("air").trim_end_matches("vacuum").trim().to_string();
            if !v.iter().any(|(x, _)| crate::results::same_line(x, &l)) {
                v.push((l.clone(), pretty_line(&l)));
            }
        };
        for l in self.params.line_labels() {
            add(&l);
        }
        if let Some(r) = &self.results {
            for l in &r.lines {
                add(&l.label);
            }
        }
        v
    }

    /// Quantities to choose from for custom plots.
    fn column_options(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = match &self.results {
            Some(r) => r.columns(),
            None => vec![],
        };
        let mut add = |g: &str, n: &str| {
            if !v.iter().any(|(a, b)| a == g && b == n) {
                v.push((g.to_string(), n.to_string()));
            }
        };
        for n in ["Depth", "A_V", "N(H)"] {
            add("Axis", n);
        }
        for n in ["Te", "Htot", "hden", "eden", "2H_2/H", "HI", "HII", "HeI", "HeII", "HeIII", "CO/C", "C1", "C2", "C3", "C4", "O1", "O2", "O3", "O4", "H2O/O", "AV(point)", "AV(extend)", "Tau912"] {
            add("Structure", n);
        }
        for (el, ions) in [("H", &["H", "H+", "H2"][..]), ("He", &["He", "He+", "He+2"]), ("C", &["C", "C+", "C+2", "C+3"]), ("N", &["N", "N+", "N+2", "N+3"]), ("O", &["O", "O+", "O+2", "O+3"]), ("S", &["S", "S+", "S+2", "S+3"])] {
            for i in ions {
                add(&format!("Ions {el}"), i);
            }
        }
        for l in self.params.line_labels() {
            add("Line emissivity", l.trim_end_matches(" air"));
        }
        add("Heating", "Total");
        add("Cooling", "Total");
        v
    }

    // ------------------------------------------------------------------ left panel
    fn params_ui(&mut self, ui: &mut egui::Ui) {
        let running = self.run.is_some();
        // everything follows the width of the panel, so that it can be narrowed freely
        ui.spacing_mut().slider_width = (ui.available_width() - 285.0).clamp(40.0, 170.0);

        // tabs
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0;
            for (tab, label) in [(LeftTab::Model, "Model"), (LeftTab::Output, "Output"), (LeftTab::Run, "Run")] {
                let sel = self.left_tab == tab;
                let txt = RichText::new(label).size(16.0).strong();
                let txt = if sel { txt.color(Color32::WHITE) } else { txt };
                let b = egui::Button::new(txt).min_size(egui::vec2(w, 34.0)).fill(if sel { ACCENT } else { ui.visuals().widgets.inactive.weak_bg_fill });
                if ui.add(b).clicked() {
                    self.left_tab = tab;
                }
            }
        });
        ui.add_space(4.0);
        if self.editor.edited {
            egui::Frame::new().fill(AMBER.gamma_multiply(0.18)).stroke(egui::Stroke::new(1.0, AMBER)).corner_radius(8.0).inner_margin(8).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("The input file was edited by hand: it is run as written and the settings below are not used.").small());
                ui.horizontal(|ui| {
                    if ui.small_button("✎ Open the editor").clicked() {
                        self.editor.open = true;
                    }
                    if ui.small_button("Use the settings again").clicked() {
                        self.editor.edited = false;
                    }
                });
            });
            ui.add_space(4.0);
        }

        // the run button stays at the bottom of the panel
        egui::Panel::bottom("left_footer").show(ui, |ui| {
            ui.add_space(8.0);
            let problems = self.preflight();
            let mut errors = problems.iter().filter(|p| p.error).count();
            // without CLOUDY only the demo can be shown
            let no_cloudy = !Path::new(self.cloudy_exe.trim()).is_file();
            if no_cloudy {
                egui::Frame::new().fill(BAD.gamma_multiply(0.13)).stroke(egui::Stroke::new(1.0, BAD)).corner_radius(8.0).inner_margin(8).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("CLOUDY is not set: runs are off, the demo data still work.").color(BAD).strong());
                    if ui.button("Set up or install CLOUDY…").clicked() {
                        self.versions_open = true;
                    }
                });
                ui.add_space(6.0);
                errors += 1;
            }
            if !problems.is_empty() {
                let col = if errors > 0 { BAD } else { AMBER };
                egui::Frame::new().fill(col.gamma_multiply(0.13)).stroke(egui::Stroke::new(1.0, col)).corner_radius(8.0).inner_margin(8).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let head = if errors > 0 { "⛔ CLOUDY would stop at once:" } else { "⚠ Worth checking before the run:" };
                        ui.label(RichText::new(head).strong().color(col));
                        help(ui, "checks");
                    });
                    egui::ScrollArea::vertical().max_height(150.0).id_salt("preflight").show(ui, |ui| {
                        // the ones that stop the run first
                        for p in problems.iter().filter(|p| p.error).chain(problems.iter().filter(|p| !p.error)) {
                            // one label, so that the wrapped lines never run over the bullet
                            let mut job = egui::text::LayoutJob::default();
                            let font = TextStyle::Small.resolve(ui.style());
                            job.append(if p.error { "●  " } else { "○  " }, 0.0, egui::text::TextFormat::simple(font.clone(), if p.error { BAD } else { AMBER }));
                            job.append(&p.text, 0.0, egui::text::TextFormat::simple(font, ui.visuals().text_color()));
                            if ui.add(egui::Label::new(job).wrap().sense(egui::Sense::click())).on_hover_text("Click to read about it in the wiki").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                let k = p.topic.to_string();
                                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::NULL.with(WIKI_REQUEST), k));
                            }
                            ui.add_space(2.0);
                        }
                    });
                });
                ui.add_space(6.0);
            }
            let b = egui::Button::new(RichText::new("▶  Run CLOUDY").size(18.0).strong().color(Color32::WHITE))
                .fill(if errors > 0 { Color32::from_gray(110) } else { ACCENT })
                .corner_radius(8.0)
                .min_size(egui::vec2(ui.available_width(), 44.0));
            if ui.add_enabled(!running && errors == 0, b).on_hover_text("Write the input file and start CLOUDY").on_disabled_hover_text("Fix what is marked in red first").clicked() {
                self.start_run();
            }
            if errors > 0 && !running && !no_cloudy && ui.small_button("Run anyway").on_hover_text("Start CLOUDY all the same: it will probably stop at once").clicked() {
                self.start_run();
            }
            ui.horizontal(|ui| {
                let w = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
                if ui.add_enabled(!running, egui::Button::new("Demo data").min_size(egui::vec2(w, 30.0))).on_hover_text("Show the plots with synthetic data, without running CLOUDY").clicked() {
                    self.load_demo();
                }
                if ui.add_enabled(!running, egui::Button::new("Open a run…").min_size(egui::vec2(w, 30.0))).on_hover_text("Choose the folder of a finished run. InCloud finds its files by itself, whatever their names; if the folder holds several runs it asks which one.").clicked() {
                    self.pick = Pick::OpenRun;
                    self.dialog.pick_directory();
                }
            });
            ui.add_space(6.0);
        });

        let tab = self.left_tab;
        let input_preview = self.deck_text();
        let line_opts = self.line_options();
        let col_opts = self.column_options();
        let auto_species = {
            let mut p = self.params.clone();
            p.chem_species.clear();
            p.chem_auto = true;
            p.save_chemistry = true;
            p.chemistry_species().join(", ")
        };
        let p = &mut self.params;
        let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if let (true, Some(y)) = (self.screenshot.is_some(), std::env::var("INCLOUD_SCROLL").ok().and_then(|v| v.parse::<f32>().ok())) {
            area = area.vertical_scroll_offset(y);
        }
        area.show(ui, |ui| {
            ui.add_space(4.0);
            match tab {
                LeftTab::Model => {
                    ui.add_enabled_ui(!running, |ui| {
                        card(ui, "Model", "model", |ui| {
                            ui.horizontal(|ui| {
                                ui.label("Name");
                                ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(ui.available_width().min(220.0)));
                            });
                            ui.horizontal(|ui| {
                                ui.add(egui::Slider::new(&mut p.log_hden, -2.0..=10.0).step_by(0.05).text("log n(H) [cm⁻³]")).on_hover_text("Hydrogen density (“hden”)");
                                help(ui, "hden");
                            });
                            if p.sed == Sed::IsmField {
                                ui.horizontal(|ui| {
                                    ui.add(egui::Slider::new(&mut p.ism_log_g, -2.0..=6.0).step_by(0.05).text("log G/G_ISM")).on_hover_text("How many times stronger than the local interstellar field (“table ism factor”); the ISM field takes no log U");
                                    help(ui, "ismfield");
                                });
                            } else {
                                ui.horizontal(|ui| {
                                    ui.add(egui::Slider::new(&mut p.log_u, -6.0..=1.0).step_by(0.05).text("log U")).on_hover_text("Ionisation parameter (“ionization parameter”)");
                                    help(ui, "ionu");
                                });
                            }
                        });
                        card(ui, "Ionising source", "source", |ui| {
                            ui.horizontal(|ui| {
                                egui::ComboBox::from_id_salt("sed").selected_text(p.sed.label()).width((ui.available_width() - 44.0).clamp(120.0, 260.0)).truncate().show_ui(ui, |ui| {
                                    for s in Sed::ALL {
                                        ui.selectable_value(&mut p.sed, s, s.label());
                                    }
                                });
                                help(ui, match p.sed {
                                    Sed::Blackbody => "blackbody",
                                    Sed::Agn => "agn",
                                    Sed::IsmField => "ismfield",
                                    Sed::Custom => "customsed",
                                });
                            });
                            if p.sed == Sed::Blackbody {
                                ui.horizontal(|ui| {
                                ui.add(egui::Slider::new(&mut p.bb_temp, 5000.0..=200000.0).logarithmic(true).text("T★ [K]"));
                                help(ui, "blackbody");
                            });
                            }
                            if p.sed == Sed::Custom {
                                if sed_editor(ui, &mut p.custom_sed) {
                                    self.pick = Pick::SedFile;
                                    self.dialog.pick_file();
                                }
                            }
                        });
                        card(ui, "Geometry", "geometry", |ui| {
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut p.geometry, Geometry::Slab, "Slab");
                                ui.radio_value(&mut p.geometry, Geometry::Sphere, "Sphere");
                            });
                            if p.geometry == Geometry::Sphere {
                                ui.horizontal(|ui| {
                                ui.add(egui::Slider::new(&mut p.log_radius, 10.0..=22.0).step_by(0.05).text("log r₀ [cm]")).on_hover_text("Inner radius (“radius”)");
                                help(ui, "radius");
                            });
                            }
                            ui.horizontal_wrapped(|ui| {
                                ui.radio_value(&mut p.density_law, DensityLaw::Constant, "Constant density");
                                ui.radio_value(&mut p.density_law, DensityLaw::ConstantPressure, "Constant pressure");
                                help(ui, "density_law");
                            });
                        });
                        card(ui, "Composition", "abundances", |ui| {
                            egui::ComboBox::from_label("Abundances").selected_text(p.abundances.label()).show_ui(ui, |ui| {
                                for a in Abundances::ALL {
                                    ui.selectable_value(&mut p.abundances, a, a.label());
                                }
                            });
                            ui.horizontal(|ui| {
                                egui::ComboBox::from_id_salt("grains").selected_text(p.grains.label()).width((ui.available_width() - 44.0).clamp(120.0, 260.0)).truncate().show_ui(ui, |ui| {
                                    for g in Grains::ALL {
                                        ui.selectable_value(&mut p.grains, g, g.label());
                                    }
                                });
                                help(ui, "grains");
                            });
                            ui.horizontal(|ui| {
                                ui.add(egui::Slider::new(&mut p.log_metals, -2.0..=1.0).step_by(0.05).text("log Z/Z☉"));
                                help(ui, "metals");
                            });
                            ui.horizontal(|ui| {
                                ui.add(egui::Slider::new(&mut p.turbulence, 0.0..=20.0).step_by(0.5).text("turbulence [km/s]"));
                                help(ui, "turbulence");
                            });
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut p.cosmic_rays, "Cosmic-ray background").on_hover_text("Needed for the molecular chemistry in the PDR");
                                help(ui, "cosmicrays");
                            });
                        });
                        card(ui, "Stopping criteria", "stop", |ui| {
                            let s = &mut p.stop;
                            stop_row(ui, &mut s.use_temp, "Lowest temperature", "stop_temp", |ui, on| {
                                ui.add_enabled(on, egui::DragValue::new(&mut s.temp).range(1.0..=1e5).speed(1.0).suffix(" K"));
                            });
                            stop_row(ui, &mut s.use_av, "Extinction A_V", "stop_av", |ui, on| {
                                ui.add_enabled(on, egui::DragValue::new(&mut s.av).range(0.001..=1000.0).speed(0.1).suffix(" mag"));
                            });
                            stop_row(ui, &mut s.use_column, "log N(H)", "stop_col", |ui, on| {
                                ui.add_enabled(on, egui::DragValue::new(&mut s.log_column).range(15.0..=26.0).speed(0.05).suffix(" cm⁻²"));
                            });
                            stop_row(ui, &mut s.use_thickness, "log thickness", "stop_thick", |ui, on| {
                                ui.add_enabled(on, egui::DragValue::new(&mut s.log_thickness).range(5.0..=24.0).speed(0.05).suffix(" cm"));
                            });
                            stop_row(ui, &mut s.use_efrac, "log electron fraction", "stop_efrac", |ui, on| {
                                ui.add_enabled(on, egui::DragValue::new(&mut s.log_efrac).range(-10.0..=0.0).speed(0.05));
                            });
                            stop_row(ui, &mut s.use_zones, "Number of zones", "stop_zone", |ui, on| {
                                ui.add_enabled(on, egui::DragValue::new(&mut s.zones).range(1..=100000));
                            });
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                let label = if p.converge { "↻  Iterate to convergence: on" } else { "↻  Iterate to convergence" };
                                ui.toggle_value(&mut p.converge, RichText::new(label).strong()).on_hover_text("Repeat the calculation until the optical depths stop changing");
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| help(ui, "converge"));
                            });
                            if p.converge {
                                ui.horizontal(|ui| {
                                    ui.label("Most iterations");
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.add(egui::DragValue::new(&mut p.converge_max).range(2..=100));
                                    });
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Tolerance").on_hover_text("Change between two iterations still accepted: 0.2 means 20%, CLOUDY's default");
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.add(egui::DragValue::new(&mut p.converge_error).range(0.001..=1.0).speed(0.005).max_decimals(3));
                                    });
                                });
                            } else {
                                ui.horizontal(|ui| {
                                    ui.label("Iterations");
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        help(ui, "iterate");
                                        ui.add(egui::DragValue::new(&mut p.iterations).range(1..=20));
                                    });
                                });
                            }
                            if !s.use_temp {
                                ui.label(RichText::new("No temperature limit: the other ticked limits decide where the run stops (CLOUDY's own 4000 K limit is turned off).").small().weak());
                            }
                        });
                    });
                }
                LeftTab::Output => {
                    ui.add_enabled_ui(!running, |ui| {
                        card(ui, "Lines to save", "lines", |ui| {
                            ui.horizontal(|ui| {
                                if ui.small_button("All").clicked() {
                                    p.lines.iter_mut().for_each(|l| l.enabled = true);
                                }
                                if ui.small_button("None").clicked() {
                                    p.lines.iter_mut().for_each(|l| l.enabled = false);
                                }
                                let n = p.lines.iter().filter(|l| l.enabled).count();
                                ui.label(RichText::new(format!("{n} selected")).weak());
                            });
                            for group in ["Optical", "Infrared", "Molecular"] {
                                egui::CollapsingHeader::new(RichText::new(group).strong()).default_open(group == "Optical").show(ui, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        for l in p.lines.iter_mut().filter(|l| l.group == group) {
                                            ui.checkbox(&mut l.enabled, l.name).on_hover_text(format!("CLOUDY label: “{}”", l.label));
                                        }
                                    });
                                });
                            }
                            ui.label("More lines (one per row, CLOUDY label):");
                            ui.add(egui::TextEdit::multiline(&mut p.custom_lines).hint_text("e.g.  Fe 2 1.25668m air").font(TextStyle::Monospace).desired_rows(2).desired_width(f32::INFINITY));
                        });
                        card(ui, "Chemistry", "chemistry", |ui| {
                            ui.checkbox(&mut p.save_chemistry, "Save reaction rates");
                            ui.add_enabled_ui(p.save_chemistry, |ui| {
                                ui.checkbox(&mut p.chem_auto, "Species of the chosen lines").on_hover_text(auto_species.clone());
                                if p.chem_auto {
                                    ui.label(RichText::new(auto_species.clone()).small().weak());
                                }
                                ui.add_space(4.0);
                                ui.label(RichText::new("Add any other species").strong());
                                let mut list = p.chem_list();
                                if crate::species::picker(ui, "chem_pick", &mut list, &self.species_known, &mut self.chem_query, crate::model::MAX_CHEM_SPECIES, &[]) {
                                    p.set_chem_list(&list);
                                }
                                ui.label(RichText::new(format!("{} species will be saved.", p.chemistry_species().len())).small().weak());
                            });
                        });
                    });
                    card(ui, "Plots", "p_custom", |ui| {
                        ui.label(RichText::new("Choose them now or after the run.").small().weak());
                        plots_ui(ui, &mut self.plots, &line_opts, &col_opts, "left");
                    });
                }
                LeftTab::Run => {
                    card(ui, "CLOUDY", "cloudy", |ui| {
                        if ui.button("⬇ Install or change version…").clicked() {
                            self.versions_open = true;
                        }
                        let found = !self.cloudy_exe.trim().is_empty() && Path::new(self.cloudy_exe.trim()).is_file();
                        ui.horizontal(|ui| {
                            ui.label("Executable");
                            if found {
                                ui.label(RichText::new("● found").color(OK));
                            } else {
                                ui.label(RichText::new("● not found").color(BAD));
                            }
                        });
                        if !self.installs.is_empty() {
                            let current = self.installs.iter().find(|i| i.exe == Path::new(self.cloudy_exe.trim())).map(|i| i.label()).unwrap_or_else(|| "choose an installation…".into());
                            let mut chosen: Option<runner::Install> = None;
                            egui::ComboBox::from_id_salt("installs").selected_text(RichText::new(current).small()).width(ui.available_width() - 8.0).truncate().show_ui(ui, |ui| {
                                for i in &self.installs {
                                    let txt = RichText::new(i.label()).color(if i.exists { ui.visuals().text_color() } else { Color32::GRAY });
                                    if ui
                                        .add_enabled(i.exists, egui::Button::selectable(i.exe == Path::new(self.cloudy_exe.trim()), txt))
                                        .on_hover_text(format!("{}\nCLOUDY_DATA_PATH = {}", i.exe.display(), i.data))
                                        .clicked()
                                    {
                                        chosen = Some(i.clone());
                                    }
                                }
                            });
                            if let Some(i) = chosen {
                                self.cloudy_exe = i.exe.display().to_string();
                                self.data_path = i.data;
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("🔍").on_hover_text("Search again in ~/.bashrc, ~/.zshrc and the usual folders").clicked() {
                                    self.installs = runner::find_installs();
                                    match runner::detect_cloudy() {
                                        Some(i) => {
                                            self.cloudy_exe = i.exe.display().to_string();
                                            self.data_path = i.data.clone();
                                            self.message = Some((format!("Found {} ({})", i.exe.display(), i.origin), true));
                                        }
                                        None => self.message = Some(("No CLOUDY executable found automatically.".into(), false)),
                                    }
                                }
                                if ui.button("📁").on_hover_text("Choose the executable").clicked() {
                                    self.pick = Pick::CloudyExe;
                                    self.dialog.pick_file();
                                }
                                ui.add(egui::TextEdit::singleline(&mut self.cloudy_exe).hint_text("…/source/cloudy.exe").desired_width(ui.available_width()));
                            });
                        });
                        ui.label("Data folder (CLOUDY_DATA_PATH)");
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("📁").clicked() {
                                    self.pick = Pick::DataPath;
                                    self.dialog.pick_directory();
                                }
                                ui.add(egui::TextEdit::singleline(&mut self.data_path).hint_text("…/data").desired_width(ui.available_width()));
                            });
                        });
                    });
                    card(ui, "Where runs go", "runfolder", |ui| {
                        ui.checkbox(&mut self.use_tmp, "Run in /tmp (temporary, not kept)").on_hover_text("Run files go to the temporary folder and are deleted when InCloud closes");
                        if self.use_tmp {
                            ui.label(RichText::new(format!("Folder: {}", std::env::temp_dir().display())).small().weak());
                        } else {
                            ui.horizontal(|ui| {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("📁").on_hover_text("Choose folder").clicked() {
                                        self.pick = Pick::OutDir;
                                        self.dialog.pick_directory();
                                    }
                                    ui.add(egui::TextEdit::singleline(&mut self.out_dir).desired_width(ui.available_width()));
                                });
                            });
                        }
                    });
                    card(ui, "Input file", "input", |ui| {
                        let edited = self.editor.edited;
                        ui.add_enabled_ui(!running && !edited, |ui| {
                            ui.label(RichText::new("Your own commands, added to the file").strong());
                            ui.add(egui::TextEdit::multiline(&mut p.extra_commands).hint_text("one command per line, e.g.  CMB").font(TextStyle::Monospace).desired_rows(3).desired_width(f32::INFINITY));
                            egui::ComboBox::from_id_salt("snippets").selected_text("Insert a command…").width(ui.available_width() - 8.0).show_ui(ui, |ui| {
                                for (cmd, what) in crate::deckedit::SNIPPETS {
                                    if ui.selectable_label(false, format!("{cmd}   —   {what}")).clicked() {
                                        if !p.extra_commands.is_empty() && !p.extra_commands.ends_with('\n') {
                                            p.extra_commands.push('\n');
                                        }
                                        p.extra_commands.push_str(cmd);
                                        p.extra_commands.push('\n');
                                    }
                                }
                            });
                        });
                        ui.add_space(6.0);
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(!running, egui::Button::new(RichText::new("✎ Edit the whole file").strong())).on_hover_text("Change any line of the input file").clicked() {
                                self.editor.open = true;
                            }
                            if ui.add_enabled(!running, egui::Button::new("Open a file…")).on_hover_text("Run an input file you already have").clicked() {
                                self.pick = Pick::OpenDeck;
                                self.dialog.pick_file();
                            }
                            if ui.button("Copy").clicked() {
                                ui.ctx().copy_text(input_preview.clone());
                            }
                        });
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical().max_height(320.0).min_scrolled_height(260.0).id_salt("inprev").show(ui, |ui| {
                            let mut t = input_preview.as_str();
                            ui.add(egui::TextEdit::multiline(&mut t).font(TextStyle::Monospace).desired_width(f32::INFINITY));
                        });
                    });
                }
            }
            ui.add_space(12.0);
        });
    }

    // ------------------------------------------------------------------ right panel
    fn progress_ui(&mut self, ui: &mut egui::Ui) {
        let Some(run) = self.run.as_ref() else { return };
        let mut cancel = false;
        ui.add_space(ui.available_height() * 0.18);
        ui.vertical_centered(|ui| {
            egui::Frame::group(ui.style()).fill(ui.visuals().extreme_bg_color).inner_margin(26.0).corner_radius(12.0).stroke(egui::Stroke::new(1.5, ACCENT)).show(ui, |ui| {
                ui.set_width(560.0);
                ui.vertical_centered(|ui| {
                    ui.heading(RichText::new("CLOUDY is computing the model…").strong());
                    ui.add_space(14.0);
                    ui.add(egui::Spinner::new().size(52.0).color(ACCENT));
                    ui.add_space(10.0);
                    ui.label(RichText::new(format!("Elapsed: {}", fmt_duration(run.elapsed()))).size(20.0).strong());
                    ui.add_space(10.0);
                    let pr = &run.progress;
                    let d = &run.deck;
                    if pr.zones == 0 {
                        ui.label(RichText::new("Reading the atomic data and preparing the first zone…").weak());
                    } else {
                        use crate::runinfo::pretty;
                        // what is reached so far, next to the limit that stops the run
                        let of = |now: String, limit: Option<String>| match limit {
                            Some(l) => format!("{now}   (stops at {l})"),
                            None => now,
                        };
                        let passes = if d.converge { format!("{} (until converged, at most {})", pr.iteration, d.iterations) } else { format!("{} of {}", pr.iteration, pr.iterations) };
                        let region = match (pr.hplus, pr.h2) {
                            (_, Some(m)) if m > 0.5 => format!("molecular (2H₂/H = {:.0}%)", m * 100.0),
                            (Some(x), _) if x > 0.5 => format!("H II (H⁺ = {:.0}%)", x * 100.0),
                            (Some(x), m) => format!("PDR (H⁺ = {:.1}%, 2H₂/H = {:.1}%)", x * 100.0, m.unwrap_or(0.0) * 100.0),
                            _ => "—".into(),
                        };
                        let mut rows: Vec<(&str, String)> = vec![
                            ("Iteration", passes),
                            ("Zone", of(pr.zones.to_string(), d.stop_zones.map(|z| z.to_string()))),
                            ("Depth", of(format!("{} cm ({} pc)", pretty(pr.depth), pretty(pr.depth / 3.0857e18)), d.stop_log_thickness.map(|t| format!("{} cm", pretty(10f64.powf(t)))))),
                            ("Column density N(H)", of(format!("{} cm⁻²", pretty(pr.column)), d.stop_log_column.map(|c| format!("{} cm⁻²", pretty(10f64.powf(c)))))),
                        ];
                        if let Some(av) = pr.av.filter(|a| a.is_finite()) {
                            rows.push(("A_V", of(format!("{av:.3} mag"), d.stop_av.map(|a| format!("{a} mag")))));
                        }
                        rows.push(("Temperature Tₑ", of(format!("{} K", pretty(pr.te)), d.stop_temp.map(|t| format!("{} K", pretty(t))))));
                        rows.push(("Where the zone is", region));
                        if let Some(n) = pr.hden {
                            rows.push(("n(H), n(e)", format!("{} , {} cm⁻³", pretty(n), pr.eden.map(pretty).unwrap_or_else(|| "—".into()))));
                        }
                        if let Some(z) = pr.zones_per_minute.filter(|z| *z > 0.0) {
                            rows.push(("Pace", format!("{z:.0} zones per minute")));
                        }
                        egui::Grid::new("run_progress").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                            for (k, v) in rows {
                                ui.label(RichText::new(k).weak());
                                ui.label(RichText::new(v).strong());
                                ui.end_row();
                            }
                        });
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("Folder: {}", run.dir.display())).small().weak());
                    ui.add_space(12.0);
                    if ui.add(egui::Button::new(RichText::new("Cancel run").size(16.0)).min_size(egui::vec2(160.0, 36.0))).clicked() {
                        cancel = true;
                    }
                });
            });
        });
        if cancel {
            if let Some(r) = self.run.as_mut() {
                r.cancel();
            }
        }
    }

    /// Look for an update of InCloud in the background.
    fn check_updates(&mut self, ctx: &egui::Context, fetch: bool) {
        use std::sync::atomic::Ordering;
        if self.update_checking.swap(true, Ordering::SeqCst) {
            return;
        }
        let (slot, busy, c) = (self.update_status.clone(), self.update_checking.clone(), ctx.clone());
        std::thread::spawn(move || {
            let s = crate::updater::check(fetch);
            if let Ok(mut g) = slot.lock() {
                *g = Some(s);
            }
            busy.store(false, Ordering::SeqCst);
            c.request_repaint();
        });
    }

    fn update_available(&self) -> bool {
        self.update_status.lock().ok().and_then(|g| g.as_ref().map(|s| s.available())).unwrap_or(false)
    }

    fn updates_ui(&mut self, ui: &mut egui::Ui) {
        use crate::updater::{self, Step};
        let status = self.update_status.lock().ok().and_then(|g| g.clone());
        let checking = self.update_checking.load(std::sync::atomic::Ordering::SeqCst);
        let job = self.update_job.as_ref().map(|j| j.view());
        if job.as_ref().is_some_and(|j| matches!(j.step, Step::Pull | Step::Build | Step::Install)) {
            ui.ctx().request_repaint_after(Duration::from_millis(300));
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("InCloud {}", updater::VERSION)).strong());
            ui.label(RichText::new(format!("built {}", updater::built_text())).weak());
        });
        match updater::source() {
            Some(d) => ui.label(RichText::new(format!("Source: {}", d.display())).small().weak()),
            None => ui.label(RichText::new("The folder it was built from is gone: it can't update itself.").small().color(AMBER)),
        };
        ui.add_space(4.0);
        if let Some(s) = &status {
            if !s.new_commits.is_empty() {
                ui.label(RichText::new(format!("⬆ {} new change{} on git:", s.new_commits.len(), if s.new_commits.len() == 1 { "" } else { "s" })).strong().color(OK));
                for c in s.new_commits.iter().take(8) {
                    ui.label(RichText::new(format!("  • {c}")).small());
                }
            }
            if !s.changed_files.is_empty() {
                let list: Vec<&str> = s.changed_files.iter().take(6).map(|x| x.as_str()).collect();
                let more = if s.changed_files.len() > 6 { format!(" and {} more", s.changed_files.len() - 6) } else { String::new() };
                ui.label(RichText::new(format!("✎ Source files changed since this build: {}{more}", list.join(", "))).color(OK));
            }
            if !s.available() && s.source.is_some() {
                ui.label(RichText::new(if s.git { "✔ Up to date with git." } else { "✔ Up to date with its source folder." }).color(OK));
            }
            if let Some(n) = &s.note {
                ui.label(RichText::new(n).small().color(AMBER));
            }
        }
        ui.horizontal_wrapped(|ui| {
            let busy = checking || self.update_job.as_ref().is_some_and(|j| j.running());
            if ui.add_enabled(!busy, egui::Button::new("🔍 Check for updates")).clicked() {
                self.check_updates(ui.ctx(), true);
            }
            if checking {
                ui.spinner();
            }
            let can = status.as_ref().is_some_and(|s| s.source.is_some());
            let label = if status.as_ref().is_some_and(|s| s.available()) { "⬆ Update now" } else { "Rebuild" };
            let tip = "Pull the new commits (if any), build InCloud again and replace this program; then restart it";
            if ui.add_enabled(can && !busy, egui::Button::new(RichText::new(label).strong())).on_hover_text(tip).clicked() {
                let pull = status.as_ref().is_some_and(|s| !s.new_commits.is_empty());
                self.update_job = Some(updater::Job::start(pull));
            }
        });
        if ui.checkbox(&mut self.update_at_start, "Look for updates when InCloud starts").changed() {
            self.settings_dirty = true;
        }
        if let Some(j) = job {
            ui.add_space(6.0);
            egui::Frame::new().fill(ui.visuals().extreme_bg_color).corner_radius(8.0).inner_margin(10).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let now = match &j.step {
                        Step::Pull => 1,
                        Step::Build => 2,
                        Step::Install => 3,
                        _ => 4,
                    };
                    for (k, name) in ["Get the changes", "Build", "Install"].iter().enumerate() {
                        let t = if k + 1 < now {
                            RichText::new(format!("✓ {name}")).color(OK)
                        } else if k + 1 == now {
                            RichText::new(format!("▶ {name}")).strong().color(ACCENT)
                        } else {
                            RichText::new(*name).weak()
                        };
                        ui.label(t);
                    }
                    ui.label(RichText::new(fmt_duration(j.started.elapsed())).weak());
                });
                match &j.step {
                    Step::Failed(e) => {
                        ui.label(RichText::new(e).color(BAD));
                    }
                    Step::Done => {
                        ui.label(RichText::new("✔ InCloud is updated. Restart it to use the new version.").color(OK).strong());
                        if let Some(exe) = j.exe.clone() {
                            if ui.button(RichText::new("↻ Restart InCloud").strong()).clicked() {
                                match updater::restart(&exe) {
                                    Ok(()) => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                                    Err(e) => self.fail(format!("Cannot restart: {e}"), None),
                                }
                            }
                        }
                    }
                    _ => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Building takes a few minutes; you can keep working meanwhile.");
                        });
                        if ui.button("Stop").clicked() {
                            if let Some(job) = &self.update_job {
                                job.cancel();
                            }
                        }
                    }
                }
                if !j.log.is_empty() {
                    egui::CollapsingHeader::new("Output").id_salt("update_log").default_open(matches!(j.step, Step::Failed(_))).show(ui, |ui| {
                        egui::ScrollArea::vertical().max_height(160.0).stick_to_bottom(true).show(ui, |ui| {
                            for l in &j.log {
                                ui.label(RichText::new(l).monospace().small());
                            }
                        });
                    });
                }
            });
        }
    }

    // ------------------------------------------------------------------ batches of runs

    /// The grid of models and the scripts that run them in parallel.
    fn batch_ui(&mut self, ui: &mut egui::Ui) {
        use crate::batch::{self, Scheduler, Spacing};
        // a first template, CLOUDY and folders from what InCloud already knows
        if self.batch.template.is_empty() {
            let deck = self.deck_text();
            let prefix = self.params.file_prefix();
            self.batch.template_from(&deck, &prefix);
            self.batch.extra_files = self.params.extra_files().unwrap_or_default();
        }
        if self.batch.exe.is_empty() {
            self.batch.exe = self.cloudy_exe.trim().to_string();
            if self.batch.data_path.is_empty() {
                self.batch.data_path = self.data_path.trim().to_string();
            }
        }
        if self.batch.out_dir.is_empty() {
            self.batch.out_dir = home().join("cloudy_grid").display().to_string();
        }
        let models = self.batch.models();
        let problems = match &models {
            Ok(m) => self.batch.problems(m),
            Err(e) => vec![(true, e.clone())],
        };
        let count = models.as_ref().map(|m| m.len()).unwrap_or(0);

        // the preview on the right
        let wide = ui.available_width() > 1100.0;
        if wide {
            let width = (ui.available_width() * 0.44).min(820.0);
            egui::Panel::right("batch_preview").exact_size(width).resizable(false).show(ui, |ui| {
                self.batch_preview(ui, &models, &problems);
            });
        }
        let mut list = egui::ScrollArea::vertical().id_salt("batch_list").auto_shrink([false, false]);
        if let (true, Some(y)) = (self.screenshot.is_some(), std::env::var("INCLOUD_SCROLL").ok().and_then(|v| v.parse::<f32>().ok())) {
            list = list.vertical_scroll_offset(y);
        }
        list.show(ui, |ui| {
            ui.set_max_width(ui.available_width() - 16.0);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Grids of models").size(22.0).strong());
                help(ui, "batch");
            });
            ui.label(RichText::new("Choose the parameters and their values: InCloud writes one input file per model and shell scripts that run them with CLOUDY, several at a time, on this computer or on a server.").weak());
            ui.add_space(8.0);

            // 1. the parameters
            card(ui, "1. Parameters", "batch", |ui| {
                let mut remove = None;
                let mut placed = vec![];
                for (i, p) in self.batch.params.iter_mut().enumerate() {
                    egui::Frame::new().stroke(ui.visuals().widgets.noninteractive.bg_stroke).corner_radius(8.0).inner_margin(8).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("{").monospace());
                            ui.add_space(-ui.spacing().item_spacing.x);
                            ui.add(egui::TextEdit::singleline(&mut p.key).desired_width(70.0).font(TextStyle::Monospace)).on_hover_text("The name of the placeholder, used in the input file and in the model names");
                            ui.add_space(-ui.spacing().item_spacing.x);
                            ui.label(RichText::new("}").monospace());
                            ui.label("in");
                            if ui.add(egui::TextEdit::singleline(&mut p.command).desired_width(ui.available_width() - 40.0).font(TextStyle::Monospace)).on_hover_text("The command written in each input file").lost_focus() {
                                placed.push(i);
                            }
                            if ui.small_button("✕").on_hover_text("Remove this parameter").clicked() {
                                remove = Some(i);
                            }
                        });
                        ui.horizontal_wrapped(|ui| {
                            egui::ComboBox::from_id_salt(("spacing", i)).selected_text(p.spacing.label()).width(160.0).show_ui(ui, |ui| {
                                for s in Spacing::ALL {
                                    ui.selectable_value(&mut p.spacing, s, s.label());
                                }
                            });
                            match p.spacing {
                                Spacing::List => {
                                    ui.add(egui::TextEdit::singleline(&mut p.list).hint_text("-3, -2.5, -1").desired_width(260.0));
                                }
                                Spacing::Step | Spacing::Dex => {
                                    ui.label("from");
                                    ui.add(egui::DragValue::new(&mut p.start).speed(0.05).max_decimals(6));
                                    ui.label("to");
                                    ui.add(egui::DragValue::new(&mut p.stop).speed(0.05).max_decimals(6));
                                    ui.label(if p.spacing == Spacing::Dex { "step [dex]" } else { "step" });
                                    ui.add(egui::DragValue::new(&mut p.step).speed(0.01).max_decimals(6));
                                }
                            }
                            ui.label("decimals");
                            ui.add(egui::DragValue::new(&mut p.decimals).range(0..=6));
                        });
                        match p.values() {
                            Ok(v) => {
                                let shown: Vec<String> = v.iter().take(12).map(|x| p.format(*x)).collect();
                                let more = if v.len() > 12 { format!(" … ({} values)", v.len()) } else { format!("  ({} value{})", v.len(), if v.len() == 1 { "" } else { "s" }) };
                                ui.label(RichText::new(format!("{}{more}", shown.join(", "))).small().weak());
                            }
                            Err(e) => {
                                ui.label(RichText::new(format!("✘ {e}")).small().color(BAD));
                            }
                        }
                    });
                    ui.add_space(4.0);
                }
                if let Some(i) = remove {
                    self.batch.params.remove(i);
                }
                for i in placed {
                    if i < self.batch.params.len() {
                        self.batch.place(i);
                    }
                }
                ui.horizontal(|ui| {
                    ui.menu_button("➕ Add a parameter", |ui| {
                        for (k, pr) in batch::PRESETS.iter().enumerate() {
                            if ui.button(pr.0).clicked() {
                                let mut p = batch::preset(k);
                                while self.batch.params.iter().any(|q| q.key == p.key) {
                                    let old = format!("{{{}}}", p.key);
                                    p.key.push('2');
                                    p.command = p.command.replace(&old, &format!("{{{}}}", p.key));
                                }
                                self.batch.params.push(p);
                                let i = self.batch.params.len() - 1;
                                self.batch.place(i);
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui.button("Free parameter: any command with a {placeholder}").clicked() {
                            let n = self.batch.params.len() + 1;
                            self.batch.params.push(batch::free(n));
                            let i = self.batch.params.len() - 1;
                            self.batch.place(i);
                            ui.close();
                        }
                    });
                    ui.label(RichText::new(format!("= {count} model{}", if count == 1 { "" } else { "s" })).strong().size(17.0).color(if count > 0 { OK } else { BAD }));
                });
            });

            // 2. the input file
            card(ui, "2. Input file of every model", "batch", |ui| {
                ui.label(RichText::new("Each {placeholder} becomes the value of its parameter, and {name} the name of the model. Write any command here, as in the Run tab.").small().weak());
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Start from the current model").on_hover_text("The model set on the left of the Simulation tab, with the parameters above put in").clicked() {
                        let deck = self.deck_text();
                        let prefix = self.params.file_prefix();
                        self.batch.template_from(&deck, &prefix);
                        self.batch.extra_files = self.params.extra_files().unwrap_or_default();
                    }
                    if ui.button("Load an input file…").on_hover_text("For example a template you already use; the parameters above are put in where their commands are").clicked() {
                        self.pick = Pick::BatchTemplate;
                        self.dialog.pick_file();
                    }
                    if ui.button("Put the parameters in").on_hover_text("Replace the line of each parameter with its command, or add it").clicked() {
                        for i in 0..self.batch.params.len() {
                            self.batch.place(i);
                        }
                    }
                });
                egui::ScrollArea::vertical().id_salt("batch_template").max_height(380.0).show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut self.batch.template).font(TextStyle::Monospace).desired_width(f32::INFINITY).desired_rows(14));
                });
                if !self.batch.extra_files.is_empty() {
                    let names: Vec<&str> = self.batch.extra_files.iter().map(|f| f.0.as_str()).collect();
                    ui.label(RichText::new(format!("Written beside the models too: {}", names.join(", "))).small().weak());
                }
            });

            // 3. names
            card(ui, "3. Model names", "batch", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut self.batch.name_pattern).font(TextStyle::Monospace).desired_width(420.0));
                    let keys: Vec<String> = self.batch.params.iter().map(|p| p.key.clone()).collect();
                    if ui.button("All parameters").on_hover_text("A name with every parameter").clicked() {
                        let base = self.batch.name_pattern.split('_').next().filter(|b| !b.contains('{') && !b.is_empty()).unwrap_or("model").to_string();
                        self.batch.name_pattern = std::iter::once(base).chain(keys.iter().map(|k| format!("{k}{{{k}}}"))).collect::<Vec<_>>().join("_");
                    }
                });
                if let Ok(m) = &models {
                    let names: Vec<&str> = m.iter().take(3).map(|x| x.name.as_str()).collect();
                    ui.label(RichText::new(format!("{}{}", names.join(",  "), if m.len() > 3 { ",  …" } else { "" })).monospace().small().weak());
                }
            });

            // 4. where and how
            card(ui, "4. Where and how they run", "batch", |ui| {
                egui::Grid::new("batch_where").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                    ui.label("CLOUDY program");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.batch.exe).hint_text("/path/to/c25.00/source/cloudy.exe").desired_width(470.0).font(TextStyle::Monospace));
                        if ui.button("This computer's").on_hover_text(format!("The CLOUDY InCloud uses: {}", self.cloudy_exe)).clicked() {
                            self.batch.exe = self.cloudy_exe.trim().to_string();
                            self.batch.data_path = self.data_path.trim().to_string();
                        }
                    });
                    ui.end_row();
                    ui.label("");
                    note(ui, "On a server, write where CLOUDY is there (for example /home/software/astro/cloudy/c22.02/source/cloudy.exe): it isn't checked from here.");
                    ui.end_row();
                    ui.label("Data folder");
                    ui.add(egui::TextEdit::singleline(&mut self.batch.data_path).hint_text("empty: CLOUDY's own (compiled in)").desired_width(470.0).font(TextStyle::Monospace));
                    ui.end_row();
                    ui.label("Write the files to");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.batch.out_dir).desired_width(470.0).font(TextStyle::Monospace));
                        if ui.button("📁").on_hover_text("Choose the folder").clicked() {
                            self.pick = Pick::BatchOut;
                            self.dialog.pick_directory();
                        }
                    });
                    ui.end_row();
                    ui.label("Run them in");
                    ui.add(egui::TextEdit::singleline(&mut self.batch.run_dir).hint_text("empty: the same folder (runs on this computer)").desired_width(470.0).font(TextStyle::Monospace));
                    ui.end_row();
                    ui.label("");
                    note(ui, "The folder where the models run and write their output, on the computer that runs them: for a server, where you copy the files.");
                    ui.end_row();
                    ui.label("At the same time");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut self.batch.parallel).range(1..=512).suffix(" runs"));
                        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
                        ui.label(RichText::new(format!("per script (this computer has {cores} cores)")).small().weak());
                    });
                    ui.end_row();
                    ui.label("Scripts");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut self.batch.scripts).range(1..=1000));
                        let n = self.batch.scripts.clamp(1, count.max(1));
                        let per: Vec<String> = (0..n.min(6)).map(|k| batch::Batch::share(count, n, k).len().to_string()).collect();
                        ui.label(RichText::new(format!("the models are shared out: {}{}", per.join(" + "), if n > 6 { " + …" } else { "" })).small().weak());
                    });
                    ui.end_row();
                    ui.label("CLOUDY is called");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.batch.use_r, true, RichText::new("cloudy.exe -r name").monospace());
                        ui.radio_value(&mut self.batch.use_r, false, RichText::new("cloudy.exe < name.in > name.out").monospace());
                    });
                    ui.end_row();
                    ui.label("Options");
                    ui.vertical(|ui| {
                        ui.checkbox(&mut self.batch.skip_done, "Skip the models already finished").on_hover_text("A script started again after a stop carries on where it was");
                        ui.checkbox(&mut self.batch.embed, "Put the input files inside the scripts").on_hover_text("Then only the scripts need copying to the server");
                    });
                    ui.end_row();
                    ui.label("Started by");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.batch.scheduler, Scheduler::Bash, "bash");
                        ui.radio_value(&mut self.batch.scheduler, Scheduler::Slurm, "SLURM (sbatch)");
                    });
                    ui.end_row();
                    if self.batch.scheduler == Scheduler::Slurm {
                        for (label, field, hint) in [
                            ("Partition", &mut self.batch.slurm_partition, "e.g. long1"),
                            ("Account", &mut self.batch.slurm_account, "e.g. astro"),
                            ("Time limit", &mut self.batch.slurm_time, "hh:mm:ss"),
                            ("Memory", &mut self.batch.slurm_mem, "e.g. 250000 (MB) or 64G"),
                            ("Job name", &mut self.batch.slurm_job, "cloudy"),
                        ] {
                            ui.label(label);
                            ui.add(egui::TextEdit::singleline(field).hint_text(hint).desired_width(240.0));
                            ui.end_row();
                        }
                    }
                    ui.label("More header lines");
                    ui.add(egui::TextEdit::multiline(&mut self.batch.header).hint_text("#SBATCH --constraint=ib\nmodule load gcc").desired_rows(2).desired_width(470.0).font(TextStyle::Monospace));
                    ui.end_row();
                });
            });

            if !wide {
                card(ui, "Preview", "batch", |ui| self.batch_preview(ui, &models, &problems));
            }
        });
    }

    /// What will be written: problems, the button, a script and an input file.
    fn batch_preview(&mut self, ui: &mut egui::Ui, models: &Result<Vec<crate::batch::Model>, String>, problems: &[(bool, String)]) {
        ui.add_space(6.0);
        let errors = problems.iter().filter(|p| p.0).count();
        let count = models.as_ref().map(|m| m.len()).unwrap_or(0);
        let n = self.batch.scripts.clamp(1, count.max(1));
        ui.label(RichText::new(format!("{count} models · {n} script{} · {} at a time in each", if n == 1 { "" } else { "s" }, self.batch.parallel.max(1))).strong().size(17.0));
        for (err, t) in problems.iter().take(8) {
            ui.add(egui::Label::new(RichText::new(format!("{} {t}", if *err { "✘" } else { "•" })).small().color(if *err { BAD } else { AMBER })).wrap());
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let b = egui::Button::new(RichText::new(format!("💾 Write {count} input files and the scripts")).strong().color(Color32::WHITE)).fill(ACCENT).min_size(egui::vec2(0.0, 34.0));
            if ui.add_enabled(errors == 0 && count > 0, b).on_disabled_hover_text("Fix what is marked ✘ first").clicked() {
                match self.batch.write() {
                    Ok(s) => self.info(format!("{s} Start them with start_all.sh.")),
                    Err(e) => self.fail(e, None),
                }
            }
        });
        ui.label(RichText::new(format!("Into {}: {}{}, start_all.sh (starts {}) and models.csv (the parameters of each model).", self.batch.out_dir, if self.batch.embed { "" } else { "the .in files, " }, if n == 1 { "batch_1.sh".to_string() } else { format!("batch_1.sh … batch_{n}.sh") }, if n == 1 { "it" } else { "them all" })).small().weak());
        ui.add_space(6.0);
        let Ok(models) = models else { return };
        if models.is_empty() {
            return;
        }
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.batch_tab, 0, "Script");
            ui.selectable_value(&mut self.batch_tab, 1, "First input file");
            ui.selectable_value(&mut self.batch_tab, 2, "All models");
            if self.batch_tab == 0 && n > 1 {
                ui.add(egui::DragValue::new(&mut self.batch_script).range(1..=n).prefix("batch_").suffix(".sh"));
            }
        });
        let text = match self.batch_tab {
            0 => {
                let mut s = self.batch.script(models, self.batch_script.clamp(1, n) - 1, n);
                // a long list of models is cut for the preview
                if s.lines().count() > 400 {
                    let lines: Vec<&str> = s.lines().collect();
                    s = format!("{}\n  …\n{}", lines[..200].join("\n"), lines[lines.len() - 120..].join("\n"));
                }
                s
            }
            1 => models[0].input.clone(),
            _ => {
                let t = self.batch.table(models);
                t.lines().take(500).collect::<Vec<_>>().join("\n")
            }
        };
        egui::ScrollArea::both().id_salt("batch_text").auto_shrink([false, false]).max_height(ui.available_height().max(300.0)).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(text).monospace().size(12.5)).extend());
        });
    }

    /// The first start: CLOUDY found, set by hand, installed, or left for later.
    fn welcome_window(&mut self, ctx: &egui::Context) {
        use crate::welcome::Choice;
        let installs = self.installs.clone();
        let close = match self.welcome.show(ctx, &installs, self.logo.as_ref()) {
            Choice::None => false,
            Choice::Use(exe, data) => {
                self.cloudy_exe = exe.display().to_string();
                self.data_path = data;
                self.installs = runner::find_installs();
                self.info(format!("CLOUDY {} is ready. Set the model on the left and press “Run CLOUDY”.", runner::version_of(&self.cloudy_exe)));
                true
            }
            Choice::Install => {
                self.versions_open = true;
                true
            }
            Choice::Demo => {
                self.load_demo();
                true
            }
            Choice::PickExe => {
                self.pick = Pick::WelcomeExe;
                self.dialog.pick_file();
                false
            }
            Choice::PickData => {
                self.pick = Pick::WelcomeData;
                self.dialog.pick_directory();
                false
            }
        };
        if close {
            self.welcome.open = false;
            self.welcomed = true;
            self.save_settings();
        }
    }

    fn welcome_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(ui.available_height() * 0.16);
        ui.vertical_centered(|ui| {
            if let Some(t) = &self.logo {
                ui.add(egui::Image::new(egui::load::SizedTexture::new(t.id(), egui::vec2(128.0, 128.0))));
            }
            ui.label(RichText::new("InCloud").size(40.0).strong().color(ACCENT));
            ui.add_space(8.0);
            ui.label(RichText::new("Set the model on the left and press “Run CLOUDY”.").size(18.0));
            ui.label(RichText::new("The plots you choose in the Output tab appear here when the run ends.").weak());
            ui.add_space(16.0);
            if ui.add(egui::Button::new(RichText::new("Try the demo data").size(17.0)).min_size(egui::vec2(240.0, 42.0))).clicked() {
                self.load_demo();
            }
        });
    }

    fn export_menu(&mut self, ui: &mut egui::Ui, default_prefix: &str, out: &mut ExportRequest) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Plots").strong());
            help(ui, "export");
        });
        ui.horizontal_wrapped(|ui| {
            for f in Format::ALL {
                ui.radio_value(&mut self.export_fmt, f, f.label());
            }
        });
        egui::CollapsingHeader::new("Size and style").id_salt("exp_style").show(ui, |ui| {
            ui.add(egui::Slider::new(&mut self.style.width_in, 3.0..=12.0).text("width [in]"));
            ui.add(egui::Slider::new(&mut self.style.height_in, 2.0..=10.0).text("height [in]"));
            ui.add(egui::Slider::new(&mut self.style.font_pt, 7.0..=16.0).text("font [pt]"));
            ui.add(egui::Slider::new(&mut self.style.dpi, 72.0..=600.0).text("DPI (PNG)"));
            ui.checkbox(&mut self.style.serif, "Serif font");
            ui.checkbox(&mut self.style.title, "Title above the plot");
        });
        ui.horizontal(|ui| {
            ui.label("File prefix");
            ui.add(egui::TextEdit::singleline(&mut self.file_prefix).hint_text(default_prefix).desired_width(200.0)).on_hover_text("Every saved file name starts with this (default: the model name)");
        });
        ui.horizontal(|ui| {
            ui.label("Folder");
            ui.add(egui::TextEdit::singleline(&mut self.export_dir).desired_width(230.0));
            if ui.button("📁").on_hover_text("Choose folder").clicked() {
                out.pick_folder = true;
            }
        });
        ui.checkbox(&mut self.export_zoom, "Save only the zoomed area of zoomed plots");
        ui.horizontal(|ui| {
            if let View::One(p) = self.view {
                if ui.button("Save this plot").clicked() {
                    out.plots = vec![p];
                }
            }
            if ui.button("Save all shown plots").clicked() {
                out.plots = self.plots.selected.clone();
            }
        });
        ui.separator();
        ui.label(RichText::new("Data tables").strong());
        egui::Grid::new("datasets").num_columns(2).show(ui, |ui| {
            for (k, d) in DataSet::ALL.iter().enumerate() {
                let mut on = self.data_sets.contains(d);
                if ui.checkbox(&mut on, d.label()).changed() {
                    if on {
                        self.data_sets.push(*d);
                    } else {
                        self.data_sets.retain(|x| x != d);
                    }
                }
                if k % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.data_tsv, false, "CSV");
            ui.radio_value(&mut self.data_tsv, true, "Text (tab-separated)");
            if ui.button("Save data").clicked() {
                out.data = true;
            }
        });
        if self.results_temporary {
            ui.separator();
            if ui.button("Keep this temporary run…").on_hover_text("Copy its files to a folder").clicked() {
                out.keep_run = true;
            }
        }
    }

    fn results_ui(&mut self, ui: &mut egui::Ui) {
        let Some(r) = self.results.as_ref() else { return };
        let default_prefix = r.prefix.clone();
        let mut open_messages = false;
        // ---- summary
        ui.horizontal_wrapped(|ui| {
            let title = if r.deck.title.is_empty() { r.prefix.clone() } else { r.deck.title.clone() };
            ui.label(RichText::new(title).strong().size(19.0));
            if r.demo {
                ui.label(RichText::new(" demo data (synthetic) ").background_color(Color32::from_rgb(250, 230, 170)).color(Color32::BLACK).size(13.0));
            }
            let fmt_front = |name: &str, d: Option<f64>| -> String {
                match d {
                    Some(d) => match r.av_at(d) {
                        Some(av) => format!("{name} {d:.2e} cm · A_V {av:.2}"),
                        None => format!("{name} {d:.2e} cm"),
                    },
                    None => format!("{name} —"),
                }
            };
            if !r.info_only {
                chip(ui, &fmt_front("IF", r.fronts.ionisation), crate::figures::role(crate::figures::Role::RegionHii), "Ionisation front: where H⁺ falls below 50%");
                chip(ui, &fmt_front("DF", r.fronts.dissociation), crate::figures::role(crate::figures::Role::RegionPdr), "Dissociation front: where 2H₂/H rises above 50%");
            }
            if let Some(t) = r.mean_te_hii() {
                chip(ui, &format!("⟨Tₑ⟩ H II {t:.0} K"), [210, 100, 120], "Mean electron temperature of the ionised gas");
            }
            if let Some(d) = r.depth.last() {
                chip(ui, &format!("{d:.2e} cm · {} zones", r.depth.len()), [150, 150, 150], "Total thickness and number of zones");
            }
            if r.info.present && !r.info.complete && !r.demo {
                ui.label(RichText::new("CLOUDY stopped before the end").color(BAD));
            }
            let (warn, caution) = (r.info.count(crate::runinfo::Level::Warning), r.info.count(crate::runinfo::Level::Caution));
            if warn + caution > 0 {
                let col = if warn > 0 { BAD } else { AMBER };
                if ui.button(RichText::new(format!("⚠ {warn} warnings · {caution} cautions")).color(col)).on_hover_text("Read them in Run info").clicked() {
                    open_messages = true;
                }
            }
        });
        if open_messages {
            self.view = View::Info;
            self.info.show("messages");
        }
        ui.add_space(4.0);

        let line_opts = self.line_options();
        let col_opts = self.column_options();

        // ---- tabs
        ui.horizontal_wrapped(|ui| {
            let only_text = self.results.as_ref().map(|r| r.info_only).unwrap_or(false);
            ui.selectable_value(&mut self.view, View::Info, RichText::new("ℹ Run info").strong());
            if only_text {
                self.view = View::Info;
                ui.label(RichText::new("No plots: this run has no saved overview (.ovr).").small().weak());
                return;
            }
            ui.selectable_value(&mut self.view, View::Dashboard, RichText::new("Dashboard").strong());
            if self.plots.selected.len() <= 5 {
                for p in self.plots.selected.clone() {
                    ui.selectable_value(&mut self.view, View::One(p), p.label(&self.plots.customs));
                }
            } else {
                // many plots: one dropdown instead of a long row of tabs
                let current = match self.view {
                    View::One(p) => p.label(&self.plots.customs),
                    _ => "One plot…".to_string(),
                };
                egui::ComboBox::from_id_salt("oneplot").selected_text(current).width(220.0).height(420.0).show_ui(ui, |ui| {
                    for p in self.plots.selected.clone() {
                        ui.selectable_value(&mut self.view, View::One(p), p.label(&self.plots.customs));
                    }
                });
            }
            ui.toggle_value(&mut self.plots_open, RichText::new("+ Plots").strong());
        });
        if let View::One(p) = self.view {
            if !self.plots.selected.contains(&p) {
                self.view = View::Dashboard;
            }
        }
        if self.view == View::Info {
            let r = self.results.as_ref().unwrap();
            match crate::infoview::show(ui, r, &mut self.info) {
                crate::infoview::Action::None => {}
                crate::infoview::Action::EditInput => {
                    self.editor.load(r.input_text.clone());
                    self.left_tab = LeftTab::Run;
                }
                crate::infoview::Action::OpenRun(name) => {
                    let dir = r.dir.clone();
                    self.open_named_run(&dir, Some(&name));
                }
            }
            return;
        }
        let shown: Vec<Plot> = match self.view {
            View::Dashboard => self.plots.selected.clone(),
            View::One(p) => vec![p],
            View::Info => vec![],
        };

        // ---- toolbar
        let mut req = ExportRequest::default();
        egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(8.0).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                // only the controls that mean something for the plots on screen
                let table_only = !shown.is_empty() && shown.iter().all(|p| (self.rates_table && *p == Plot::Rates) || (self.abund_table && crate::abund::is_abundance(*p)));
                let depth = !table_only && shown.iter().any(|p| p.uses_depth());
                let sed = shown.contains(&Plot::Continuum);
                if depth {
                    ui.label("x:");
                    egui::ComboBox::from_id_salt("xaxis").selected_text(self.xaxis.label()).show_ui(ui, |ui| {
                        for x in XAxis::ALL {
                            ui.selectable_value(&mut self.xaxis, x, x.label());
                        }
                    });
                }
                if sed {
                    ui.label(if depth { "SED x:" } else { "x:" });
                    egui::ComboBox::from_id_salt("contunit").selected_text(self.cont_unit.label()).show_ui(ui, |ui| {
                        for u in ContUnit::ALL {
                            ui.selectable_value(&mut self.cont_unit, u, u.label());
                        }
                    });
                    let has_lines = self.results.as_ref().is_some_and(|r| !r.lines.is_empty());
                    ui.add_enabled(has_lines, egui::Checkbox::new(&mut self.sed_lines, "Saved lines")).on_hover_text("Each saved line as a thin spike standing on the total continuum. Lines too faint to rise above it are left out").on_disabled_hover_text("This run saved no lines");
                }
                if depth {
                    ui.checkbox(&mut self.show_regions, "Regions");
                }
                if !table_only && shown.iter().any(|p| *p != Plot::Network) {
                    if ui.checkbox(&mut self.show_grid, "Grid").changed() {
                        self.save_settings();
                    }
                }
                let r = self.results.as_ref().unwrap();
                if shown.contains(&Plot::Ions) {
                    ui.separator();
                    ui.label("Element");
                    egui::ComboBox::from_id_salt("element").selected_text(self.element.clone()).width(60.0).show_ui(ui, |ui| {
                        // the elements this run saved, or the usual ones
                        let saved: Vec<String> = r.elements.iter().map(|e| e.0.clone()).collect();
                        let list: Vec<String> = if saved.is_empty() { ELEMENTS.iter().map(|e| e.0.to_string()).collect() } else { saved };
                        for sym in list {
                            ui.selectable_value(&mut self.element, sym.clone(), sym);
                        }
                    });
                }
                if shown.contains(&Plot::Rates) {
                    ui.separator();
                    ui.label("Species");
                    egui::ComboBox::from_id_salt("species").selected_text(markup_to_unicode(&ion_markup(&self.species))).width(80.0).show_ui(ui, |ui| {
                        for c in &r.chemistry {
                            ui.selectable_value(&mut self.species, c.species.clone(), markup_to_unicode(&ion_markup(&c.species)));
                        }
                    });
                    ui.toggle_value(&mut self.net_open, "+ Species").on_hover_text("Choose other species");
                    ui.selectable_value(&mut self.rates_table, false, "Plot");
                    ui.selectable_value(&mut self.rates_table, true, "Table");
                }
                let abundances = shown.iter().any(|p| crate::abund::is_abundance(*p));
                if abundances {
                    ui.separator();
                    ui.label("Abundances");
                    ui.selectable_value(&mut self.abund_table, false, "Plot");
                    ui.selectable_value(&mut self.abund_table, true, "Table");
                }
                if shown.contains(&Plot::Network) {
                    ui.separator();
                    let names: Vec<String> = self.net_species.iter().map(|s| markup_to_unicode(&ion_markup(s))).collect();
                    let label = if names.len() <= 3 { names.join(", ") } else { format!("{} species", names.len()) };
                    ui.toggle_value(&mut self.net_open, format!("Network: {label}"));
                }
                if shown.contains(&Plot::Network) || (shown.contains(&Plot::Rates) && self.rates_table) || (abundances && self.abund_table) {
                    egui::ComboBox::from_id_salt("zone").selected_text(self.zone.label()).show_ui(ui, |ui| {
                        for z in Zone::ALL {
                            ui.selectable_value(&mut self.zone, z, z.label());
                        }
                    });
                }
                if shown.contains(&Plot::Bpt) {
                    ui.separator();
                    ui.label("BPT");
                    egui::ComboBox::from_id_salt("bptk").selected_text(self.plots.bpt.label()).show_ui(ui, |ui| {
                        for k in BptKind::ALL {
                            ui.selectable_value(&mut self.plots.bpt, k, k.label());
                        }
                    });
                    ui.toggle_value(&mut self.runs_open, format!("Runs on diagram ({})", self.bpt_runs.iter().filter(|r| r.visible).count() + 1));
                }
                ui.separator();
                ui.toggle_value(&mut self.export_open, RichText::new("⬇ Export").strong());
                if shown.iter().any(|p| self.views.get(&p.key()).map(|v| v.zoomed).unwrap_or(false)) {
                    if ui.button("Reset zoom").clicked() {
                        for p in &shown {
                            if let Some(v) = self.views.get_mut(&p.key()) {
                                v.reset = true;
                            }
                        }
                    }
                }
                // the hint takes only the room that is left, so the plots keep their place and width
                let room = ui.available_size_before_wrap().x - 44.0;
                let weak = ui.visuals().weak_text_color();
                let font = TextStyle::Small.resolve(ui.style());
                let hint = ["drag: zoom in · − / Ctrl+wheel: zoom out · Ctrl+Z: back · right-drag: move", "drag: zoom · −: zoom out · Ctrl+Z: back", "Ctrl+Z: back"].into_iter().find(|h| ui.painter().layout_no_wrap(h.to_string(), font.clone(), weak).size().x <= room);
                if let Some(h) = hint {
                    ui.label(RichText::new(h).small().weak());
                }
                help(ui, "zoom");
            });
        });
        ui.add_space(6.0);

        let r = self.results.as_ref().unwrap();
        let ctx = Ctx {
            r,
            xaxis: self.xaxis,
            element: &self.element,
            species: &self.species,
            show_regions: self.show_regions,
            ratios: &self.plots.ratios,
            customs: &self.plots.customs,
            bpt: self.plots.bpt,
            others: &self.bpt_runs,
            cont_unit: self.cont_unit,
            sed_lines: self.sed_lines,
        };
        let mut enlarge: Option<Plot> = None;
        let mut remove: Option<Plot> = None;
        let customs = self.plots.customs.clone();
        let zone = self.zone;
        let net = self.net_species.clone();
        let rates_table = self.rates_table;
        let abund_table = self.abund_table;
        let grid = self.show_grid;
        let gen = self.plot_gen;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.view {
            View::Dashboard => {
                if shown.is_empty() {
                    ui.add_space(40.0);
                    ui.vertical_centered(|ui| ui.label(RichText::new("No plots chosen: use “+ Plots” above.").size(16.0).weak()));
                    return;
                }
                let cols = if ui.available_width() > 1100.0 { 2 } else { 1 };
                let gap = 16.0;
                // leave room for the scroll bar, so that nothing sits under it
                let room = ui.available_width() - ui.spacing().scroll.bar_width - ui.spacing().scroll.bar_outer_margin - 10.0;
                let w = (room - gap * (cols as f32 - 1.0)) / cols as f32;
                let h = ((ui.available_height() - 60.0) / 2.0).clamp(280.0, 460.0);
                egui::Grid::new("dashboard").num_columns(cols).spacing([gap, gap]).show(ui, |ui| {
                    for (i, p) in shown.iter().enumerate() {
                        ui.vertical(|ui| {
                            ui.set_width(w);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(p.label(&customs)).strong().size(16.0));
                                help(ui, p.wiki_key());
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("✕").on_hover_text("Hide this plot").clicked() {
                                        remove = Some(*p);
                                    }
                                    if ui.small_button("⬇").on_hover_text("Save this plot (format and folder in Export)").clicked() {
                                        req.plots = vec![*p];
                                    }
                                    if ui.small_button("⛶").on_hover_text("Enlarge").clicked() {
                                        enlarge = Some(*p);
                                    }
                                });
                            });
                            show_plot(ui, &ctx, *p, &net, rates_table, abund_table, zone, gen, h, w, grid, &mut self.views);
                        });
                        if (i + 1) % cols == 0 {
                            ui.end_row();
                        }
                    }
                });
            }
            View::One(p) => {
                let f = ctx.figure(p);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(markup_to_unicode(&f.title)).strong().size(18.0));
                    help(ui, p.wiki_key());
                });
                let h = (ui.available_height() - 30.0).max(380.0);
                let w = ui.available_width();
                show_plot(ui, &ctx, p, &net, rates_table, abund_table, zone, gen, h, w, grid, &mut self.views);
            }
            View::Info => {}
        });
        if let Some(p) = enlarge {
            self.view = View::One(p);
        }
        if let Some(p) = remove {
            self.plots.selected.retain(|x| *x != p);
        }

        let ectx = ui.ctx().clone();
        let mut open = self.plots_open;
        egui::Window::new("Choose plots").open(&mut open).default_width(480.0).default_height(540.0).vscroll(true).show(&ectx, |ui| {
            plots_ui(ui, &mut self.plots, &line_opts, &col_opts, "win");
        });
        self.plots_open = open;
        let mut open = self.export_open;
        egui::Window::new("Export").open(&mut open).default_width(430.0).vscroll(true).show(&ectx, |ui| {
            self.export_menu(ui, &default_prefix, &mut req);
        });
        self.export_open = open;
        let mut open = self.net_open;
        let count = |t: &Option<crate::results::Table>| t.as_ref().map(|t| t.cols.len().saturating_sub(1)).unwrap_or(0);
        let chem: Vec<(String, usize)> = self.results.as_ref().map(|r| r.chemistry.iter().map(|c| (c.species.clone(), count(&c.formation) + count(&c.destruction))).collect()).unwrap_or_default();
        let names: Vec<String> = chem.iter().map(|c| c.0.clone()).collect();
        let empty: Vec<String> = self.results.as_ref().map(|r| r.deck.chemistry.iter().map(|c| c.0.clone()).filter(|s| !names.contains(s)).collect()).unwrap_or_default();
        // every species takes two of the save commands CLOUDY allows
        let saves = self.results.as_ref().map(|r| crate::model::deck_saves(&r.input_text)).unwrap_or(0);
        let room = (crate::model::MAX_SAVES.saturating_sub(saves) / 2).min(crate::model::MAX_CHEM_SPECIES);
        let running = self.run.is_some();
        let mut rerun = false;
        egui::Window::new("Species in the network").open(&mut open).default_width(400.0).default_height(560.0).vscroll(true).show(&ectx, |ui| {
            ui.label(RichText::new("Saved in this run").strong());
            if chem.is_empty() {
                ui.label(RichText::new("No reaction rates were saved.").small().weak());
            } else {
                ui.label(RichText::new("Tick two or more to see how they connect.").small().weak());
            }
            ui.horizontal_wrapped(|ui| {
                for (sp, n) in &chem {
                    let mut on = self.net_species.contains(sp);
                    if ui.checkbox(&mut on, markup_to_unicode(&ion_markup(sp))).on_hover_text(format!("{n} reactions")).changed() {
                        if on {
                            self.net_species.push(sp.clone());
                        } else if self.net_species.len() > 1 {
                            self.net_species.retain(|x| x != sp);
                        }
                    }
                }
            });
            if !empty.is_empty() {
                let list: Vec<String> = empty.iter().map(|s| crate::species::pretty(s)).collect();
                ui.label(RichText::new(format!("No reactions in CLOUDY's network: {}", list.join(", "))).small().weak());
            }
            ui.separator();
            ui.label(RichText::new("Other species").strong());
            ui.label(RichText::new("Any species, even if no saved line comes from it. CLOUDY has to compute the model again to save its rates.").small().weak());
            // above the list, so that it is always in view
            if !self.net_more.is_empty() {
                let text = RichText::new(format!("▶  Run again with {} more", self.net_more.len())).strong().color(Color32::WHITE);
                if ui.add_enabled(!running, egui::Button::new(text).fill(ACCENT)).clicked() {
                    rerun = true;
                }
                ui.add_space(4.0);
            }
            crate::species::picker(ui, "net_pick", &mut self.net_more, &self.species_known, &mut self.net_query, room, &names);
        });
        self.net_open = open;
        if rerun {
            self.rerun_with_species();
        }
        let mut open = self.runs_open;
        egui::Window::new("Runs on the BPT diagram").open(&mut open).default_width(420.0).show(&ectx, |ui| {
            ui.label(RichText::new("The current model is always shown.").small().weak());
            let mut del = None;
            let kind = self.plots.bpt;
            egui::ScrollArea::vertical().max_height(300.0).id_salt("bptruns").show(ui, |ui| {
                for (i, run) in self.bpt_runs.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        let missing = crate::figures::bpt_point_run(run, kind).is_none();
                        let (name, source) = (run.name.clone(), run.source.clone());
                        ui.checkbox(&mut run.visible, name).on_hover_text(source);
                        if missing {
                            ui.label(RichText::new("lines missing").small().color(BAD));
                        }
                        if ui.small_button("✕").on_hover_text("Take off the diagram").clicked() {
                            del = Some(i);
                        }
                    });
                }
            });
            if let Some(i) = del {
                self.bpt_runs.remove(i);
            }
            ui.separator();
            if ui.button("Add runs from a folder…").on_hover_text("Reads every CLOUDY .out file in the folder and its sub-folders").clicked() {
                req.add_runs = true;
            }
            if ui.button("Run another model with the current settings").on_hover_text("This model stays on the diagram; change the parameters on the left first").clicked() {
                req.run_again = true;
            }
        });
        self.runs_open = open;

        let prefix = if self.file_prefix.trim().is_empty() { default_prefix.clone() } else { self.file_prefix.trim().to_string() };
        let dir = PathBuf::from(self.export_dir.trim());
        if !req.plots.is_empty() {
            self.style.grid = self.show_grid;
            let r = self.results.as_ref().unwrap();
            let ctx = Ctx {
                r,
                xaxis: self.xaxis,
                element: &self.element,
                species: &self.species,
                show_regions: self.show_regions,
                ratios: &self.plots.ratios,
                customs: &self.plots.customs,
                bpt: self.plots.bpt,
                others: &self.bpt_runs,
                cont_unit: self.cont_unit,
                sed_lines: self.sed_lines,
            };
            let mut written = vec![];
            let mut errors = vec![];
            for p in req.plots {
                let suffix = match p {
                    Plot::Ions => format!("_{}", self.element),
                    Plot::Rates => format!("_{}", crate::model::species_file(&self.species)),
                    Plot::Network => format!("_{}_{:?}", self.net_species.iter().map(|s| crate::model::species_file(s)).collect::<Vec<_>>().join("-"), self.zone).to_ascii_lowercase(),
                    _ => String::new(),
                };
                let stem = format!("{prefix}_{}{suffix}", p.key());
                let res = if p == Plot::Network {
                    export_network(r, &self.net_species, self.zone, self.export_fmt, &self.style, &dir, &stem)
                } else if p == Plot::Rates && self.rates_table {
                    save_rates_table(r, &self.species, self.zone, &dir, &stem)
                } else if self.abund_table && crate::abund::is_abundance(p) {
                    crate::abund::build(&ctx, p, self.zone).and_then(|t| {
                        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
                        let path = dir.join(format!("{stem}_table.csv"));
                        std::fs::write(&path, crate::abund::csv(&t)).map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
                        Ok(path)
                    })
                } else {
                    let mut f = ctx.figure(p);
                    if let Some(v) = self.views.get(&p.key()) {
                        f.series.retain(|se| !v.hidden.contains(&se.name));
                        if let (true, true, Some(b)) = (self.export_zoom, v.zoomed, v.bounds) {
                            f = export::zoomed(&f, b);
                        }
                    }
                    if f.is_empty() {
                        Err(format!("{}: no data", p.label(&self.plots.customs)))
                    } else {
                        export::export(&f, self.export_fmt, &self.style, &dir, &stem)
                    }
                };
                match res {
                    Ok(path) => written.push(path),
                    Err(e) => errors.push(e),
                }
            }
            if errors.is_empty() {
                let what = if written.len() == 1 { written[0].display().to_string() } else { format!("{} files in {}", written.len(), dir.display()) };
                self.info(format!("Saved: {what}"));
                self.save_settings();
            } else {
                self.fail(format!("Saved {} file(s); problems: {}", written.len(), errors.join("; ")), None);
            }
        }
        if req.data {
            let r = self.results.as_ref().unwrap();
            match data::save(r, &self.data_sets, &dir, &prefix, self.data_tsv, &self.plots.ratios) {
                Ok(files) => self.info(format!("Saved {} data file(s) in {}", files.len(), dir.display())),
                Err(e) => self.fail(format!("Could not save the data: {e}"), None),
            }
        }
        if req.pick_folder {
            self.pick = Pick::ExportDir;
            self.dialog.pick_directory();
        }
        if req.keep_run {
            self.pick = Pick::SaveRun;
            self.dialog.pick_directory();
        }
        if req.add_runs {
            self.pick = Pick::AddRuns;
            self.dialog.pick_directory();
        }
        if req.run_again {
            self.start_run();
        }
    }

    fn ensure_library(&mut self, ctx: &egui::Context) {
        if self.library.is_some() || self.library_tried {
            return;
        }
        self.library_tried = true;
        let mut starts = vec![PathBuf::from(self.cloudy_exe.trim())];
        starts.extend(self.data_path.split(':').filter(|p| p.trim().len() > 1).map(|p| PathBuf::from(p.trim())));
        starts.extend(self.installs.iter().map(|i| i.exe.clone()));
        if let Some(d) = pdfview::find_docs(&starts) {
            self.library = Library::open(&d, ctx);
        }
    }

    fn no_manuals_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("The CLOUDY manuals (hazy1.pdf, hazy2.pdf, hazy3.pdf) were not found beside your CLOUDY installation.").weak());
        if ui.button("Choose the folder with the manuals…").clicked() {
            self.pick = Pick::HazyDir;
            self.dialog.pick_directory();
        }
    }

    fn manuals_ui(&mut self, ui: &mut egui::Ui) {
        self.ensure_library(ui.ctx());
        match self.library.as_mut() {
            Some(lib) => lib.ui(ui, &mut self.reader),
            None => {
                ui.add_space(40.0);
                ui.heading("Manuals");
                self.no_manuals_ui(ui);
            }
        }
    }

    fn wiki_ui(&mut self, ui: &mut egui::Ui) {
        self.ensure_library(ui.ctx());
        egui::Panel::left("wiki_topics").default_size(290.0).size_range(210.0..=420.0).show(ui, |ui| {
            ui.add_space(4.0);
            let search = ui.add(egui::TextEdit::singleline(&mut self.wiki_search).hint_text("Search the wiki…  (Ctrl+F)").desired_width(f32::INFINITY));
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::F)) {
                search.request_focus();
            }
            ui.add_space(4.0);
            let q = self.wiki_search.to_lowercase();
            egui::ScrollArea::vertical().id_salt("wiki_list").auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(ui.available_width() - 14.0);
                for group in wiki::GROUPS {
                    let items: Vec<&wiki::Topic> = wiki::TOPICS
                        .iter()
                        .filter(|t| t.group == group && (q.is_empty() || t.title.to_lowercase().contains(&q) || t.text.to_lowercase().contains(&q) || t.command.to_lowercase().contains(&q)))
                        .collect();
                    if items.is_empty() {
                        continue;
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label(RichText::new(group).strong().color(ACCENT));
                    });
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        for t in items {
                            let b = egui::Button::selectable(self.wiki_topic == t.key, t.title).wrap_mode(egui::TextWrapMode::Truncate);
                            if ui.add(b).clicked() {
                                self.wiki_topic = t.key.to_string();
                            }
                        }
                    });
                }
            });
        });
        let topic = wiki::topic(&self.wiki_topic).unwrap_or(&wiki::TOPICS[0]);
        let mut open: Option<(u8, usize)> = None;
        let mut goto: Option<&'static str> = None;
        let mut rect = ui.available_rect_before_wrap();
        rect.min.x += 32.0;
        rect.max.x -= 12.0;
        let mut page_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
        let ui = &mut page_ui;
        egui::ScrollArea::vertical().id_salt(("wiki_page", topic.key)).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(760.0));
            ui.add_space(10.0);
            ui.label(RichText::new(topic.group).small().weak());
            ui.heading(RichText::new(topic.title).strong().size(26.0));
            ui.add_space(8.0);
            ui.label(RichText::new(topic.text).size(16.5));
            if !topic.command.is_empty() {
                ui.add_space(14.0);
                ui.label(RichText::new("CLOUDY command").strong());
                egui::Frame::new().fill(ui.visuals().extreme_bg_color).corner_radius(6.0).inner_margin(10.0).show(ui, |ui| {
                    ui.label(RichText::new(topic.command).monospace().size(15.0));
                });
            }
            if !topic.refs.is_empty() {
                ui.add_space(14.0);
                ui.label(RichText::new("Read in the manual").strong());
                for (vol, query) in topic.refs {
                    match self.library.as_ref().and_then(|l| l.find(*vol, query)) {
                        Some(item) => {
                            if ui.link(format!("Hazy {vol}, page {} — {}", item.page, item.title)).clicked() {
                                open = Some((*vol, item.page));
                            }
                        }
                        None => {
                            ui.label(RichText::new(format!("Hazy {vol} — {query}")).weak());
                        }
                    }
                }
                if self.library.is_none() {
                    ui.add_space(6.0);
                    self.no_manuals_ui(ui);
                }
            }
            ui.add_space(14.0);
            ui.label(RichText::new("See also").strong());
            ui.horizontal_wrapped(|ui| {
                for t in wiki::TOPICS.iter().filter(|t| t.group == topic.group && t.key != topic.key) {
                    if ui.small_button(t.title).clicked() {
                        goto = Some(t.key);
                    }
                }
            });
        });
        if let Some(k) = goto {
            self.wiki_topic = k.to_string();
        }
        if let Some((vol, page)) = open {
            self.reader.vol = vol;
            self.reader.goto = Some(page);
            self.mode = Mode::Manuals;
        }
    }

    // ------------------------------------------------------------------ settings
    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let before = crate::theme::get();
        let mut look = before;
        let mut changed = false;

        // the plot on the right follows every change
        let wide = ui.available_width() > 980.0;
        if wide {
            let width = (ui.available_width() * 0.5).min(760.0);
            egui::Panel::right("settings_preview").exact_size(width).resizable(false).show(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Preview").strong().size(18.0));
                    egui::ComboBox::from_id_salt("preview_plot").selected_text(self.preview.label(&[])).width(210.0).show_ui(ui, |ui| {
                        for p in [Plot::Continuum, Plot::Structure, Plot::Thermal, Plot::Ions, Plot::Molecules, Plot::Density, Plot::HeatCool, Plot::Lines, Plot::Bpt] {
                            ui.selectable_value(&mut self.preview, p, p.label(&[]));
                        }
                    });
                });
                ui.add_space(4.0);
                match self.results.as_ref().filter(|r| !r.info_only) {
                    Some(r) => {
                        let ctx = Ctx {
                            r,
                            xaxis: self.xaxis,
                            element: &self.element,
                            species: &self.species,
                            show_regions: self.show_regions,
                            ratios: &self.plots.ratios,
                            customs: &self.plots.customs,
                            bpt: self.plots.bpt,
                            others: &self.bpt_runs,
                            cont_unit: self.cont_unit,
                            sed_lines: true,
                        };
                        let f = ctx.figure(self.preview);
                        let (w, h) = (ui.available_width() - 8.0, (ui.available_height() - 16.0).clamp(300.0, 640.0));
                        show_figure(ui, &f, &format!("preview_{}", self.preview.key()), self.plot_gen, h, w, self.show_grid, &mut self.views);
                    }
                    None => {
                        ui.add_space(40.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("Open a run to see your colours on a real plot.").weak());
                            if ui.button("Show the demo data").clicked() {
                                self.load_demo();
                            }
                        });
                    }
                }
            });
        }

        let mut list = egui::ScrollArea::vertical().id_salt("settings_list").auto_shrink([false, false]);
        if let (true, Some(y)) = (self.screenshot.is_some(), std::env::var("INCLOUD_SCROLL").ok().and_then(|v| v.parse::<f32>().ok())) {
            list = list.vertical_scroll_offset(y);
        }
        list.show(ui, |ui| {
            ui.set_max_width(ui.available_width() - 16.0);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Settings").strong().size(22.0));
                help(ui, "settings");
            });
            ui.add_space(6.0);

            card(ui, "Colour sets", "settings", |ui| {
                for (name, colours) in crate::theme::PRESETS {
                    ui.horizontal(|ui| {
                        if ui.selectable_label(look.palette == colours, name).clicked() {
                            look.use_palette(colours);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(150.0, 16.0), egui::Sense::hover());
                            let w = rect.width() / 6.0;
                            for (i, c) in colours.iter().take(6).enumerate() {
                                let r = egui::Rect::from_min_size(rect.min + egui::vec2(i as f32 * w, 0.0), egui::vec2(w - 3.0, rect.height()));
                                ui.painter().rect_filled(r, 3.0, c32(*c));
                            }
                        });
                    });
                }
                ui.add_space(4.0);
                if ui.button("Reset the plots to the original look").clicked() {
                    look = crate::theme::Theme::DEFAULT;
                }
            });

            card(ui, "Curve colours", "settings", |ui| {
                ui.label(RichText::new("Click a colour to change it, or type its code.").small().weak());
                for group in crate::theme::ROLE_GROUPS {
                    ui.add_space(6.0);
                    ui.label(RichText::new(group).strong());
                    for (r, g, name, _) in crate::theme::ROLES {
                        if g == group {
                            colour_row(ui, name, look.colour_mut(r));
                        }
                    }
                }
                ui.add_space(6.0);
                ui.label(RichText::new("Other curves, in order").strong());
                ui.label(RichText::new("Ions, line ratios, emissivities, heating and cooling, custom plots.").small().weak());
                ui.horizontal_wrapped(|ui| {
                    for c in look.palette.iter_mut() {
                        egui::color_picker::color_edit_button_srgb(ui, c);
                    }
                });
            });

            card(ui, "Plot background", "settings", |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (m, name) in [(0u8, "Same as the app"), (1, "White"), (2, "Black"), (3, "Custom")] {
                        ui.selectable_value(&mut look.bg_mode, m, name);
                    }
                });
                if look.bg_mode == 3 {
                    colour_row(ui, "Background colour", &mut look.bg);
                }
                ui.checkbox(&mut look.export_bg, "Use it in saved figures too").on_hover_text("Otherwise saved figures are always on white paper");
            });

            card(ui, "Lines and shading", "settings", |ui| {
                ui.add(egui::Slider::new(&mut look.width, 0.5..=2.5).text("curve thickness").fixed_decimals(1));
                let mut band = look.band * 100.0;
                if ui.add(egui::Slider::new(&mut band, 0.0..=40.0).text("region shading").suffix(" %").fixed_decimals(0)).changed() {
                    look.band = band / 100.0;
                }
                let mut least = look.line_min * 100.0;
                if ui.add(egui::Slider::new(&mut least, 0.0..=100.0).text("faintest line on the SED").suffix(" %").fixed_decimals(0)).on_hover_text("A saved line is drawn when it adds at least this share to the continuum below it").changed() {
                    look.line_min = least / 100.0;
                }
                ui.checkbox(&mut look.line_names, "Write the names of the saved lines");
            });

            card(ui, "Plots at the start", "settings", |ui| {
                changed |= ui.checkbox(&mut self.show_grid, "Grid").changed();
                changed |= ui.checkbox(&mut self.show_regions, "H II / PDR regions").changed();
                changed |= ui.checkbox(&mut self.sed_lines, "Saved lines on the SED").changed();
                ui.horizontal(|ui| {
                    ui.label("Depth axis");
                    egui::ComboBox::from_id_salt("set_xaxis").selected_text(self.xaxis.label()).show_ui(ui, |ui| {
                        for x in XAxis::ALL {
                            changed |= ui.selectable_value(&mut self.xaxis, x, x.label()).changed();
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("SED axis");
                    egui::ComboBox::from_id_salt("set_contunit").selected_text(self.cont_unit.label()).show_ui(ui, |ui| {
                        for u in ContUnit::ALL {
                            changed |= ui.selectable_value(&mut self.cont_unit, u, u.label()).changed();
                        }
                    });
                });
            });

            card(ui, "Saved figures", "export", |ui| {
                ui.horizontal_wrapped(|ui| {
                    for f in Format::ALL {
                        changed |= ui.radio_value(&mut self.export_fmt, f, f.label()).changed();
                    }
                });
                changed |= ui.add(egui::Slider::new(&mut self.style.width_in, 3.0..=12.0).text("width [in]")).changed();
                changed |= ui.add(egui::Slider::new(&mut self.style.height_in, 2.0..=10.0).text("height [in]")).changed();
                changed |= ui.add(egui::Slider::new(&mut self.style.font_pt, 7.0..=16.0).text("font [pt]")).changed();
                changed |= ui.add(egui::Slider::new(&mut self.style.dpi, 72.0..=600.0).text("DPI (PNG)")).changed();
                changed |= ui.checkbox(&mut self.style.serif, "Serif font").changed();
                changed |= ui.checkbox(&mut self.style.title, "Title above the plot").changed();
            });

            card(ui, "Fonts", "settings", |ui| {
                let font_row = |ui: &mut egui::Ui, id: &str, label: &str, value: &mut u8| {
                    ui.horizontal(|ui| {
                        ui.label(label);
                        egui::ComboBox::from_id_salt(id).selected_text(crate::theme::FONTS[*value as usize]).show_ui(ui, |ui| {
                            for (k, name) in crate::theme::FONTS.iter().enumerate() {
                                ui.selectable_value(value, k as u8, *name);
                            }
                        });
                    });
                };
                ui.label(RichText::new("Interface").strong());
                font_row(ui, "font_ui", "Font", &mut look.ui_font);
                let mut t = look.ui_text * 100.0;
                if ui.add(egui::Slider::new(&mut t, 80.0..=150.0).step_by(5.0).fixed_decimals(0).text("text size").suffix(" %")).changed() {
                    look.ui_text = t / 100.0;
                }
                ui.add_space(6.0);
                ui.label(RichText::new("Plots on screen").strong());
                font_row(ui, "font_plot", "Font", &mut look.plot_font);
                let mut t = look.plot_text * 100.0;
                if ui.add(egui::Slider::new(&mut t, 70.0..=180.0).step_by(5.0).fixed_decimals(0).text("text size").suffix(" %")).changed() {
                    look.plot_text = t / 100.0;
                }
                ui.add_space(6.0);
                ui.label(RichText::new("Saved figures").strong());
                changed |= ui.checkbox(&mut self.style.serif, "Serif font").changed();
                changed |= ui.add(egui::Slider::new(&mut self.style.font_pt, 7.0..=16.0).text("font [pt]")).changed();
            });

            // reached from "Update available": scroll here once and light the card up for a moment
            let glow = match self.goto_updates {
                Some((first, t)) => {
                    if first {
                        ui.scroll_to_cursor(Some(egui::Align::TOP));
                        self.goto_updates = Some((false, t));
                    }
                    let k = 1.0 - t.elapsed().as_secs_f32() / 4.0;
                    if k <= 0.0 && self.screenshot.is_none() {
                        self.goto_updates = None;
                    }
                    ui.ctx().request_repaint();
                    k.clamp(0.0, 1.0)
                }
                None => 0.0,
            };
            if glow > 0.0 {
                egui::Frame::new().stroke(egui::Stroke::new(2.5, OK.gamma_multiply(glow))).corner_radius(12.0).inner_margin(egui::Margin::same(2)).show(ui, |ui| {
                    card(ui, "Updates", "update", |ui| self.updates_ui(ui));
                });
            } else {
                card(ui, "Updates", "update", |ui| self.updates_ui(ui));
            }

            card(ui, "Interface", "settings", |ui| {
                if ui.button("👋 Show the welcome screen again").on_hover_text("The first-start window: what's new since the previous version, and CLOUDY found, set by hand or installed").clicked() {
                    self.welcome.open = true;
                    self.welcome.updated = false;
                    self.welcome.prefer = self.cloudy_exe.clone();
                    if self.welcome.whats_new.is_none() {
                        self.welcome.whats_new = crate::whatsnew::stored();
                    }
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Theme");
                    let mut dark = self.dark;
                    ui.selectable_value(&mut dark, false, "Light");
                    ui.selectable_value(&mut dark, true, "Dark");
                    if dark != self.dark {
                        self.dark = dark;
                        apply_style(ui.ctx(), dark);
                        changed = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Size");
                    let mut zoom = self.ui_zoom;
                    if ui.add_enabled(zoom > 0.75, egui::Button::new("−")).clicked() {
                        zoom -= 0.1;
                    }
                    ui.label(format!("{:.0}%", zoom * 100.0));
                    if ui.add_enabled(zoom < 1.55, egui::Button::new("+")).clicked() {
                        zoom += 0.1;
                    }
                    if ui.add_enabled((zoom - 1.0).abs() > 0.01, egui::Button::new("100%")).clicked() {
                        zoom = 1.0;
                    }
                    if zoom != self.ui_zoom {
                        self.ui_zoom = zoom;
                        ui.ctx().set_zoom_factor(zoom);
                        changed = true;
                    }
                });
            });

            card(ui, "CLOUDY", "cloudy", |ui| {
                if ui.button("⬇ Install or change version…").clicked() {
                    self.versions_open = true;
                }
                ui.label("Executable");
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("📁").on_hover_text("Choose the executable").clicked() {
                            self.pick = Pick::CloudyExe;
                            self.dialog.pick_file();
                        }
                        changed |= ui.add(egui::TextEdit::singleline(&mut self.cloudy_exe).desired_width(ui.available_width())).changed();
                    });
                });
                ui.label("Data folder (CLOUDY_DATA_PATH)");
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("📁").clicked() {
                            self.pick = Pick::DataPath;
                            self.dialog.pick_directory();
                        }
                        changed |= ui.add(egui::TextEdit::singleline(&mut self.data_path).desired_width(ui.available_width())).changed();
                    });
                });
                changed |= ui.checkbox(&mut self.use_tmp, "Run in /tmp (temporary, not kept)").changed();
            });
            ui.add_space(12.0);
        });

        if look != before {
            let fonts = (look.ui_font, look.ui_text, look.plot_font) != (before.ui_font, before.ui_text, before.plot_font);
            crate::theme::set(look);
            if fonts {
                apply_style(ui.ctx(), self.dark);
            }
            changed = true;
        }
        self.settings_dirty |= changed;
    }

    // ------------------------------------------------------------------ versions of CLOUDY
    fn versions_ui(&mut self, ctx: &egui::Context) {
        use crate::installer::{self, Stage};
        // a finished installation becomes the version in use
        if let Some(v) = self.job.as_ref().map(|j| j.view()) {
            if v.stage.step() <= 4 {
                ctx.request_repaint_after(Duration::from_millis(400));
            }
            if let (Stage::Done, Some(exe)) = (&v.stage, v.exe.clone()) {
                self.cloudy_exe = exe.display().to_string();
                self.data_path = v.data.clone();
                self.installs = runner::find_installs();
                self.message = Some((format!("CLOUDY {} is installed and now in use.", v.release.label()), true));
                self.job = None;
                self.save_settings();
            }
        }
        if !self.versions_open {
            self.tools = None;
            return;
        }
        if self.tools.is_none() {
            self.tools = Some(installer::check_tools());
            self.installs = runner::find_installs();
            if !self.releases_asked {
                self.releases_asked = true;
                let (shared, c) = (self.releases.clone(), ctx.clone());
                std::thread::spawn(move || {
                    if let Some(list) = installer::fetch_releases() {
                        if let Ok(mut g) = shared.lock() {
                            *g = list;
                        }
                        c.request_repaint();
                    }
                });
            }
        }
        let tools = self.tools.clone().unwrap_or_default();
        let releases = self.releases.lock().map(|g| g.clone()).unwrap_or_default();
        let root = installer::root();
        let spaces = root.display().to_string().contains(char::is_whitespace);
        let job = self.job.as_ref().map(|j| j.view());
        let busy = job.as_ref().is_some_and(|v| v.stage.step() <= 4);
        enum Act {
            Use(PathBuf, String),
            Install(installer::Release),
            Cancel,
            Dismiss,
            PickExe,
            PickRoot,
            DefaultRoot,
            Search,
            Recheck,
        }
        let mut act: Option<Act> = None;
        let mut open = true;
        let screen = ctx.content_rect();
        egui::Window::new("CLOUDY versions").open(&mut open).default_size([700.0, (screen.height() - 90.0).min(900.0)]).max_height(screen.height() - 70.0).collapsible(false).vscroll(true).show(ctx, |ui| {
            // the installation in progress, or how it ended
            if let Some(v) = &job {
                egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(8.0).inner_margin(10).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("CLOUDY {}", v.release.label())).strong());
                        let stopped = matches!(v.stage, Stage::Failed(_) | Stage::Cancelled);
                        for (k, name) in ["Download", "Unpack", "Compile", "Test"].iter().enumerate() {
                            let step = k + 1;
                            // where it stopped is the last step reached, not the end
                            let now = if stopped { v.reached } else { v.stage.step() };
                            let t = if step < now {
                                RichText::new(format!("✓ {name}")).color(OK)
                            } else if step == now && stopped {
                                RichText::new(format!("✘ {name}")).strong().color(BAD)
                            } else if step == now {
                                RichText::new(format!("▶ {name}")).strong().color(ACCENT)
                            } else {
                                RichText::new(*name).weak()
                            };
                            ui.label(t);
                        }
                    });
                    match &v.stage {
                        Stage::Failed(e) => {
                            ui.label(RichText::new(e).color(BAD));
                        }
                        Stage::Cancelled => {
                            ui.label(RichText::new("Stopped. What was downloaded is kept, so starting again carries on.").weak());
                        }
                        _ => {
                            let bar = match v.fraction {
                                Some(f) => egui::ProgressBar::new(f).show_percentage(),
                                None => egui::ProgressBar::new(0.0).animate(true),
                            };
                            ui.add(bar.text(v.detail.clone()));
                            ui.label(RichText::new(format!("{} so far", fmt_duration(v.started.elapsed()))).small().weak());
                        }
                    }
                    ui.horizontal(|ui| {
                        if v.stage.step() <= 4 {
                            if ui.button("Cancel").clicked() {
                                act = Some(Act::Cancel);
                            }
                        } else if ui.button("Close").clicked() {
                            act = Some(Act::Dismiss);
                        }
                    });
                    if !v.log.is_empty() {
                        egui::CollapsingHeader::new("Output of the commands").id_salt("install_log").default_open(matches!(v.stage, Stage::Failed(_))).show(ui, |ui| {
                            egui::ScrollArea::vertical().max_height(180.0).stick_to_bottom(true).show(ui, |ui| {
                                for l in &v.log {
                                    ui.label(RichText::new(l).monospace().small());
                                }
                            });
                        });
                    }
                });
                ui.add_space(6.0);
            }

            ui.label(RichText::new("On this computer").strong().size(16.0));
            if self.installs.is_empty() {
                ui.label(RichText::new("No CLOUDY found yet.").weak());
            }
            let mut absent = vec![];
            for i in &self.installs {
                // what the shell files name but isn't there is kept out of the way
                if !i.exists {
                    absent.push(i);
                    continue;
                }
                ui.horizontal(|ui| {
                    if i.exe == Path::new(self.cloudy_exe.trim()) {
                        ui.label(RichText::new("● in use").color(OK));
                    } else if ui.add_enabled(i.exists, egui::Button::new("Use")).clicked() {
                        act = Some(Act::Use(i.exe.clone(), i.data.clone()));
                    }
                    ui.label(RichText::new(if i.version.is_empty() { "CLOUDY" } else { i.version.as_str() }).strong());
                    ui.label(RichText::new(&i.origin).weak());
                    if !i.exists {
                        ui.label(RichText::new("missing").color(BAD));
                    }
                });
                ui.add(egui::Label::new(RichText::new(i.exe.display().to_string()).small().weak()).truncate());
            }
            if !absent.is_empty() {
                egui::CollapsingHeader::new(RichText::new(format!("{} more named in your shell files, but not found", absent.len())).weak()).id_salt("absent_installs").show(ui, |ui| {
                    for i in &absent {
                        ui.label(RichText::new(format!("{}  ·  {}", if i.version.is_empty() { "CLOUDY" } else { i.version.as_str() }, i.origin)).small());
                        ui.add(egui::Label::new(RichText::new(i.exe.display().to_string()).small().weak()).truncate());
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui.button("🔍 Search again").clicked() {
                    act = Some(Act::Search);
                }
                if ui.button("Choose an executable…").clicked() {
                    act = Some(Act::PickExe);
                }
            });
            ui.separator();

            ui.horizontal(|ui| {
                ui.label(RichText::new("Install a version").strong().size(16.0));
                help(ui, "cloudy");
            });
            ui.label(RichText::new("InCloud downloads the official source code from nublado.org, compiles it on this computer and checks it with CLOUDY's own quick test. It takes from a few minutes to half an hour, and you can keep working meanwhile.").small().weak());
            ui.horizontal(|ui| {
                ui.label("Folder");
                ui.add(egui::Label::new(RichText::new(root.display().to_string()).monospace().small()).truncate());
            });
            ui.horizontal(|ui| {
                if ui.add_enabled(!busy, egui::Button::new("Change folder…")).clicked() {
                    act = Some(Act::PickRoot);
                }
                if root != installer::default_root() && ui.add_enabled(!busy, egui::Button::new("Default folder")).clicked() {
                    act = Some(Act::DefaultRoot);
                }
            });
            if spaces {
                ui.label(RichText::new("CLOUDY can't be compiled in a folder whose path has spaces: choose another one.").color(AMBER));
            }
            let can_write = installer::writable(&root);
            if !can_write {
                ui.label(RichText::new("InCloud can't write in this folder: choose another one with “Change folder…”.").color(AMBER));
            }
            if !tools.ready() {
                egui::Frame::new().fill(AMBER.gamma_multiply(0.15)).stroke(egui::Stroke::new(1.0, AMBER)).corner_radius(8.0).inner_margin(10).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(format!("To compile CLOUDY this computer needs: {}.", tools.missing.join(", "))).strong());
                    ui.label("Install them once, for example by typing this in a terminal:");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&tools.hint).monospace());
                        if ui.small_button("Copy").clicked() {
                            ui.ctx().copy_text(tools.hint.clone());
                        }
                    });
                    if ui.button("Check again").clicked() {
                        act = Some(Act::Recheck);
                    }
                });
            }
            ui.add_space(6.0);

            egui::Grid::new("releases").num_columns(3).spacing([24.0, 8.0]).show(ui, |ui| {
                for (k, r) in releases.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(r.label()).strong());
                        if k == 0 {
                            ui.label(RichText::new("newest").small().color(OK));
                        }
                    });
                    ui.label(RichText::new(if r.bytes > 0 { format!("{:.0} MB", r.bytes as f64 / 1e6) } else { String::new() }).weak());
                    let have = self.installs.iter().any(|i| i.exists && i.version.eq_ignore_ascii_case(&r.label()));
                    if have {
                        ui.label(RichText::new("✓ installed").color(OK));
                    } else if ui.add_enabled(tools.ready() && !busy && !spaces && can_write, egui::Button::new("⬇ Install")).on_disabled_hover_text("Choose a folder InCloud can write in").clicked() {
                        act = Some(Act::Install(r.clone()));
                    }
                    ui.end_row();
                }
            });
            if tools.host {
                ui.label(RichText::new("InCloud runs in a sandbox: it compiles with the tools of the computer itself.").small().weak());
            }
        });
        self.versions_open = open;
        match act {
            Some(Act::Use(exe, data)) => {
                self.cloudy_exe = exe.display().to_string();
                self.data_path = data;
                self.save_settings();
            }
            Some(Act::Install(r)) => self.job = Some(installer::Job::start(r, tools)),
            Some(Act::Cancel) => {
                if let Some(j) = &self.job {
                    j.cancel();
                }
            }
            Some(Act::Dismiss) => self.job = None,
            Some(Act::PickExe) => {
                self.pick = Pick::CloudyExe;
                self.dialog.pick_file();
            }
            Some(Act::PickRoot) => {
                self.pick = Pick::InstallRoot;
                self.dialog.pick_directory();
            }
            Some(Act::DefaultRoot) => {
                self.install_root.clear();
                installer::set_root("");
                self.save_settings();
            }
            Some(Act::Search) => self.installs = runner::find_installs(),
            Some(Act::Recheck) => self.tools = None,
            None => {}
        }
    }

    fn editor_ui(&mut self, ctx: &egui::Context) {
        let generated = self.params.input_file();
        let running = self.run.is_some();
        match self.editor.window(ctx, &generated, &self.params.output_commands(), running) {
            crate::deckedit::Action::None => {}
            crate::deckedit::Action::Run => self.start_run(),
            crate::deckedit::Action::Open => {
                self.pick = Pick::OpenDeck;
                self.dialog.pick_file();
            }
            crate::deckedit::Action::SaveAs => {
                self.pick = Pick::SaveDeck;
                self.dialog.save_file();
            }
        }
    }

    fn handle_dialog(&mut self, ctx: &egui::Context) {
        self.editor_ui(ctx);
        self.dialog.update(ctx);
        if let Some(p) = self.dialog.take_picked() {
            let s = p.display().to_string();
            match self.pick {
                Pick::CloudyExe => {
                    self.data_path = runner::data_for_exe(&p);
                    self.cloudy_exe = s;
                }
                Pick::DataPath => self.data_path = s,
                Pick::OutDir => self.out_dir = s,
                Pick::BatchOut => self.batch.out_dir = s,
                Pick::BatchTemplate => match std::fs::read(&p) {
                    Ok(bytes) => {
                        self.batch.adopt(&String::from_utf8_lossy(&bytes));
                        self.batch.extra_files.clear();
                        self.info(format!("{} is the input file of every model now; check where the parameters went.", p.display()));
                    }
                    Err(e) => self.fail(format!("Cannot read the file: {e}"), None),
                },
                Pick::ExportDir => self.export_dir = s,
                Pick::OpenRun => self.open_run(&p),
                Pick::HazyDir => {
                    self.library = pdfview::find_docs(&[p.clone()]).and_then(|d| Library::open(&d, ctx));
                    if self.library.is_none() {
                        self.message = Some((format!("No hazy1.pdf found in {}", p.display()), false));
                    }
                }
                Pick::AddRuns => {
                    let found = scan_runs(&p, 500);
                    let usable = found.iter().filter(|r| crate::figures::bpt_point_run(r, self.plots.bpt).is_some()).count();
                    let n = found.len();
                    for run in found {
                        if !self.bpt_runs.iter().any(|x| x.source == run.source) {
                            self.bpt_runs.push(run);
                        }
                    }
                    if n == 0 {
                        self.message = Some((format!("No CLOUDY .out file with a line table found in {}", p.display()), false));
                    } else {
                        self.message = Some((format!("Added {n} run(s) from {} ({usable} have the lines for this BPT diagram).", p.display()), true));
                    }
                }
                Pick::SedFile => match std::fs::read(&p) {
                    Ok(bytes) => {
                        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        match self.params.custom_sed.load_text(&String::from_utf8_lossy(&bytes), &name) {
                            Ok(n) => self.message = Some((format!("Loaded {n} points from {name}. Check the units of the two columns."), true)),
                            Err(e) => self.message = Some((format!("{name}: {e}"), false)),
                        }
                    }
                    Err(e) => self.message = Some((format!("Cannot read the file: {e}"), false)),
                },
                Pick::SaveRun => {
                    if let Some(r) = &self.results {
                        let dest = p.join(format!("{}_{}", r.prefix, runner::timestamp()));
                        match runner::copy_dir(&r.dir, &dest) {
                            Ok(n) => self.message = Some((format!("Run kept: {n} files copied to {}", dest.display()), true)),
                            Err(e) => self.message = Some((format!("Copy failed: {e}"), false)),
                        }
                    }
                }
                Pick::OpenDeck => match std::fs::read(&p) {
                    Ok(bytes) => {
                        self.editor.load(String::from_utf8_lossy(&bytes).into_owned());
                        self.left_tab = LeftTab::Run;
                        self.message = Some((format!("Input file {} loaded: it will be run as written.", p.display()), true));
                    }
                    Err(e) => self.message = Some((format!("Cannot read the file: {e}"), false)),
                },
                Pick::SaveDeck => {
                    let text = self.deck_text();
                    match std::fs::write(&p, text) {
                        Ok(()) => self.message = Some((format!("Input file saved as {}", p.display()), true)),
                        Err(e) => self.message = Some((format!("Cannot save the file: {e}"), false)),
                    }
                }
                Pick::WelcomeExe => {
                    self.welcome.exe = s;
                    if self.welcome.data.trim().is_empty() || !crate::welcome::data_ok(&self.welcome.data) {
                        self.welcome.data = runner::data_for_exe(&p);
                    }
                }
                Pick::WelcomeData => self.welcome.data = s,
                Pick::InstallRoot => {
                    self.install_root = s;
                    crate::installer::set_root(&self.install_root);
                }
                Pick::None => {}
            }
            self.pick = Pick::None;
            self.save_settings();
        }
    }

    fn screenshot_tick(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot.clone() else { return };
        self.frame += 1;
        ctx.request_repaint();
        let due = std::env::var("INCLOUD_SHOT").ok().and_then(|v| v.parse().ok()).unwrap_or(120);
        let waiting = std::env::var("INCLOUD_WAIT_FIND").is_ok() && !self.reader.finished && self.frame < 6000;
        if self.frame >= due && !waiting && !self.shot_sent {
            self.shot_sent = true;
            let c = ctx.clone();
            ctx.request_screenshot(move |img: Arc<egui::ColorImage>| {
                let [w, h] = img.size;
                let mut buf = Vec::with_capacity(w * h * 4);
                for p in &img.pixels {
                    buf.extend_from_slice(&p.to_array());
                }
                if let Some(d) = path.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                match image::RgbaImage::from_raw(w as u32, h as u32, buf) {
                    Some(im) => {
                        if let Err(e) = im.save(&path) {
                            eprintln!("screenshot: {e}");
                        }
                    }
                    None => eprintln!("screenshot: bad buffer"),
                }
                c.send_viewport_cmd(egui::ViewportCommand::Close);
            });
        }
        if self.frame > due.max(if waiting { 6000 } else { 0 }) + 600 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

#[derive(Default)]
struct ExportRequest {
    plots: Vec<Plot>,
    data: bool,
    keep_run: bool,
    pick_folder: bool,
    add_runs: bool,
    run_again: bool,
}

fn export_network(r: &Results, species: &[String], zone: Zone, fmt: Format, st: &Style, dir: &Path, stem: &str) -> Result<PathBuf, String> {
    let (svg, csv) = if species.len() > 1 {
        let map = network::build_map(r, species, zone)?;
        (network::map_to_svg(&map, st), network::map_to_csv(&map))
    } else {
        let name = species.first().ok_or("No species chosen")?;
        let net = network::build(r, name, zone)?;
        (network::to_svg(&net, st), network::to_csv(&net))
    };
    let svg = export::paper_svg(svg);
    std::fs::create_dir_all(dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    let path = dir.join(format!("{stem}.{}", fmt.ext()));
    let bytes = match fmt {
        Format::Svg => svg.into_bytes(),
        Format::Png => export::svg_to_png(&svg, st.dpi)?,
        Format::Pdf => export::svg_to_pdf(&svg)?,
        Format::Csv => csv.into_bytes(),
    };
    std::fs::write(&path, bytes).map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
    Ok(path)
}

/// The reaction-rates table as CSV text.
fn rates_csv(r: &Results, species: &str, zone: Zone) -> Result<String, String> {
    let c = r.chemistry.iter().find(|c| c.species == species).ok_or_else(|| format!("No rates saved for {species}"))?;
    let (d0, d1) = zone.range(r).ok_or_else(|| format!("This model has no {}", zone.label()))?;
    let mut s = format!("# reaction rates of {species}, {}\nkind,reaction,share,integrated_rate_cm-2_s-1,peak_rate_cm-3_s-1,peak_depth_cm\n", zone.label());
    for row in c.table(d0, d1) {
        s.push_str(&format!(
            "{},\"{}\",{:.6},{:.6e},{:.6e},{:.6e}\n",
            if row.formation { "formation" } else { "destruction" },
            row.reaction,
            row.fraction,
            row.integrated,
            row.peak,
            row.peak_depth
        ));
    }
    Ok(s)
}

fn save_rates_table(r: &Results, species: &str, zone: Zone, dir: &Path, stem: &str) -> Result<PathBuf, String> {
    let text = rates_csv(r, species, zone)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    let path = dir.join(format!("{stem}_table.csv"));
    std::fs::write(&path, text).map_err(|e| format!("Cannot write {}: {e}", path.display()))?;
    Ok(path)
}

/// Every reaction forming or destroying a species, with its share of the total.
fn rates_table_ui(ui: &mut egui::Ui, r: &Results, species: &str, zone: Zone, id: &str, height: f32, width: f32) {
    let Some(c) = r.chemistry.iter().find(|c| c.species == species) else {
        ui.label(RichText::new(format!("No rates saved for {species}")).weak());
        return;
    };
    let Some((d0, d1)) = zone.range(r) else {
        ui.label(RichText::new(format!("This model has no {}", zone.label())).weak());
        return;
    };
    let rows = c.table(d0, d1);
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} reactions · {}", rows.len(), zone.label())).weak());
        if ui.small_button("Copy table").on_hover_text("Copy as CSV, to paste in a spreadsheet").clicked() {
            if let Ok(t) = rates_csv(r, species, zone) {
                ui.ctx().copy_text(t);
            }
        }
    });
    egui::ScrollArea::both().id_salt(format!("{id}_rates")).max_height(height - 34.0).max_width(width).auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new(format!("{id}_ratesgrid")).striped(true).num_columns(6).spacing([18.0, 6.0]).show(ui, |ui| {
            for h in ["", "Reaction", "Share", "∫ rate dr [cm⁻² s⁻¹]", "Peak rate [cm⁻³ s⁻¹]", "Peak at depth [cm]"] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for row in rows.iter().take(400) {
                let (txt, col) = if row.formation { ("Formation", c32(crate::figures::pal(0))) } else { ("Destruction", c32(crate::figures::pal(1))) };
                ui.label(RichText::new(txt).color(col));
                ui.label(pretty_reaction(&row.reaction));
                ui.label(if row.fraction >= 0.001 { format!("{:.2}%", row.fraction * 100.0) } else { format!("{:.1e}%", row.fraction * 100.0) });
                ui.label(format!("{:.3e}", row.integrated));
                ui.label(format!("{:.3e}", row.peak));
                ui.label(format!("{:.3e}", row.peak_depth));
                ui.end_row();
            }
        });
    });
}

/// Editor of a custom incident spectrum. Returns true when the person asks to load a file.
fn sed_editor(ui: &mut egui::Ui, c: &mut crate::model::CustomSed) -> bool {
    let mut load = false;
    egui::CollapsingHeader::new("How to write your spectrum").id_salt("sedhelp").show(ui, |ui| {
        for t in [
            "1. Write one point per row: first the x value, then the flux, separated by a space.",
            "2. Say below what the two columns are (for example wavelength in µm and νFν).",
            "3. Only the shape counts: the brightness is set by log U. Any scale is fine.",
            "4. Cover the ionising range (energies above 1 Ryd, wavelengths below 0.0912 µm).",
            "Rows starting with # are ignored.",
        ] {
            ui.label(RichText::new(t).small());
        }
    });
    ui.horizontal(|ui| {
        if ui.button("Load a file…").clicked() {
            load = true;
        }
        if ui.button("Example").on_hover_text("Fill in a small example to start from").clicked() {
            c.text = SED_EXAMPLE.to_string();
            c.x = SedX::Micron;
            c.y = SedY::NuFnu;
            c.y_is_log = false;
            c.source.clear();
        }
        if !c.source.is_empty() {
            ui.label(RichText::new(c.source.clone()).small().weak());
        }
    });
    egui::Grid::new("sedunits").num_columns(2).show(ui, |ui| {
        ui.label("Column 1");
        egui::ComboBox::from_id_salt("sedx").selected_text(c.x.label()).width(170.0).show_ui(ui, |ui| {
            for u in SedX::ALL {
                ui.selectable_value(&mut c.x, u, u.label());
            }
        });
        ui.end_row();
        ui.label("Column 2");
        egui::ComboBox::from_id_salt("sedy").selected_text(c.y.label()).width(170.0).show_ui(ui, |ui| {
            for u in SedY::ALL {
                ui.selectable_value(&mut c.y, u, u.label());
            }
        });
        ui.end_row();
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut c.y_is_log, "Column 2 is log₁₀");
        ui.checkbox(&mut c.extrapolate, "Extend to low energies").on_hover_text("Continue the spectrum below its lowest energy with a power law");
    });
    ui.add(egui::TextEdit::multiline(&mut c.text).font(TextStyle::Monospace).hint_text("1000   1.0e-6\n10     1.0e-3\n1      0.1\n0.1    0.8").desired_rows(7).desired_width(f32::INFINITY));
    match c.points() {
        Ok(pts) => {
            ui.label(RichText::new(format!("✔ {} points", pts.len())).color(OK));
            if let Some((_, b)) = c.energy_range() {
                if b < 1.0 {
                    ui.label(RichText::new(format!("Highest energy is {b:.3} Ryd: there are no ionising photons, so log U cannot be used.")).small().color(BAD));
                }
            }
            let line: Vec<[f64; 2]> = pts.iter().map(|(x, y)| [x.log10(), y.log10()]).collect();
            EPlot::new("sedpreview")
                .height(130.0)
                .allow_drag(false)
                .allow_zoom(false)
                .allow_scroll(false)
                .show_grid(false)
                .x_axis_formatter(|m: GridMark, _r| log_tick_label(m.value, m.step_size))
                .y_axis_formatter(|m: GridMark, _r| log_tick_label(m.value, m.step_size))
                .show(ui, |pu| {
                    pu.line(Line::new("", PlotPoints::from(line)).color(ACCENT).width(2.0));
                });
        }
        Err(e) => {
            ui.label(RichText::new(e).small().color(BAD));
        }
    }
    load
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(r) = self.run.as_mut() {
            r.cancel();
        }
        for d in &self.temp_dirs {
            if d.starts_with(std::env::temp_dir()) {
                let _ = std::fs::remove_dir_all(d);
            }
        }
    }
}

impl eframe::App for App {
    /// Only for checking the program in pictures: a scripted mouse and keyboard, as
    /// `frame:move:x:y`, `frame:down:x:y`, `frame:up:x:y`, `frame:key:ctrl+z`, `frame:text:words`.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if self.screenshot.is_none() {
            return;
        }
        // pictures without the mouse pointer resting on some button
        if std::env::var("INCLOUD_QUIET").is_ok() {
            raw.events.retain(|e| !matches!(e, egui::Event::PointerMoved(_)));
            raw.events.push(egui::Event::PointerGone);
        }
        let Ok(script) = std::env::var("INCLOUD_SCRIPT") else { return };
        raw.focused = true;
        for step in script.split(';') {
            let f: Vec<&str> = step.split(':').collect();
            if f.len() < 3 || f[0].parse::<u64>().ok() != Some(self.frame) {
                continue;
            }
            let at = || egui::pos2(f.get(2).and_then(|v| v.parse().ok()).unwrap_or(0.0), f.get(3).and_then(|v| v.parse().ok()).unwrap_or(0.0));
            match f[1] {
                "move" => raw.events.push(egui::Event::PointerMoved(at())),
                "down" | "up" => raw.events.push(egui::Event::PointerButton { pos: at(), button: egui::PointerButton::Primary, pressed: f[1] == "down", modifiers: egui::Modifiers::NONE }),
                "rdown" | "rup" => raw.events.push(egui::Event::PointerButton { pos: at(), button: egui::PointerButton::Secondary, pressed: f[1] == "rdown", modifiers: egui::Modifiers::NONE }),
                "text" => raw.events.push(egui::Event::Text(f[2].to_string())),
                "key" => {
                    let (mods, name) = match f[2].rsplit_once('+') {
                        Some(("ctrl", k)) => (egui::Modifiers::COMMAND, k),
                        Some(("shift", k)) => (egui::Modifiers::SHIFT, k),
                        _ => (egui::Modifiers::NONE, f[2]),
                    };
                    let key = match name {
                        "f" => egui::Key::F,
                        "z" => egui::Key::Z,
                        "f3" => egui::Key::F3,
                        "esc" => egui::Key::Escape,
                        _ => egui::Key::Enter,
                    };
                    raw.events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: mods });
                    raw.events.push(egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: mods });
                }
                _ => {}
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // line labels follow the CLOUDY version of the chosen executable (C23+ need "air")
        let ver = runner::version_of(self.cloudy_exe.trim());
        self.params.modern_labels = crate::model::major_version(&ver).map(|m| m >= 23).unwrap_or(true);
        self.poll_run(&ctx);
        if self.species_known.is_empty() || self.species_path != self.data_path {
            self.species_path = self.data_path.clone();
            self.species_known = crate::species::known(&self.data_path);
        }
        self.screenshot_tick(&ctx);

        egui::Panel::top("top").show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if self.mode == Mode::Results {
                    let (label, tip) = if self.left_open { ("◀", "Hide the side panel") } else { ("▶", "Show the side panel") };
                    if ui.add(egui::Button::new(label).min_size(egui::vec2(34.0, 30.0))).on_hover_text(tip).clicked() {
                        self.left_open = !self.left_open;
                    }
                }
                if let Some(t) = &self.logo {
                    ui.add(egui::Image::new(egui::load::SizedTexture::new(t.id(), egui::vec2(38.0, 38.0))));
                }
                ui.label(RichText::new("In").size(24.0).strong().color(ACCENT));
                ui.add_space(-ui.spacing().item_spacing.x);
                ui.label(RichText::new("Cloud").size(24.0).strong().color(ACCENT2));
                ui.add_space(14.0);
                ui.selectable_value(&mut self.mode, Mode::Results, RichText::new("Simulation").size(16.0));
                ui.selectable_value(&mut self.mode, Mode::Batch, RichText::new("Grids").size(16.0)).on_hover_text("Many models at once: input files and parallel scripts for this computer or a server");
                ui.selectable_value(&mut self.mode, Mode::Wiki, RichText::new("Wiki").size(16.0));
                ui.selectable_value(&mut self.mode, Mode::Manuals, RichText::new("Manuals").size(16.0));
                ui.selectable_value(&mut self.mode, Mode::Settings, RichText::new("⚙ Settings").size(16.0));
                if self.mode == Mode::Results && !self.left_open {
                    ui.add_space(10.0);
                    let b = egui::Button::new(RichText::new("▶ Run CLOUDY").color(Color32::WHITE)).fill(ACCENT);
                    if ui.add_enabled(self.run.is_none(), b).clicked() {
                        self.start_run();
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.dark { "☀ Light" } else { "🌙 Dark" };
                    if self.update_available() {
                        if ui.add(egui::Button::new(RichText::new("⬆ Update available").color(OK))).on_hover_text("A newer InCloud is ready to build: opens Settings → Updates").clicked() {
                            self.mode = Mode::Settings;
                            self.goto_updates = Some((true, Instant::now()));
                        }
                    }
                    if ui.add(egui::Button::new(label).min_size(egui::vec2(90.0, 30.0))).on_hover_text("Switch light / dark mode").clicked() {
                        self.dark = !self.dark;
                        apply_style(&ctx, self.dark);
                        self.save_settings();
                    }
                    ui.add_space(8.0);
                    let found = !self.cloudy_exe.trim().is_empty() && Path::new(self.cloudy_exe.trim()).is_file();
                    let installing = self.job.as_ref().map(|j| j.view()).filter(|v| v.stage.step() <= 4);
                    let (text, col, tip) = if let Some(v) = &installing {
                        let share = v.fraction.map(|f| format!("{:.0}%", f * 100.0)).unwrap_or_else(|| "…".into());
                        (format!("Installing {} · {share}", v.release.label()), AMBER, "Click to follow the installation".to_string())
                    } else if found {
                        (format!("● CLOUDY {}  ▾", runner::version_of(self.cloudy_exe.trim())), OK, format!("{}\nClick to change or install a version", self.cloudy_exe))
                    } else {
                        ("● CLOUDY not found: install it  ▾".to_string(), BAD, "Click to find or install CLOUDY".to_string())
                    };
                    if ui.add(egui::Button::new(RichText::new(text).color(col)).frame(false)).on_hover_text(tip).clicked() {
                        self.versions_open = true;
                    }
                });
            });
            if let Some((m, ok)) = self.message.clone() {
                let col = if ok { OK } else { BAD };
                egui::Frame::new().fill(col.gamma_multiply(0.12)).corner_radius(6.0).inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    // long messages (paths of big folders) wrap instead of running off the window
                    ui.horizontal_top(|ui| {
                        let w = (ui.available_width() - 34.0).max(100.0);
                        ui.allocate_ui_with_layout(egui::vec2(w, 0.0), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                            ui.set_width(w);
                            ui.add(egui::Label::new(RichText::new(m).color(col)).wrap());
                        });
                        if ui.small_button("✕").on_hover_text("Hide this message").clicked() {
                            self.message = None;
                            self.error_detail = None;
                        }
                    });
                    if let Some(d) = self.error_detail.clone() {
                        egui::CollapsingHeader::new("Error details").id_salt("errdet").show(ui, |ui| {
                            egui::ScrollArea::vertical().max_height(170.0).show(ui, |ui| {
                                ui.label(RichText::new(d).monospace().small());
                            });
                        });
                    }
                });
            }
            ui.add_space(6.0);
        });

        if self.left_open && self.mode == Mode::Results {
            let width = std::env::var("INCLOUD_LEFT").ok().and_then(|v| v.parse().ok()).unwrap_or(400.0);
            egui::Panel::left("params").default_size(width).size_range(370.0..=640.0).show(ui, |ui| {
                self.params_ui(ui);
            });
        }

        egui::CentralPanel::default_margins().show(ui, |ui| {
            if self.mode == Mode::Wiki {
                self.wiki_ui(ui);
            } else if self.mode == Mode::Manuals {
                self.manuals_ui(ui);
            } else if self.mode == Mode::Settings {
                self.settings_ui(ui);
            } else if self.mode == Mode::Batch {
                self.batch_ui(ui);
            } else if self.run.is_some() {
                self.progress_ui(ui);
            } else if self.results.is_some() {
                self.results_ui(ui);
            } else {
                self.welcome_ui(ui);
            }
        });

        self.handle_dialog(&ctx);
        self.welcome_window(&ctx);
        self.run_search_ui(&ctx);
        self.run_choice_ui(&ctx);
        self.versions_ui(&ctx);
        // written once the mouse is released, not at every step of a slider
        if self.settings_dirty && !ctx.input(|i| i.pointer.any_down()) {
            self.settings_dirty = false;
            self.save_settings();
        }
        // a "?" button somewhere asked for a wiki page
        if let Some(k) = ctx.data_mut(|d| d.remove_temp::<String>(egui::Id::NULL.with(WIKI_REQUEST))) {
            self.wiki_topic = k;
            self.mode = Mode::Wiki;
        }
    }
}

/// Choose which plots are shown and define ratios, BPT and custom plots.
fn plots_ui(ui: &mut egui::Ui, cfg: &mut PlotConfig, lines: &[(String, String)], cols: &[(String, String)], id: &str) {
    ui.horizontal(|ui| {
        if ui.small_button("Default set").clicked() {
            cfg.selected = Plot::DEFAULT.to_vec();
        }
        if ui.small_button("All").clicked() {
            cfg.selected = Plot::BUILTIN.to_vec();
            for i in 0..cfg.customs.len() {
                cfg.selected.push(Plot::Custom(i));
            }
        }
        if ui.small_button("None").clicked() {
            cfg.selected.clear();
        }
    });
    ui.horizontal_wrapped(|ui| {
        for p in Plot::BUILTIN.iter() {
            let mut on = cfg.selected.contains(p);
            if ui.checkbox(&mut on, p.label(&cfg.customs)).changed() {
                if on {
                    cfg.selected.push(*p);
                } else {
                    cfg.selected.retain(|x| x != p);
                }
            }
        }
    });

    ui.add_space(6.0);
    egui::CollapsingHeader::new(RichText::new("Line ratios").strong()).id_salt(format!("{id}_ratios")).default_open(false).show(ui, |ui| {
        let name_of = |l: &str| lines.iter().find(|(a, _)| crate::results::same_line(a, l)).map(|(_, b)| b.clone()).unwrap_or_else(|| pretty_line(l));
        let mut del = None;
        for (i, r) in cfg.ratios.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                line_combo(ui, &format!("{id}_rn{i}"), &mut r.num, lines, &name_of);
                ui.label("/");
                line_combo(ui, &format!("{id}_rd{i}"), &mut r.den, lines, &name_of);
                if ui.small_button("✕").clicked() {
                    del = Some(i);
                }
            });
        }
        if let Some(i) = del {
            cfg.ratios.remove(i);
        }
        if ui.button("+ Add ratio").clicked() {
            cfg.ratios.push(RatioSpec { num: crate::figures::O3.into(), den: crate::figures::HB.into() });
            if !cfg.selected.contains(&Plot::Ratios) {
                cfg.selected.push(Plot::Ratios);
            }
        }
    });
    egui::CollapsingHeader::new(RichText::new("BPT diagram").strong()).id_salt(format!("{id}_bpt")).default_open(false).show(ui, |ui| {
        for k in BptKind::ALL {
            ui.radio_value(&mut cfg.bpt, k, k.label());
        }
        ui.label(RichText::new("Points of the other runs of this session are shown too.").small().weak());
    });
    egui::CollapsingHeader::new(RichText::new("Custom plots").strong()).id_salt(format!("{id}_custom")).default_open(false).show(ui, |ui| {
        let mut del = None;
        for i in 0..cfg.customs.len() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                let mut on = cfg.selected.contains(&Plot::Custom(i));
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut on, "Show").changed() {
                        if on {
                            cfg.selected.push(Plot::Custom(i));
                        } else {
                            cfg.selected.retain(|x| *x != Plot::Custom(i));
                        }
                    }
                    ui.add(egui::TextEdit::singleline(&mut cfg.customs[i].title).hint_text(format!("Custom plot {}", i + 1)).desired_width((ui.available_width() - 90.0).clamp(80.0, 170.0)));
                    if ui.small_button("Delete").clicked() {
                        del = Some(i);
                    }
                });
                let c = &mut cfg.customs[i];
                ui.horizontal(|ui| {
                    ui.label("x:");
                    col_combo(ui, &format!("{id}_cx{i}"), &mut c.x, cols);
                    ui.checkbox(&mut c.xlog, "log");
                });
                let mut ydel = None;
                for (j, y) in c.ys.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label("y:");
                        col_combo(ui, &format!("{id}_cy{i}_{j}"), y, cols);
                        if ui.small_button("✕").clicked() {
                            ydel = Some(j);
                        }
                    });
                }
                if let Some(j) = ydel {
                    c.ys.remove(j);
                }
                ui.horizontal(|ui| {
                    if ui.small_button("+ y quantity").clicked() {
                        c.ys.push(("Structure".into(), "eden".into()));
                    }
                    ui.checkbox(&mut c.ylog, "log y");
                });
            });
        }
        if let Some(i) = del {
            cfg.customs.remove(i);
            cfg.selected.retain(|x| *x != Plot::Custom(i));
            for p in cfg.selected.iter_mut() {
                if let Plot::Custom(k) = p {
                    if *k > i {
                        *k -= 1;
                    }
                }
            }
        }
        if ui.button("+ New custom plot").clicked() {
            cfg.customs.push(CustomSpec::default());
            cfg.selected.push(Plot::Custom(cfg.customs.len() - 1));
        }
    });
}

fn line_combo(ui: &mut egui::Ui, id: &str, value: &mut String, lines: &[(String, String)], name_of: &dyn Fn(&str) -> String) {
    egui::ComboBox::from_id_salt(id).selected_text(name_of(value)).width(((ui.available_width() - 70.0) / 2.0).clamp(80.0, 150.0)).truncate().height(320.0).show_ui(ui, |ui| {
        for (l, n) in lines {
            if ui.selectable_label(crate::results::same_line(l, value), n).clicked() {
                *value = l.clone();
            }
        }
    });
}

fn col_combo(ui: &mut egui::Ui, id: &str, value: &mut (String, String), cols: &[(String, String)]) {
    let shown = |g: &str, n: &str| if g.starts_with("Line") { format!("{g}: {}", pretty_line(n)) } else { format!("{g}: {n}") };
    egui::ComboBox::from_id_salt(id).selected_text(shown(&value.0, &value.1)).width((ui.available_width() - 80.0).clamp(100.0, 240.0)).truncate().height(360.0).show_ui(ui, |ui| {
        let mut last_group = String::new();
        for (g, n) in cols {
            if *g != last_group {
                ui.label(RichText::new(g).strong().small());
                last_group = g.clone();
            }
            if ui.selectable_label(value.0 == *g && value.1 == *n, shown(g, n)).clicked() {
                *value = (g.clone(), n.clone());
            }
        }
    });
}

const WIKI_REQUEST: &str = "incloud_wiki_request";

/// A small "?" that opens the wiki page about the thing next to it.
pub(crate) fn help(ui: &mut egui::Ui, key: &str) {
    let tip = wiki::topic(key).map(|t| format!("Wiki: {}", t.title)).unwrap_or_else(|| "Wiki".into());
    let b = egui::Button::new(RichText::new("?").size(12.0).strong()).corner_radius(10.0).min_size(egui::vec2(20.0, 20.0));
    let scope = ui.scope(|ui| {
        ui.spacing_mut().button_padding = egui::vec2(2.0, 0.0);
        ui.add(b).on_hover_text(tip)
    });
    if scope.inner.clicked() {
        let k = key.to_string();
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::NULL.with(WIKI_REQUEST), k));
    }
}

/// A colour with its name: a button that opens the picker, and its code to type.
fn colour_row(ui: &mut egui::Ui, name: &str, c: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        ui.label(name);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let id = ui.make_persistent_id(("colour_code", name));
            let mut code = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| crate::theme::hex(*c));
            let out = ui.add(egui::TextEdit::singleline(&mut code).id(id.with("edit")).font(TextStyle::Monospace).desired_width(84.0));
            if out.changed() {
                if let Some(v) = crate::theme::parse_hex(&code) {
                    *c = v;
                }
            }
            egui::color_picker::color_edit_button_srgb(ui, c);
            // while it is not being typed in, the code follows the colour
            let shown = if out.has_focus() { code } else { crate::theme::hex(*c) };
            ui.data_mut(|d| d.insert_temp(id, shown));
        });
    });
}

/// One stopping criterion: a tick box on the left, its value and "?" lined up on the right.
fn stop_row(ui: &mut egui::Ui, on: &mut bool, label: &str, key: &str, value: impl FnOnce(&mut egui::Ui, bool)) {
    ui.horizontal(|ui| {
        ui.checkbox(on, label);
        let enabled = *on;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            help(ui, key);
            value(ui, enabled);
        });
    });
}

/// A titled group of settings in a light frame.
fn card(ui: &mut egui::Ui, title: &str, key: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(10.0)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).strong().size(16.0));
                help(ui, key);
            });
            ui.add_space(4.0);
            add(ui);
        });
    ui.add_space(10.0);
}

/// A small grey explanation that wraps within the width of a form field.
fn note(ui: &mut egui::Ui, text: &str) {
    ui.vertical(|ui| {
        ui.set_max_width(520.0);
        ui.add(egui::Label::new(RichText::new(text).small().weak()).wrap());
    });
}

/// A coloured pill. It is a single widget of known size, so a row of pills wraps
/// to the next line instead of pushing the panel wider than the window.
fn chip(ui: &mut egui::Ui, text: &str, c: [u8; 3], tip: &str) {
    let col = Color32::from_rgb(c[0], c[1], c[2]);
    let b = egui::Button::new(RichText::new(text).size(13.0).color(ui.visuals().text_color()))
        .fill(Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 32))
        .stroke(egui::Stroke::new(1.2, col))
        .corner_radius(12.0)
        .wrap_mode(egui::TextWrapMode::Extend)
        .sense(egui::Sense::hover());
    ui.add(b).on_hover_text(tip);
}

fn c32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Ticks of a logarithmic axis: on a wide range, whole decades far enough apart (`room` points)
/// for each of them to get its label, whatever the size of the plot.
fn decades(input: egui_plot::GridInput, room: f64) -> Vec<GridMark> {
    let (lo, hi) = input.bounds;
    if hi - lo < 5.0 {
        return egui_plot::log_grid_spacer(10)(input);
    }
    // the library asks for lines at least 8 points apart
    let points_per_decade = 8.0 / input.base_step_size.max(1e-12);
    let step = [1.0, 2.0, 5.0, 10.0, 20.0].into_iter().find(|s| s * points_per_decade >= room).unwrap_or(50.0);
    let (first, last) = ((lo / step).ceil() as i64, (hi / step).floor() as i64);
    (first..=last).map(|k| GridMark { value: k as f64 * step, step_size: step }).collect()
}

fn log_tick_label(v: f64, step: f64) -> String {
    if step >= 0.99 {
        if (v - v.round()).abs() < 1e-6 {
            markup_to_unicode(&format!("10^{{{}}}", v.round() as i64))
        } else {
            String::new()
        }
    } else {
        let x = 10f64.powf(v);
        if (0.01..1e5).contains(&x) {
            let dec = if x >= 100.0 { 0 } else if x >= 10.0 { 1 } else if x >= 1.0 { 2 } else { 3 };
            let s = format!("{:.*}", dec, x);
            if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
        } else {
            format!("{:.1e}", x)
        }
    }
}

fn fmt_ratio(v: f64) -> String {
    if (0.01..1000.0).contains(&v) {
        format!("{v:.3}")
    } else {
        format!("{v:.2e}")
    }
}

/// A value read under the cursor, back in data units.
fn fmt_value(v: f64, log: bool) -> String {
    let x = if log { 10f64.powf(v) } else { v };
    let a = x.abs();
    if a != 0.0 && (a >= 1e5 || a < 1e-3) {
        format!("{x:.4e}")
    } else {
        format!("{x:.4}")
    }
}

fn lin_tick_label(v: f64) -> String {
    let a = v.abs();
    if a != 0.0 && (a >= 1e5 || a < 1e-3) {
        format!("{v:.1e}")
    } else {
        let s = format!("{v:.3}");
        let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        if s == "-0" { "0".into() } else { s }
    }
}

/// One plot cell: the network drawing, the rates table or a figure.
#[allow(clippy::too_many_arguments)]
fn show_plot(ui: &mut egui::Ui, ctx: &Ctx, p: Plot, net: &[String], rates_table: bool, abund_table: bool, zone: Zone, gen: u64, height: f32, width: f32, grid: bool, views: &mut HashMap<String, PlotView>) {
    if p == Plot::Network {
        let (w, h) = (width - 16.0, height - 16.0);
        let problem = |ui: &mut egui::Ui, e: String| {
            ui.set_min_size(egui::vec2(w, h));
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(e).weak());
            });
        };
        paper(ui, |ui, m| {
            let (w, h) = (w - m, h - m);
            egui::Frame::group(ui.style()).show(ui, |ui| {
            if net.len() > 1 {
                match network::build_map(ctx.r, net, zone) {
                    Ok(map) => network::draw_map(ui, &map, w, h),
                    Err(e) => problem(ui, e),
                }
            } else {
                match network::build(ctx.r, net.first().map(|s| s.as_str()).unwrap_or(ctx.species), zone) {
                    Ok(n) => network::draw(ui, &n, w, h),
                    Err(e) => problem(ui, e),
                }
            }
        });
        });
        return;
    }
    if p == Plot::Rates && rates_table {
        rates_table_ui(ui, ctx.r, ctx.species, zone, &p.key(), height, width);
        return;
    }
    if abund_table && crate::abund::is_abundance(p) {
        match crate::abund::build(ctx, p, zone) {
            Ok(t) => crate::abund::show(ui, &t, &p.key(), height, width),
            Err(e) => {
                ui.label(RichText::new(e).weak());
            }
        }
        return;
    }
    let f = ctx.figure(p);
    show_figure(ui, &f, &p.key(), gen, height, width, grid, views);
    if let Some(n) = &f.note {
        ui.label(RichText::new(n).small().weak());
    }
}

/// The legend, above the plot so that it can never cover a curve. Clicking an entry hides its curve.
fn legend_row(ui: &mut egui::Ui, f: &Figure, view: &mut PlotView, width: f32) -> f32 {
    let items: Vec<&crate::figures::Series> = f.series.iter().filter(|s| s.legend && !s.name.is_empty()).collect();
    if items.is_empty() {
        return 0.0;
    }
    let text_col = ui.visuals().text_color();
    let r = ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
        ui.set_max_width(width);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(14.0, 2.0);
            for s in items {
                let off = view.hidden.contains(&s.name);
                let dim = if off { 0.3 } else { 1.0 };
                let col = (if s.ink { text_col } else { c32(s.color) }).gamma_multiply(dim);
                let g = ui.painter().layout_no_wrap(markup_to_unicode(&s.name), plot_font(13.5), text_col.gamma_multiply(dim));
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(30.0 + g.size().x, g.size().y.max(20.0)), egui::Sense::click());
                let y = rect.center().y;
                let x = rect.left();
                let stroke = egui::Stroke::new(s.width.max(1.6), col);
                if s.markers && !s.line {
                    ui.painter().circle_filled(egui::pos2(x + 12.0, y), 5.0, col);
                } else if s.dashed {
                    for k in 0..3 {
                        let x0 = x + k as f32 * 9.0;
                        ui.painter().line_segment([egui::pos2(x0, y), egui::pos2(x0 + 6.0, y)], stroke);
                    }
                } else {
                    ui.painter().line_segment([egui::pos2(x, y), egui::pos2(x + 24.0, y)], stroke);
                }
                ui.painter().galley(egui::pos2(x + 30.0, y - g.size().y / 2.0), g, text_col);
                if resp.on_hover_text("Click to show or hide").clicked() {
                    if off {
                        view.hidden.retain(|h| h != &s.name);
                    } else {
                        view.hidden.push(s.name.clone());
                    }
                }
            }
        });
    });
    r.response.rect.height() + 4.0
}

/// Draw a figure on screen with egui_plot (log axes are drawn in log10 space).
/// Remembers each view the mouse leaves behind, and goes back through them with the "Back"
/// button drawn on the plot or with Ctrl+Z while the pointer is on the plot.
fn zoom_history(ui: &mut egui::Ui, view: &mut PlotView, r: &egui::Response, frame: egui::Rect, key: &str) {
    let Some(now) = view.bounds else { return };
    let moved = |a: &[f64; 4], b: &[f64; 4]| {
        let (w, h) = ((a[2] - a[0]).abs().max(1e-12), (a[3] - a[1]).abs().max(1e-12));
        (a[0] - b[0]).abs() > 1e-4 * w || (a[2] - b[2]).abs() > 1e-4 * w || (a[1] - b[1]).abs() > 1e-4 * h || (a[3] - b[3]).abs() > 1e-4 * h
    };
    if r.double_clicked() {
        view.history.clear();
        view.armed = 0;
    }
    if r.drag_stopped() {
        view.armed = 3;
    }
    if !r.dragged() {
        match view.settled {
            Some(old) if moved(&old, &now) => {
                if view.armed > 0 {
                    view.history.push(old);
                    view.armed = 0;
                }
                view.settled = Some(now);
            }
            None => view.settled = Some(now),
            _ => {}
        }
        view.armed = view.armed.saturating_sub(1);
    }
    // zoom out, zoom in and fit: always at hand in the top right corner
    let bs = egui::vec2(30.0, 28.0);
    let fit_w = 44.0;
    let tools = egui::Rect::from_min_size(egui::pos2(frame.right() - 2.0 * bs.x - fit_w - 12.0, frame.top() + 8.0), egui::vec2(2.0 * bs.x + fit_w + 4.0, bs.y));
    let near = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| frame.expand(8.0).contains(p));
    let mut zoom: Option<f64> = None;
    let mut fit = false;
    // shown while the pointer is over the plot, so that they never hide the data
    if near || std::env::var("INCLOUD_ZOOMTOOLS").is_ok() {
        let mut over = ui.new_child(egui::UiBuilder::new().id_salt(("zoom_tools", key)).max_rect(tools).layout(egui::Layout::left_to_right(egui::Align::Center)));
        over.spacing_mut().item_spacing.x = 2.0;
        if over.add_sized(bs, egui::Button::new("−")).on_hover_text("Zoom out: show more around this view").clicked() {
            zoom = Some(2.0);
        }
        if over.add_sized(bs, egui::Button::new("+")).on_hover_text("Zoom in on the middle (or drag a rectangle)").clicked() {
            zoom = Some(0.5);
        }
        if over.add_sized(egui::vec2(fit_w, bs.y), egui::Button::new("Fit")).on_hover_text("Fit the whole plot again (or double-click it)").clicked() {
            fit = true;
        }
    }
    if r.contains_pointer() && ui.ctx().memory(|m| m.focused().is_none()) {
        ui.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Minus) {
                zoom = Some(2.0);
            }
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Plus) || i.consume_key(egui::Modifiers::NONE, egui::Key::Equals) {
                zoom = Some(0.5);
            }
        });
    }
    if let Some(k) = zoom {
        let (cx, cy) = ((now[0] + now[2]) / 2.0, (now[1] + now[3]) / 2.0);
        let (hw, hh) = ((now[2] - now[0]) / 2.0 * k, (now[3] - now[1]) / 2.0 * k);
        view.history.push(now);
        view.restore = Some([cx - hw, cy - hh, cx + hw, cy + hh]);
        view.settled = None;
        view.armed = 0;
        ui.ctx().request_repaint();
    }
    if fit {
        view.history.clear();
        view.reset = true;
        view.armed = 0;
        ui.ctx().request_repaint();
    }
    if view.history.is_empty() {
        return;
    }
    let mut back = false;
    let size = egui::vec2(82.0, 28.0);
    let place = egui::Rect::from_min_size(egui::pos2(if near { tools.left() - size.x - 6.0 } else { frame.right() - size.x - 8.0 }, frame.top() + 8.0), size);
    let mut over = ui.new_child(egui::UiBuilder::new().id_salt(("zoom_back", key)).max_rect(place));
    if over.add_sized(size, egui::Button::new("↩ Back")).on_hover_text("Back to the previous view (Ctrl+Z)").clicked() {
        back = true;
    }
    if r.contains_pointer() && ui.ctx().memory(|m| m.focused().is_none()) {
        back |= ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z));
    }
    if back {
        if let Some(old) = view.history.pop() {
            // the first view of all is the whole plot, which follows the data again
            if view.history.is_empty() {
                view.reset = true;
            } else {
                view.restore = Some(old);
            }
            view.armed = 0;
            ui.ctx().request_repaint();
        }
    }
}

/// The view that shows every curve, in drawn coordinates (xmin, ymin, xmax, ymax): x where the
/// curves are above the floor, y from the floor (or the lowest point) to the highest point, with
/// a little room around so that nothing touches the frame.
fn fit_bounds(curves: &[&[[f64; 2]]], floor: Option<f64>, ylog: bool) -> Option<[f64; 4]> {
    let above = |y: f64| floor.is_none_or(|f| y > f + 1e-9);
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in curves.iter().flat_map(|c| c.iter()) {
        if !p[0].is_finite() || !p[1].is_finite() {
            continue;
        }
        if above(p[1]) {
            x0 = x0.min(p[0]);
            x1 = x1.max(p[0]);
            y0 = y0.min(p[1]);
            y1 = y1.max(p[1]);
        }
    }
    if x0 > x1 {
        // everything lies on the floor: show all of it
        for p in curves.iter().flat_map(|c| c.iter()).filter(|p| p[0].is_finite() && p[1].is_finite()) {
            x0 = x0.min(p[0]);
            x1 = x1.max(p[0]);
            y0 = y0.min(p[1]);
            y1 = y1.max(p[1]);
        }
    }
    if x0 > x1 {
        return None;
    }
    if let Some(f) = floor {
        y0 = y0.max(f);
    }
    let (dx, dy) = ((x1 - x0).abs(), (y1 - y0).abs());
    let (mx, my) = (if dx > 0.0 { dx * 0.03 } else { 0.5 }, if dy > 0.0 { dy * 0.07 } else if ylog { 0.5 } else { y1.abs().max(1.0) * 0.1 });
    Some([x0 - mx, y0 - my, x1 + mx, y1 + my])
}

/// Words are written afterwards, in the free space of the view being shown.
#[allow(clippy::too_many_arguments)]
fn show_figure(ui: &mut egui::Ui, f: &Figure, key: &str, gen: u64, height: f32, width: f32, grid: bool, views: &mut HashMap<String, PlotView>) {
    ui.scope(|ui| {
        // ticks, axis names and the read-out of the plot in the plot font
        let st = ui.style_mut();
        st.text_styles.insert(TextStyle::Body, plot_font(14.0));
        st.text_styles.insert(TextStyle::Small, plot_font(12.0));
        paper(ui, |ui, m| show_figure_on(ui, f, key, gen, height - m, width - m, grid, views));
    });
}

/// A font for the words of the plots on screen, as chosen in Settings.
fn plot_font(size: f32) -> FontId {
    FontId::new(size * crate::theme::get().plot_text, egui::FontFamily::Name("plot".into()))
}

/// Draw on the plot background chosen in Settings; `add` is told how much room the frame takes.
fn paper<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui, f32) -> R) -> R {
    let Some(c) = crate::theme::get().plot_bg() else { return add(ui, 0.0) };
    let col = c32(c);
    egui::Frame::new()
        .fill(col)
        .corner_radius(8.0)
        .inner_margin(8)
        .show(ui, |ui| {
            let light = 0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32 > 140.0;
            let mut v = if light { egui::Visuals::light() } else { egui::Visuals::dark() };
            v.extreme_bg_color = col;
            v.panel_fill = col;
            v.faint_bg_color = col;
            v.selection.bg_fill = ACCENT;
            ui.style_mut().visuals = v;
            add(ui, 16.0)
        })
        .inner
}

#[allow(clippy::too_many_arguments)]
fn show_figure_on(ui: &mut egui::Ui, f: &Figure, key: &str, gen: u64, height: f32, width: f32, grid: bool, views: &mut HashMap<String, PlotView>) {
    if f.is_empty() {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_min_size(egui::vec2(width - 16.0, height - 16.0));
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new(f.note.clone().unwrap_or_else(|| "No data for this plot".into())).weak());
            });
        });
        return;
    }
    let tx = |v: f64, log: bool| -> Option<f64> {
        if !v.is_finite() {
            None
        } else if log {
            (v > 0.0).then(|| v.log10())
        } else {
            Some(v)
        }
    };
    let text_col = ui.visuals().text_color();
    let sig = format!("{gen}|{}|{}|{}", f.title, f.xlabel, f.ylabel);
    let view = views.entry(key.to_string()).or_default();
    if view.sig != sig {
        *view = PlotView { sig: sig.clone(), hidden: std::mem::take(&mut view.hidden), ..Default::default() };
    }
    let reset = std::mem::take(&mut view.reset);
    if reset {
        view.history.clear();
        view.armed = 0;
    }
    let restore = view.restore.take().map(|b| egui_plot::PlotBounds::from_min_max([b[0], b[1]], [b[2], b[3]]));
    let id = format!("plot_{key}_{sig}");

    if let Some(b) = &f.bars {
        let vals: Vec<f64> = b.values.iter().filter(|v| **v > 0.0).map(|v| v.log10()).collect();
        let lo = (vals.iter().cloned().fold(f64::MAX, f64::min) - 0.3).floor();
        let top = vals.iter().cloned().fold(f64::MIN, f64::max) + 1.3;
        let n = b.labels.len();
        let bars: Vec<Bar> = b
            .values
            .iter()
            .enumerate()
            .filter(|(_, v)| **v > 0.0)
            .map(|(i, v)| Bar::new(-(i as f64), v.log10() - lo).base_offset(lo).fill(c32(b.colors[i]).gamma_multiply(0.85)).name(format!("{}: {:.3e}", b.labels[i], v)))
            .collect();
        let chart = BarChart::new("I/I(ref)", bars).horizontal().width(0.72);
        let mut plot = EPlot::new(id.as_str())
            .height(height)
            .width(width)
            .show_grid(grid)
            .x_axis_label(markup_to_unicode(&f.xlabel))
            .x_axis_formatter(|m: GridMark, _r| log_tick_label(m.value, m.step_size.max(1.0)))
            .x_grid_spacer(egui_plot::uniform_grid_spacer(|_| [1.0, 1.0, 1.0]))
            .show_y(false)
            .y_axis_formatter(|_m: GridMark, _r| String::new())
            .y_axis_min_width(4.0)
            .include_x(lo)
            .include_x(top)
            .allow_scroll(false)
            .boxed_zoom_pointer_button(egui::PointerButton::Primary)
            .pan_pointer_button(egui::PointerButton::Secondary)
            .coordinates_formatter(Corner::LeftBottom, CoordinatesFormatter::new(|p, _| format!("I/I(ref) = {}", fmt_value(p.x, true))));
        if reset {
            plot = plot.reset();
        }
        let resp = plot.show(ui, |pu| {
            if let Some(b) = restore {
                pu.set_plot_bounds(b);
            }
            pu.bar_chart(chart)
        });
        let t = &resp.transform;
        let bd = t.bounds();
        view.bounds = Some([bd.min()[0], bd.min()[1], bd.max()[0], bd.max()[1]]);
        view.zoomed = bd.max()[0] - bd.min()[0] < 0.97 * (top - lo) || bd.max()[1] - bd.min()[1] < 0.97 * (n as f64 - 0.28);
        // names and values only where the rows are tall enough for them
        let frame = *t.frame();
        let row = (t.position_from_point(&PlotPoint::new(0.0, 0.0)).y - t.position_from_point(&PlotPoint::new(0.0, 1.0)).y).abs();
        if row >= 11.0 {
            let painter = ui.painter_at(frame);
            let size = (row * 0.8).clamp(9.0, 13.5);
            for (i, v) in b.values.iter().enumerate().filter(|(_, v)| **v > 0.0) {
                let p = t.position_from_point(&PlotPoint::new(v.log10(), -(i as f64)));
                if p.y < frame.top() + size / 2.0 || p.y > frame.bottom() - size / 2.0 {
                    continue;
                }
                let txt = if row >= 17.0 { format!("{}  {}", b.labels[i], fmt_ratio(*v)) } else { b.labels[i].clone() };
                let g = painter.layout_no_wrap(txt, plot_font(size), text_col);
                let xe = p.x.min(frame.right());
                let (w, h) = (g.size().x, g.size().y);
                if xe + 5.0 + w <= frame.right() {
                    painter.galley(egui::pos2(xe + 5.0, p.y - h / 2.0), g, text_col);
                } else if xe - frame.left() > w + 8.0 {
                    let g = painter.layout_no_wrap(if row >= 17.0 { format!("{}  {}", b.labels[i], fmt_ratio(*v)) } else { b.labels[i].clone() }, plot_font(size), Color32::WHITE);
                    painter.galley(egui::pos2(xe - 5.0 - w, p.y - h / 2.0), g, Color32::WHITE);
                }
            }
        }
        zoom_history(ui, view, &resp.response, frame, key);
        return;
    }

    let legend_h = legend_row(ui, f, view, width);
    let plot_h = (height - legend_h).max(170.0);
    let floor = f.y_floor.and_then(|v| tx(v, f.ylog));
    let (xlog, ylog) = (f.xlog, f.ylog);
    // how far under the floor a curve may still be drawn (it shows when zooming out)
    let deep = if ylog { 3.0 } else { floor.map(|fl| f.y_ceil.and_then(|c| tx(c, false)).map(|c| (c - fl).abs()).unwrap_or(1.0)).unwrap_or(1.0) };
    let mut plot = EPlot::new(id.as_str())
        .height(plot_h)
        .width(width)
        .show_grid(grid)
        .x_axis_label(markup_to_unicode(&f.xlabel))
        .y_axis_label(markup_to_unicode(&f.ylabel))
        .allow_scroll(false)
        .boxed_zoom_pointer_button(egui::PointerButton::Primary)
        .pan_pointer_button(egui::PointerButton::Secondary)
        .coordinates_formatter(Corner::LeftBottom, CoordinatesFormatter::new(move |p, _| format!("x = {}   y = {}", fmt_value(p.x, xlog), fmt_value(p.y, ylog))))
        .label_formatter(move |pos: &HoverPosition<'_>| match pos {
            HoverPosition::NearDataPoint { plot_name, position, .. } if !plot_name.is_empty() => {
                Some(format!("{plot_name}\nx = {}\ny = {}", fmt_value(position.x, xlog), fmt_value(position.y, ylog)))
            }
            _ => None,
        });
    if reset {
        plot = plot.reset();
    }
    // the first view fits what there is to see: computed below from the curves
    plot = if xlog { plot.x_grid_spacer(|g| decades(g, 84.0)).x_axis_formatter(|m: GridMark, _r| log_tick_label(m.value, m.step_size)) } else { plot.x_axis_formatter(|m: GridMark, _r| lin_tick_label(m.value)) };
    plot = if ylog { plot.y_grid_spacer(|g| decades(g, 32.0)).y_axis_formatter(|m: GridMark, _r| log_tick_label(m.value, m.step_size)) } else { plot.y_axis_formatter(|m: GridMark, _r| lin_tick_label(m.value)) };

    // the curves still switched on, in drawn coordinates
    let drawn: Vec<(&crate::figures::Series, Vec<[f64; 2]>)> = f
        .series
        .iter()
        .filter(|s| !view.hidden.contains(&s.name))
        .map(|s| {
            let pts: Vec<[f64; 2]> = s
                .x
                .iter()
                .zip(&s.y)
                .filter_map(|(x, y)| {
                    let a = tx(*x, xlog)?;
                    // zero on a log axis, or far below the floor: just under the view
                    let b = match (tx(*y, ylog), floor) {
                        (Some(b), Some(fl)) => b.max(fl - deep),
                        (Some(b), None) => b,
                        (None, Some(fl)) if ylog => fl - deep,
                        (None, _) => return None,
                    };
                    Some([a, b])
                })
                .collect();
            (s, pts)
        })
        .filter(|(_, p)| !p.is_empty())
        .collect();
    let marks: Vec<(f64, &crate::figures::Mark)> = f.marks.iter().filter_map(|m| tx(m.x, xlog).map(|a| (a, m))).collect();

    // the first (and the "fit") view: every curve above the floor, with a little room around
    let fitted = fit_bounds(&drawn.iter().map(|(_, p)| p.as_slice()).collect::<Vec<_>>(), floor, ylog);
    let xb = f.x_range.map(|(a, b)| (a, b)).or(fitted.map(|b| (b[0], b[2])));
    let yb = f.y_range.map(|(a, b)| (a, b)).or(fitted.map(|b| (b[1], b[3])));
    if let Some((a, b)) = xb.filter(|(a, b)| b > a) {
        plot = plot.default_x_bounds(a, b);
    }
    if let Some((a, b)) = yb.filter(|(a, b)| b > a) {
        plot = plot.default_y_bounds(a, b);
    }

    let resp = plot.show(ui, |pu| {
        if let Some(b) = restore {
            pu.set_plot_bounds(b);
        }
        for r in &f.regions {
            if let (Some(a), Some(bb)) = (tx(r.x0, xlog), tx(r.x1, xlog)) {
                let c = r.color;
                pu.span(Span::new("", a..=bb).fill(Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (crate::theme::get().band * 255.0) as u8)).border_width(0.0));
            }
        }
        for (a, m) in &marks {
            pu.vline(VLine::new("", *a).color(c32(m.color)).style(LineStyle::Dashed { length: 6.0 }).width(1.4));
        }
        for (s, pts) in &drawn {
            let color = if s.ink { text_col } else { c32(s.color) };
            // the name of a saved line shows when the pointer is at the top of its spike
            let name = if !s.tip.is_empty() {
                s.tip.clone()
            } else if s.legend {
                markup_to_unicode(&s.name)
            } else {
                String::new()
            };
            if s.line {
                let mut l = Line::new(name.clone(), PlotPoints::from(pts.clone())).color(color).width(s.width * 1.1);
                if s.dashed {
                    l = l.style(LineStyle::Dashed { length: 9.0 });
                }
                pu.line(l);
            }
            if s.markers {
                let nm = if s.line { String::new() } else { name };
                pu.points(Points::new(nm, PlotPoints::from(pts.clone())).color(color).radius(2.5 + s.width).shape(MarkerShape::Circle).filled(true));
            }
        }
    });

    // remember the view, for "Reset zoom" and for saving just this area
    let t = &resp.transform;
    let bd = t.bounds();
    view.bounds = Some([bd.min()[0], bd.min()[1], bd.max()[0], bd.max()[1]]);
    let span = |get: &dyn Fn(&[f64; 2]) -> f64| {
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for p in drawn.iter().flat_map(|(_, pts)| pts.iter()) {
            lo = lo.min(get(p));
            hi = hi.max(get(p));
        }
        (hi - lo).max(0.0)
    };
    let full_x = xb.map(|(a, b)| b - a).unwrap_or_else(|| span(&|p| p[0]));
    let full_y = yb.map(|(a, b)| b - a).unwrap_or_else(|| span(&|p| p[1]));
    view.zoomed = bd.max()[0] - bd.min()[0] < 0.97 * full_x || bd.max()[1] - bd.min()[1] < 0.97 * full_y;

    // words go only where they touch neither a curve nor another word
    let frame = *t.frame();
    let at = |x: f64, y: f64| t.position_from_point(&PlotPoint::new(x, y));
    let mut pl = Placer::new([frame.left() as f64, frame.top() as f64, frame.right() as f64, frame.bottom() as f64]);
    pl.reserve([frame.left() as f64, frame.bottom() as f64 - 26.0, frame.left() as f64 + 290.0, frame.bottom() as f64]);
    for (s, pts) in &drawn {
        let screen: Vec<(f64, f64)> = pts.iter().map(|p| { let q = at(p[0], p[1]); (q.x as f64, q.y as f64) }).collect();
        if s.markers {
            let r = 4.0 + s.width as f64;
            for (x, y) in &screen {
                pl.reserve([x - r, y - r, x + r, y + r]);
            }
        }
        if s.line {
            pl.add_line(screen);
        }
    }
    let mut mark_x: Vec<(f64, &crate::figures::Mark)> = marks.iter().map(|(a, m)| (at(*a, 0.0).x as f64, *m)).filter(|(x, _)| *x >= frame.left() as f64 && *x <= frame.right() as f64).collect();
    mark_x.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    for (x, _) in &mark_x {
        pl.add_line([(*x, frame.top() as f64), (*x, frame.bottom() as f64)]);
    }
    let painter = ui.painter_at(frame);
    let write = |pl: &mut Placer, text: String, font: FontId, col: Color32, spots: &dyn Fn(&Placer, f64, f64) -> Vec<(f64, f64)>| {
        let g = painter.layout_no_wrap(text, font, col);
        let (w, h) = (g.size().x as f64, g.size().y as f64);
        let c = spots(pl, w, h);
        if let Some((cx, cy)) = pl.place(w, h, &c) {
            painter.galley(egui::pos2((cx - w / 2.0) as f32, (cy - h / 2.0) as f32), g, col);
        }
    };
    for (x, m) in &mark_x {
        let x = *x;
        write(&mut pl, m.label.clone(), plot_font(12.5), c32(m.color), &|p, w, h| p.beside_marker(x, w, h));
    }
    for r in &f.regions {
        if let (Some(a), Some(b)) = (tx(r.x0, xlog), tx(r.x1, xlog)) {
            let (xa, xb) = (at(a, 0.0).x as f64, at(b, 0.0).x as f64);
            write(&mut pl, markup_to_unicode(&r.label), plot_font(12.5), text_col.gamma_multiply(0.65), &|p, w, h| p.inside_band(xa, xb, w, h));
        }
    }
    for n in &f.labels {
        if let (Some(a), Some(b)) = (tx(n.x, xlog), tx(n.y, ylog)) {
            let q = at(a, b);
            let (qx, qy) = (q.x as f64, q.y as f64);
            write(&mut pl, n.text.clone(), plot_font(14.0), text_col.gamma_multiply(0.6), &|p, _w, h| p.around(qx, qy, h));
        }
    }
    for (s, pts) in drawn.iter().filter(|(s, _)| !s.tip.is_empty() && crate::theme::get().line_names) {
        if let Some(top) = pts.last() {
            let q = at(top[0], top[1]);
            if frame.contains(q) {
                let (qx, qy) = (q.x as f64, q.y as f64);
                write(&mut pl, s.tip.clone(), plot_font(12.5), c32(s.color), &|p, w, h| p.beside_tip(qx, qy, w, h));
            }
        }
    }
    zoom_history(ui, view, &resp.response, frame, key);
}

/// Light or dark theme with large controls and clear borders.
fn apply_style(ctx: &egui::Context, dark: bool) {
    let look = crate::theme::get();
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("dejavu".to_owned(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/fallback.ttf"))));
    fonts.font_data.insert("dejavuserif".to_owned(), Arc::new(egui::FontData::from_static(include_bytes!("../assets/fallback-serif.ttf"))));
    let base = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
    // the chosen font first, the others behind it for the letters it lacks
    let family = |choice: u8| -> Vec<String> {
        let mut v: Vec<String> = match choice {
            1 => vec!["dejavu".into()],
            2 => vec!["dejavuserif".into()],
            _ => vec![],
        };
        v.extend(base.iter().cloned());
        v.push("dejavu".into());
        v.dedup();
        v
    };
    fonts.families.insert(egui::FontFamily::Proportional, family(look.ui_font));
    fonts.families.insert(egui::FontFamily::Name("plot".into()), family(look.plot_font));
    if let Some(v) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
        v.push("dejavu".to_owned());
    }
    ctx.set_fonts(fonts);
    let k = look.ui_text;
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        let is_dark = theme == egui::Theme::Dark;
        ctx.style_mut_of(theme, |s| {
            s.text_styles.insert(TextStyle::Heading, FontId::proportional(22.0 * k));
            s.text_styles.insert(TextStyle::Body, FontId::proportional(15.0 * k));
            s.text_styles.insert(TextStyle::Button, FontId::proportional(15.0 * k));
            s.text_styles.insert(TextStyle::Small, FontId::proportional(12.5 * k));
            s.text_styles.insert(TextStyle::Monospace, FontId::monospace(13.5 * k));
            s.spacing.item_spacing = egui::vec2(10.0, 8.0);
            s.spacing.button_padding = egui::vec2(12.0, 6.0);
            s.spacing.interact_size = egui::vec2(44.0, 30.0);
            s.spacing.slider_width = 180.0;
            s.spacing.icon_width = 20.0;
            s.spacing.icon_width_inner = 12.0;
            let border = if is_dark { Color32::from_gray(95) } else { Color32::from_gray(175) };
            let w = &mut s.visuals.widgets;
            w.noninteractive.bg_stroke = egui::Stroke::new(1.0, if is_dark { Color32::from_gray(62) } else { Color32::from_gray(218) });
            w.inactive.bg_stroke = egui::Stroke::new(1.5, border);
            w.hovered.bg_stroke = egui::Stroke::new(2.0, ACCENT);
            w.active.bg_stroke = egui::Stroke::new(2.0, ACCENT);
            w.open.bg_stroke = egui::Stroke::new(1.5, ACCENT);
            for st in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
                st.corner_radius = egui::CornerRadius::same(7);
            }
            s.visuals.selection.bg_fill = ACCENT;
            s.visuals.selection.stroke = egui::Stroke::new(1.5, Color32::WHITE);
            s.visuals.window_stroke = egui::Stroke::new(1.0, border);
            if is_dark {
                s.visuals.panel_fill = Color32::from_rgb(27, 29, 35);
                s.visuals.window_fill = Color32::from_rgb(32, 35, 42);
                s.visuals.extreme_bg_color = Color32::from_rgb(18, 19, 24);
                s.visuals.faint_bg_color = Color32::from_rgb(35, 38, 46);
                s.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(46, 50, 60);
                s.visuals.widgets.inactive.bg_fill = Color32::from_rgb(46, 50, 60);
                s.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(58, 64, 78);
            } else {
                s.visuals.panel_fill = Color32::from_rgb(248, 249, 251);
                s.visuals.window_fill = Color32::from_rgb(252, 252, 253);
                s.visuals.extreme_bg_color = Color32::WHITE;
                s.visuals.faint_bg_color = Color32::from_rgb(240, 242, 246);
                s.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(232, 235, 241);
                s.visuals.widgets.inactive.bg_fill = Color32::from_rgb(232, 235, 241);
                s.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(220, 227, 238);
            }
        });
    }
}
