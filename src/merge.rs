//! Tiles from another map: its ground, painting, water, objects and roads put into this
//! map at a place chosen, with fresh IDCodes (links among the tiles taken kept, those
//! running out of them cut), its ground textures found among this map's or added after
//! them in the same order, its start points along. Lines, timetable and AI lists stay with
//! the other map. Both maps must lie on the plain grid (no `[worldcoordinates]`: their
//! tiles are scaled by latitude).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use omsi_map::{GlobalCfg, TERRAIN_SAMPLES};

use crate::doc::{mask_size, next_rev, Document, TileDoc, TileKey};
use crate::history::History;
use crate::i18n::tr;

/// The other map, its tiles not read yet.
pub struct Source {
    pub global: GlobalCfg,
    pub dir: PathBuf,
    /// The tiles it has a file for.
    pub keys: Vec<TileKey>,
    pub name: String,
}

pub fn open(cfg: &Path) -> Result<Source, String> {
    let global = GlobalCfg::load(cfg).map_err(|e| e.to_string())?;
    if global.world_coordinates {
        return Err(tr("This map lies on the world's grid ([worldcoordinates]): its tiles cannot be taken yet", "Эта карта лежит на мировой сетке ([worldcoordinates]): её тайлы пока нельзя взять").into());
    }
    let dir = cfg.parent().unwrap_or(Path::new(".")).to_path_buf();
    let keys = global.tiles.iter().filter(|t| dir.join(&t.file).is_file()).map(|t| (t.x, t.y)).collect();
    let name = if global.name.is_empty() { dir.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default() } else { global.name.clone() };
    Ok(Source { global, dir, keys, name })
}

/// Where each of the other map's ground layers goes in this one, in order: the same
/// texture (and mask size) further down the list than the last one found, else a new layer
/// at the end. Its base ground stays the base when it is the same texture; else it becomes
/// a layer painted all over.
fn layers(doc: &mut Document, src: &GlobalCfg) -> Vec<usize> {
    let mut out = Vec::new();
    let mut floor = 0;
    for (li, g) in src.ground_textures.iter().enumerate() {
        let same = |t: &omsi_map::GroundTex| t.texture.eq_ignore_ascii_case(&g.texture) && mask_size(t) == mask_size(g);
        if li == 0 && doc.global.ground_textures.first().map(|b| b.texture.eq_ignore_ascii_case(&g.texture)).unwrap_or(false) {
            out.push(0);
            continue;
        }
        let at = doc.global.ground_textures.iter().enumerate().skip(floor + 1).find(|(_, t)| same(t)).map(|(i, _)| i);
        let i = at.unwrap_or_else(|| {
            doc.global.ground_textures.push(g.clone());
            doc.global.ground_textures.len() - 1
        });
        out.push(i);
        floor = i;
    }
    doc.sync_mask_slots();
    out
}

/// The ground near a new tile's edges eased to meet the tiles beside it (the difference at
/// the edge fades out over some 60 m), so no step is left where the maps meet.
fn blend_edges(doc: &Document, k: TileKey, t: &mut TileDoc) {
    let s = TERRAIN_SAMPLES;
    let band = 12;
    let mut add = vec![0.0f32; s * s];
    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        let Some(nb) = doc.tiles.get(&(k.0 + dx, k.1 + dy)) else { continue };
        for i in 0..s {
            let (mine, theirs) = match (dx, dy) {
                (-1, _) => ((0, i), (s - 1, i)),
                (1, _) => ((s - 1, i), (0, i)),
                (_, -1) => ((i, 0), (i, s - 1)),
                _ => ((i, s - 1), (i, 0)),
            };
            let d = nb.terrain.height_at(theirs.0, theirs.1) - t.terrain.height_at(mine.0, mine.1);
            for j in 0..band {
                let (x, y) = match (dx, dy) {
                    (-1, _) => (j, i),
                    (1, _) => (s - 1 - j, i),
                    (_, -1) => (i, j),
                    _ => (i, s - 1 - j),
                };
                let w = 1.0 - j as f32 / band as f32;
                // (a corner between two sides takes the larger pull)
                let v = d * w * w * (3.0 - 2.0 * w);
                if v.abs() > add[y * s + x].abs() {
                    add[y * s + x] = v;
                }
            }
        }
    }
    for (h, a) in t.terrain.heights.iter_mut().zip(add) {
        *h += a;
    }
}

