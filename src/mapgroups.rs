//! Groups on the map: things put together under a name (a stop with its shelter and signs,
//! a house with its fence) that a click selects and a drag moves as one. OMSI 2's files have
//! no such thing, so they are kept beside the map in `<map>/openMapper/groups.json` (the
//! game ignores that folder), by IDCode.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::doc::{Document, Sel};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapGroup {
    pub name: String,
    pub ids: Vec<i64>,
}

fn file(map_dir: &Path) -> PathBuf {
    crate::geo::dir_of(map_dir).join("groups.json")
}

pub fn load(map_dir: &Path) -> Vec<MapGroup> {
    std::fs::read_to_string(file(map_dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(map_dir: &Path, list: &[MapGroup]) -> std::io::Result<()> {
    let p = file(map_dir);
    if list.is_empty() {
        return match std::fs::remove_file(&p) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    std::fs::create_dir_all(p.parent().unwrap())?;
    crate::doc::safe::write_atomic(&p, serde_json::to_string_pretty(list).unwrap_or_default().as_bytes())
}

pub fn id_of(s: Sel) -> i64 {
    match s {
        Sel::Object(_, id) | Sel::Spline(_, id) => id,
    }
}

/// What on the map has this IDCode now (an object or a road; it may have gone to another
/// tile).
pub fn sel_of(doc: &Document, id: i64) -> Option<Sel> {
    doc.tiles.iter().find_map(|(k, t)| {
        if t.tile.objects.iter().chain(&t.tile.attach_objects).any(|o| o.id == id) {
            Some(Sel::Object(*k, id))
        } else if t.tile.splines.iter().any(|s| s.id == id && !s.deleted) {
            Some(Sel::Spline(*k, id))
        } else {
            None
        }
    })
}

/// The group a thing belongs to.
pub fn group_of(groups: &[MapGroup], id: i64) -> Option<usize> {
    groups.iter().position(|g| g.ids.contains(&id))
}

/// A selection grown to the whole of every group it touches (what is not on the map any
/// more left out), the first thing kept first.
pub fn expand(doc: &Document, groups: &[MapGroup], sels: &[Sel]) -> Vec<Sel> {
    let mut out: Vec<Sel> = Vec::new();
    for s in sels {
        if !out.contains(s) {
            out.push(*s);
        }
        if let Some(g) = group_of(groups, id_of(*s)) {
            for id in &groups[g].ids {
                if let Some(m) = sel_of(doc, *id) {
                    if !out.contains(&m) {
                        out.push(m);
                    }
                }
            }
        }
    }
    out
}

/// A name no group has yet ("Group 3").
pub fn free_name(groups: &[MapGroup], base: &str) -> String {
    (1..).map(|n| format!("{base} {n}")).find(|n| !groups.iter().any(|g| &g.name == n)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_group_grows_a_selection_and_keeps_the_first_first() {
        let mut doc = crate::selection::tests::flat_map(2);
        for (k, id) in [((0, 0), 1), ((0, 0), 2), ((1, 0), 3)] {
            doc.tiles.get_mut(&k).unwrap().tile.objects.push(omsi_map::MapObject { id, ..Default::default() });
        }
        let groups = vec![MapGroup { name: "Stop".into(), ids: vec![1, 3, 99] }];
        assert_eq!(expand(&doc, &groups, &[Sel::Object((1, 0), 3)]), [Sel::Object((1, 0), 3), Sel::Object((0, 0), 1)]);
        assert_eq!(expand(&doc, &groups, &[Sel::Object((0, 0), 2)]), [Sel::Object((0, 0), 2)]);
        assert_eq!(free_name(&groups, "Group"), "Group 1");
    }
}
