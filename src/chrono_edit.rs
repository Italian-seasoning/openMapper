//! Changes made in a chrono scenario rather than the map, as OMSI 2's editor does it: what
//! is selected put into the scenario (it appears from the scenario's first day), taken away
//! from that day on (`[selobject]` / `[selspline]` with `[delete]`), or given another type
//! (`[typ]`). Each is written into the scenario's patch of the tile at once.

use std::path::{Path, PathBuf};

use omsi_map::Tile;

use crate::doc::save::{object, spline, utf16};
use crate::doc::{Document, Sel, TileKey};

/// What a patch gets.
pub enum Change {
    /// Objects and splines that appear (the map's own copies are taken away by the caller).
    Add(Tile),
    /// The map's objects and splines (IDCode, is a spline) that go from the scenario's day.
    Delete(Vec<(i64, bool)>),
    /// The map's objects (and splines) that take another type.
    Retype(Vec<(i64, bool)>, String),
}

/// The text a change adds to a patch.
fn text(c: &Change) -> String {
    let mut s = String::new();
    match c {
        Change::Add(t) => {
            for sp in t.splines.iter().filter(|s| !s.deleted) {
                spline(&mut s, sp);
            }
            for o in &t.objects {
                object(&mut s, o, false);
            }
            for o in &t.attach_objects {
                object(&mut s, o, true);
            }
        }
        Change::Delete(ids) => {
            for (id, is_spline) in ids {
                s.push_str(&format!("[{}]\n{id}\n\n[delete]\n\n", if *is_spline { "selspline" } else { "selobject" }));
            }
        }
        Change::Retype(ids, file) => {
            for (id, is_spline) in ids {
                s.push_str(&format!("[{}]\n{id}\n\n[typ]\n{file}\n\n", if *is_spline { "selspline" } else { "selobject" }));
            }
        }
    }
    s
}

/// Add a change to the scenario folder `dir`'s patch of the tile kept in `file` (made when
/// it has none: only its version, so the tile keeps the map's ground).
pub fn write(dir: &Path, file: &str, c: &Change) -> anyhow::Result<PathBuf> {
    let p = omsi_cfg::resolve_path(dir, file);
    let old = std::fs::read(&p).map(|b| omsi_cfg::decode_text(&b)).unwrap_or_default();
    let mut t = if old.trim().is_empty() { format!("{}[version]\n14\n\n", crate::doc::save::header()) } else { old.replace("\r\n", "\n") };
    if !t.ends_with("\n\n") {
        t.push('\n');
    }
    t.push_str(&text(c));
    std::fs::create_dir_all(dir)?;
    crate::doc::safe::write_atomic(&p, &utf16(&t))?;
    Ok(p)
}

/// The selection, tile by tile: the file each tile is kept in, and the selected things there.
pub fn by_tile(doc: &Document, sels: &[Sel]) -> Vec<(TileKey, String, Vec<Sel>)> {
    let mut out: Vec<(TileKey, String, Vec<Sel>)> = Vec::new();
    for s in sels {
        let k = match *s {
            Sel::Object(k, _) | Sel::Spline(k, _) => k,
        };
        let file = doc.global.tiles.iter().find(|t| (t.x, t.y) == k).map(|t| t.file.clone()).unwrap_or_else(|| crate::doc::save::tile_file(k));
        match out.iter_mut().find(|o| o.0 == k) {
            Some(o) => o.2.push(*s),
            None => out.push((k, file, vec![*s])),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use omsi_map::{MapObject, MapSpline};

    /// What a scenario gets is what the game reads from its patch: new things, and the
    /// map's things taken away or of another type.
    #[test]
    fn patches_read_back_as_written() {
        let dir = std::env::temp_dir().join(format!("openmapper-patch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut add = Tile::default();
        add.objects.push(MapObject { file: "Sceneryobjects\\a.sco".into(), id: 50, pos: [1.0, 2.0, 0.0], ..Default::default() });
        add.splines.push(MapSpline { file: "Splines\\r.sli".into(), id: 51, length: 10.0, ..Default::default() });
        write(&dir, "tile_0_0.map", &Change::Add(add)).unwrap();
        write(&dir, "tile_0_0.map", &Change::Delete(vec![(7, false), (8, true)])).unwrap();
        let p = write(&dir, "tile_0_0.map", &Change::Retype(vec![(9, false)], "Sceneryobjects\\b.sco".into())).unwrap();
        let patch = Tile::load(&p).unwrap();
        assert!(!patch.has_terrain, "the map's ground stays");
        assert_eq!((patch.objects.len(), patch.splines.len()), (1, 1));
        let mut base = Tile::default();
        base.objects.push(MapObject { file: "x.sco".into(), id: 7, ..Default::default() });
        base.objects.push(MapObject { file: "x.sco".into(), id: 9, ..Default::default() });
        base.splines.push(MapSpline { file: "y.sli".into(), id: 8, ..Default::default() });
        assert_eq!(base.apply_chrono(&patch), 0);
        assert!(!base.objects.iter().any(|o| o.id == 7));
        assert!(base.splines.iter().any(|s| s.id == 8 && s.deleted));
        assert_eq!(base.objects.iter().find(|o| o.id == 9).unwrap().file, "Sceneryobjects\\b.sco");
        assert!(base.objects.iter().any(|o| o.id == 50));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
