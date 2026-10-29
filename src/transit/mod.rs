//! Public transport of Modern maps, as openOMSI (0.3.42 and later) reads it: the modes a map is
//! made for, one bare keyword each in `global.cfg` (`[tram_transport]`, …), and the network in
//! the text files of its `transport` folder (stations, routes of segments and services, track
//! rules, overhead wire and third rail, switches, depots, signals and blocks).
//!
//! A map that names a mode is a **Modern** map: openOMSI runs only vehicles of its modes on it,
//! and OMSI 2 cannot run its routes, wires or blocks (it skips the keywords and the folder). A
//! map that names none is a **Classic** map, as OMSI 2 makes them.

pub mod blocks;
pub mod kit;
pub mod lay;
pub mod osm;
pub mod overhead;
pub mod platforms;
pub mod sample;
pub mod vehicles;
pub mod wire;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub use blocks::{Block, BlockFile};

use crate::doc::safe::Op;
use crate::i18n::tr;

/// The folder of a map that holds the files.
pub const FOLDER: &str = "transport";

/// A kind of public transport (openOMSI's `TransportMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mode {
    Bus,
    Tram,
    Railway,
    Metro,
    Trolleybus,
}

impl Mode {
    pub const ALL: [Mode; 5] = [Mode::Bus, Mode::Tram, Mode::Trolleybus, Mode::Metro, Mode::Railway];

    /// As the files write it.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Bus => "bus",
            Mode::Tram => "tram",
            Mode::Railway => "railway",
            Mode::Metro => "metro",
            Mode::Trolleybus => "trolleybus",
        }
    }

    pub fn keyword(self) -> String {
        format!("{}_transport", self.name())
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Bus => tr("Bus", "Автобус"),
            Mode::Tram => tr("Tram", "Трамвай"),
            Mode::Railway => tr("Railway", "Железная дорога"),
            Mode::Metro => tr("Metro", "Метро"),
            Mode::Trolleybus => tr("Trolleybus", "Троллейбус"),
        }
    }

    /// What a map of the mode has, in a few words.
    pub fn hint(self) -> &'static str {
        match self {
            Mode::Bus => tr("buses on the streets, stops at the kerb", "автобусы на улицах, остановки у бордюра"),
            Mode::Tram => tr("street track under a contact wire, low platforms", "пути на улице под контактным проводом, низкие платформы"),
            Mode::Railway => tr("mainline track, stations with platforms, signals and blocks", "магистральные пути, станции с платформами, сигналы и блоки"),
            Mode::Metro => tr("track with a third rail, tunnels, high platforms", "пути с контактным рельсом, тоннели, высокие платформы"),
            Mode::Trolleybus => tr("buses under a pair of wires, frogs at junctions", "автобусы под двумя проводами, стрелки на развилках"),
        }
    }

    /// Runs on rails.
    pub fn is_rail(self) -> bool {
        matches!(self, Mode::Tram | Mode::Railway | Mode::Metro)
    }

    /// The mode a name stands for (`tram`, `u-bahn`, …), as openOMSI reads it.
    pub fn from_name(s: &str) -> Option<Mode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "bus" | "coach" => Some(Mode::Bus),
            "tram" | "streetcar" | "strassenbahn" | "tramway" => Some(Mode::Tram),
            "railway" | "rail" | "train" | "sbahn" | "s-bahn" | "freight" => Some(Mode::Railway),
            "metro" | "subway" | "ubahn" | "u-bahn" | "underground" => Some(Mode::Metro),
            "trolleybus" | "trolley" | "trolleybuses" | "trolleycoach" => Some(Mode::Trolleybus),
            _ => None,
        }
    }

    pub fn from_keyword(k: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.keyword().eq_ignore_ascii_case(k.trim()))
    }

    fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// A set of modes; empty: none named.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Modes(u8);

impl Modes {
    pub fn of(list: &[Mode]) -> Modes {
        let mut m = Modes::default();
        list.iter().for_each(|x| m.set(*x, true));
        m
    }

    pub fn contains(self, m: Mode) -> bool {
        self.0 & m.bit() != 0
    }

