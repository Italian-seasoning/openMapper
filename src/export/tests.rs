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


#[test]
fn restricted_export_round_trips_as_a_dependency_without_copying_publisher_bytes() {
    let _map = crate::doc::one_map();
    let base = std::env::temp_dir().join(format!("openmapper-reference-export-{}", std::process::id()));
    let (first, second) = (base.join("first"), base.join("second"));
    for root in [&first, &second] { std::fs::create_dir_all(root.join("Packs")).unwrap(); }
    let signer = omsi_pack::SigningKey::generate(&ring::rand::SystemRandom::new()).unwrap();
    let mut trust = omsi_pack::TrustedPublishers::default();
    trust.insert("export-fixture", signer.key_id(), signer.public_key());
    let bytes = b"[friendlyname]\nOwned export fixture\n";
    let relative = "Sceneryobjects/ReferenceFixture/renamed.sco";
    for root in [&first, &second] {
        trust.save_file(&root.join("Publishers.json")).unwrap();
        omsi_pack::create_pack(&root.join("Packs/objects.oopp"), omsi_pack::PackMetadata {
            publisher_id: "export-fixture".into(), author: "Fixture".into(), pack_id: "objects".into(),
            version: "1.0.0".into(), license: "LicenseRef-Owned-Fixture".into(), redistribution: false,
            key_id: None, dependencies: vec![],
        }, &[omsi_pack::AssetInput { id: "object".into(), path: "Sceneryobjects/ReferenceFixture/original.sco".into(),
            source_sha256: omsi_pack::sha256_hex(b"original owned source"), bytes: bytes.to_vec() }],
            &signer, None, &std::sync::atomic::AtomicBool::new(false)).unwrap();
    }
    let owned = first.join(relative);
    std::fs::create_dir_all(owned.parent().unwrap()).unwrap();
    std::fs::write(&owned, bytes).unwrap();
    let roots = Roots { omsi: base.join("stock"), content: first.clone() };
    let mut doc = Document::new_map(&roots, "References", "References", 1);
    // This owned fixture has no installation template to copy.
    doc.source = None;
    doc.tiles.get_mut(&(0, 0)).unwrap().tile.objects.push(omsi_map::MapObject {
        file: relative.replace('/', "\\"), id: 1, pos: [100.0, 100.0, 0.0], ..Default::default()
    });
    crate::doc::save(&mut doc).unwrap();
    let zip = base.join("references.zip");
    let (planned, _) = export(&doc, &roots, &zip).unwrap();
    assert!(!planned.files.contains_key(relative), "restricted source was copied by the actual exporter");
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&zip).unwrap()).unwrap();
    assert!(archive.by_name(relative).is_err());
    assert!(archive.by_name("maps/References/dependencies.json").is_ok());
    drop(archive);
    crate::import::import(&zip, &second).unwrap();
    assert!(!second.join(relative).exists());
    omsi_cfg::add_content_root(second.clone());
    let roots2 = Roots { omsi: roots.omsi.clone(), content: second.clone() };
    let back = Document::load(&roots2, &second.join("maps/References/global.cfg"), |_| {}).unwrap();
    let alias = omsi_cfg::resolve_path(&back.dir, relative);
    assert!(omsi_scenery::SceneryObject::load(&alias).is_ok());
    assert_eq!(omsi_cfg::vfs::read(&alias).unwrap(), bytes);
    // Re-export an owned overlay without losing the references it already carries.
    let second_zip = base.join("again.zip");
    export(&back, &roots2, &second_zip).unwrap();
    let mut archive = zip::ZipArchive::new(std::fs::File::open(second_zip).unwrap()).unwrap();
    assert!(archive.by_name("maps/References/dependencies.json").is_ok());
    drop(archive);
    drop(back);
    omsi_cfg::remove_content_root(&second);
    std::fs::remove_dir_all(base).unwrap();
}


#[test]
fn a_failed_export_leaves_no_partial_archive() {
    let root = std::env::temp_dir().join(format!("openmapper-export-cleanup-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let out = root.join("output.zip");
    std::fs::write(&out, b"existing user archive").unwrap();
    let mut plan = Plan::default();
    plan.files.insert("Sceneryobjects/missing.sco".into(), root.join("missing.sco"));
    assert!(write_zip(&plan, "owned fixture", &out).is_err());
    assert_eq!(std::fs::read(&out).unwrap(), b"existing user archive");
    assert!(!out.with_extension("zip.openmapper-new").exists());
    std::fs::remove_dir_all(root).unwrap();
}
