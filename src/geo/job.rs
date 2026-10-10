//! Loading imagery and heights for a list of tiles on a background thread, one tile after
//! another, telling the window how far it is.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use glam::DVec2;

use super::fetch;
use super::sources::{GeoPrefs, Source};
use super::{elevation, imagery, Georef, ImageryMeta, TerrainOpts};
use crate::doc::TileKey;
use crate::i18n::tr;

/// What to load.
pub struct Spec {
    pub dir: PathBuf,
    pub geo: Georef,
    pub ts: f64,
    /// The tiles and their terrain cells.
    pub tiles: Vec<(TileKey, usize)>,
    /// Imagery: the source, the zoom (0: the finest it has there) and the picture's edge
    /// (0: the source's own detail).
    pub imagery: Option<(Source, u8, u32)>,
    /// Heights: the source, the options and the tile whose mean is taken off.
    pub terrain: Option<(Source, TerrainOpts, TileKey)>,
    pub prefs: GeoPrefs,
}

pub enum Msg {
    /// A tile's imagery is written.
    Image(TileKey, ImageryMeta),
    /// A tile's new heights (offsets applied).
    Heights(TileKey, Vec<f32>),
    /// The height `relative` takes off, fixed now.
    Base(f32),
    /// The zoom the imagery is made at, and its detail (m a pixel).
    Zoom(u8, f64),
    Error(TileKey, String),
    /// Ended (Some: stopped by this error).
    Done(Option<String>),
}

pub struct Job {
    pub rx: Receiver<Msg>,
    pub cancel: Arc<AtomicBool>,
    pub done: Arc<AtomicUsize>,
    pub total: usize,
    pub dir: PathBuf,
}

pub fn start(spec: Spec) -> Job {
    let (tx, rx) = std::sync::mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicUsize::new(0));
    let job = Job { rx, cancel: cancel.clone(), done: done.clone(), total: spec.tiles.len(), dir: spec.dir.clone() };
    std::thread::spawn(move || {
        let end = run(&spec, &tx, &cancel, &done);
        let _ = tx.send(Msg::Done(end.err()));
    });
    job
}

fn run(spec: &Spec, tx: &Sender<Msg>, cancel: &AtomicBool, done: &AtomicUsize) -> Result<(), String> {
    let mid = spec.tiles.first().map(|t| crate::doc::tile_origin(t.0) + DVec2::splat(spec.ts * 0.5)).unwrap_or_default();
    let (lat, lon) = spec.geo.lat_lon(mid);
    // the zoom and the picture's size, the same for every tile (one source, one zoom: the
    // tiles meet without a seam)
    let img = match &spec.imagery {
        Some((src, z, size)) => {
            let z = if *z > 0 { *z } else { fetch::finest_zoom(src, &spec.prefs, lat, lon)?.unwrap_or_else(|| src.useful_zoom(lat).min(17)) };
            let mpp = src.metres_per_pixel(lat, z);
            let size = if *size > 0 { *size } else { imagery::image_size(spec.ts, mpp) };
            let _ = tx.send(Msg::Zoom(z, mpp));
            Some((src, z, size))
        }
        None => None,
    };
    let terrain = match &spec.terrain {
        Some((src, opts, anchor)) => {
            let cells = spec.tiles.first().map(|t| t.1).unwrap_or(60);
            let z = elevation::zoom_for(lat, spec.ts / cells as f64, src.max_zoom);
            let fetch = |z: u8, x: u32, y: u32| fetch::tile(src, &spec.prefs, z, x, y);
            let base = match (opts.relative, opts.base) {
                (false, _) => 0.0,
                (true, Some(b)) => b,
                (true, None) => {
                    let raw = elevation::sample_tile(&spec.geo, *anchor, spec.ts, cells, z, 0.0, &fetch, cancel)?;
                    let b = (elevation::mean(&raw) * 100.0).round() / 100.0;
                    let _ = tx.send(Msg::Base(b));
                    b
                }
            };
            Some((src, opts, z, base))
        }
        None => None,
    };
    let mut failed = 0;
    for &(k, cells) in &spec.tiles {
        if cancel.load(Ordering::Relaxed) {
            return Err(tr("Cancelled", "Отменено").into());
        }
        let one = || -> Result<(), String> {
            if let Some((src, z, size)) = &img {
                let fetch = |z: u8, x: u32, y: u32| fetch::tile(src, &spec.prefs, z, x, y);
                let (pic, coarser) = imagery::build(&spec.geo, k, spec.ts, *z, *size, &fetch, cancel)?;
                imagery::save_jpeg(&pic, &super::imagery_path(&spec.dir, k))?;
                let _ = tx.send(Msg::Image(k, ImageryMeta { source: src.name.clone(), zoom: *z, stamp: spec.geo.stamp(), coarser }));
            }
            if let Some((src, opts, z, base)) = &terrain {
                let fetch = |z: u8, x: u32, y: u32| fetch::tile(src, &spec.prefs, z, x, y);
                let raw = elevation::sample_tile(&spec.geo, k, spec.ts, cells, *z, opts.smooth as f64, &fetch, cancel)?;
                let _ = tx.send(Msg::Heights(k, raw.iter().map(|h| h - base + opts.offset).collect()));
            }
            Ok(())
        };
        match one() {
            Ok(()) => failed = 0,
            Err(e) => {
                if cancel.load(Ordering::Relaxed) {
                    return Err(e);
                }
                let _ = tx.send(Msg::Error(k, e.clone()));
                failed += 1;
                // (offline, or the server refuses: no use trying every tile)
                if failed >= 3 {
                    return Err(e);
                }
            }
        }
        done.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}
