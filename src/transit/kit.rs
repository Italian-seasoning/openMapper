//! openMapper's transit kit: what a Modern map needs that OMSI 2 does not ship, made by
//! openMapper itself (no file of the installation is copied, so the kit may go with any map).
//!
//! * tracks: street track for trams (rails in paving, flush with the road), tram track on
//!   grass, railway track on ballast, metro track on a concrete slab with a third rail, and the
//!   metro track in a tunnel;
//! * platforms for trams, metro and railway, each with a walkway for the passengers;
//! * the overhead: a tram or railway contact wire with its messenger, and trolleybus wire pairs
//!   over one lane or two;
//! * objects: catenary masts and trolleybus poles (arm left or right), a metro sign, a platform
//!   shelter, a buffer stop and a station stop (OMSI's `[busstop]`, the place a station of
//!   `stations.cfg` is found at).
//!
//! Splines go to `Splines\openMapper\Transit\` of the content folder, objects to
//! `Sceneryobjects\openMapper\Transit\`. Their names say their mode (`tram`, `metro`,
//! `railway`), so the game classes the tracks right even without `network.cfg`, and a Modern
//! map gets a `network.cfg` rule for each kind anyway.

use std::fmt::Write as _;
use std::path::Path;

use glam::{Mat4, Vec3};
use openmapper_mesh::primitives::Primitive;
use openmapper_mesh::{Material, Mesh, Model, Part};

use super::{Block, File, Mode, Modes};
use crate::doc::Document;
use crate::editable::{Light, LightKind, PartVar, Source, When};
use crate::install::Roots;

/// Bumped when the kit changes: an older kit in the content folder is made again.
const VERSION: u32 = 4;
const SPLINES: &str = "Splines\\openMapper\\Transit";
const OBJECTS: &str = "Sceneryobjects\\openMapper\\Transit";

/// Standard gauge between the rails' inner faces (m) and a rail head's width.
const GAUGE: f32 = 1.435;
const HEAD: f32 = 0.07;

