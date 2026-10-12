//! openMapper: a simple map editor for openOMSI.
//!
//! Needs an installed OMSI 2. Maps are read with openOMSI's own readers and saved in OMSI's
//! format into openOMSI's content folder; the original installation is never written to.

mod app;
mod assets;
mod camera;
mod check;
mod clipboard;
mod descriptions;
mod doc;
mod draw;
mod export;
mod geo;
mod editor;
mod gpu;
mod history;
mod i18n;
mod icons;
mod import;
mod install;
mod junction;
mod lanes;
mod library;
mod models;
mod pbr_maker;
mod parallel;
mod place;
mod plants;
mod render;
mod reshape;
mod road_maker;
mod roads;
mod roundabout;
mod rows;
mod selection;
mod signals;
mod switches;
mod texts;
mod timetable;
mod traffic;
mod ui;
mod ui_assets;
mod ui_edit;
mod ui_find;
mod ui_groups;
mod ui_geo;
mod sample;
mod ui_guide;
mod ui_calendar;
mod ui_chrono;
mod ui_mapsettings;
mod ui_signals;
mod ui_minimap;
mod ui_pbr;
mod ui_roads;
mod ui_newmap;
mod ui_timetable;
mod ui_traffic;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn,naga=warn")).init();
    // `openmapper --check path/to/global.cfg` checks a map and prints what it found
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.get(1).map(|a| a == "--check").unwrap_or(false) {
        let Some(cfg) = args.get(2) else {
            eprintln!("openmapper --check path/to/global.cfg");
            std::process::exit(2);
        };
        std::process::exit(check::cli(std::path::Path::new(cfg)));
    }
    // `openmapper --export path/to/global.cfg out.zip` / `--import file` (no window)
    match (args.get(1).and_then(|a| a.to_str()), args.get(2), args.get(3)) {
        (Some("--export"), Some(cfg), Some(out)) => std::process::exit(export::cli_export(std::path::Path::new(cfg), std::path::Path::new(out))),
        (Some("--import"), Some(f), _) => std::process::exit(export::cli_import(std::path::Path::new(f))),
        _ => {}
    }
    // `openmapper path/to/global.cfg` opens that map
    let open = std::env::args_os().nth(1).map(std::path::PathBuf::from).filter(|p| p.is_file());
    let event_loop = match winit::event_loop::EventLoop::new() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("openMapper: {e}");
            std::process::exit(1);
        }
    };
    let mut app = app::App::new(open);
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("openMapper: {e}");
    }
}
