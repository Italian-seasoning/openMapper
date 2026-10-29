use super::*;

/// The small metro line of openOMSI's own example (docs/examples/transport-metro).
fn metro() -> Transit {
    let mut t = Transit { modes: Modes::of(&[Mode::Metro]), ..Default::default() };
    let st = t.file_mut(File::Stations);
    for (id, name) in [("Sokol", "Sokol"), ("Voykovskaya", "Voykovskaya"), ("Aeroport", "Aeroport"), ("Depot_stop", "Depot Sokol")] {
        st.blocks.push(Block::with("station", &[("id", id), ("name", name), ("modes", "metro")]));
    }
    st.add_child(0, Block::with("platform", &[("id", "1"), ("side", "island"), ("length", "150 m")]), STATION_CHILDREN);
    t.file_mut(File::Depots).blocks.push(Block::with("depot", &[("id", "Depot_Sokol"), ("modes", "metro"), ("gate", "0 0 0"), ("station", "Depot_stop")]));
    let r = t.file_mut(File::Routes);
    r.blocks.push(Block::with("route", &[("id", "M2"), ("name", "Green line"), ("mode", "metro"), ("colour", "40 160 60")]));
    r.add_child(0, Block::with("segment", &[("id", "trunk"), ("from", "Sokol"), ("to", "Aeroport"), ("via", "Voykovskaya"), ("dwell", "25 s")]), ROUTE_CHILDREN);
    r.add_child(0, Block::with("service", &[("id", "all_day"), ("segments", "trunk"), ("headway", "3 min"), ("from", "05:30"), ("to", "25:00"), ("depot", "Depot_Sokol")]), ROUTE_CHILDREN);
    t
}

#[test]
fn what_openmapper_writes_the_game_reads_without_a_problem() {
    let t = metro();
    let p = t.problems();
    assert!(p.is_empty(), "{p:?}");
}

#[test]
fn the_games_checks_find_what_is_wrong() {
    let mut t = metro();
    // a segment to a station that is not there, and a service without a headway or trips
    let r = t.file_mut(File::Routes);
    r.blocks[1].set("to", "Nowhere");
    r.blocks[2].set("headway", "");
    let p = t.problems();
    assert!(p.iter().any(|x| x.error && x.message.contains("Nowhere")), "{p:?}");
    assert!(p.iter().any(|x| x.message.contains("headway")), "{p:?}");
    // a tram station on a map made for metro alone
    let mut t = metro();
    t.file_mut(File::Stations).blocks[0].set("modes", "tram");
    assert!(t.problems().iter().any(|x| x.file == "stations.cfg"));
}

#[test]
fn a_modern_map_keeps_its_modes_and_files_through_a_save() {
    let dir = std::env::temp_dir().join(format!("om-transit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("global.cfg"), "[name]\nM\n\n[tram_transport]\n\n[METRO_TRANSPORT]\n\n").unwrap();
    let t = Transit::load(&dir);
    assert_eq!(t.modes, Modes::of(&[Mode::Tram, Mode::Metro]));
    assert!(t.is_modern());
    assert_eq!(t.keywords(), vec!["tram_transport", "metro_transport"]);
    let mut m = metro();
    for op in m.plan(&dir) {
        if let Op::Write(p, b) = op {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b).unwrap();
        }
    }
    m.saved();
    let back = Transit::load(&dir);
    for f in File::ALL {
        assert_eq!(back.file(f), m.file(f), "{}", f.name());
    }
    // a file emptied here is taken away on the next save; one not changed is not written
    m.file_mut(File::Depots).blocks.clear();
    let ops = m.plan(&dir);
    assert_eq!(ops.len(), 1);
    assert!(matches!(&ops[0], Op::Remove(p) if p.ends_with("depots.cfg")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn modes_by_name_and_keyword() {
    assert_eq!(Mode::from_name("U-Bahn"), Some(Mode::Metro));
    assert_eq!(Mode::from_keyword("Trolleybus_Transport"), Some(Mode::Trolleybus));
    let m = Modes::parse("tram, railway");
    assert_eq!(m.names(), "tram railway");
    assert!(m.any_rail());
    assert!(!Modes::of(&[Mode::Bus, Mode::Trolleybus]).any_rail());
}

#[test]
fn the_trolleybus_wire_is_read_as_the_trolleybuses_read_it() {
    let mut t = Transit { modes: Modes::of(&[Mode::Trolleybus]), ..Default::default() };
    let f = t.file_mut(File::Trolley);
    f.blocks.push(Block::with("trolley_net", &[("voltage", "600")]));
    f.blocks.push(Block::with("trolley_section", &[("id", "1"), ("name", "A")]));
    f.blocks.push(Block::with("trolley_span", &[("section", "1"), ("p", "0 0 6")]));
    f.blocks.last_mut().unwrap().set_all("p", &["0 0 6".into(), "50 0 6".into()]);
    assert!(t.problems().is_empty(), "{:?}", t.problems());
    // a span of a section that is not there, a frog on no joint
    let f = t.file_mut(File::Trolley);
    f.blocks.push(Block::with("trolley_span", &[("section", "S2")]));
    f.blocks.last_mut().unwrap().set_all("p", &["50 0 6".into(), "90 0 6".into()]);
    f.blocks.push(Block::with("trolley_switch", &[("pos", "20 0 6")]));
    let p = t.problems();
    assert!(p.iter().any(|x| x.error && x.message.contains("no section")), "{p:?}");
    assert!(p.iter().any(|x| x.message.contains("frogs")), "{p:?}");
}
