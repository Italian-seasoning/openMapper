//! Scripts for maps, of the two kinds the games run:
//!
//! * openOMSI's Lua plugins: a map's own script is a plugin folder `plugins/map_<folder>/` of
//!   the content folder whose `main.lua` has a header (generated here) that makes `on` and
//!   `every` work on this map only, and the part the author writes below it;
//! * OMSI 2's object scripts (`.osc`): what a scenery object runs every frame. They are
//!   checked with openOMSI's own compiler, so what is reported is what the game would say.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::editable::{self, Source, When};
use crate::i18n::tr;

/// A problem in a script: the line (1-based, 0 when it belongs to no line) and what is wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct Diag {
    pub line: usize,
    pub message: String,
}

// ------------------------------------------------------------------------------------ Lua

/// What a Lua script is checked for: that it reads as Lua 5.4, which openOMSI runs.
pub fn check_lua(text: &str) -> Vec<Diag> {
    match full_moon::parse(text) {
        Ok(_) => Vec::new(),
        Err(errors) => errors.iter().map(|e| Diag { line: e.range().0.line(), message: e.error_message().into_owned() }).collect(),
    }
}

const MARK: &str = "-- ===== your script below =====";

/// A folder name a plugin can have.
fn plugin_name(map_folder: &str) -> String {
    let s: String = map_folder.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("map_{s}")
}

/// A map's Lua script (`plugins/map_<folder>/main.lua` of the content folder).
pub fn lua_path(content: &Path, map_folder: &str) -> PathBuf {
    content.join("plugins").join(plugin_name(map_folder)).join("main.lua")
}

fn lua_header(map_folder: &str, friendly: &str) -> String {
    let lua_string = |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace(['\r', '\n'], " "));
    let mut h = String::new();
    let _ = writeln!(h, "-- A script of the map {} (openMapper: Map > Scripts).", friendly.replace(['\r', '\n'], " "));
    let _ = writeln!(h, "-- It runs on this map only: `on` and `every` below are `omsi.on` and `omsi.every`");
    let _ = writeln!(h, "-- for this map. Everything above the line is made by openMapper; write below it.");
    let _ = writeln!(h, "local MAP = {}", lua_string(map_folder));
    let _ = writeln!(h, "local function here() return omsi.map.name() == MAP end");
    let _ = writeln!(h, "local function on(event, fn) omsi.on(event, function(...) if here() then return fn(...) end end) end");
    let _ = writeln!(h, "local function every(seconds, fn) omsi.every(seconds, function() if here() then fn() end end) end");
    let _ = writeln!(h, "{MARK}");
    h
}

/// The part of the map's script the author writes (None: the map has no script yet).
pub fn read_lua(content: &Path, map_folder: &str) -> Option<String> {
    let text = omsi_cfg::decode_text(&std::fs::read(lua_path(content, map_folder)).ok()?).replace("\r\n", "\n");
    Some(match text.find(MARK) {
        Some(i) => text[i + MARK.len()..].trim_start_matches('\n').to_string(),
        // (a file written by hand: all of it is the author's)
        None => text,
    })
}

/// Write the map's script: openMapper's header, then `body`.
pub fn write_lua(content: &Path, map_folder: &str, friendly: &str, body: &str) -> anyhow::Result<()> {
    let path = lua_path(content, map_folder);
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let text = format!("{}{}", lua_header(map_folder, friendly), body.replace("\r\n", "\n"));
    crate::doc::safe::write_atomic(&path, text.as_bytes())?;
    Ok(())
}

pub fn delete_lua(content: &Path, map_folder: &str) -> std::io::Result<()> {
    let path = lua_path(content, map_folder);
    std::fs::remove_file(&path)?;
    // (the folder, if nothing else is in it)
    if let Some(d) = path.parent() {
        let _ = std::fs::remove_dir(d);
    }
    Ok(())
}

pub struct Template {
    pub title: &'static str,
    pub text: &'static str,
}

