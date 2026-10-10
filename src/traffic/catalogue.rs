//! What the traffic can be made of: the vehicles (`Vehicles\…\*.bus`, `*.ovh`, trains
//! `Trains\*.zug`), the parked cars (`*.sco` in `Vehicles`), the people (`Humans\…\*.hum`)
//! and the depots (`*.hof` beside a vehicle), from the content folder and the installation.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Item {
    /// As the lists name it (`Vehicles\VW_Golf_2\AI_VW_Golf_2.bus`).
    pub rel: String,
    /// What it is called: the vehicle's `[friendlyname]` (maker, model, colour), else the
    /// file's name.
    pub name: String,
}

#[derive(Debug, Default)]
pub struct Catalogue {
    pub vehicles: Vec<Item>,
    pub parked: Vec<Item>,
    pub humans: Vec<Item>,
    roots: Vec<PathBuf>,
}

impl Catalogue {
    pub fn scan(roots: &[PathBuf]) -> Catalogue {
        let mut found: [BTreeMap<String, Item>; 3] = Default::default();
        for root in roots {
            for (top, exts, slot) in [("Vehicles", &["bus", "ovh"][..], 0), ("Trains", &["zug"][..], 0), ("Vehicles", &["sco"][..], 1), ("Humans", &["hum"][..], 2)] {
                let Some(dir) = child(root, top) else { continue };
                let mut files = Vec::new();
                walk(&dir, &mut files, 0);
                for p in files {
                    let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                    if !exts.contains(&ext.as_str()) {
                        continue;
                    }
                    let Ok(rest) = p.strip_prefix(&dir) else { continue };
                    let rel = format!("{top}\\{}", rest.to_string_lossy().replace('/', "\\"));
                    let key = rel.to_lowercase();
                    if found[slot].contains_key(&key) {
                        continue;
                    }
                    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    // (AI cars have no friendly name: their folder says what they are)
                    let folder = p.parent().and_then(|d| d.file_name()).map(|f| f.to_string_lossy().replace('_', " ")).unwrap_or_default();
                    let name = match (slot, ext.as_str()) {
                        (0, "zug") => stem,
                        (0, _) => friendly(&p).unwrap_or_else(|| format!("{folder} · {stem}")),
                        _ => format!("{folder} · {stem}"),
                    };
                    found[slot].insert(key, Item { rel, name });
                }
            }
        }
        let [v, p, h] = found;
        let sorted = |m: BTreeMap<String, Item>| {
            let mut v: Vec<Item> = m.into_values().collect();
            v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.rel.cmp(&b.rel)));
            v
        };
        Catalogue { vehicles: sorted(v), parked: sorted(p), humans: sorted(h), roots: roots.to_vec() }
    }

    pub fn name_of<'a>(&'a self, rel: &'a str) -> &'a str {
        let k = rel.to_lowercase().replace('/', "\\");
        self.vehicles.iter().chain(&self.parked).chain(&self.humans).find(|i| i.rel.to_lowercase() == k).map(|i| i.name.as_str()).unwrap_or(rel)
    }

    /// The depots a vehicle can come from: the `.hof` files beside it.
    pub fn hofs_of(&self, vehicle: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in &self.roots {
            let mut p = r.clone();
            for part in vehicle.split(['\\', '/']).filter(|x| !x.is_empty()) {
                p = child(&p, part).unwrap_or_else(|| p.join(part));
            }
            let Some(dir) = p.parent() else { continue };
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let f = e.path();
                if f.extension().map(|x| x.eq_ignore_ascii_case("hof")).unwrap_or(false) {
                    let n = f.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                    if !out.iter().any(|o| o.eq_ignore_ascii_case(&n)) {
                        out.push(n);
                    }
                }
            }
        }
        out.sort();
        out
    }
}

/// A folder's entry by name, whatever its case (the folder itself: `resolve_path` would lay
/// the content folder over the installation).
fn child(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.exists() {
        return Some(exact);
    }
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.file_name().map(|f| f.to_string_lossy().eq_ignore_ascii_case(name)).unwrap_or(false))
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 4 {
        return;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out, depth + 1);
        } else {
            out.push(p);
        }
    }
}

/// A vehicle file's `[friendlyname]`: maker, model and colour.
fn friendly(p: &Path) -> Option<String> {
    let f = omsi_cfg::CfgFile::read(p).ok()?;
    let i = f.lines.iter().position(|l| l.trim().eq_ignore_ascii_case("[friendlyname]"))?;
    let parts: Vec<&str> = f.lines.iter().skip(i + 1).take(3).map(|l| l.trim()).take_while(|l| !l.starts_with('[')).filter(|l| !l.is_empty()).collect();
    (!parts.is_empty()).then(|| match parts.as_slice() {
        [maker, model, colour] => format!("{maker} {model} ({colour})"),
        _ => parts.join(" "),
    })
}