/// A spline of the kit: its file stem, what it is for.
pub struct KitSpline {
    pub stem: &'static str,
    pub modes: &'static [Mode],
    pub kind: Piece,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece {
    Track,
    Platform,
    Wire,
}

pub const KIT_SPLINES: &[KitSpline] = &[
    KitSpline { stem: "om_tram_street_track", modes: &[Mode::Tram], kind: Piece::Track },
    KitSpline { stem: "om_tram_grass_track", modes: &[Mode::Tram], kind: Piece::Track },
    KitSpline { stem: "om_railway_track", modes: &[Mode::Railway], kind: Piece::Track },
    KitSpline { stem: "om_metro_track", modes: &[Mode::Metro], kind: Piece::Track },
    KitSpline { stem: "om_metro_tunnel_track", modes: &[Mode::Metro], kind: Piece::Track },
    KitSpline { stem: "om_tram_platform", modes: &[Mode::Tram], kind: Piece::Platform },
    KitSpline { stem: "om_metro_platform_side", modes: &[Mode::Metro], kind: Piece::Platform },
    KitSpline { stem: "om_metro_platform_island", modes: &[Mode::Metro], kind: Piece::Platform },
    KitSpline { stem: "om_railway_platform_side", modes: &[Mode::Railway], kind: Piece::Platform },
    KitSpline { stem: "om_railway_platform_island", modes: &[Mode::Railway], kind: Piece::Platform },
    KitSpline { stem: "om_catenary_wire", modes: &[Mode::Tram, Mode::Railway], kind: Piece::Wire },
    KitSpline { stem: "om_trolleybus_wire_pair", modes: &[Mode::Trolleybus], kind: Piece::Wire },
    KitSpline { stem: "om_trolleybus_wires_3.0m_lanes", modes: &[Mode::Trolleybus], kind: Piece::Wire },
    KitSpline { stem: "om_trolleybus_wires_3.5m_lanes", modes: &[Mode::Trolleybus], kind: Piece::Wire },
];

/// A spline of the kit as the map names it.
pub fn spline_rel(stem: &str) -> String {
    format!("{SPLINES}\\{stem}.sli")
}

/// An object of the kit as the map names it.
pub fn object_rel(stem: &str) -> String {
    format!("{OBJECTS}\\{stem}\\{stem}.sco")
}

/// The trolleybus wire splines and how far each pair hangs right of the road's middle (m; a
/// pair as far left for the other way, 0: one pair over the middle).
pub const TROLLEY_WIRES: &[(&str, f64)] = &[("om_trolleybus_wire_pair", 0.0), ("om_trolleybus_wires_3.0m_lanes", 1.5), ("om_trolleybus_wires_3.5m_lanes", 1.75)];

/// The objects of the kit (file stems).
/// (masts and poles of other reaches are made when wiring asks for them: `overhead`)
pub const KIT_OBJECTS: &[&str] = &["om_catenary_mast_r30", "om_trolleybus_pole_h22_57", "om_trolleybus_frog", "om_section_insulator", "om_section_insulator_trolley", "om_metro_sign", "om_platform_shelter", "om_buffer_stop", "om_station_stop"];

/// The wire heights over the spline they are laid on (m): a track's rails stand 0.45 m over
/// its spline, a road's surface 0.1 m.
pub const CATENARY_HEIGHT: f64 = 5.95;
pub const TROLLEY_HEIGHT: f64 = 5.9;

/// Whether the kit in the content folder is there and up to date.
pub fn installed(content: &Path) -> bool {
    std::fs::read_to_string(omsi_cfg::resolve_path(content, &format!("{SPLINES}\\kit_version.txt"))).ok().and_then(|s| s.trim().parse::<u32>().ok()) == Some(VERSION)
}

/// Make the kit in the content folder (again, if it is older than this openMapper's). How many
/// files were written.
pub fn install(roots: &Roots) -> anyhow::Result<usize> {
    if installed(&roots.content) {
        return Ok(0);
    }
    let dir = omsi_cfg::resolve_path(&roots.content, SPLINES);
    let tex = dir.join("texture");
    std::fs::create_dir_all(&tex)?;
    let mut n = 0;
    for (name, img) in textures() {
        img.save_with_format(tex.join(name), image::ImageFormat::Bmp)?;
        n += 1;
    }
    droppers().save_with_format(tex.join("om_droppers.tga"), image::ImageFormat::Tga)?;
    n += 1;
    for s in KIT_SPLINES {
        let text = spline_text(s.stem);
        crate::doc::safe::write_atomic(&dir.join(format!("{}.sli", s.stem)), &omsi_cfg::encode_text(&text.replace('\n', "\r\n")))?;
        n += 1;
    }
    for stem in KIT_OBJECTS {
        super::overhead::save(roots, &object_rel(stem), &object(stem))?;
        n += 1;
    }
    std::fs::write(dir.join("kit_version.txt"), VERSION.to_string())?;
    log::info!("transit kit: {n} files made in {}", dir.display());
    Ok(n)
}

/// A map made Modern for `modes`: the keywords, the kit, `network.cfg` rules for the kit's
/// tracks, the overhead wire's and the third rail's networks with a live section each, and
/// real rail (trams and trains follow their switches).
pub fn start_modern(doc: &mut Document, roots: &Roots, modes: Modes) {
    doc.transit.modes = modes;
    doc.global_dirty = true;
    if let Err(e) = install(roots) {
        log::warn!("transit kit: {e}");
    }
    if modes.any_rail() {
        doc.global.real_rail = true;
    }
    seed(&mut doc.transit, modes);
}

/// The rules and networks a Modern map of `modes` starts with (what is there stays).
pub fn seed(t: &mut super::Transit, modes: Modes) {
    let rules: &[(Mode, &str, &[(&str, &str)])] = &[
        (Mode::Tram, "om_tram_*track.sli", &[("modes", "tram"), ("street", "yes"), ("power", "catenary"), ("speed", "50 km/h"), ("gauge", "1435")]),
        (Mode::Railway, "om_railway_track.sli", &[("modes", "railway"), ("street", "no"), ("power", "catenary"), ("speed", "120 km/h"), ("gauge", "1435")]),
        (Mode::Metro, "om_metro_track.sli", &[("modes", "metro"), ("street", "no"), ("power", "third_rail"), ("tunnel", "no"), ("speed", "80 km/h"), ("gauge", "1435")]),
        (Mode::Metro, "om_metro_tunnel_track.sli", &[("modes", "metro"), ("street", "no"), ("power", "third_rail"), ("tunnel", "yes"), ("speed", "80 km/h"), ("gauge", "1435")]),
    ];
    for (m, file, keys) in rules {
        if !modes.contains(*m) {
            continue;
        }
        let path = format!("{SPLINES}\\{file}");
        let net = t.file(File::Network);
        if net.of("track").any(|i| net.blocks[i].get("file").is_some_and(|f| f.eq_ignore_ascii_case(&path))) {
            continue;
        }
        let mut b = Block::with("track", &[("file", &path)]);
        for (k, v) in keys.iter() {
            b.set(k, v);
        }
        t.file_mut(File::Network).blocks.push(b);
    }
    // one overhead network per map (openOMSI reads one [catenary_net]): a tram's 600 V, else a
    // railway's 15 kV
    if (modes.contains(Mode::Tram) || modes.contains(Mode::Railway)) && t.file(File::Catenary).of("catenary_net").next().is_none() {
        let tram = modes.contains(Mode::Tram);
        let f = t.file_mut(File::Catenary);
        let net_modes = Modes::of(&[Mode::Tram, Mode::Railway]).iter().filter(|m| modes.contains(*m)).map(Mode::name).collect::<Vec<_>>().join(" ");
        f.blocks.insert(0, Block::with("catenary_net", &[("kind", if tram { "dc" } else { "ac" }), ("voltage", if tram { "600" } else { "15000" }), ("wires", "1"), ("height", if tram { "5.5 m" } else { "5.5 m" }), ("modes", &net_modes)]));
        f.blocks.insert(1, Block::with("catenary_section", &[("id", "S1"), ("name", "Substation 1"), ("live", "yes")]));
    }
    if modes.contains(Mode::Metro) && t.file(File::ThirdRail).of("thirdrail_net").next().is_none() {
        let f = t.file_mut(File::ThirdRail);
        f.blocks.insert(0, Block::with("thirdrail_net", &[("voltage", "750"), ("side", "right"), ("modes", "metro")]));
        f.blocks.insert(1, Block::with("thirdrail_section", &[("id", "T1"), ("name", "Substation 1"), ("live", "yes")]));
    }
    if modes.contains(Mode::Trolleybus) && t.file(File::Trolley).of("trolley_net").next().is_none() {
        let f = t.file_mut(File::Trolley);
        f.blocks.insert(0, Block::with("trolley_net", &[("voltage", "600"), ("spacing", "0.6")]));
        f.blocks.insert(1, Block::with("trolley_section", &[("id", "1"), ("name", "Substation 1"), ("live", "1")]));
    }
}

// ----- textures -----

/// A repeatable noise value 0..1 for a cell.
fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263) ^ seed.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

