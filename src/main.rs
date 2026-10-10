//! openMapper: a simple map editor for openOMSI.
//!
//! Needs an installed OMSI 2. Maps are read with openOMSI's own readers and saved in OMSI's
//! format into openOMSI's content folder; the original installation is never written to.

mod app;
mod assets;
mod camera;
mod check;
mod clipboard;
mod doc;
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
mod place;
mod render;
mod reshape;
mod roads;
mod selection;
mod timetable;
mod ui;
mod ui_edit;
mod ui_minimap;
mod ui_timetable;

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