    pub fn set(&mut self, m: Mode, on: bool) {
        if on {
            self.0 |= m.bit();
        } else {
            self.0 &= !m.bit();
        }
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn iter(self) -> impl Iterator<Item = Mode> {
        Mode::ALL.into_iter().filter(move |m| self.contains(*m))
    }

    pub fn any_rail(self) -> bool {
        self.iter().any(Mode::is_rail)
    }

    /// `metro tram`, as the files write a list.
    pub fn names(self) -> String {
        self.iter().map(Mode::name).collect::<Vec<_>>().join(" ")
    }

    /// `Tram, Metro`.
    pub fn labels(self) -> String {
        self.iter().map(Mode::label).collect::<Vec<_>>().join(", ")
    }

    pub fn parse(s: &str) -> Modes {
        let mut m = Modes::default();
        for w in s.split(|c: char| c == ',' || c == ';' || c.is_whitespace()) {
            if let Some(x) = Mode::from_name(w) {
                m.set(x, true);
            }
        }
        m
    }
}

/// A file of the folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum File {
    Network,
    Stations,
    Routes,
    Catenary,
    ThirdRail,
    Switches,
    Depots,
    Signals,
    /// `trolley.txt` beside global.cfg: the trolleybus wire openOMSI's trolleybuses run on
    /// (docs/TROLLEYBUS.md; the same `[block]` lines).
    Trolley,
}

impl File {
    pub const ALL: [File; 9] = [File::Network, File::Stations, File::Routes, File::Catenary, File::ThirdRail, File::Switches, File::Depots, File::Signals, File::Trolley];

    pub fn name(self) -> &'static str {
        match self {
            File::Network => "network.cfg",
            File::Stations => "stations.cfg",
            File::Routes => "routes.cfg",
            File::Catenary => "catenary.cfg",
            File::ThirdRail => "thirdrail.cfg",
            File::Switches => "switches.cfg",
            File::Depots => "depots.cfg",
            File::Signals => "signals.cfg",
            File::Trolley => "trolley.txt",
        }
    }

    /// Where it is kept: the transport folder, or (trolley.txt) the map's own.
    pub fn path(self, map_dir: &Path) -> std::path::PathBuf {
        match self {
            File::Trolley => map_dir.join(self.name()),
            _ => map_dir.join(FOLDER).join(self.name()),
        }
    }

    fn title(self) -> &'static str {
        match self {
            File::Network => "which tracks serve which mode and how they are powered",
            File::Stations => "stations and their platforms",
            File::Routes => "routes: segments between stations and the services over them",
            File::Catenary => "the overhead wire",
            File::ThirdRail => "the third rail",
            File::Switches => "track and wire switches",
            File::Depots => "depots",
            File::Signals => "signals and blocks",
            File::Trolley => "the trolleybus wire",
        }
    }
}

/// The blocks that belong to the block before them, by file.
pub const STATION_CHILDREN: &[&str] = &["platform"];
pub const ROUTE_CHILDREN: &[&str] = &["segment", "service", "trip"];

/// A map's modes and transport folder.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Transit {
    pub modes: Modes,
    files: BTreeMap<File, BlockFile>,
    /// The files to write (or remove, when empty) on the next save.
    changed: BTreeSet<File>,
    /// Something changed since the last save (the modes are written with global.cfg).
    pub dirty: bool,
}

static EMPTY: BlockFile = BlockFile { blocks: Vec::new() };

impl Transit {
    /// The modes `global.cfg` names and the files of the `transport` folder beside it.
    pub fn load(map_dir: &Path) -> Transit {
        let mut t = Transit::default();
        if let Ok(f) = omsi_cfg::CfgFile::read(&map_dir.join("global.cfg")) {
            for l in &f.lines {
                let l = l.trim();
                if let Some(m) = l.strip_prefix('[').and_then(|r| r.strip_suffix(']')).and_then(Mode::from_keyword) {
                    t.modes.set(m, true);
                }
            }
        }
        let dir = omsi_cfg::resolve_path(map_dir, FOLDER);
        for f in File::ALL {
            let base = if f == File::Trolley { map_dir.to_path_buf() } else { dir.clone() };
            if let Ok(bytes) = std::fs::read(omsi_cfg::resolve_path(&base, f.name())) {
                t.files.insert(f, BlockFile::parse(&omsi_cfg::decode_text(&bytes)));
            }
        }
        t
    }

