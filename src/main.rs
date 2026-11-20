#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod abund;
mod app;
mod batch;
mod data;
mod demo;
mod detect;
mod export;
mod figures;
mod logo;
mod matrix;
mod model;
mod network;
mod pdffind;
mod pdfview;
mod place;
mod results;
mod runinfo;
mod infoview;
mod installer;
mod deckedit;
mod runner;
mod species;
mod texbuild;
mod theme;
mod updater;
mod welcome;
mod whatsnew;
mod wiki;

use std::path::PathBuf;

fn print_info(r: &results::Results) {
    let i = &r.info;
    println!("run {} siblings {:?} only_text {}", r.prefix, r.siblings, r.info_only);
    println!("version {:?} title {:?} model {:?}", i.version, i.title, i.model);
    println!("complete {} exited_ok {:?} zones {:?} iterations {:?} time {:?} threads {:?}", i.complete, i.exited_ok, i.zones, i.iterations, i.exec_time, i.threads);
    println!("stop {:?} iteration {:?} converge asked {} converged {:?}", i.stop_reason, i.iteration, i.converge_asked, i.converged);
    for l in runinfo::Level::ALL {
        println!("{}: {}", l.plural(), i.count(l));
    }
    for m in i.messages.iter().filter(|m| m.level == runinfo::Level::Warning || m.level == runinfo::Level::Caution) {
        println!("  {:?}: {}", m.level, m.text);
    }
    println!("problems {} input lines {} gas {} dust {} molecules {} grains {} tables {:?}", i.problems.len(), i.input.len(), i.gas.len(), i.dust.len(), i.molecules.len(), i.grains.len(), i.tables.iter().map(|t| (t.title.as_str(), t.rows.len())).collect::<Vec<_>>());
    for s in &i.sections {
        println!("[{}]", s.title);
        for it in &s.items {
            println!("  {} = {}", it.name, it.value);
        }
    }
    for p in &i.passes {
        println!("pass {} stop {:?} heat {:?} err {:?} T {:?}-{:?} t {:?} notconv {:?}", p.number, p.stop, p.log_heat, p.error, p.te_low, p.te_high, p.exec, p.not_converged);
    }
    for (l, g, rel) in i.strongest.iter().take(8) {
        println!("line {l} log {g:.3} rel {rel:?}");
    }
    println!("stats {:?}", i.stats);
    for c in &r.chemistry {
        println!("rates {}: {} formation, {} destruction", c.species, c.formation.as_ref().map(|t| t.cols.len() - 1).unwrap_or(0), c.destruction.as_ref().map(|t| t.cols.len() - 1).unwrap_or(0));
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut screenshot: Option<PathBuf> = None;
    let mut open_dir: Option<PathBuf> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--screenshot" => {
                if let Some(p) = args.get(i + 1) {
                    screenshot = Some(PathBuf::from(p));
                    i += 1;
                }
            }
            "--export-demo" => {
                // developer check: load demo results (or a run folder given after the output
                // folder) and export every figure in every format, plus every data table
                let out = PathBuf::from(args.get(i + 1).cloned().unwrap_or_else(|| "export-demo".into()));
                if let Some(m) = std::env::var("INCLOUD_PLOTBG").ok().and_then(|v| v.parse::<u8>().ok()) {
                    let mut look = theme::get();
                    look.bg_mode = m;
                    look.export_bg = true;
                    theme::set(look);
                }
                if let Some(k) = std::env::var("INCLOUD_PRESET").ok().and_then(|v| v.parse::<usize>().ok()) {
                    let mut look = theme::get();
                    look.use_palette(theme::PRESETS[k.min(theme::PRESETS.len() - 1)].1);
                    theme::set(look);
                }
                let params = model::Params::default();
                let dir = runner::temp_run_dir();
                let r = match args.get(i + 2) {
                    Some(run) => results::Results::load(std::path::Path::new(run), None).expect("load run"),
                    None => {
                        let prefix = demo::write_demo(&params, &dir).expect("demo");
                        results::Results::load(&dir, Some(&prefix)).expect("load")
                    }
                };
                let style = export::Style::default();
                let ratios = figures::default_ratios();
                let customs = vec![figures::CustomSpec::default()];
                let species = r.chemistry.first().map(|c| c.species.clone()).unwrap_or_default();
                let ctx = figures::Ctx {
                    r: &r,
                    xaxis: figures::XAxis::DepthLog,
                    element: "O",
                    species: &species,
                    show_regions: true,
                    ratios: &ratios,
                    customs: &customs,
                    bpt: figures::BptKind::Nii,
                    others: &[],
                    cont_unit: figures::ContUnit::Micron,
                    sed_lines: std::env::var("INCLOUD_SEDLINES").is_ok(),
                };
                let mut plots = figures::Plot::BUILTIN.to_vec();
                plots.push(figures::Plot::Custom(0));
                for p in plots {
                    if p == figures::Plot::Network {
                        match network::build(&r, &species, network::Zone::Whole) {
                            Ok(net) => {
                                let svg = network::to_svg(&net, &style);
                                let _ = std::fs::create_dir_all(&out);
                                let _ = std::fs::write(out.join("network.svg"), &svg);
                                match export::svg_to_png(&svg, 150.0) {
                                    Ok(png) => {
                                        let _ = std::fs::write(out.join("network.png"), png);
                                        println!("ok network");
                                    }
                                    Err(e) => println!("ERR network: {e}"),
                                }
                            }
                            Err(e) => println!("ERR network: {e}"),
                        }
                        continue;
                    }
                    let f = ctx.figure(p);
                    if let Some(n) = &f.note {
                        println!("note {}: {n}", f.key);
                    }
                    for fmt in export::Format::ALL {
                        match export::export(&f, fmt, &style, &out, &f.key) {
                            Ok(p) => println!("ok {}", p.display()),
                            Err(e) => println!("ERR {} {:?}: {e}", f.key, fmt),
                        }
                    }
                }
                // the same plots cut to a zoomed view
                let zf = ctx.figure(figures::Plot::Structure);
                let _ = export::export(&export::zoomed(&zf, [18.7, -2.5, 20.0, 0.25]), export::Format::Png, &style, &out, "structure_zoom");
                let lf = ctx.figure(figures::Plot::Lines);
                let _ = export::export(&export::zoomed(&lf, [-2.0, -12.4, 1.0, -4.6]), export::Format::Png, &style, &out, "lines_zoom");
                match data::save(&r, &data::DataSet::ALL, &out, "data", false, &ratios) {
                    Ok(files) => println!("ok {} data files", files.len()),
                    Err(e) => println!("ERR data: {e}"),
                }
                let _ = std::fs::remove_dir_all(&dir);
                return Ok(());
            }
            "--run-headless" => {
                // developer check: run an executable as CLOUDY and print the progress estimate
                let exe = PathBuf::from(args.get(i + 1).cloned().unwrap_or_default());
                let params = model::Params::default();
                let dir = runner::temp_run_dir();
                let mut run = runner::Run::start(&exe, "", dir.clone(), &params.file_prefix(), &params.input_file(), &[], true).expect("start");
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(800));
                    run.poll();
                    let p = &run.progress;
                    println!("t={:.1}s it={}/{} zones={} frac={:?} remaining={:?}", run.elapsed().as_secs_f64(), p.iteration, p.iterations, p.zones, p.fraction.map(|f| (f * 100.0).round()), run.remaining().map(|d| d.as_secs()));
                    if let Some(e) = run.ended() {
                        println!("end: {e:?}");
                        break;
                    }
                }
                match results::Results::load(&dir, Some(&params.file_prefix())) {
                    Ok(r) => println!("loaded: {} zones, IF={:?}, DF={:?}, lines={}", r.depth.len(), r.fronts.ionisation, r.fronts.dissociation, r.lines.len()),
                    Err(e) => println!("load error: {e}"),
                }
                let _ = std::fs::remove_dir_all(&dir);
                return Ok(());
            }
            "--find-cloudy" => {
                for inst in runner::find_installs() {
                    println!("{}\n    exe:  {}\n    data: {}", inst.label(), inst.exe.display(), inst.data);
                }
                return Ok(());
            }
            "--sed-test" => {
                // developer check: a short real run with a custom SED, then read it back for the BPT diagram
                let exe = PathBuf::from(args.get(i + 1).cloned().unwrap_or_default());
                let data = args.get(i + 2).cloned().unwrap_or_default();
                let mut params = model::Params::default();
                params.sed = model::Sed::Custom;
                params.custom_sed.text = model::SED_EXAMPLE.to_string();
                params.stop.use_av = false;
                params.stop.use_zones = true;
                params.stop.zones = 15;
                params.iterations = 1;
                let dir = runner::temp_run_dir();
                let extra = params.extra_files().expect("sed");
                let mut run = runner::Run::start(&exe, &data, dir.clone(), &params.file_prefix(), &params.input_file(), &extra, true).expect("start");
                let end = loop {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    if let Some(e) = run.ended() {
                        break e;
                    }
                    run.poll();
                };
                println!("end: {end:?}");
                match results::Results::load(&dir, Some(&params.file_prefix())) {
                    Ok(r) => println!("loaded: {} zones, {} lines, {} chem, bpt={:?}", r.depth.len(), r.lines.len(), r.chemistry.len(), figures::bpt_point(&r, figures::BptKind::Nii)),
                    Err(e) => println!("load error: {e}"),
                }
                for run in results::scan_runs(&dir, 10) {
                    for k in figures::BptKind::ALL {
                        println!("from .out: {} {} lines {k:?} {:?}", run.name, run.lines.len(), figures::bpt_point_run(&run, k));
                    }
                }
                let _ = std::fs::remove_dir_all(&dir);
                return Ok(());
            }
            "--wiki-check" => {
                // developer check: every wiki link must find its section in the manuals
                let dir = PathBuf::from(args.get(i + 1).cloned().unwrap_or_default());
                let lib = pdfview::Library::open(&dir, &eframe::egui::Context::default()).expect("manuals");
                let (mut good, mut bad) = (0, 0);
                for t in wiki::TOPICS {
                    for (vol, q) in t.refs {
                        match lib.find(*vol, q) {
                            Some(_) => good += 1,
                            None => {
                                bad += 1;
                                println!("NOT FOUND: {} -> Hazy {vol}: {q}", t.key);
                            }
                        }
                    }
                }
                println!("{good} links found, {bad} not found, {} topics", wiki::TOPICS.len());
                return Ok(());
            }
            "--run-deck" => {
                // developer check: run an input file the way the program does and report what it finds
                let exe = PathBuf::from(args.get(i + 1).cloned().unwrap_or_default());
                let data = args.get(i + 2).cloned().unwrap_or_default();
                let deck = std::fs::read_to_string(args.get(i + 3).cloned().unwrap_or_default()).expect("input file");
                let prefix = model::deck_prefix(&deck).unwrap_or_else(|| "hand".into());
                for n in model::check_deck(&deck) {
                    println!("note line {}: {}", n.line, n.text);
                }
                let dir = runner::temp_run_dir();
                let mut run = runner::Run::start(&exe, &data, dir.clone(), &prefix, &deck, &[], true).expect("start");
                let end = loop {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    run.poll();
                    if let Some(e) = run.ended() {
                        break e;
                    }
                };
                println!("end: {}", if matches!(end, runner::RunEnd::Ok) { "finished".to_string() } else { format!("{end:?}") });
                println!("progress: iteration {} planned {} converge {}", run.progress.iteration, run.progress.iterations, run.deck.converge);
                match results::Results::load(&dir, Some(&prefix)) {
                    Ok(r) => print_info(&r),
                    Err(e) => println!("load error: {e}"),
                }
                return Ok(());
            }
            "--texbuild-check" => {
                texbuild::check(std::path::Path::new(args.get(i + 1).map(|s| s.as_str()).unwrap_or(".")));
                return Ok(());
            }
            "--batch-check" => {
                batch::check(std::path::Path::new(args.get(i + 1).map(|s| s.as_str()).unwrap_or("batch-check")), args.get(i + 2).map(|s| s.as_str()).unwrap_or("/usr/bin/cloudy.exe"));
                return Ok(());
            }
            "--whatsnew-check" => {
                // uses XDG_CONFIG_HOME, so it can be tried on a scratch folder
                match whatsnew::on_start(std::env::var("KNOWN").is_ok()) {
                    Some(w) => println!("updated from {} to {}: {} notes, {} commits, {} files, in_app {}", w.from_text(), w.to_text(), w.notes.iter().map(|n| n.1.len()).sum::<usize>(), w.commits.len(), w.files.len(), w.in_app),
                    None => println!("nothing to show; stored: {}", whatsnew::stored().is_some()),
                }
                return Ok(());
            }
            "--detect-check" => {
                detect::check(std::path::Path::new(args.get(i + 1).map(|s| s.as_str()).unwrap_or(".")));
                return Ok(());
            }
            "--info-check" => {
                // developer check: what is read from the output of a finished run
                match results::Results::load(std::path::Path::new(args.get(i + 1).map(|s| s.as_str()).unwrap_or(".")), None) {
                    Ok(r) => print_info(&r),
                    Err(e) => println!("load error: {e}"),
                }
                return Ok(());
            }
            "--install-check" => {
                // developer check: install one version of CLOUDY, printing every step
                if let Some(root) = args.get(i + 2) {
                    installer::set_root(root);
                }
                installer::check(args.get(i + 1).map(|s| s.as_str()).unwrap_or("c17.03"));
                return Ok(());
            }
            "--find-check" => {
                // developer check: read the manuals and report what some words find
                let dir = PathBuf::from(args.get(i + 1).cloned().unwrap_or_default());
                pdffind::check(&dir, args.get(i + 2..).unwrap_or(&[]));
                return Ok(());
            }
            "--deck-matrix" => {
                // developer check: input files for every choice of the panel
                let out = PathBuf::from(args.get(i + 1).cloned().unwrap_or_else(|| "matrix".into()));
                matrix::write(&out, args.get(i + 2).map(|s| s != "full").unwrap_or(true));
                return Ok(());
            }
            "--print-input" => {
                let mut p = model::Params::default();
                if let Ok(v) = std::env::var("INCLOUD_CHEM") {
                    p.chem_species = v;
                }
                print!("{}", p.input_file());
                return Ok(());
            }
            "--write-icon" => {
                // used by install.sh: the application icon as a PNG
                let out = PathBuf::from(args.get(i + 1).cloned().unwrap_or_else(|| "incloud.png".into()));
                let px = logo::rgba(256, true);
                match image::RgbaImage::from_raw(256, 256, px).map(|im| im.save(&out)) {
                    Some(Ok(())) => println!("{}", out.display()),
                    _ => eprintln!("cannot write {}", out.display()),
                }
                return Ok(());
            }
            "--update-check" => {
                // developer check: what the updater sees
                let fetch = args.iter().any(|a| a == "fetch");
                let dir = args.get(i + 1).filter(|a| *a != "fetch").map(PathBuf::from);
                let s = if dir.is_some() { updater::check_in(dir, fetch) } else { updater::check(fetch) };
                println!("built {} source {:?} git {} new {:?} changed {:?} note {:?}", updater::built_text(), s.source, s.git, s.new_commits, s.changed_files, s.note);
                return Ok(());
            }
            "--update-run" => {
                updater::run_check(false);
                return Ok(());
            }
            "--write-logo" => {
                // the logo without its dark square, for documents
                let size: u32 = args.get(i + 2).and_then(|v| v.parse().ok()).unwrap_or(256);
                let out = PathBuf::from(args.get(i + 1).cloned().unwrap_or_else(|| "logo.png".into()));
                let _ = image::RgbaImage::from_raw(size, size, logo::rgba(size, false)).map(|im| im.save(&out));
                return Ok(());
            }
            "--shrink" => {
                // a smaller copy of a picture: --shrink IN OUT WIDTH
                let (a, b) = (args.get(i + 1).cloned().unwrap_or_default(), args.get(i + 2).cloned().unwrap_or_default());
                let w: u32 = args.get(i + 3).and_then(|v| v.parse().ok()).unwrap_or(1400);
                match image::open(&a) {
                    Ok(im) => {
                        let h = (im.height() as f64 * w as f64 / im.width() as f64).round() as u32;
                        let small = image::imageops::resize(&im.to_rgba8(), w, h, image::imageops::FilterType::Lanczos3);
                        if let Err(e) = small.save(&b) {
                            eprintln!("{e}");
                        }
                    }
                    Err(e) => eprintln!("{a}: {e}"),
                }
                return Ok(());
            }
            "--version" | "-V" => {
                println!("InCloud {}", updater::VERSION);
                return Ok(());
            }
            "--help" | "-h" => {
                println!("InCloud {} — set up, run and plot CLOUDY models\n\nUsage: incloud [RUN_FOLDER]\n\n  RUN_FOLDER   open a finished CLOUDY run (a folder with its .out / .ovr files)\n  --version    print the version\n  --help       this text", updater::VERSION);
                return Ok(());
            }
            other => {
                let p = PathBuf::from(other);
                if p.is_dir() {
                    open_dir = Some(p);
                }
            }
        }
        i += 1;
    }

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("InCloud")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([900.0, 600.0])
        .with_app_id("incloud")
        .with_icon(eframe::egui::IconData { rgba: logo::rgba(128, true), width: 128, height: 128 });
    if screenshot.is_some() {
        viewport = viewport.with_active(false);
        #[cfg(target_os = "windows")]
        {
            viewport = viewport.with_window_level(eframe::egui::WindowLevel::AlwaysOnBottom);
        }
    }
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions { viewport, ..Default::default() };
    #[cfg(target_os = "macos")]
    if screenshot.is_some() {
        options.event_loop_builder = Some(Box::new(|b| {
            use eframe::egui_winit::winit::platform::macos::EventLoopBuilderExtMacOS;
            b.with_activate_ignoring_other_apps(false);
        }));
    }
    eframe::run_native(
        "InCloud",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, screenshot, open_dir)))),
    )
}
