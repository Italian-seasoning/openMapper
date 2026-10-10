use std::sync::atomic::AtomicBool;

use glam::DVec2;

use super::fetch::Tile;
use super::imagery;
use super::sources::{self, GeoPrefs};
use super::*;

fn spandau() -> Anchor {
    Anchor { tile: [0, 0], lat: 52.535_41, lon: 13.199_64, rotation: 0.0 }
}

#[test]
fn merc_round_trip() {
    for (lat, lon) in [(52.5354, 13.1996), (55.7558, 37.6173), (-33.86, 151.2), (0.0, 0.0), (70.0, -150.0)] {
        let (la, lo) = lat_lon_of_merc(merc_of(lat, lon));
        assert!((la - lat).abs() < 1e-10 && (lo - lon).abs() < 1e-10, "{lat} {lon}: {la} {lo}");
    }
    // the XYZ tile of Spandau's town hall at zoom 16 (the one openOMSI's tile 2402, 11279 is)
    let m = merc_of(52.535_41, 13.199_64);
    assert_eq!(((m.x * 65536.0) as i32, (m.y * 65536.0) as i32), (2402 + 32768, 32767 - 11279));
}

#[test]
fn local_round_trip_and_true_metres() {
    let g = Georef::local(&Anchor { rotation: 12.5, ..spandau() }, 300.0);
    for p in [DVec2::new(0.0, 0.0), DVec2::new(1500.0, -900.0), DVec2::new(-3000.0, 3000.0), DVec2::new(150.0, 150.0)] {
        let (la, lo) = g.lat_lon(p);
        let q = g.world(la, lo);
        assert!((q - p).length() < 1e-6, "{p} -> {q}");
    }
    // the anchor tile's middle is where it was put
    let (la, lo) = g.lat_lon(DVec2::new(150.0, 150.0));
    assert!((la - 52.535_41).abs() < 1e-12 && (lo - 13.199_64).abs() < 1e-12);
    // 3 km north and 3 km east are 3 km on the ellipsoid (Vincenty-grade: within 2 mm)
    let g = Georef::local(&spandau(), 300.0);
    let (la0, lo0) = g.lat_lon(DVec2::new(150.0, 150.0));
    let (la1, _) = g.lat_lon(DVec2::new(150.0, 3150.0));
    let phi = ((la0 + la1) / 2.0).to_radians();
    let arc = (la1 - la0).to_radians() * meridian_radius(phi);
    assert!((arc - 3000.0).abs() < 0.002, "{arc}");
    let (la2, lo2) = g.lat_lon(DVec2::new(3150.0, 150.0));
    assert!((la2 - la0).abs() < 1e-12);
    let par = (lo2 - lo0).to_radians() * prime_radius(la0.to_radians()) * la0.to_radians().cos();
    assert!((par - 3000.0).abs() < 0.002, "{par}");
}

#[test]
fn ten_by_ten_tiles_meet_exactly() {
    // the corners of every tile of a 10 x 10 map, reached from each of the (up to) four
    // tiles that share them: the same point on the Earth, so imagery cannot part or drift
    for g in [Georef::local(&Anchor { rotation: 7.0, ..spandau() }, 300.0), Georef::World { ts: omsi_map::world_row_width(11281) }] {
        let ts = match g {
            Georef::World { ts } => ts,
            _ => 300.0,
        };
        let mut worst: f64 = 0.0;
        for ty in -5..5 {
            for tx in -5..5 {
                let o = crate::doc::tile_origin((tx, ty));
                let _ = o;
                for (cx, cy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    // the corner as tile (tx, ty) sees it and as the tile it is the origin of
                    let a = g.merc(DVec2::new(tx as f64 * ts, ty as f64 * ts) + DVec2::new(cx as f64, cy as f64) * ts);
                    let b = g.merc(DVec2::new((tx + cx) as f64 * ts, (ty + cy) as f64 * ts));
                    worst = worst.max((a - b).length());
                }
            }
        }
        // (in Mercator units: 1e-15 is a few nanometres)
        assert!(worst < 1e-14, "{worst}");
        // and the tiles' sizes on the ground stay the same across the map (no drift)
        if let Georef::Local { .. } = g {
            let size = |tx: i32, ty: i32| {
                let (a, b) = (g.lat_lon(DVec2::new(tx as f64 * ts, ty as f64 * ts + 150.0)), g.lat_lon(DVec2::new((tx + 1) as f64 * ts, ty as f64 * ts + 150.0)));
                let phi = a.0.to_radians();
                ((b.1 - a.1).to_radians() * prime_radius(phi) * phi.cos()).hypot((b.0 - a.0).to_radians() * meridian_radius(phi))
            };
            // (true along the meridian and the parallels; turned 7 degrees and 1.5 km off
            // the anchor's meridian a tile's edge is a centimetre long: a flat map on the
            // ellipsoid cannot do better everywhere)
            for (tx, ty) in [(-5, -5), (4, 4), (-5, 4), (0, 0)] {
                assert!((size(tx, ty) - 300.0).abs() < 0.03, "{tx} {ty}: {}", size(tx, ty));
            }
        }
    }
}