    /// A Modern map: it names its modes.
    pub fn is_modern(&self) -> bool {
        !self.modes.is_empty()
    }

    /// The keywords global.cfg carries.
    pub fn keywords(&self) -> Vec<String> {
        self.modes.iter().map(Mode::keyword).collect()
    }

    pub fn file(&self, f: File) -> &BlockFile {
        self.files.get(&f).unwrap_or(&EMPTY)
    }

    /// A file to change (written on the next save).
    pub fn file_mut(&mut self, f: File) -> &mut BlockFile {
        self.changed.insert(f);
        self.dirty = true;
        self.files.entry(f).or_default()
    }

    pub fn has_any_file(&self) -> bool {
        self.files.values().any(|f| !f.is_empty())
    }

    /// What saving writes into the map's folder: the changed files (an emptied one removed).
    pub fn plan(&self, map_dir: &Path) -> Vec<Op> {
        self.changed
            .iter()
            .map(|f| {
                let path = f.path(map_dir);
                match self.files.get(f).filter(|b| !b.is_empty()) {
                    Some(b) => Op::Write(path, b.text(&format!("{}: {}", f.name(), f.title())).replace('\n', "\r\n").into_bytes()),
                    None => Op::Remove(path),
                }
            })
            .filter(|o| !matches!(o, Op::Remove(p) if !p.exists()))
            .collect()
    }

    pub fn saved(&mut self) {
        self.changed.clear();
        self.dirty = false;
    }

    /// Put back by undo, redo or an autosave: the files that differ from `old` are written.
    pub fn restored(&mut self, old: &Transit) {
        for f in File::ALL {
            if self.file(f) != old.file(f) {
                self.changed.insert(f);
            }
        }
        self.changed.extend(old.changed.iter().copied());
        self.dirty = true;
    }

    /// The modes and the files as bytes, to pass on to another editor (editing together).
    pub fn to_bytes(&self) -> Vec<u8> {
        let files: BTreeMap<&str, String> = self.files.iter().filter(|(_, b)| !b.is_empty()).map(|(f, b)| (f.name(), b.text(""))).collect();
        serde_json::to_vec(&serde_json::json!({ "modes": self.modes.names(), "files": files })).unwrap_or_default()
    }

