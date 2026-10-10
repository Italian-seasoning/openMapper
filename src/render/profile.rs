//! `OPENMAPPER_PROFILE=1`: how long loading and each part of a frame take, logged every few
//! seconds (average, 95th percentile, worst). `OPENMAPPER_PROFILE=orbit` also turns the
//! camera round slowly, so every frame looks at something new. Frames are not held to the
//! screen's refresh while profiling.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

fn mode() -> &'static str {
    static MODE: OnceLock<String> = OnceLock::new();
    MODE.get_or_init(|| std::env::var("OPENMAPPER_PROFILE").unwrap_or_default())
}

pub fn enabled() -> bool {
    !mode().is_empty() && mode() != "0"
}

/// The camera turns by itself (a repeatable load for the measurements).
pub fn orbit() -> bool {
    mode() == "orbit"
}

/// Milliseconds since `t`.
pub fn ms(t: Instant) -> f32 {
    t.elapsed().as_secs_f32() * 1000.0
}

/// Log how long something took (only while profiling).
pub fn log_time(what: &str, t: Instant) {
    if enabled() {
        log::info!("profile: {what}: {:.1} ms", ms(t));
    }
}

#[derive(Default)]
pub struct Profile {
    times: Vec<(&'static str, Vec<f32>)>,
    counts: Vec<(&'static str, Vec<u32>)>,
    since: Option<Instant>,
    /// When the map was opened, until everything on it is built.
    pub opened: Option<Instant>,
}

impl Profile {
    /// A part of this frame took `ms` milliseconds.
    pub fn add(&mut self, name: &'static str, ms: f32) {
        if !enabled() {
            return;
        }
        match self.times.iter_mut().find(|t| t.0 == name) {
            Some(t) => t.1.push(ms),
            None => self.times.push((name, vec![ms])),
        }
    }

    /// Something was counted this frame (draw calls, instances).
    pub fn count(&mut self, name: &'static str, n: u32) {
        if !enabled() {
            return;
        }
        match self.counts.iter_mut().find(|t| t.0 == name) {
            Some(t) => t.1.push(n),
            None => self.counts.push((name, vec![n])),
        }
    }

    /// The map is wholly built: how long that took since it was opened.
    pub fn ready(&mut self) {
        if let Some(t) = self.opened.take() {
            if enabled() {
                log::info!("profile: map fully built {:.0} ms after opening", ms(t));
            }
        }
    }

    /// The frame is over; every few seconds the numbers so far are logged.
    pub fn end_frame(&mut self) {
        if !enabled() {
            return;
        }
        let since = *self.since.get_or_insert_with(Instant::now);
        if since.elapsed() < Duration::from_secs(3) {
            return;
        }
        let mut line = String::new();
        for (name, v) in &mut self.times {
            if v.is_empty() {
                continue;
            }
            v.sort_by(f32::total_cmp);
            let avg = v.iter().sum::<f32>() / v.len() as f32;
            let p95 = v[(v.len() * 95 / 100).min(v.len() - 1)];
            line.push_str(&format!(" | {name} {avg:.2} ms (p95 {p95:.2}, max {:.2})", v[v.len() - 1]));
        }
        for (name, v) in &self.counts {
            if v.is_empty() {
                continue;
            }
            let avg = v.iter().map(|&n| n as u64).sum::<u64>() / v.len() as u64;
            line.push_str(&format!(" | {name} {avg}"));
        }
        let frames = self.times.iter().map(|t| t.1.len()).max().unwrap_or(0);
        log::info!("profile: {frames} frames in {:.1} s{line}", since.elapsed().as_secs_f32());
        self.times.iter_mut().for_each(|t| t.1.clear());
        self.counts.iter_mut().for_each(|t| t.1.clear());
        self.since = Some(Instant::now());
    }
}
