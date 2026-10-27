//! The aerial photos OMSI's own editor lays on the ground: `maps/aerials/<the map's folder>/`
//! holds one picture per tile, named after the tile it covers (`tile_X_Y.map.aerial.jpg`), so
//! nothing about the map's files says where it lies - the name does, and the pictures are
//! simply copied into the map's own imagery folder where the overlay reads them.
//!
//! The batch downloaded by OMSI tools writes exactly those names, at whatever detail the
//! service gave (4096 px a side in some maps, 1024 in others); the picture is used as it is.

use std::path::{Path, PathBuf};

use super::{dir_of, imagery_path, TileKey};

/// The name the panel shows for pictures taken from OMSI's folder (a source's name, as the
/// downloaded ones are named).
pub const SOURCE: &str = "OMSI aerial";

/// A tile's photo beside the map's own files, as OMSI names it.
pub fn file_of(folder: &Path, k: TileKey) -> PathBuf {
    folder.join(format!("tile_{}_{}.map.aerial.jpg", k.0, k.1))
}

/// Where OMSI keeps the photos of the map at `map`, or of the map it was read from
/// (`source`, when openMapper holds a copy of it): the `aerials` folder beside the map's own.
pub fn folder_of(map: &Path, source: Option<&Path>) -> Option<PathBuf> {
    for dir in source.into_iter().chain(std::iter::once(map)) {
        let (Some(parent), Some(name)) = (dir.parent(), dir.file_name()) else { continue };
        let p = parent.join("aerials").join(name);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

/// The photos in a folder, each with the tile its name gives (anything that is not a photo
/// named after a tile is left out).
pub fn scan(folder: &Path) -> Vec<(TileKey, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut out: Vec<(TileKey, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let k = tile_of(&path)?;
            // (the name has to be the one OMSI writes for that tile: a file spelling the number
            // another way would claim a tile two files could then both claim)
            let same = matches!((path.file_name(), file_of(folder, k).file_name()), (Some(a), Some(b)) if a.eq_ignore_ascii_case(b));
            same.then_some((k, path))
        })
        .collect();
    // (a fixed order, so a map with many tiles imports the same way every time)
    out.sort_by(|a, b| (a.0 .0, a.0 .1).cmp(&(b.0 .0, b.0 .1)));
    out
}

/// The tile a photo's name gives: `tile_X_Y.map.aerial.jpg` (the coordinates may be
/// negative), else None for any other file.
pub fn tile_of(path: &Path) -> Option<TileKey> {
    let name = path.file_name()?.to_str()?.to_lowercase();
    let rest = name.strip_prefix("tile_")?.strip_suffix(".map.aerial.jpg")?;
    let (x, y) = rest.split_once('_')?;
    Some((x.parse().ok()?, y.parse().ok()?))
}

/// Why a folder's photos cannot be taken over a map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NotTaken {
    /// The folder has no photo of a tile named as OMSI names them.
    NoPhotos,
    /// It has photos, but of tiles this map does not have.
    OtherTiles,
}

/// The photos of `folder` that are of the map's `tiles`, or why there are none to take.
pub fn pick(folder: &Path, tiles: &std::collections::HashSet<TileKey>) -> Result<Vec<(TileKey, PathBuf)>, NotTaken> {
    let all = scan(folder);
    if all.is_empty() {
        return Err(NotTaken::NoPhotos);
    }
    let wanted: Vec<(TileKey, PathBuf)> = all.into_iter().filter(|(k, _)| tiles.contains(k)).collect();
    if wanted.is_empty() {
        return Err(NotTaken::OtherTiles);
    }
    Ok(wanted)
}

/// Record in `file` that these tiles show their photo now, taken at `stamp`.
pub fn note(file: &mut super::GeoFile, done: &[TileKey], stamp: u64) {
    for k in done {
        // (the picture is of this tile by its name, so it holds wherever the map lies: the
        // stamp only records the placement it was taken at)
        file.tile_mut(*k).imagery = Some(super::ImageryMeta { source: SOURCE.into(), zoom: 0, stamp, coarser: false, kept: true });
    }
}