    /// Another editor's modes and files in place of these (the files that differ written on
    /// the next save). False: not readable.
    pub fn take_bytes(&mut self, bytes: &[u8]) -> bool {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) else { return false };
        let Some(modes) = v.get("modes").and_then(|m| m.as_str()) else { return false };
        let old = std::mem::take(self);
        self.modes = Modes::parse(modes);
        for f in File::ALL {
            if let Some(t) = v.get("files").and_then(|x| x.get(f.name())).and_then(|t| t.as_str()) {
                self.files.insert(f, BlockFile::parse(t));
            }
        }
        self.restored(&old);
        true
    }

    /// Every file marked to be written (a Modern map made here, or brought from elsewhere).
    pub fn mark_all(&mut self) {
        self.changed.extend(self.files.keys().copied());
        self.dirty = true;
    }

    /// The problems openOMSI's own reader finds in the files as they would be written, with
    /// its checks across them (routes against stations and depots, the map's modes).
    pub fn problems(&self) -> Vec<Problem> {
        use omsi_transit::{Depots, Electrification, Network, Routes, Signals, Stations, Switches, TransitData};
        let mut issues = Vec::new();
        let text = |f: File| self.files.get(&f).filter(|b| !b.is_empty()).map(|b| b.text(""));
        let mut data = TransitData::default();
        if let Some(t) = text(File::Network) {
            data.network = Network::parse(&t, File::Network.name(), &mut issues);
        }
        if let Some(t) = text(File::Stations) {
            data.stations = Stations::parse(&t, File::Stations.name(), &mut issues);
        }
        if let Some(t) = text(File::Depots) {
            data.depots = Depots::parse(&t, File::Depots.name(), &mut issues);
        }
        if let Some(t) = text(File::Routes) {
            data.routes = Routes::parse(&t, File::Routes.name(), &mut issues);
        }
        if let Some(t) = text(File::Catenary) {
            data.catenary = Some(Electrification::parse(&t, File::Catenary.name(), false, &mut issues));
        }
        if let Some(t) = text(File::ThirdRail) {
            data.third_rail = Some(Electrification::parse(&t, File::ThirdRail.name(), true, &mut issues));
        }
        if let Some(t) = text(File::Switches) {
            data.switches = Switches::parse(&t, File::Switches.name(), &mut issues);
        }
        if let Some(t) = text(File::Signals) {
            data.signals = Signals::parse(&t, File::Signals.name(), &mut issues);
        }
        let mut modes = omsi_cfg_transit::transport::ModeSet::EMPTY;
        for m in self.modes.iter() {
            modes.declare_keyword(&m.keyword());
        }
        data.check(modes, &mut issues);
        let mut out: Vec<Problem> = issues.into_iter().map(|i| Problem { file: i.file, error: i.error, message: i.message }).collect();
        out.extend(self.trolley_problems());
        out
    }

    /// The trolleybus wire as openOMSI's trolleybuses read it (`omsi-trolley`): spans in no
    /// section of the file (a section's id is a number there), frogs not on a joint of three
    /// spans (left out by the game).
    fn trolley_problems(&self) -> Vec<Problem> {
        let f = self.file(File::Trolley);
        if f.is_empty() {
            return Vec::new();
        }
        let net = omsi_trolley::WireNet::parse(&f.text(""));
        let p = |error: bool, message: String| Problem { file: File::Trolley.name().into(), error, message };
        let mut out = Vec::new();
        let ids: Vec<i64> = f.of("trolley_section").filter_map(|i| f.blocks[i].num("id")).map(|v| v as i64).collect();
        for i in f.of("trolley_section") {
            if f.blocks[i].num("id").is_none() {
                out.push(p(true, format!("section {}: its id must be a number", f.blocks[i].id())));
            }
        }
        let lost = f.of("trolley_span").filter(|i| !f.blocks[*i].num("section").is_some_and(|s| ids.contains(&(s as i64)))).count();
        if lost > 0 {
            out.push(p(true, format!("{lost} spans are in no section of the file: no current on them")));
        }
        let frogs = f.of("trolley_switch").count();
        if net.switches.len() < frogs {
            out.push(p(false, format!("{} of {frogs} frogs are not on a joint of three spans and are left out", frogs - net.switches.len())));
        }
        if net.is_empty() {
            out.push(p(false, "no wire laid yet (Power: wire the selected roads)".into()));
        }
        out
    }
}

/// Every station's stop object into the timetable's list of stops (`Busstops.cfg`): openOMSI
/// finds a station among those, by its object, else by its name. Whether the list changed.
pub fn sync_station_stops(doc: &mut crate::doc::Document) -> bool {
    let st = doc.transit.file(File::Stations);
    let wanted: Vec<i64> = st.of("station").filter_map(|i| st.blocks[i].get("object").and_then(|o| o.trim().parse().ok())).collect();
    if wanted.is_empty() {
        return false;
    }
    let ids: Vec<(crate::doc::TileKey, i64)> = doc.tiles.iter().flat_map(|(k, t)| t.tile.objects.iter().filter(|o| o.flag == 7 && wanted.contains(&o.id)).map(move |o| (*k, o.id))).collect();
    let entries = crate::timetable::stop_entries(doc, &ids);
    doc.timetable.sync_stops(entries)
}

/// A problem in a transport file (the line numbers of the reader are left out: openMapper
/// writes the files itself).
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub file: String,
    pub error: bool,
    pub message: String,
}

#[cfg(test)]
mod tests;