/// Ready-made Lua scripts to start from.
pub fn lua_templates() -> Vec<Template> {
    vec![
        Template {
            title: tr("Empty script", "Пустой скрипт"),
            text: "-- Write what should happen on this map. Events: https://openomsi.org/\n-- on(\"stop_arrive\", function(name, id, number, delay) ... end)\n-- every(10, function() ... end)\n",
        },
        Template {
            title: tr("A greeting when the bus is taken", "Приветствие, когда садишься в автобус"),
            text: "on(\"vehicle\", function(name)\n  if name then\n    omsi.message(\"Welcome! You drive the \" .. name .. \" on \" .. MAP, 6)\n  end\nend)\n",
        },
        Template {
            title: tr("Announce every stop", "Объявлять каждую остановку"),
            text: "-- the stop's name and how late (or early) the bus is\non(\"stop_arrive\", function(name, id, number, delay)\n  omsi.ui.toast(string.format(\"%d. %s, %s\", number, name, omsi.fmt.delay(delay)))\nend)\n",
        },
        Template {
            title: tr("Traffic by the hour", "Движение по часам"),
            text: "-- busy in the rush hours, quiet at night (cars around the camera)\non(\"hour\", function(h)\n  if h >= 7 and h < 9 or h >= 16 and h < 18 then\n    omsi.traffic.set_density(300)\n  elseif h >= 22 or h < 5 then\n    omsi.traffic.set_density(40)\n  else\n    omsi.traffic.set_density(150)\n  end\nend)\n",
        },
        Template {
            title: tr("Weather at a time", "Погода в заданное время"),
            text: "-- the sky closes at six in the evening\non(\"hour\", function(h)\n  if h == 18 then\n    omsi.weather.set({ clouds = \"Overcast 1\" })\n  end\nend)\n",
        },
        Template {
            title: tr("Speed limit warning", "Предупреждение о скорости"),
            text: "-- once a second: over the lane's speed limit\nevery(1, function()\n  local limit = omsi.map.speed_limit()\n  local kmh = omsi.var(\"Velocity\")\n  if limit and kmh and kmh > limit + 5 then\n    omsi.message(string.format(\"Slow down: %.0f km/h, the limit is %.0f\", kmh, limit), 2)\n  end\nend)\n",
        },
        Template {
            title: tr("Next stop on the screen", "Следующая остановка на экране"),
            text: "-- on a new trip, say where it ends\non(\"trip_start\", function(trip, name, terminus)\n  omsi.message(string.format(\"Trip %d: to %s\", trip, terminus), 6)\nend)\n",
        },
    ]
}

// ------------------------------------------------------------------------------------ OMSI 2

/// The variables a script reads or writes, as `(X.L.name)` / `(X.$.name)`: the object's own
/// numbers and its texts (system variables and constants are not the object's).
pub fn used_vars(osc: &str) -> (Vec<String>, Vec<String>) {
    let (mut nums, mut strs) = (Vec::new(), Vec::new());
    for line in osc.lines() {
        // (the rest of a line after ' is a comment)
        let code = line.split('\'').next().unwrap_or("");
        let mut rest = code;
        while let Some(i) = rest.find('(') {
            rest = &rest[i + 1..];
            let Some(j) = rest.find(')') else { break };
            let inner = &rest[..j];
            let mut parts = inner.splitn(3, '.');
            if let (Some(a), Some(b), Some(name)) = (parts.next(), parts.next(), parts.next()) {
                let name = name.trim();
                if matches!(a, "L" | "S") && !name.is_empty() && !name.contains(char::is_whitespace) {
                    let list = match b {
                        "L" => Some(&mut nums),
                        "$" => Some(&mut strs),
                        _ => None,
                    };
                    if let Some(list) = list {
                        if !list.iter().any(|x: &String| x.eq_ignore_ascii_case(name)) {
                            list.push(name.to_string());
                        }
                    }
                }
            }
            rest = &rest[j + 1..];
        }
    }
    (nums, strs)
}

/// An object's scripts as its `.sco` lists them: the script files, the lists of its variables
/// and of its texts, the constant files (each as written, relative to the object's folder).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ScoScripts {
    pub scripts: Vec<String>,
    pub varlists: Vec<String>,
    pub strlists: Vec<String>,
    pub consts: Vec<String>,
}

pub fn sco_scripts(sco: &str) -> ScoScripts {
    let lines = omsi_cfg::split_lines(sco);
    let mut out = ScoScripts::default();
    let mut i = 0;
    while i < lines.len() {
        let target = match omsi_cfg::keyword_of(&lines[i]).map(str::to_ascii_lowercase).as_deref() {
            Some("script") => Some(&mut out.scripts),
            Some("varnamelist") => Some(&mut out.varlists),
            Some("stringvarnamelist") => Some(&mut out.strlists),
            Some("constfile") => Some(&mut out.consts),
            _ => None,
        };
        i += 1;
        if let Some(list) = target {
            let n: usize = lines.get(i).and_then(|l| l.trim().parse().ok()).unwrap_or(0);
            i += 1;
            for _ in 0..n {
                if let Some(l) = lines.get(i).filter(|l| omsi_cfg::keyword_of(l).is_none()) {
                    let l = l.trim();
                    if !l.is_empty() {
                        list.push(l.to_string());
                    }
                    i += 1;
                }
            }
        }
    }
    out
}