/// Copy the photos into the map's imagery folder, one file per tile the overlay reads them
/// from. Returns the tiles that landed there; one that cannot be copied is logged and left out.
/// `each` is given how many of how many are done after each (a folder of a big map's photos is
/// a hundred megabytes: the window copies them on a thread of its own and says how far it is).
pub fn import(map: &Path, found: &[(TileKey, PathBuf)], mut each: impl FnMut(usize, usize)) -> std::io::Result<Vec<TileKey>> {
    std::fs::create_dir_all(dir_of(map).join("imagery"))?;
    let n = found.len();
    let mut done = Vec::new();
    for (i, (k, src)) in found.iter().enumerate() {
        let to = imagery_path(map, *k);
        // (already there, as a map imported once before: nothing to copy)
        if src == &to {
            done.push(*k);
        } else {
            match std::fs::copy(src, &to) {
                Ok(_) => done.push(*k),
                Err(e) => log::warn!("aerial {}: {e}", src.display()),
            }
        }
        each(i + 1, n);
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("openmapper-aerial-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_photos_name_gives_its_tile() {
        let k = |n: &str| tile_of(Path::new(n));
        assert_eq!(k("tile_20770_4174.map.aerial.jpg"), Some((20770, 4174)));
        // (a map laid west of the meridian has a negative index)
        assert_eq!(k("tile_-74_27.map.aerial.jpg"), Some((-74, 27)));
        // (the case of the name is the file system's business)
        assert_eq!(k("TILE_1_2.MAP.AERIAL.JPG"), Some((1, 2)));
        // (the map file itself, and anything else, is not a photo)
        assert_eq!(k("tile_20770_4174.map"), None);
        assert_eq!(k("README.txt"), None);
        assert_eq!(k("tile_x_1.map.aerial.jpg"), None);
        assert_eq!(k("tile_1_2_3.map.aerial.jpg"), None);
    }

    #[test]
    fn the_folder_is_the_aerials_one_beside_the_map() {
        let root = temp("folder");
        let map = root.join("maps").join("NT_merged_v2");
        let aerials = root.join("maps").join("aerials").join("NT_merged_v2");
        std::fs::create_dir_all(&aerials).unwrap();
        assert_eq!(folder_of(&map, None), Some(aerials.clone()));
        // (the map openMapper holds is a copy: the photos stay beside the map it was read from)
        assert_eq!(folder_of(&root.join("content").join("copy"), Some(&map)), Some(aerials));
        // (a map with no photos of its own, and no map it came from)
        assert_eq!(folder_of(&root.join("maps").join("plain"), None), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_photos_are_read_and_copied_where_the_overlay_looks() {
        let root = temp("import");
        let map = root.join("maps").join("m");
        let folder = root.join("maps").join("aerials").join("m");
        std::fs::create_dir_all(&map).unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        for (n, body) in [("tile_1_2.map.aerial.jpg", b"ab".as_slice()), ("tile_1_3.map.aerial.jpg", b"cde".as_slice()), ("README.txt", b"skip".as_slice())] {
            std::fs::write(folder.join(n), body).unwrap();
        }
        let found = scan(&folder);
        assert_eq!(found.len(), 2, "only the photos named after a tile are taken");
        assert_eq!(found[0].0, (1, 2));
        assert_eq!(found[1].0, (1, 3));
        // (a whole folder of photos, as a map downloaded for OMSI's editor brings)
        let done = import(&map, &found, |_, _| {}).unwrap();
        assert_eq!(done, vec![(1, 2), (1, 3)]);
        assert_eq!(std::fs::read(imagery_path(&map, (1, 2))).unwrap(), b"ab");
        assert_eq!(std::fs::read(imagery_path(&map, (1, 3))).unwrap(), b"cde");
        // (importing again copies nothing new and says the same)
        assert_eq!(import(&map, &found, |_, _| {}).unwrap(), vec![(1, 2), (1, 3)]);
        // (and it says how far it is, for a folder that takes a while)
        let mut said = Vec::new();
        import(&map, &found, |i, n| said.push((i, n))).unwrap();
        assert_eq!(said, vec![(1, 2), (2, 2)]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_photo_spelled_another_way_is_not_taken() {
        let root = temp("spelling");
        for n in ["tile_1_2.map.aerial.jpg", "tile_01_2.map.aerial.jpg", "TILE_1_3.MAP.AERIAL.JPG"] {
            std::fs::write(root.join(n), b"x").unwrap();
        }
        let found = scan(&root);
        // (only the name OMSI writes: `tile_01_2` would claim tile 1,2 a second time; the case
        // of a name is the file system's business, so `TILE_1_3` is the same photo)
        assert_eq!(found.iter().map(|(k, _)| *k).collect::<Vec<_>>(), vec![(1, 2), (1, 3)]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn taking_a_folder_leaves_the_map_reading_its_photos() {
        let root = temp("take");
        let map = root.join("content").join("maps").join("m");
        let source = root.join("omsi").join("maps").join("m");
        let folder = root.join("omsi").join("maps").join("aerials").join("m");
        for d in [&map, &source, &folder] {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(source.join("global.cfg"), b"[map]\n").unwrap();
        for n in ["tile_1_2.map.aerial.jpg", "tile_1_3.map.aerial.jpg", "tile_9_9.map.aerial.jpg"] {
            std::fs::write(folder.join(n), n.as_bytes()).unwrap();
        }
        // (the map is a copy of the one the photos lie beside)
        assert_eq!(folder_of(&map, Some(&source)), Some(folder.clone()));
        let tiles: std::collections::HashSet<TileKey> = [(1, 2), (1, 3), (5, 5)].into_iter().collect();
        let mut file = super::super::GeoFile::default();
        // (the two steps the window runs, the second on a thread: what to copy, then copy it)
        let wanted = pick(&folder, &tiles).expect("the photos of this map's tiles");
        assert_eq!(wanted.iter().map(|(k, _)| *k).collect::<Vec<_>>(), vec![(1, 2), (1, 3)]);
        let done = import(&map, &wanted, |_, _| {}).unwrap();
        note(&mut file, &done, 7);
        // (the photo of a tile the map does not have is left out)
        assert!(!file.tiles.contains_key("9_9"));
        assert!(!file.tiles.contains_key("5_5"));
        for k in [(1, 2), (1, 3)] {
            let m = file.tile(k).imagery.expect("the tile shows its photo");
            assert_eq!((m.source.as_str(), m.kept, m.stamp), (SOURCE, true, 7));
            // (exactly where the overlay looks for a kept picture)
            assert!(imagery_path(&map, k).is_file());
        }
        // (saved beside the map and read back: the panel finds the photos again)
        file.save(&map).unwrap();
        let back = super::super::GeoFile::load(&map);
        assert_eq!(back, file);
        assert!(back.tile((1, 2)).imagery.map(|m| m.kept).unwrap_or(false));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_folder_of_other_tiles_is_not_taken() {
        let root = temp("other");
        let map = root.join("map");
        let folder = root.join("photos");
        std::fs::create_dir_all(&map).unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        let file = super::super::GeoFile::default();
        let tiles: std::collections::HashSet<TileKey> = [(1, 2)].into_iter().collect();
        // (nothing there at all)
        assert_eq!(pick(&folder, &tiles), Err(NotTaken::NoPhotos));
        // (photos, but of another map's tiles)
        std::fs::write(folder.join("tile_40_50.map.aerial.jpg"), b"x").unwrap();
        assert_eq!(pick(&folder, &tiles), Err(NotTaken::OtherTiles));
        assert!(file.tiles.is_empty(), "nothing is recorded for a tile that is not there");
        assert!(!imagery_path(&map, (40, 50)).exists(), "nothing is copied either");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The Hong Kong content's folder for OMSI's editor: one photo a tile, as the overlay needs.
    #[test]
    fn a_real_maps_photos_are_all_its_tiles() {
        let Some(omsi) = crate::install::find_omsi_root(&crate::install::Prefs::default()) else {
            eprintln!("no OMSI 2 installation: skipped");
            return;
        };
        let map = omsi.join("maps").join("NT_merged_v2");
        if !map.is_dir() {
            eprintln!("no NT_merged_v2: skipped");
            return;
        }
        let folder = folder_of(&map, None).expect("OMSI keeps the photos of a map beside it");
        let found = scan(&folder);
        assert!(!found.is_empty(), "the photos are found ({})", folder.display());
        // (the name the scanner reads is the name OMSI writes)
        assert!(found.iter().all(|(k, p)| p == &file_of(&folder, *k) && p.is_file()));
        // (every photo names a tile the map has, which is what makes them layable as they are)
        let tiles: std::collections::HashSet<TileKey> = std::fs::read_dir(&map)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().to_lowercase();
                let rest = n.strip_prefix("tile_")?.strip_suffix(".map")?;
                let (x, y) = rest.split_once('_')?;
                Some((x.parse().ok()?, y.parse().ok()?))
            })
            .collect();
        assert!(!tiles.is_empty(), "the map has tiles");
        let known = found.iter().filter(|(k, _)| tiles.contains(k)).count();
        assert_eq!(known, found.len(), "every photo belongs to a tile of the map");
    }
}
