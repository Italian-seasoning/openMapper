//! The map's AI traffic: the vehicle groups and depots of `ailists.cfg`, which groups drive
//! about unscheduled (`unsched_vehgroups.txt`) and how much (`unsched_trafficdens.txt`), the
//! parked cars (`parklist_p.txt`) and the people (`humans.txt`, `drivers.txt`). Read with
//! openOMSI's own `omsi-map`, written in OMSI 2's formats; only what was changed is written.

pub mod catalogue;

use std::collections::BTreeSet;
use std::path::Path;

use omsi_cfg::CfgFile;
use omsi_map::ailists::{AiTypGroup, AiVehicleEntry, UnschedGroup};
use omsi_map::{AiGroup, DepotEntry};

use crate::timetable::encode;

/// The files this module writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum File {
    AiLists,
    VehGroups,
    Density,
    Parked,
    Humans,
    Drivers,
}

impl File {
    pub fn name(self) -> &'static str {
        match self {
            File::AiLists => "ailists.cfg",
            File::VehGroups => "unsched_vehgroups.txt",
            File::Density => "unsched_trafficdens.txt",
            File::Parked => "parklist_p.txt",
            File::Humans => "humans.txt",
            File::Drivers => "drivers.txt",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Traffic {
    /// Car groups and depots, in the file's order.
    pub groups: Vec<AiGroup>,
    /// Unscheduled traffic: group, default density class (0: none by default, 1: the
    /// first class…).
    pub veh_groups: Vec<(String, i32)>,
    /// Unscheduled traffic per group: a factor and its curves over the day.
    pub density: Vec<UnschedGroup>,
    pub parked: Vec<String>,
    pub humans: Vec<String>,
    pub drivers: Vec<String>,
    /// The map has `unsched_vehgroups.txt`: without it the first group drives about.
    pub has_veh_groups: bool,
    /// The file was in OMSI 2's first format (`[ailist]`, `[aigroup]`): it is written in
    /// the second when it is changed.
    pub legacy: bool,
    pub dirty: bool,
    changed: BTreeSet<File>,
}

impl Traffic {
    /// Read the files of the map folder `dir`.
    pub fn load(dir: &Path) -> Traffic {
        let read = |name: &str| CfgFile::read(omsi_cfg::resolve_path(dir, name)).ok();
        let mut t = Traffic::default();
        if let Some(f) = read("ailists.cfg") {
            t.legacy = is_legacy(&f);
            t.groups = if t.legacy { parse_legacy(&f) } else { omsi_map::AiLists::parse(&f).groups };
        }
        if let Some(f) = read("unsched_vehgroups.txt") {
            t.veh_groups = omsi_map::ailists::parse_unsched_vehgroups(&f);
            t.has_veh_groups = true;
        }
        if let Some(f) = read("unsched_trafficdens.txt") {
            t.density = omsi_map::ailists::parse_unsched_trafficdens(&f);
        }
        let list = |name: &str| omsi_map::ailists::load_list(&omsi_cfg::resolve_path(dir, name));
        t.parked = list("parklist_p.txt");
        t.humans = list("humans.txt");
        t.drivers = list("drivers.txt");
        t
    }

    pub fn touch(&mut self, f: File) {
        self.changed.insert(f);
        self.dirty = true;
    }

    /// Put back as it was (undo, redo): every file of it is written again.
    pub fn restored(&mut self) {
        self.changed = [File::AiLists, File::Parked, File::Humans, File::Drivers].into_iter().collect();
        if self.has_veh_groups {
            self.changed.extend([File::VehGroups, File::Density]);
        }
        self.dirty = true;
    }

    pub fn depots(&self) -> impl Iterator<Item = &AiGroup> {
        self.groups.iter().filter(|g| g.is_depot)
    }

    /// The unscheduled traffic's share of group `name`: (drives about, default class,
    /// factor).
    pub fn unscheduled(&self, name: &str) -> (bool, i32, f32) {
        let f = self.density.iter().find(|g| g.name.eq_ignore_ascii_case(name)).map(|g| g.factor).unwrap_or(1.0);
        if !self.has_veh_groups {
            // (OMSI then takes the default group: the first)
            return (self.groups.first().map(|g| g.name.eq_ignore_ascii_case(name)).unwrap_or(false), 1, f);
        }
        let v = self.veh_groups.iter().find(|g| g.0.eq_ignore_ascii_case(name));
        (v.is_some(), v.map(|v| v.1).unwrap_or(1), f)
    }

    /// Let group `name` drive about unscheduled (or not), with a factor on the density.
    pub fn set_unscheduled(&mut self, name: &str, on: bool, class: i32, factor: f32) {
        if !self.has_veh_groups {
            // the list starts with what drove about without it
            if let Some(first) = self.groups.first().filter(|g| !g.is_depot) {
                self.veh_groups = vec![(first.name.clone(), 1)];
            }
            self.has_veh_groups = true;
        }
        let i = self.veh_groups.iter().position(|g| g.0.eq_ignore_ascii_case(name));
        match (on, i) {
            (true, Some(i)) => self.veh_groups[i].1 = class,
            (true, None) => self.veh_groups.push((name.to_string(), class)),
            (false, Some(i)) => {
                self.veh_groups.remove(i);
            }
            (false, None) => {}
        }
        match self.density.iter_mut().find(|g| g.name.eq_ignore_ascii_case(name)) {
            Some(g) => g.factor = factor,
            None if on => self.density.push(UnschedGroup { name: name.to_string(), factor, densities: Vec::new() }),
            None => {}
        }
        self.touch(File::VehGroups);
        self.touch(File::Density);
    }

    /// A group's new name, everywhere it is named.
    pub fn rename_group(&mut self, i: usize, new: &str) {
        let old = std::mem::replace(&mut self.groups[i].name, new.to_string());
        for g in &mut self.veh_groups {
            if g.0.eq_ignore_ascii_case(&old) {
                g.0 = new.to_string();
            }
        }
        for g in &mut self.density {
            if g.name.eq_ignore_ascii_case(&old) {
                g.name = new.to_string();
            }
        }
        self.touch(File::AiLists);
        self.touch(File::VehGroups);
        self.touch(File::Density);
    }

    /// A name no group has yet.
    pub fn unique_name(&self, base: &str) -> String {
        let taken = |n: &str| self.groups.iter().any(|g| g.name.eq_ignore_ascii_case(n));
        if !taken(base) {
            return base.to_string();
        }
        (2..).map(|i| format!("{base} {i}")).find(|n| !taken(n)).unwrap()
    }
}

/// OMSI 2's first format: `[ailist]` with the vehicles, `[aigroup]` naming them by number.
fn is_legacy(f: &CfgFile) -> bool {
    f.lines.iter().any(|l| {
        let l = l.trim().to_ascii_lowercase();
        l == "[ailist]" || l == "[aigroup]"
    })
}

/// The first format as groups of the second: each `[aigroup]` (name, count, the vehicles'
/// numbers in `[ailist]`, its depot) with weight 1 per vehicle; with no default group
/// (-1) the vehicles in no group make "NotInGroup", as OMSI does.
fn parse_legacy(f: &CfgFile) -> Vec<AiGroup> {
    let mut r = f.reader().with_rule(omsi_cfg::KeywordRule::AnyCase);
    let mut list: Vec<String> = Vec::new();
    let mut default = -1;
    let mut groups: Vec<AiGroup> = Vec::new();
    let mut used = BTreeSet::new();
    while let Some(k) = r.next_keyword() {
        match k.as_str() {
            "ailist" => {
                let _ = r.str();
                default = r.str().trim().parse::<i32>().unwrap_or(-1);
                let n = r.str().trim().parse::<usize>().unwrap_or(0).min(10_000);
                list = (0..n).map(|_| r.str().trim().to_string()).collect();
            }
            "aigroup" => {
                let name = r.str().trim().to_string();
                let n = r.str().trim().parse::<usize>().unwrap_or(0).min(10_000);
                let idx: Vec<usize> = (0..n).filter_map(|_| r.str().trim().parse::<usize>().ok()).collect();
                let hof = r.str().trim().to_string();
                let vehicles = idx
                    .iter()
                    .filter_map(|&i| {
                        used.insert(i);
                        list.get(i).map(|f| AiVehicleEntry { file: f.clone(), weight: 1.0, ..Default::default() })
                    })
                    .collect();
                groups.push(AiGroup { name, vehicles, hof: (!hof.is_empty()).then_some(hof), ..Default::default() });
            }
            _ => {}
        }
    }
    if default < 0 {
        let rest: Vec<AiVehicleEntry> = list.iter().enumerate().filter(|(i, f)| !used.contains(i) && !f.is_empty()).map(|(_, f)| AiVehicleEntry { file: f.clone(), weight: 1.0, ..Default::default() }).collect();
        if !rest.is_empty() {
            groups.push(AiGroup { name: "NotInGroup".into(), vehicles: rest, ..Default::default() });
        }
    }
    groups
}

/// A number the way the files have them (whole ones without a point).
fn num(v: f32) -> String {
    if v.fract() == 0.0 && v.abs() < 1e9 {
        format!("{}", v as i64)
    } else {
        format!("{v:.3}")
    }
}

pub fn ailists_text(groups: &[AiGroup]) -> String {
    let mut o = format!("File created with openMapper {}\n\n", env!("CARGO_PKG_VERSION"));
    for g in groups {
        if g.is_depot {
            o.push_str(&format!("[aigroup_depot]\n{}\n{}\n\n", g.name, g.hof.as_deref().unwrap_or("")));
            for tg in &g.typgroups {
                o.push_str(&format!("[aigroup_depot_typgroup_2]\n{}\n", tg.file));
                for e in &tg.entries {
                    o.push_str(&depot_line(e));
                    o.push('\n');
                }
                o.push_str("[end]\n\n");
            }
        } else {
            o.push_str(&format!("[aigroup_2]\n{}\n{}\n", g.name, g.hof.as_deref().unwrap_or("")));
            for v in &g.vehicles {
                o.push_str(&format!("{}\t{}\n", v.file, num(v.weight)));
            }
            o.push_str("[end]\n\n");
        }
    }
    o
}

/// A depot vehicle's line: fleet number, plate, repaint, first and last day (tabs between,
/// the empty ones at the end left off).
pub fn depot_line(e: &DepotEntry) -> String {
    let date = |d: Option<i32>| d.map(|d| d.to_string()).unwrap_or_default();
    let mut f = vec![e.number.clone(), e.registration.clone(), e.paint.clone(), date(e.from), date(e.to)];
    while f.len() > 1 && f.last().map(|x| x.is_empty()).unwrap_or(false) {
        f.pop();
    }
    f.join("\t")
}

pub fn vehgroups_text(groups: &[(String, i32)]) -> String {
    let mut o = String::from("\tUnscheduled Vehicle Group (UVG)\n\n\t[group]\n\t<aigroup-name>\n\t<default density? 0 = no traffic by default, 1 = use density of first UVG by default, 2 = ...second..., ...;\n\t\tif this is the first UVG, then 1 = use density class Medium by default\n\n\n");
    for (n, c) in groups {
        o.push_str(&format!("[group]\n{n}\n{c}\n\n"));
    }
    o
}

pub fn density_text(groups: &[UnschedGroup]) -> String {
    let mut o = String::from("\n\t[group]\n\t<aigroup-name>\n\t<density factor> will be multiplied with traffic density functions\n\n\t[set_day_of_week]\n\t0 = all days - else: +1 = weekday, +2 = saturday, +4 = sunday\n\n\t[trafficdensity]\n\t<time> (hours)\n\t<density> (can be > 1 !)\n\n\n");
    for g in groups {
        o.push_str(&format!("[group]\n{}\n{}\n\n", g.name, g.factor));
        for (mask, curve) in &g.densities {
            o.push_str(&format!("[set_day_of_week]\n{mask}\n\n"));
            for (h, d) in curve {
                o.push_str(&format!("[trafficdensity]\n{h:.3}\n{d:.3}\n\n"));
            }
        }
    }
    o
}

pub fn list_text(items: &[String]) -> String {
    items.iter().map(|s| format!("{s}\n")).collect()
}

/// Write what changed into the map folder.
pub fn save_to(t: &mut Traffic, map_dir: &Path) -> std::io::Result<()> {
    if !t.dirty {
        return Ok(());
    }
    let changed = std::mem::take(&mut t.changed);
    for f in &changed {
        let text = match f {
            File::AiLists => ailists_text(&t.groups),
            File::VehGroups => vehgroups_text(&t.veh_groups),
            File::Density => density_text(&t.density),
            File::Parked => list_text(&t.parked),
            File::Humans => list_text(&t.humans),
            File::Drivers => list_text(&t.drivers),
        };
        let p = omsi_cfg::resolve_path(map_dir, f.name());
        std::fs::write(p, encode(&text))?;
    }
    if changed.contains(&File::AiLists) {
        t.legacy = false;
    }
    t.dirty = false;
    if !changed.is_empty() {
        log::info!("traffic: {} file(s) written to {}", changed.len(), map_dir.display());
    }
    Ok(())
}

/// Write what changed into the map's folder.
pub fn save(doc: &mut crate::doc::Document) -> std::io::Result<()> {
    let dir = doc.dir.clone();
    save_to(&mut doc.traffic, &dir)
}

/// A new car group.
pub fn car_group(name: &str) -> AiGroup {
    AiGroup { name: name.to_string(), ..Default::default() }
}

/// The bus openMapper's own depot runs: OMSI 2's MAN city bus, in the Grundorf depot's
/// livery.
pub const DEPOT_BUS: &str = "Vehicles\\MAN_NL_NG\\MAN_GN92_main.bus";

/// A depot for a map without one: six of `DEPOT_BUS` with Grundorf's fleet numbers.
pub fn stock_depot(name: &str) -> AiGroup {
    let mut d = depot(name, "Grundorf", DEPOT_BUS);
    d.typgroups[0].entries = (1..=6).map(|n| DepotEntry { number: (2530 + n).to_string(), registration: format!("GRN-V {}", 30 + n), ..Default::default() }).collect();
    d
}

/// A new depot with one type of bus.
pub fn depot(name: &str, hof: &str, bus: &str) -> AiGroup {
    AiGroup {
        name: name.to_string(),
        hof: Some(hof.to_string()),
        is_depot: true,
        typgroups: vec![AiTypGroup { file: bus.to_string(), entries: vec![DepotEntry { number: "1".into(), ..Default::default() }] }],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests;