#[test]
fn world_coordinates_are_openomsi_tiles() {
    let ts = omsi_map::world_row_width(11279);
    let g = Georef::World { ts };
    // the stop "U Rathaus Spandau" of tile_2402_11279.map (openOMSI's own test): its
    // local metres scaled onto the grid as openOMSI places them
    let (w0, w1) = (omsi_map::world_row_width(11279), omsi_map::world_row_width(11280));
    let (lx, ly) = (342.786665653207 / ((w0 + w1) / 2.0) * ts, 276.13607466369 / w1 * ts);
    let p = DVec2::new(2402.0 * ts + lx, 11279.0 * ts + ly);
    let (la, lo) = g.lat_lon(p);
    let (ela, elo) = omsi_map::tile_local_to_lat_lon(2402, 11279, 342.786665653207, 276.13607466369);
    assert!((la - ela).abs() < 1e-9 && (lo - elo).abs() < 1e-9, "{la} {lo} vs {ela} {elo}");
    let q = g.world(la, lo);
    assert!((q - p).length() < 1e-6);
}

#[test]
fn terrarium_decode() {
    assert_eq!(imagery::terrarium(128, 0, 0), 0.0);
    assert_eq!(imagery::terrarium(128, 34, 128), 34.5);
    assert_eq!(imagery::terrarium(127, 255, 0), -1.0);
    assert_eq!(imagery::terrarium(0, 0, 0), -32768.0);
}

/// A synthetic source: each tile of zoom `z` is a picture whose pixels encode their global
/// pixel position (so where a sample came from can be read back).
fn synthetic(z_have: u8) -> impl Fn(u8, u32, u32) -> Result<Tile, String> + Sync {
    move |z, x, y| {
        if z > z_have {
            return Ok(Tile::Missing);
        }
        let img = image::RgbImage::from_fn(256, 256, |i, j| {
            let (gx, gy) = (x * 256 + i, y * 256 + j);
            image::Rgb([(gx % 256) as u8, (gy % 256) as u8, z * 10])
        });
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out).write_image(img.as_raw(), 256, 256, image::ExtendedColorType::Rgb8).unwrap();
        Ok(Tile::Data(std::sync::Arc::new(out)))
    }
}

use image::ImageEncoder;

#[test]
fn stitched_and_cropped_exactly() {
    let g = Georef::local(&spandau(), 300.0);
    let f = synthetic(18);
    let no = AtomicBool::new(false);
    let size = 512;
    let (a, coarse) = imagery::build(&g, (0, 0), 300.0, 18, size, &f, &no).unwrap();
    assert!(!coarse);
    let (b, _) = imagery::build(&g, (1, 0), 300.0, 18, size, &f, &no).unwrap();
    let (c, _) = imagery::build(&g, (0, 1), 300.0, 18, size, &f, &no).unwrap();
    // each pixel holds the source pixel its world position falls on (red: x, green: y,
    // modulo 256): a pixel's colour against where its centre lies in the Mercator
    let px = |m: DVec2| ((m.x * 256.0 * (1 << 18) as f64), (m.y * 256.0 * (1 << 18) as f64));
    let mut worst: f64 = 0.0;
    for (i, j) in [(0u32, 0u32), (511, 0), (0, 511), (511, 511), (256, 100), (37, 400)] {
        let w = crate::doc::tile_origin((0, 0)) + DVec2::new((i as f64 + 0.5) / size as f64 * 300.0, (1.0 - (j as f64 + 0.5) / size as f64) * 300.0);
        let (sx, sy) = px(g.merc(w));
        let got = a.get_pixel(i, j).0;
        // (bilinear between neighbours of a ramp: the value is the position minus half)
        let dx = ((sx - 0.5).rem_euclid(256.0) - got[0] as f64).abs();
        let dy = ((sy - 0.5).rem_euclid(256.0) - got[1] as f64).abs();
        worst = worst.max(dx.min(256.0 - dx)).max(dy.min(256.0 - dy));
    }
    assert!(worst <= 1.0, "{worst}");
    // across the edge to the east and the north the ramps go on without a jump: the last
    // column of one and the first of the next are one output pixel apart in the source
    let step = 300.0 / size as f64 / omsi_scale(&g);
    for j in [0u32, 200, 511] {
        let (l, r) = (a.get_pixel(size - 1, j).0[0] as f64, b.get_pixel(0, j).0[0] as f64);
        let d = (r - l).rem_euclid(256.0);
        assert!((d - step).abs() <= 1.5, "east edge, row {j}: {l} -> {r} ({step})");
    }
    for i in [0u32, 300, 511] {
        let (s, n) = (a.get_pixel(i, 0).0[1] as f64, c.get_pixel(i, size - 1).0[1] as f64);
        let d = (s - n).rem_euclid(256.0);
        assert!((d - step).abs() <= 1.5, "north edge, column {i}: {s} -> {n} ({step})");
    }
}