fn names_in(text: &str) -> Vec<String> {
    text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

static SCRATCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Compile `osc` with the variables `vars` and the texts `strvars` declared and the constant
/// files `consts`, as the game would; the problems it finds.
pub fn check_osc(osc: &str, vars: &[String], strvars: &[String], consts: &[PathBuf]) -> Vec<Diag> {
    let dir = std::env::temp_dir().join(format!("openmapper-osc-{}-{}", std::process::id(), SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    if std::fs::create_dir_all(&dir).is_err() {
        return Vec::new();
    }
    let script = dir.join("script.osc");
    let (v, s) = (dir.join("vars.txt"), dir.join("strings.txt"));
    let _ = std::fs::write(&script, osc.replace("\r\n", "\n").replace('\n', "\r\n"));
    let _ = std::fs::write(&v, vars.join("\r\n"));
    let _ = std::fs::write(&s, strvars.join("\r\n"));
    let program = omsi_script::compile(&omsi_script::CompileInput { varlists: vec![v], stringvarlists: vec![s], constfiles: consts.to_vec(), scripts: vec![script.clone()], ..Default::default() });
    let _ = std::fs::remove_dir_all(&dir);
    program
        .errors
        .iter()
        .map(|e| Diag { line: if e.file == script { e.line } else { 0 }, message: e.message.clone() })
        .collect()
}

/// The variables and texts an object's script lists name, and its constant files, from the
/// object's folder `dir` (what the check declares).
pub fn declared(dir: &Path, sco: &ScoScripts) -> (Vec<String>, Vec<String>, Vec<PathBuf>) {
    let read = |list: &[String]| -> Vec<String> { list.iter().flat_map(|f| std::fs::read(omsi_cfg::resolve_path(dir, f)).map(|b| names_in(&omsi_cfg::decode_text(&b))).unwrap_or_default()).collect() };
    (read(&sco.varlists), read(&sco.strlists), sco.consts.iter().map(|f| omsi_cfg::resolve_path(dir, f)).collect())
}

/// What a scripted light does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Behaviour {
    /// Lit from one hour of the day to another (a shop's sign).
    Hours { from: f32, to: f32 },
    /// Lit and dark in turn, a whole period in seconds (a beacon).
    Blink { period: f32 },
    /// Lit through the months from one to another, 1 to 12 (Christmas lights).
    Months { from: u8, to: u8 },
}

impl Behaviour {
    pub fn label(self) -> &'static str {
        match self {
            Behaviour::Hours { .. } => tr("Lit during certain hours", "Горит в определённые часы"),
            Behaviour::Blink { .. } => tr("Blinks", "Мигает"),
            Behaviour::Months { .. } => tr("Lit in certain months", "Горит в определённые месяцы"),
        }
    }

    /// The script that sets `lit`.
    pub fn osc(self) -> String {
        let mut s = String::from("' A light made in openMapper: the variable lit says whether it shines.\n' Change the rule below as you like; keep the {frame} ... {end} around it.\n\n{frame}\n");
        match self {
            Behaviour::Hours { from, to } => {
                // (past midnight when the end comes before the start)
                let join = if from <= to { "&&" } else { "||" };
                let _ = write!(s, "\t' minutes since midnight\n\t(L.S.Time) 60 / s0\n\t{} >= l0 {} < {join}\n\t(S.L.lit)\n", (from * 60.0).round(), (to * 60.0).round());
            }
            Behaviour::Blink { period } => {
                let p = period.max(0.2);
                let _ = write!(s, "\t' seconds of play, shining for the first half of every period\n\t(L.S.GetTime) {p} % {} < (S.L.lit)\n", p / 2.0);
            }
            Behaviour::Months { from, to } => {
                let join = if from <= to { "&&" } else { "||" };
                let _ = write!(s, "\t(L.S.Month) s0\n\t{from} >= l0 {to} <= {join}\n\t(S.L.lit)\n");
            }
        }
        s.push_str("{end}\n");
        s
    }
}

/// A light that a script of its own switches: where the files go and what to write.
pub fn scripted_light(color: [u8; 3], range: f32, stem: &str) -> Source {
    let mut src = crate::makers::light_source(&crate::makers::Settings { light_color: color, light_range: range, light_night_only: false, ..Default::default() });
    for l in &mut src.lights {
        l.when = When::Var("lit".into());
    }
    src.name = "Scripted light".into();
    src.extra = format!("[script]\n1\nscript\\{stem}.osc\n\n[varnamelist]\n1\nscript\\{stem}_varlist.txt\n");
    src
}

/// Write the script of a scripted light beside the object (after the object itself is saved).
pub fn write_light_script(dir: &Path, stem: &str, b: Behaviour) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir.join("script"))?;
    std::fs::write(dir.join("script").join(format!("{stem}.osc")), b.osc().replace('\n', "\r\n"))?;
    std::fs::write(dir.join("script").join(format!("{stem}_varlist.txt")), "lit\r\n")?;
    Ok(())
}