/// The other map's tiles `chosen` put into this one moved by `offset` tiles and raised by
/// `dz` metres, in one undo step; `abs`: whether an object stands at a height of its own
/// (else on the ground). Tiles that would land on one of this map's are left out. How many
/// tiles were added.
#[allow(clippy::too_many_arguments)]
pub fn add_tiles(doc: &mut Document, hist: &mut History, src: &Source, chosen: &[TileKey], offset: (i32, i32), dz: f32, blend: bool, abs: impl Fn(&str) -> bool) -> Result<usize, String> {
    if doc.global.world_coordinates {
        return Err(tr("This map lies on the world's grid ([worldcoordinates]): tiles cannot be added from another yet", "Эта карта лежит на мировой сетке ([worldcoordinates]): тайлы из другой пока нельзя добавить").into());
    }
    let to = |k: TileKey| (k.0 + offset.0, k.1 + offset.1);
    let mut read: Vec<(TileKey, TileDoc)> = Vec::new();
    for t in src.global.tiles.iter().filter(|t| chosen.contains(&(t.x, t.y)) && !doc.tiles.contains_key(&to((t.x, t.y)))) {
        if let Some(td) = crate::doc::load_tile(&src.dir, &src.global, &[], t) {
            read.push(((t.x, t.y), td));
        }
    }
    if read.is_empty() {
        return Ok(0);
    }
    hist.begin(tr("Tiles from another map", "Тайлы из другой карты"));
    hist.touch_global(doc);
    let layer = layers(doc, &src.global);
    // fresh IDCodes for all they hold
    let mut ids: HashMap<i64, i64> = HashMap::new();
    for (_, t) in &read {
        let tile = &t.tile;
        for id in tile.objects.iter().map(|o| o.id).chain(tile.attach_objects.iter().map(|o| o.id)).chain(tile.splines.iter().map(|s| s.id)).chain(tile.spline_attachments.iter().map(|a| a.id)) {
            ids.insert(id, doc.new_id());
        }
    }
    let link = |id: i64| ids.get(&id).copied().unwrap_or(0);
    let n_layers = doc.global.ground_textures.len();
    let mut added = Vec::new();
    for (k, mut t) in read {
        let nk = to(k);
        hist.touch_tile(doc, nk);
        let tile = &mut t.tile;
        for o in &mut tile.objects {
            o.id = link(o.id);
            o.var_parent = o.var_parent.and_then(|p| ids.get(&p).copied());
            if abs(&o.file) {
                o.pos[2] += dz as f64;
            }
        }
        // (what hangs on something left behind goes too)
        tile.attach_objects.retain(|a| a.parent_id.map(|p| ids.contains_key(&p)).unwrap_or(false));
        for a in &mut tile.attach_objects {
            a.id = link(a.id);
            a.parent_id = a.parent_id.map(link);
        }
        for s in &mut tile.splines {
            s.id = link(s.id);
            s.prev_id = link(s.prev_id);
            s.next_id = link(s.next_id);
            s.pos[2] += dz as f64;
        }
        for a in &mut tile.spline_attachments {
            a.id = link(a.id);
        }
        // (the other map's scenarios and files are not this one's)
        tile.chrono_changes.clear();
        tile.terrain_from = None;
        tile.water_from = None;
        t.chrono.clear();
        t.terrain.heights.iter_mut().for_each(|h| *h += dz);
        t.water = t.water.map(|w| w.map(|h| h + dz));
        // the painting, layer by layer into this map's
        let mut masks: Vec<Option<Vec<u8>>> = vec![None; n_layers];
        for (li, &ti) in layer.iter().enumerate() {
            if ti == 0 {
                continue;
            }
            let size = mask_size(&doc.global.ground_textures[ti]);
            masks[ti] = if li == 0 {
                Some(vec![255; size * size])
            } else {
                t.masks.get(li).cloned().flatten().map(|m| {
                    let from = (m.len() as f64).sqrt() as usize;
                    crate::doc::resample_mask(&m, from, size)
                })
            };
        }
        t.masks = masks;
        if blend {
            blend_edges(doc, nk, &mut t);
        }
        t.terrain_rev = next_rev();
        t.mask_rev = next_rev();
        t.content_rev = next_rev();
        t.dirty = true;
        doc.tiles.insert(nk, t);
        if !doc.order.contains(&nk) {
            doc.order.push(nk);
        }
        added.push(nk);
    }
    // its start points on the tiles taken
    let eps: Vec<omsi_map::EntryPoint> = src.global.entry_points.iter().filter(|e| ids.contains_key(&e.object_id) && added.iter().any(|k| doc.tiles[k].tile.objects.iter().any(|o| o.id == ids[&e.object_id]))).map(|e| omsi_map::EntryPoint { object_id: ids[&e.object_id], ..e.clone() }).collect();
    if !eps.is_empty() {
        doc.global.entry_points.extend(eps);
        crate::editor::sync_entry_points(doc);
    }
    doc.global_dirty = true;
    doc.global_rev = next_rev();
    hist.commit();
    Ok(added.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::tests::flat_map;

    #[test]
    fn layers_are_found_in_order_or_added_and_masks_resampled() {
        let mut doc = flat_map(1);
        let g = |t: &str, p: f32| omsi_map::GroundTex { texture: t.into(), detail_texture: String::new(), params: [p, 16.0, 4.0] };
        doc.global.ground_textures = vec![g("grass.bmp", 8.0), g("sand.bmp", 8.0), g("rock.bmp", 8.0)];
        doc.sync_mask_slots();
        let src = GlobalCfg { ground_textures: vec![g("Grass.bmp", 8.0), g("rock.bmp", 8.0), g("sand.bmp", 8.0), g("snow.bmp", 7.0)], ..Default::default() };
        // rock found at 2; sand only above it would keep the order: added, as is snow
        assert_eq!(layers(&mut doc, &src), vec![0, 2, 3, 4]);
        assert_eq!(doc.global.ground_textures.len(), 5);
        assert!(doc.tiles.values().all(|t| t.masks.len() == 5));
        // another base ground becomes a layer painted all over
        let src = GlobalCfg { ground_textures: vec![g("mud.bmp", 8.0)], ..Default::default() };
        assert_eq!(layers(&mut doc, &src), vec![5]);
        let m: Vec<u8> = (0..16).map(|i| i as u8).collect();
        // (shrunk: each block's mean; grown: smooth, the corners kept)
        assert_eq!(crate::doc::resample_mask(&m, 4, 2), vec![2, 4, 10, 12]);
        let big = crate::doc::resample_mask(&m, 4, 8);
        assert_eq!((big.len(), big[0], big[63]), (64, 0, 15));
        assert!(big.windows(2).take(7).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn a_step_at_the_edge_is_eased_out() {
        let mut doc = flat_map(1);
        let mut t = TileDoc::new_flat(10.0);
        let s = TERRAIN_SAMPLES;
        // the map's tile (0, 0) at 0 m lies west of the new tile (1, 0) at 10 m
        blend_edges(&doc, (1, 0), &mut t);
        assert!(t.terrain.height_at(0, 30).abs() < 1e-4);
        assert!((t.terrain.height_at(s - 1, 30) - 10.0).abs() < 1e-4);
        assert!((t.terrain.height_at(30, 30) - 10.0).abs() < 1e-4);
        let row: Vec<f32> = (0..s).map(|x| t.terrain.height_at(x, 30)).collect();
        assert!(row.windows(2).all(|w| w[1] >= w[0] - 1e-5));
        doc.tiles.clear();
    }

    #[test]
    fn tiles_of_another_map_come_with_fresh_ids_links_and_start_points() {
        let roots = crate::doc::tests::scratch("merge");
        let mut other = crate::doc::tests::sample_map(&roots);
        crate::doc::save(&mut other).unwrap();
        let src = open(&other.dir.join("global.cfg")).unwrap();
        assert_eq!(src.keys.len(), 3);
        let mut doc = flat_map(1);
        doc.global.ground_textures = vec![omsi_map::GroundTex { texture: "Texture\\other.bmp".into(), detail_texture: String::new(), params: [0.0, 1.0, 60.0] }];
        doc.sync_mask_slots();
        let mut hist = History::default();
        let all = src.keys.clone();
        // (one would land on this map's own tile: left out)
        let n = add_tiles(&mut doc, &mut hist, &src, &all, (0, 0), 5.0, false, |_| false).unwrap();
        assert_eq!(n, 2);
        hist.undo(&mut doc);
        assert_eq!(doc.tiles.len(), 1);
        let n = add_tiles(&mut doc, &mut hist, &src, &all, (1, 0), 5.0, true, |_| false).unwrap();
        assert_eq!(n, 3);
        let t = &doc.tiles[&(1, 0)];
        assert_eq!(t.tile.objects.len(), 2);
        assert!(t.tile.objects.iter().all(|o| o.id >= 100 && o.id != 7 && o.id != 8));
        // the road across the two tiles still runs on from one into the other
        let a = &t.tile.splines[0];
        let b = &doc.tiles[&(2, 0)].tile.splines[0];
        assert_eq!((a.next_id, b.prev_id), (b.id, a.id));
        assert!((a.pos[2] - 7.0).abs() < 1e-9);
        // the other base ground painted all over as a layer, its sand after it
        assert_eq!(doc.global.ground_textures.len(), 3);
        assert!(t.masks[1].as_ref().unwrap().iter().all(|a| *a == 255));
        assert!(t.masks[2].as_ref().unwrap().iter().any(|a| *a == 200));
        assert_eq!(t.water.map(|w| w[0]), Some(6.5));
        // its start point along, on the new object
        let ep = doc.global.entry_points.iter().find(|e| e.name == "Start 1").unwrap();
        assert!(t.tile.objects.iter().any(|o| o.id == ep.object_id));
        // (the edge next to this map's flat tile eased down to it)
        assert!(doc.tiles[&(1, 0)].terrain.height_at(0, 30).abs() < 1e-4);
        let _ = std::fs::remove_dir_all(&roots.content);
    }
}