/// Ground metres of a source pixel of zoom 18 at the anchor.
fn omsi_scale(g: &Georef) -> f64 {
    let (la, _) = g.lat_lon(DVec2::new(150.0, 150.0));
    metres_per_pixel(la, 18)
}

#[test]
fn coarser_tiles_fill_in() {
    let g = Georef::local(&spandau(), 300.0);
    let no = AtomicBool::new(false);
    // the source has only zoom 16: zoom 18 is made from it, and it says so
    let (img, coarse) = imagery::build(&g, (0, 0), 300.0, 18, 256, &synthetic(16), &no).unwrap();
    assert!(coarse);
    assert_eq!(img.get_pixel(10, 10).0[2], 160);
    // nothing at all: grey
    let (img, coarse) = imagery::build(&g, (0, 0), 300.0, 18, 256, &|_, _, _| Ok(Tile::Missing), &no).unwrap();
    assert!(coarse && img.get_pixel(5, 5).0 == [128, 128, 128]);
    // offline: the error comes back
    assert!(imagery::build(&g, (0, 0), 300.0, 18, 256, &|_, _, _| Err("offline".to_string()), &no).is_err());
}

#[test]
fn heights_meet_on_shared_edges() {
    // a smooth hill in Terrarium tiles of zoom 15
    let hill = |z: u8, x: u32, y: u32| -> Result<Tile, String> {
        let img = image::RgbImage::from_fn(256, 256, |i, j| {
            let (gx, gy) = ((x * 256 + i) as f64, (y * 256 + j) as f64);
            let h = 40.0 + 25.0 * ((gx * 0.05).sin() + (gy * 0.07).cos());
            let v = h + 32768.0;
            image::Rgb([(v / 256.0) as u8, (v % 256.0) as u8, ((v.fract()) * 256.0) as u8])
        });
        let _ = z;
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out).write_image(img.as_raw(), 256, 256, image::ExtendedColorType::Rgb8).unwrap();
        Ok(Tile::Data(std::sync::Arc::new(out)))
    };
    let g = Georef::local(&Anchor { rotation: 20.0, ..spandau() }, 300.0);
    let no = AtomicBool::new(false);
    for smooth in [0.0, 15.0] {
        let t = |k: (i32, i32)| elevation::sample_tile(&g, k, 300.0, 60, 15, smooth, &hill, &no).unwrap();
        let (a, east, north) = (t((0, 0)), t((1, 0)), t((0, 1)));
        let n = 61;
        for i in 0..n {
            assert_eq!(a[i * n + 60], east[i * n], "east edge {i}, smooth {smooth}");
            assert_eq!(a[60 * n + i], north[i], "north edge {i}, smooth {smooth}");
        }
        assert!(a.iter().all(|h| (-11.0..91.0).contains(h)), "{:?}", &a[..5]);
    }
    assert_eq!(elevation::zoom_for(52.5, 5.0, 15), 15);
    assert_eq!(elevation::zoom_for(52.5, 20.0, 15), 13);
}