/// Save a scripted light as a new object of the map: its relative path.
pub fn make_light(roots: &crate::install::Roots, map: &str, b: Behaviour, color: [u8; 3], range: f32) -> anyhow::Result<String> {
    let rel = editable::new_rel(roots, map, "ScriptedLight");
    let (dir, stem) = editable::folder_of(roots, &rel);
    let src = scripted_light(color, range, &stem);
    editable::save(roots, &rel, &src)?;
    write_light_script(&dir, &stem, b)?;
    Ok(rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_errors_have_lines() {
        assert!(check_lua("on(\"hour\", function(h)\n  print(h)\nend)\n").is_empty());
        let d = check_lua("local x = 1\nif x then\n  print(x)\n");
        assert!(!d.is_empty(), "an if without end");
        let d = check_lua("local a = \nprint(a)\nlocal = 3");
        assert!(d.iter().any(|d| d.line >= 1));
        // Lua 5.4: integer division and goto are fine
        assert!(check_lua("local a = 7 // 2\ngoto done\n::done::").is_empty());
    }

    #[test]
    fn a_map_script_keeps_its_header_apart() {
        let dir = std::env::temp_dir().join(format!("om-script-{}", std::process::id()));
        write_lua(&dir, "My map", "My \"map\"", "on(\"hour\", function(h) end)\n").unwrap();
        let path = lua_path(&dir, "My map");
        assert!(path.ends_with("plugins/map_My_map/main.lua"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("local MAP = \"My map\""));
        assert!(check_lua(&text).is_empty(), "the whole file reads as Lua");
        assert_eq!(read_lua(&dir, "My map").as_deref(), Some("on(\"hour\", function(h) end)\n"));
        // writing again does not pile up headers
        write_lua(&dir, "My map", "My map", &read_lua(&dir, "My map").unwrap()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap().matches(MARK).count(), 1);
        delete_lua(&dir, "My map").unwrap();
        assert!(read_lua(&dir, "My map").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn templates_are_valid_lua() {
        for t in lua_templates() {
            assert!(check_lua(t.text).is_empty(), "{}", t.title);
        }
    }

    #[test]
    fn variables_are_found_in_a_script() {
        let (n, s) = used_vars("{frame}\n (L.S.Time) 60 / s0 ' (L.L.nope)\n (S.L.lit)\n (L.L.lit) (S.$.name) (C.L.k)\n{end}");
        assert_eq!(n, vec!["lit"]);
        assert_eq!(s, vec!["name"]);
    }

    #[test]
    fn the_game_compiler_checks_a_script() {
        let ok = Behaviour::Hours { from: 18.0, to: 23.0 }.osc();
        assert!(check_osc(&ok, &["lit".into()], &[], &[]).is_empty(), "{:?}", check_osc(&ok, &["lit".into()], &[], &[]));
        for b in [Behaviour::Blink { period: 2.0 }, Behaviour::Months { from: 12, to: 1 }, Behaviour::Hours { from: 22.0, to: 5.0 }] {
            let d = check_osc(&b.osc(), &["lit".into()], &[], &[]);
            assert!(d.is_empty(), "{b:?}: {d:?}");
        }
        // a variable the object does not list is a problem the game would report
        let d = check_osc("{frame}\n 1 (S.L.unknown)\n{end}\n", &[], &[], &[]);
        assert!(!d.is_empty());
    }

    #[test]
    fn an_object_lists_its_scripts() {
        let s = sco_scripts("[model]\nx.x\n\n[script]\n2\nscript\\a.osc\nscript\\b.osc\n\n[varnamelist]\n1\nscript\\v.txt\n\n[constfile]\n1\nscript\\c.cfg\n");
        assert_eq!(s.scripts, vec!["script\\a.osc", "script\\b.osc"]);
        assert_eq!(s.varlists, vec!["script\\v.txt"]);
        assert_eq!(s.consts, vec!["script\\c.cfg"]);
    }
}
