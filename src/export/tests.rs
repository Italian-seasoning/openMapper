use super::*;

/// A map with an imported model (and its texture and PBR map) and a stock bus stop,
/// exported and imported into a fresh content folder: it opens with every file there, the
/// model's own files packed and the installation's left out.
#[test]
fn an_exported_map_imports_whole() {
    let Some(omsi) = crate::install::find_omsi_root(&crate::install::Prefs::default()) else { return };
    let _map = crate::doc::one_map();
    let base = std::env::temp_dir().join(format!("openmapper-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let (c1, c2, src) = (base.join("content1"), base.join("content2"), base.join("model"));
    for d in [&c1, &c2, &src] {
        std::fs::create_dir_all(d).unwrap();
    }
    // a model the way a beginner brings one in
    std::fs::write(src.join("kiosk.mtl"), "newmtl a\nKd 1 1 1\nmap_Kd kiosk.png\n").unwrap();
    std::fs::write(src.join("kiosk.obj"), "mtllib kiosk.mtl\nv 0 0 0\nv 2 0 0\nv 2 2 0\nv 0 2 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nusemtl a\nf 1/1 2/2 3/3 4/4\n").unwrap();
    let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([200, 100, 50, 255]));
    img.save(src.join("kiosk.png")).unwrap();
    img.save(src.join("kiosk_nn.png")).unwrap();
    let r = crate::import::import(&src.join("kiosk.obj"), &c1).unwrap();
    let kiosk = r.objects[0].clone();

    let roots = Roots { omsi: omsi.clone(), content: c1.clone() };
    let mut doc = Document::new_map(&roots, "ExportTest", "Export test", 1);
    doc.global.description = "A map to share.".into();
    let t = doc.tiles.get_mut(&(0, 0)).unwrap();
    for (id, file) in [(1, kiosk.clone()), (2, "Sceneryobjects\\Generic\\bus_stop.sco".to_string())] {
        t.tile.objects.push(omsi_map::MapObject { file, id, pos: [100.0 + id as f64 * 10.0, 100.0, 0.0], ..Default::default() });
    }
    crate::doc::save(&mut doc).unwrap();

    let zip = base.join("ExportTest.zip");
    let (plan, bytes) = export(&doc, &roots, &zip).unwrap();
    assert!(bytes > 0);
    let names: Vec<&str> = plan.files.keys().map(|s| s.as_str()).collect();
    let has = |part: &str| names.iter().any(|n| n.to_lowercase().contains(&part.to_lowercase()));
    assert!(has("maps/ExportTest/global.cfg") && has("maps/ExportTest/tile_0_0.map"), "{names:?}");
    assert!(has("Imported/kiosk/kiosk.sco") && has("kiosk/model/kiosk_") && has("kiosk/texture/kiosk.png") && has("kiosk_nn.png"), "{names:?}");
    // (and the source openMapper edits it from, so the map's recipient can too)
    assert!(has("kiosk/kiosk.openmapper.json"), "{names:?}");
    assert!(!has("Generic"), "stock files are not packed: {names:?}");
    assert!(plan.stock.keys().any(|k| k.eq_ignore_ascii_case("Sceneryobjects\\Generic")));
    assert!(plan.missing.is_empty(), "{:?}", plan.missing);
    let text = readme(&doc, &plan);
    assert!(text.contains("Export test") && text.contains("A map to share.") && text.contains("Sceneryobjects\\Generic"));

    // into a fresh content folder: the map and its model are there, nothing is missing
    crate::import::import(&zip, &c2).unwrap();
    let roots2 = Roots { omsi: omsi.clone(), content: c2.clone() };
    let cfg = c2.join("maps").join("ExportTest").join("global.cfg");
    let back = Document::load(&roots2, &cfg, |_| {}).unwrap();
    assert_eq!(back.tiles.len(), 1);
    let dirs = [c2.clone(), omsi.clone()];
    for o in back.tiles.values().flat_map(|t| t.tile.objects.iter()) {
        let p = find_in(&c2, &o.file).or_else(|| find_in(&omsi, &o.file)).unwrap_or_else(|| panic!("{} missing", o.file));
        assert!(crate::check::missing_textures(&p, false, &dirs).is_empty(), "{}", o.file);
    }
    let _ = std::fs::remove_dir_all(&base);
}