#[test]
fn sidecar_round_trip() {
    let dir = std::env::temp_dir().join(format!("openmapper-geo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut f = GeoFile { anchor: Some(Anchor { rotation: 3.5, ..spandau() }), source: sources::ESRI.into(), zoom: 19, ..Default::default() };
    f.tile_mut((2, -1)).opacity = 0.4;
    f.tile_mut((2, -1)).imagery = Some(ImageryMeta { source: sources::ESRI.into(), zoom: 19, stamp: 7, coarser: true });
    f.tile_mut((0, 0)).visible = false;
    f.terrain.base = Some(31.25);
    f.save(&dir).unwrap();
    let g = GeoFile::load(&dir);
    assert_eq!(g, f);
    assert_eq!(g.tile((5, 5)), TileGeo::default());
    // an older or hand-written file with fields missing reads with the defaults
    std::fs::write(GeoFile::path(&dir), r#"{"anchor":{"tile":[0,0],"lat":55.75,"lon":37.61}}"#).unwrap();
    let g = GeoFile::load(&dir);
    assert_eq!(g.size, 2048);
    assert!(g.overlay && g.auto_new && g.terrain.relative);
    // the imagery stays out of an export unless asked for
    let mut files = std::collections::BTreeMap::new();
    files.insert("maps/x/global.cfg".to_string(), dir.join("global.cfg"));
    files.insert("maps/x/openMapper/imagery/tile_0_0.jpg".to_string(), imagery_path(&dir, (0, 0)));
    files.insert("maps/x/openMapper/geo.json".to_string(), GeoFile::path(&dir));
    export_filter(&dir, &mut files);
    assert_eq!(files.len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn google_answers() {
    let s = sources::parse_google_session(r#"{"session": "IgAB-tok", "expiry": "1361483420", "tileWidth": 256, "tileHeight": 256, "imageFormat": "jpeg"}"#).unwrap();
    assert_eq!(s, sources::GoogleSession { token: "IgAB-tok".into(), expiry: 1361483420 });
    assert!(sources::parse_google_session(r#"{"error": {"code": 403, "message": "Map Tiles API has not been used in project 1 before", "status": "PERMISSION_DENIED"}}"#).unwrap_err().contains("Map Tiles API"));
    let v = sources::parse_google_viewport(
        r#"{"copyright": "Imagery ©2025 Airbus, Maxar Technologies", "maxZoomRects": [
            {"maxZoom": 19, "north": 90, "south": -90, "east": 180, "west": -180},
            {"maxZoom": 21, "north": 52.6, "south": 52.4, "east": 13.4, "west": 13.0},
            {"maxZoom": 22, "north": 40.8, "south": 40.7, "east": -73.9, "west": -74.1}]}"#,
        52.5354,
        13.1996,
    )
    .unwrap();
    assert_eq!(v, sources::Viewport { copyright: "Imagery ©2025 Airbus, Maxar Technologies".into(), max_zoom: Some(21) });
}

#[test]
fn source_urls_and_zooms() {
    let all = GeoPrefs::default().all();
    let esri = all.iter().find(|s| s.name == sources::ESRI).unwrap();
    assert_eq!(esri.tile_url(19, 281365, 171951, None, None), "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/19/171951/281365?blankTile=false");
    let berlin = all.iter().find(|s| s.name.starts_with("Berlin")).unwrap();
    let u = berlin.tile_url(1, 0, 0, None, None);
    assert!(u.contains("BBOX=-20037508.343,0.000,0.000,20037508.343") && u.contains("WIDTH=512&HEIGHT=512"), "{u}");
    assert!(berlin.covers(52.52, 13.4) && !berlin.covers(55.75, 37.6));
    // 20 cm a pixel at Berlin's latitude: zoom 18 of 512 px tiles (about 18 cm)
    assert_eq!(berlin.useful_zoom(52.52), 18);
    assert!((berlin.metres_per_pixel(52.52, 18) - 0.2).abs() < 1e-9);
    assert_eq!(esri.useful_zoom(52.52), 21);
    // keys: a keyed source is usable once its key is in
    let mut p = GeoPrefs::default();
    let mapbox = all.iter().find(|s| s.name.starts_with("Mapbox")).unwrap();
    assert!(!p.usable(mapbox));
    p.keys.insert(mapbox.name.clone(), " pk.abc ".into());
    assert!(p.usable(mapbox));
    assert!(mapbox.tile_url(3, 1, 2, p.key(&mapbox.name), None).ends_with("/3/1/2@2x.jpg90?access_token=pk.abc"));
    assert_eq!(mapbox.slug(), "mapbox-satellite");
}