/// Noise smooth over cells of `cell` pixels, wrapping at `size` (the texture repeats).
fn smooth(x: u32, y: u32, cell: u32, size: u32, seed: u32) -> f32 {
    let n = (size / cell) as i32;
    let (fx, fy) = (x as f32 / cell as f32, y as f32 / cell as f32);
    let (x0, y0) = (fx.floor() as i32, fy.floor() as i32);
    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
    let at = |i: i32, j: i32| hash(i.rem_euclid(n), j.rem_euclid(n), seed);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let a = at(x0, y0) + (at(x0 + 1, y0) - at(x0, y0)) * s(tx);
    let b = at(x0, y0 + 1) + (at(x0 + 1, y0 + 1) - at(x0, y0 + 1)) * s(tx);
    a + (b - a) * s(ty)
}

fn shade(c: [f32; 3], f: f32) -> image::Rgb<u8> {
    image::Rgb([(c[0] * f).clamp(0.0, 255.0) as u8, (c[1] * f).clamp(0.0, 255.0) as u8, (c[2] * f).clamp(0.0, 255.0) as u8])
}

const SIZE: u32 = 256;

/// u across (0 left, 1 right), v along: a texture of `width` m across.
fn texture(seed: u32, f: impl Fn(f32, f32, f32) -> [f32; 3]) -> image::RgbImage {
    image::RgbImage::from_fn(SIZE, SIZE, |x, y| {
        let n = 0.5 * smooth(x, y, 4, SIZE, seed) + 0.3 * smooth(x, y, 16, SIZE, seed + 1) + 0.2 * smooth(x, y, 64, SIZE, seed + 2);
        let c = f(x as f32 / SIZE as f32, y as f32 / SIZE as f32, n);
        shade(c, 1.0)
    })
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn scale(c: [f32; 3], f: f32) -> [f32; 3] {
    [c[0] * f, c[1] * f, c[2] * f]
}

/// The kit's textures (BMP, 256 pixels square).
fn textures() -> Vec<(&'static str, image::RgbImage)> {
    let ballast = [120.0, 112.0, 102.0];
    let concrete = [150.0, 150.0, 146.0];
    // ballast with concrete sleepers across the middle: the texture is 3 m along (5 sleepers)
    let sleepers = texture(11, move |u, v, n| {
        let stones = scale(ballast, 0.65 + 0.6 * n);
        let on_sleeper = (0.12..0.88).contains(&u) && (v * 5.0).fract() < 0.38;
        if on_sleeper { scale(concrete, 0.85 + 0.25 * n) } else { stones }
    });
    let slab = texture(12, move |u, v, n| {
        // a slab with its joints every 6 m and plinths under the rails
        let joint = (v * 0.5).fract() < 0.01;
        let plinth = ((0.24..0.33).contains(&u) || (0.67..0.76).contains(&u)) && (v * 5.0).fract() < 0.7;
        let c = if plinth { scale(concrete, 1.08) } else { concrete };
        scale(c, if joint { 0.5 } else { 0.82 + 0.25 * n })
    });
    // paving round the rails of a street track: 3 m across, the grooves inside the heads
    let street = texture(13, |u, v, n| {
        let setts = ((u * 3.0 * 6.0).fract() < 0.06) || ((v * 6.0 + if (u * 18.0) as i32 % 2 == 0 { 0.0 } else { 0.5 }).fract() < 0.06);
        let c = if setts { [70.0, 70.0, 72.0] } else { scale([118.0, 116.0, 112.0], 0.8 + 0.35 * n) };
        let rail = |x: f32| ((u - x).abs() * 3.0) < HEAD / 2.0;
        let groove = |x: f32| ((u - x).abs() * 3.0) < HEAD / 2.0 + 0.04;
        let (l, r) = (0.5 - (GAUGE / 2.0 + HEAD / 2.0) / 3.0, 0.5 + (GAUGE / 2.0 + HEAD / 2.0) / 3.0);
        if rail(l) || rail(r) {
            scale([190.0, 192.0, 196.0], 0.9 + 0.15 * n)
        } else if groove(l + 0.025 / 3.0) || groove(r - 0.025 / 3.0) {
            [40.0, 38.0, 36.0]
        } else {
            c
        }
    });
    let grass = texture(14, |_, _, n| mix([70.0, 104.0, 46.0], [104.0, 132.0, 62.0], n));
    let steel = texture(15, |u, _, n| if (0.3..0.7).contains(&u) { scale([200.0, 202.0, 206.0], 0.9 + 0.15 * n) } else { scale([96.0, 82.0, 70.0], 0.85 + 0.3 * n) });
    let third = texture(16, |_, _, n| scale([214.0, 178.0, 52.0], 0.85 + 0.25 * n));
    // a platform's top: tiles, a white line and the tactile strip at both edges (u 0 and 1)
    let platform = texture(17, |u, v, n| {
        let edge = u.min(1.0 - u);
        if edge < 0.025 {
            [235.0, 235.0, 230.0]
        } else if edge < 0.07 {
            let bumps = ((u * 160.0).fract() - 0.5).powi(2) + ((v * 40.0).fract() - 0.5).powi(2) < 0.1;
            scale([228.0, 190.0, 40.0], if bumps { 1.0 } else { 0.85 })
        } else {
            let grout = (u * 12.0).fract() < 0.03 || (v * 8.0).fract() < 0.03;
            if grout { [96.0, 96.0, 96.0] } else { scale([168.0, 166.0, 160.0], 0.85 + 0.25 * n) }
        }
    });
    let wall = texture(18, |u, v, n| {
        let ring = (v * 4.0).fract() < 0.02;
        let tray = (0.62..0.66).contains(&u);
        if tray { [60.0, 60.0, 62.0] } else { scale([132.0, 130.0, 124.0], if ring { 0.55 } else { 0.8 + 0.3 * n }) }
    });
    let wire = texture(19, |_, _, n| scale([64.0, 56.0, 48.0], 0.9 + 0.2 * n));
    // the contact wire's copper, dull brown with the bright groove the collectors polish, and
    // the messenger's galvanized strands
    let copper = texture(20, |u, _, n| if (0.42..0.58).contains(&u) { scale([196.0, 140.0, 96.0], 0.9 + 0.2 * n) } else { scale([116.0, 76.0, 50.0], 0.85 + 0.3 * n) });
    let strands = texture(21, |u, v, n| scale([150.0, 152.0, 156.0], 0.7 + 0.3 * ((u * 4.0 + v * 24.0).fract() - 0.5).abs() * 2.0 + 0.1 * n));
    vec![("om_copper_wire.bmp", copper), ("om_messenger.bmp", strands), ("om_ballast.bmp", sleepers), ("om_slab.bmp", slab), ("om_street_track.bmp", street), ("om_grass.bmp", grass), ("om_rail.bmp", steel), ("om_third_rail.bmp", third), ("om_platform.bmp", platform), ("om_tunnel.bmp", wall), ("om_wire.bmp", wire)]
}

/// A tunnel's shell `width` m wide (its walls' inner faces, inner height `dig::CLEAR`): walls
/// and a roof seen from inside, a lamp strip along each wall; laid over a road or track that
/// runs under the ground. Made in the kit's folder when first asked for; its file as a map
/// names it.
pub fn ensure_shell(content: &Path, width: f32) -> anyhow::Result<String> {
    let dm = (width * 10.0).round().clamp(30.0, 300.0) as i32;
    let stem = format!("om_tunnel_shell_w{dm}");
    let dir = omsi_cfg::resolve_path(content, SPLINES);
    let path = dir.join(format!("{stem}.sli"));
    if !path.is_file() {
        std::fs::create_dir_all(dir.join("texture"))?;
        if !dir.join("texture").join("om_tunnel.bmp").is_file() {
            for (name, img) in textures().into_iter().filter(|t| t.0 == "om_tunnel.bmp" || t.0 == "om_slab.bmp") {
                img.save_with_format(dir.join("texture").join(name), image::ImageFormat::Bmp)?;
            }
        }
        let w = dm as f32 / 20.0;
        let h = crate::dig::CLEAR as f32;
        let mut sl = Sli::new(&format!("Tunnel shell {:.1} m (openMapper)", w * 2.0));
        // walls facing in (left wall top to bottom, right wall bottom to top), the roof facing
        // down, a footing strip each side at the bottom
        sl.profile("om_tunnel.bmp", 0.25, &[(-w, h, 1.0), (-w, -0.2, 0.0)]);
        sl.profile("om_tunnel.bmp", 0.25, &[(w, -0.2, 0.0), (w, h, 1.0)]);
        sl.profile("om_tunnel.bmp", 0.25, &[(w, h, 0.0), (-w, h, 1.0)]);
        // the floor the road or track lies on, wall to wall
        sl.profile("om_slab.bmp", 0.2, &[(-w, 0.04, 0.0), (w, 0.04, 1.0)]);
        sl.height(-w, w, 0.04);
        // the outside (seen at a portal's edge and from the ramps): back faces of the same
        sl.profile("om_slab.bmp", 0.2, &[(-w - 0.4, -0.2, 0.0), (-w - 0.4, h + 0.4, 1.0)]);
        sl.profile("om_slab.bmp", 0.2, &[(w + 0.4, h + 0.4, 1.0), (w + 0.4, -0.2, 0.0)]);
        sl.profile("om_slab.bmp", 0.2, &[(-w - 0.4, h + 0.4, 0.0), (w + 0.4, h + 0.4, 1.0)]);
        crate::doc::safe::write_atomic(&path, &omsi_cfg::encode_text(&sl.text.replace('\n', "\r\n")))?;
    }
    Ok(format!("{SPLINES}\\{stem}.sli"))
}

/// The droppers between the messenger and the contact wire: a strip (u from the contact wire up
/// to the messenger, v along: one dropper every 8 m at the spline's v rate) clear between them.
fn droppers() -> image::RgbaImage {
    // (1024 pixels for the 8 m: a pixel is 8 mm, a dropper's rod one pixel, its clamps three)
    image::RgbaImage::from_fn(16, 1024, |x, y| {
        let u = x as f32 / 16.0;
        let rod = y == 0;
        let clamp = y < 3 && !(0.12..0.88).contains(&u);
        if rod || clamp { image::Rgba([140, 132, 118, 255]) } else { image::Rgba([0, 0, 0, 0]) }
    })
}

// ----- splines -----

/// A spline's text being made: OMSI 2's `.sli` keywords.
struct Sli {
    text: String,
    textures: Vec<&'static str>,
}

impl Sli {
    fn new(name: &str) -> Sli {
        Sli { text: format!("File created with openMapper (transit kit)\n\n[friendlyname]\n{name}\n\n"), textures: Vec::new() }
    }

    fn tex(&mut self, t: &'static str) -> usize {
        if let Some(i) = self.textures.iter().position(|x| *x == t) {
            return i;
        }
        let _ = write!(self.text, "[texture]\n{t}\n\n");
        self.textures.push(t);
        self.textures.len() - 1
    }

    /// The surface wheels, feet and the ground take from `a` to `b` across, at height `h`.
    fn height(&mut self, a: f32, b: f32, h: f32) {
        let _ = write!(self.text, "[heightprofile]\n{a:.3}\n{b:.3}\n{h:.3}\n{h:.3}\n\n");
    }

    /// A strip through the points (x, z, u), its texture `vrate` times per metre along.
    fn profile(&mut self, tex: &'static str, vrate: f32, pts: &[(f32, f32, f32)]) {
        let t = self.tex(tex);
        let _ = write!(self.text, "[profile]\n{t}\n\n");
        for (x, z, u) in pts {
            let _ = write!(self.text, "[profilepnt]\n{x:.3}\n{z:.3}\n{u:.3}\n{vrate:.4}\n\n");
        }
    }

    /// A wire: a round conductor of radius `r` at `x`, height `z` (eight faces, seen from
    /// every side).
    fn tube(&mut self, tex: &'static str, x: f32, z: f32, r: f32) {
        // (the faces outwards: round the section clockwise as seen along the spline)
        let pts: Vec<(f32, f32, f32)> = (0..=8).map(|i| {
            let a = std::f32::consts::FRAC_PI_2 - std::f32::consts::TAU * i as f32 / 8.0;
            (x + r * a.cos(), z + r * a.sin(), i as f32 / 8.0)
        }).collect();
        self.profile(tex, 0.5, &pts);
    }

    /// A strip of a texture with alpha (cut out), seen from both sides: from (x, za) up to
    /// (x, zb), `vrate` times along per metre.
    fn cutout(&mut self, tex: &'static str, x: f32, za: f32, zb: f32, vrate: f32) {
        let t = self.textures.len();
        let _ = write!(self.text, "[texture]\n{tex}\n[matl_alpha]\n1\n\n");
        self.textures.push(tex);
        for pts in [[(x, za, 0.0), (x, zb, 1.0)], [(x, zb, 1.0), (x, za, 0.0)]] {
            let _ = write!(self.text, "[profile]\n{t}\n\n");
            for (px, pz, u) in pts {
                let _ = write!(self.text, "[profilepnt]\n{px:.3}\n{pz:.3}\n{u:.3}\n{vrate:.4}\n\n");
            }
        }
    }

    /// A rail: a box of `HEAD` width centred on `x` from `base` up to `top`.
    fn rail(&mut self, x: f32, base: f32, top: f32) {
        let (l, r) = (x - HEAD / 2.0, x + HEAD / 2.0);
        self.profile("om_rail.bmp", 0.25, &[(l, base, 0.0), (l, top, 0.3), (r, top, 0.7), (r, base, 1.0)]);
    }

    /// A pair of rails of standard gauge.
    fn rails(&mut self, base: f32, top: f32) {
        let c = GAUGE / 2.0 + HEAD / 2.0;
        self.rail(-c, base, top);
        self.rail(c, base, top);
    }

    /// A lane: type 0 road, 1 footway, 2 rail; both ways.
    fn path(&mut self, kind: u8, x: f32, h: f32, w: f32) {
        let _ = write!(self.text, "[path]\n{kind}\n{x:.3}\n{h:.3}\n{w:.3}\n2\n\n");
        if kind == 2 {
            // (OMSI 2's track quality: rail length, then the lateral and vertical waviness)
            self.text.push_str("[rail_enh]\n200\n0\n30\n0.02\n7\n30\n0.02\n7\n\n");
        }
    }
}

/// A platform `w` wide and `h` high (both edges dressed: a side platform is laid with one
/// edge to the track, an island between two).
fn platform(s: &mut Sli, w: f32, h: f32) {
    let (l, r) = (-w / 2.0, w / 2.0);
    s.height(l, r, h);
    s.profile("om_platform.bmp", 0.25, &[(l, h, 0.0), (r, h, 1.0)]);
    s.profile("om_tunnel.bmp", 0.25, &[(l, 0.0, 0.0), (l, h, 0.2)]);
    s.profile("om_tunnel.bmp", 0.25, &[(r, h, 0.2), (r, 0.0, 0.0)]);
    s.path(1, 0.0, h, w * 0.6);
}

fn spline_text(stem: &str) -> String {
    match stem {
        "om_tram_street_track" => {
            // paving 3 m wide flush with a road's surface, the rails in it
            let mut s = Sli::new("Tram street track (openMapper)");
            let h = 0.12;
            s.height(-1.5, 1.5, h);
            s.profile("om_street_track.bmp", 0.33, &[(-1.5, h, 0.0), (1.5, h, 1.0)]);
            s.path(2, 0.0, h, 0.0);
            s.text
        }
        "om_tram_grass_track" => {
            let mut s = Sli::new("Tram track on grass (openMapper)");
            let (h, top) = (0.15, 0.30);
            s.height(-1.6, 1.6, h);
            s.profile("om_grass.bmp", 0.25, &[(-1.6, 0.0, 0.0), (-1.4, h, 0.05), (1.4, h, 0.95), (1.6, 0.0, 1.0)]);
            s.rails(h, top);
            s.path(2, 0.0, top, 0.0);
            s.text
        }
        "om_railway_track" => {
            // a ballast bed 4.2 m wide with sloping shoulders, as OMSI 2's own tracks
            let mut s = Sli::new("Railway track on ballast (openMapper)");
            let (bed, top) = (0.30, 0.45);
            s.height(-2.1, -1.5, 0.0);
            s.height(-1.5, 1.5, bed);
            s.height(1.5, 2.1, 0.0);
            s.profile("om_ballast.bmp", 0.333, &[(-2.1, 0.0, 0.0), (-1.5, bed, 0.06), (1.5, bed, 0.94), (2.1, 0.0, 1.0)]);
            s.rails(bed, top);
            s.path(2, 0.0, top, 0.0);
            s.text
        }
        "om_metro_track" | "om_metro_tunnel_track" => {
            let tunnel = stem.contains("tunnel");
            let mut s = Sli::new(if tunnel { "Metro track in a tunnel (openMapper)" } else { "Metro track with third rail (openMapper)" });
            let (slab, top) = (0.25, 0.40);
            s.height(-1.8, 1.8, slab);
            s.profile("om_slab.bmp", 0.2, &[(-1.8, 0.0, 0.0), (-1.7, slab, 0.02), (1.7, slab, 0.98), (1.8, 0.0, 1.0)]);
            s.rails(slab, top);
            // the third rail outside the right rail, its cover yellow
            let x = 1.45;
            s.profile("om_third_rail.bmp", 0.25, &[(x - 0.05, slab, 0.0), (x - 0.05, 0.47, 0.3), (x + 0.05, 0.47, 0.7), (x + 0.05, slab, 1.0)]);
            s.path(2, 0.0, top, 0.0);
            // (OMSI 2's own third rail of the track: where it is, top contact, 750 V DC, its
            // conductance, as the stock S-Bahn track has it)
            let _ = write!(s.text, "[third_rail]\n{x:.3}\n0.470\n0\n750\n0\n0.051\n\n");
            if tunnel {
                // the tube seen from inside: walls facing in, the vault facing down
                let (w, roof) = (2.4, 4.4);
                s.profile("om_tunnel.bmp", 0.25, &[(-w, roof, 1.0), (-w, 0.0, 0.0)]);
                s.profile("om_tunnel.bmp", 0.25, &[(w, 0.0, 0.0), (w, roof, 1.0)]);
                s.profile("om_tunnel.bmp", 0.25, &[(w, roof, 0.0), (0.0, roof + 0.5, 0.5), (-w, roof, 1.0)]);
                s.profile("om_slab.bmp", 0.2, &[(-w, 0.0, 0.0), (-1.8, 0.0, 0.1)]);
                s.profile("om_slab.bmp", 0.2, &[(1.8, 0.0, 0.9), (w, 0.0, 1.0)]);
            }
            s.text
        }
        "om_tram_platform" => {
            let mut s = Sli::new("Tram platform 3 m (openMapper)");
            platform(&mut s, 3.0, 0.30);
            s.text
        }
        "om_metro_platform_side" => {
            let mut s = Sli::new("Metro side platform 4 m (openMapper)");
            platform(&mut s, 4.0, 1.10);
            s.text
        }
        "om_metro_platform_island" => {
            let mut s = Sli::new("Metro island platform 10 m (openMapper)");
            platform(&mut s, 10.0, 1.10);
            s.text
        }
        "om_railway_platform_side" => {
            let mut s = Sli::new("Railway side platform 4 m (openMapper)");
            platform(&mut s, 4.0, 0.76);
            s.text
        }
        "om_railway_platform_island" => {
            let mut s = Sli::new("Railway island platform 8 m (openMapper)");
            platform(&mut s, 8.0, 0.76);
            s.text
        }
        "om_catenary_wire" => {
            // the contact wire 5.5 m over the rails, the messenger wire 1.1 m above it
            let mut s = Sli::new("Catenary wire (openMapper)");
            let (c, m) = (CATENARY_HEIGHT as f32, CATENARY_HEIGHT as f32 + 1.1);
            s.tube("om_copper_wire.bmp", 0.0, c, 0.007);
            s.tube("om_messenger.bmp", 0.0, m, 0.006);
            // the droppers: one every 8 m (the texture repeats every 8 m along)
            s.cutout("om_droppers.tga", 0.0, c + 0.005, m - 0.005, 0.125);
            s.text
        }
        _ => {
            // trolleybus wires: a pair 0.6 m apart over each way's lane (or one over the middle)
            let off = TROLLEY_WIRES.iter().find(|(s, _)| *s == stem).map(|w| w.1).unwrap_or(0.0) as f32;
            let mut s = Sli::new(match stem {
                "om_trolleybus_wire_pair" => "Trolleybus wire pair (openMapper)",
                "om_trolleybus_wires_3.0m_lanes" => "Trolleybus wires, 3.0 m lanes (openMapper)",
                _ => "Trolleybus wires, 3.5 m lanes (openMapper)",
            });
            let centres: Vec<f32> = if off == 0.0 { vec![0.0] } else { vec![-off, off] };
            for c in centres {
                for x in [c - 0.3, c + 0.3] {
                    s.tube("om_copper_wire.bmp", x, TROLLEY_HEIGHT as f32, 0.0075);
                }
            }
            s.text
        }
    }
}

// ----- objects -----

fn shape(p: Primitive, m: Mat4, material: u32) -> Mesh {
    let mut mesh = p.build();
    mesh.transform(m);
    for f in &mut mesh.faces {
        f.material = material;
        f.smooth = p.round();
    }
    mesh
}

fn cube(size: Vec3, at: Vec3, material: u32) -> Mesh {
    shape(Primitive::Cube { size }, Mat4::from_translation(at), material)
}

/// A pole of radius `r` and height `h` standing at `at`.
fn pole(r: f32, h: f32, at: Vec3, material: u32) -> Mesh {
    shape(Primitive::Cylinder { radius: r, height: h, segments: 12 }, Mat4::from_translation(at), material)
}

/// A bar of radius `r` from `a` to `b`.
fn bar(r: f32, a: Vec3, b: Vec3, material: u32) -> Mesh {
    let d = b - a;
    let rot = glam::Quat::from_rotation_arc(Vec3::Z, d.normalize_or_zero());
    shape(Primitive::Cylinder { radius: r, height: d.length(), segments: 10 }, Mat4::from_translation(a) * Mat4::from_quat(rot), material)
}

fn one(name: &str, materials: Vec<Material>, meshes: Vec<Mesh>) -> Source {
    let mut body = Mesh::default();
    for m in &meshes {
        body.append(m, 0);
    }
    Source { name: name.into(), model: Model { parts: vec![Part::new("Body", body)], materials }, collision: true, ..Default::default() }
}

fn object(stem: &str) -> Source {
    if let Some(src) = super::overhead::of_stem(stem) {
        return src;
    }
    let steel = || Material::colored("Steel", [0.40, 0.42, 0.42, 1.0]);
    match stem {
        "om_metro_sign" => {
            // a red M on a white lit box on a pole, lit at night
            let mut materials = vec![steel(), Material { name: "Lit".into(), color: [0.95, 0.95, 0.95, 1.0], emissive: [0.0; 3], ..Default::default() }, Material::colored("Red", [0.85, 0.08, 0.1, 1.0])];
            materials.push(Material { name: "Lit night".into(), color: [0.95, 0.95, 0.95, 1.0], emissive: [0.9, 0.9, 0.85], ..Default::default() });
            let z = 3.4;
            let mut body = Mesh::default();
            body.append(&pole(0.08, 3.0, Vec3::ZERO, 0), 0);
            body.append(&cube(Vec3::new(0.9, 0.35, 0.9), Vec3::new(0.0, 0.0, z), 1), 0);
            // the letter, on both faces
            for y in [-0.21f32, 0.21] {
                for (a, b) in [((-0.3, -0.3), (-0.3, 0.3)), ((-0.3, 0.3), (0.0, -0.05)), ((0.0, -0.05), (0.3, 0.3)), ((0.3, 0.3), (0.3, -0.3))] {
                    body.append(&bar(0.05, Vec3::new(a.0, y, z + a.1), Vec3::new(b.0, y, z + b.1), 2), 0);
                }
            }
            let glow = cube(Vec3::new(0.92, 0.37, 0.92), Vec3::new(0.0, 0.0, z), 3);
            Source {
                name: "Metro sign (openMapper)".into(),
                model: Model { parts: vec![Part::new("Body", body), Part::new("NightGlow", glow)], materials },
                lights: vec![Light { kind: LightKind::Glow, pos: Vec3::new(0.0, 0.0, z), dir: Vec3::NEG_Z, color: [255, 250, 240], size: 1.2, range: 0.0, cone: None, when: When::Night, star: false, brightness: 0.6 }],
                part_vars: vec![PartVar { part: "NightGlow".into(), var: "NightlightA".into(), value: 1.0 }],
                collision: true,
                ..Default::default()
            }
        }
        "om_platform_shelter" => one(
            "Platform shelter (openMapper)",
            vec![steel(), Material::colored("Roof", [0.25, 0.27, 0.3, 1.0]), Material { name: "Glass".into(), color: [0.75, 0.85, 0.9, 0.35], alpha: 2, ..Default::default() }, Material::colored("Wood", [0.55, 0.38, 0.22, 1.0])],
            vec![
                pole(0.05, 2.6, Vec3::new(-2.4, 0.6, 0.0), 0),
                pole(0.05, 2.6, Vec3::new(2.4, 0.6, 0.0), 0),
                cube(Vec3::new(5.4, 2.2, 0.12), Vec3::new(0.0, 0.0, 2.65), 1),
                cube(Vec3::new(4.8, 0.03, 2.0), Vec3::new(0.0, 0.7, 1.2), 2),
                cube(Vec3::new(2.0, 0.45, 0.06), Vec3::new(0.0, 0.35, 0.47), 3),
                cube(Vec3::new(0.08, 0.08, 0.45), Vec3::new(-0.9, 0.35, 0.22), 0),
                cube(Vec3::new(0.08, 0.08, 0.45), Vec3::new(0.9, 0.35, 0.22), 0),
            ],
        ),
        "om_buffer_stop" => one(
            "Buffer stop (openMapper)",
            vec![steel(), Material::colored("Red", [0.8, 0.1, 0.1, 1.0]), Material::colored("White", [0.92, 0.92, 0.9, 1.0])],
            vec![
                cube(Vec3::new(0.25, 0.8, 1.0), Vec3::new(-0.75, 0.0, 0.8), 0),
                cube(Vec3::new(0.25, 0.8, 1.0), Vec3::new(0.75, 0.0, 0.8), 0),
                cube(Vec3::new(2.4, 0.3, 0.35), Vec3::new(0.0, -0.3, 1.25), 1),
                cube(Vec3::new(0.6, 0.31, 0.35), Vec3::new(0.0, -0.3, 1.25), 2),
                bar(0.06, Vec3::new(-0.75, 0.4, 0.4), Vec3::new(-0.75, 2.0, 0.4), 0),
                bar(0.06, Vec3::new(0.75, 0.4, 0.4), Vec3::new(0.75, 2.0, 0.4), 0),
            ],
        ),
        _ => {
            // the station stop: a small post with a sign, OMSI's [busstop] (the stations of
            // stations.cfg are found by it, and passengers wait at it)
            let mut s = one(
                "Station stop (openMapper)",
                vec![steel(), Material::colored("Sign", [0.1, 0.35, 0.7, 1.0]), Material::colored("White", [0.95, 0.95, 0.95, 1.0])],
                vec![pole(0.04, 2.3, Vec3::ZERO, 0), cube(Vec3::new(0.6, 0.04, 0.4), Vec3::new(0.0, 0.0, 2.3), 1), cube(Vec3::new(0.3, 0.05, 0.08), Vec3::new(0.0, 0.0, 2.35), 2)],
            );
            s.collision = false;
            s.extra = "[busstop]\n\n".into();
            s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kit_is_made_and_the_game_reads_its_splines_and_objects() {
        let dir = std::env::temp_dir().join(format!("om-kit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let roots = Roots { omsi: dir.join("omsi"), content: dir.join("content") };
        let n = install(&roots).unwrap();
        assert_eq!(n, 11 + 1 + KIT_SPLINES.len() + KIT_OBJECTS.len());
        assert!(installed(&roots.content));
        assert_eq!(install(&roots).unwrap(), 0, "an installed kit is not made again");
        for s in KIT_SPLINES {
            let path = omsi_cfg::resolve_path(&roots.content, &spline_rel(s.stem));
            let sli = omsi_scenery::sli::Spline::load(&path).unwrap_or_else(|e| panic!("{}: {e}", s.stem));
            assert!(!sli.profiles.is_empty(), "{}", s.stem);
            let rails = sli.paths.iter().filter(|p| p.kind == 2).count();
            assert_eq!(rails, usize::from(s.kind == Piece::Track), "{}", s.stem);
            for t in &sli.textures {
                assert!(path.parent().unwrap().join("texture").join(&t.file).is_file(), "{}: {}", s.stem, t.file);
            }
            assert_eq!(sli.third_rail.len(), usize::from(s.stem.starts_with("om_metro") && s.kind == Piece::Track), "{}", s.stem);
        }
        for stem in KIT_OBJECTS {
            let sco = omsi_scenery::sco::SceneryObject::load(&omsi_cfg::resolve_path(&roots.content, &object_rel(stem))).unwrap();
            assert!(!sco.model.meshes.is_empty(), "{stem}");
            assert_eq!(sco.is_bus_stop, *stem == "om_station_stop", "{stem}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_modern_map_starts_with_rules_and_networks_for_its_modes() {
        let mut t = super::super::Transit { modes: Modes::of(&[Mode::Tram, Mode::Metro, Mode::Trolleybus]), ..Default::default() };
        let modes = t.modes;
        seed(&mut t, modes);
        seed(&mut t, modes);
        assert_eq!(t.file(File::Network).of("track").count(), 3, "once each");
        assert_eq!(t.file(File::Catenary).blocks[0].get("voltage"), Some("600"));
        assert!(t.file(File::ThirdRail).of("thirdrail_net").next().is_some());
        assert!(t.file(File::Trolley).of("trolley_net").next().is_some());
        let p = t.problems();
        assert!(p.iter().all(|x| !x.error), "{p:?}");
    }
}
