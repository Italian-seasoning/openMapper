//! The overhead of the transit kit as objects: catenary masts with their cantilever (bracket,
//! registration and steady arm, insulators) reaching to where the contact wire hangs, trolleybus
//! poles with a bracket carrying an insulated hanger over each pair of wires, the trolleybus
//! wire's frogs, and section insulators. Masts and poles are made to measure: their file stem
//! says how far they reach (`om_catenary_mast_r30`: 3.0 m; `om_trolleybus_pole_h35_70`:
//! hangers 3.5 m and 7.0 m out), and one not made yet is made when wiring asks for it.
//!
//! Their textures are openMapper's own: galvanized steel, painted steel, copper, porcelain,
//! the frogs' cast housing.

use glam::{Mat4, Quat, Vec3};
use openmapper_mesh::primitives::Primitive;
use openmapper_mesh::{Material, Mesh, Model, Part};

use crate::editable::Source;
use crate::install::Roots;

/// Height of the contact wire and its messenger over the ground the mast stands on, and of the
/// trolleybus wires (as the kit's wire splines hang over a track and a road).
const CONTACT: f32 = 5.95;
const MESSENGER: f32 = CONTACT + 1.1;
const TROLLEY: f32 = 5.9 + 0.1;

fn shape(p: Primitive, m: Mat4, material: u32) -> Mesh {
    let mut mesh = p.build();
    mesh.transform(m);
    for f in &mut mesh.faces {
        f.material = material;
        f.smooth = p.round();
    }
    mesh
}

/// A round bar of radius `r` from `a` to `b`.
fn bar(r: f32, a: Vec3, b: Vec3, material: u32) -> Mesh {
    let d = b - a;
    let rot = Quat::from_rotation_arc(Vec3::Z, d.normalize_or_zero());
    shape(Primitive::Cylinder { radius: r, height: d.length(), segments: 10 }, Mat4::from_translation(a) * Mat4::from_quat(rot), material)
}

/// A tapered tube from `a` (radius `ra`) to `b` (radius `rb`).
fn taper(ra: f32, rb: f32, a: Vec3, b: Vec3, material: u32) -> Mesh {
    let d = b - a;
    let rot = Quat::from_rotation_arc(Vec3::Z, d.normalize_or_zero());
    shape(Primitive::Cone { radius_bottom: ra, radius_top: rb, height: d.length(), segments: 14 }, Mat4::from_translation(a) * Mat4::from_quat(rot), material)
}

fn cube(size: Vec3, at: Vec3, material: u32) -> Mesh {
    shape(Primitive::Cube { size }, Mat4::from_translation(at), material)
}

/// A string of porcelain discs from `a` to `b` (an insulator), its rod of steel.
fn insulator(a: Vec3, b: Vec3, porcelain: u32, steel: u32) -> Mesh {
    let mut m = bar(0.012, a, b, steel);
    let d = b - a;
    let n = ((d.length() / 0.09).floor() as usize).clamp(2, 8);
    let rot = Quat::from_rotation_arc(Vec3::Z, d.normalize_or_zero());
    for i in 0..n {
        let at = a + d * ((i as f32 + 0.5) / n as f32);
        m.append(&shape(Primitive::Cylinder { radius: 0.045, height: 0.03, segments: 12 }, Mat4::from_translation(at) * Mat4::from_quat(rot), porcelain), 0);
    }
    m
}

fn textured(name: &str, texture: &str) -> Material {
    Material { name: name.into(), color: [1.0, 1.0, 1.0, 1.0], texture: Some(texture.into()), ..Default::default() }
}

/// The materials every overhead object uses, in this order.
fn materials() -> Vec<Material> {
    vec![textured("Galvanized", "om_galvanized.bmp"), textured("Painted", "om_painted.bmp"), textured("Porcelain", "om_porcelain.bmp"), textured("Copper", "om_copper.bmp"), textured("Housing", "om_housing.bmp"), Material::colored("Concrete", [0.55, 0.55, 0.52, 1.0]), textured("Cast concrete", "om_concrete.bmp")]
}
const GALV: u32 = 0;
const PAINT: u32 = 1;
const PORC: u32 = 2;
const COPPER: u32 = 3;
const HOUSING: u32 = 4;
const CONCRETE: u32 = 5;
const CONCRETE_TEX: u32 = 6;

fn source(name: &str, body: Mesh, collision: bool) -> Source {
    Source { name: name.into(), model: Model { parts: vec![Part::new("Body", body)], materials: materials() }, collision, ..Default::default() }
}

/// A catenary mast reaching `reach` m to its right (+x) to the wire: a tapered steel mast on a
/// concrete footing, the cantilever's bracket and stay with their insulators, the registration
/// arm down to the contact wire with its steady arm, and the messenger's support on top.
pub fn catenary_mast(reach: f32) -> Source {
    let r = reach.clamp(1.5, 8.0);
    let mut m = Mesh::default();
    m.append(&cube(Vec3::new(0.7, 0.7, 0.3), Vec3::new(0.0, 0.0, 0.1), CONCRETE), 0);
    m.append(&taper(0.15, 0.09, Vec3::new(0.0, 0.0, 0.25), Vec3::new(0.0, 0.0, MESSENGER + 0.6), GALV), 0);
    m.append(&shape(Primitive::Cylinder { radius: 0.11, height: 0.04, segments: 14 }, Mat4::from_translation(Vec3::new(0.0, 0.0, MESSENGER + 0.6)), GALV), 0);
    // the hinges on the mast, insulated from it
    let (low, high) = (Vec3::new(0.0, 0.0, CONTACT + 0.35), Vec3::new(0.0, 0.0, MESSENGER + 0.15));
    m.append(&insulator(low, low + Vec3::X * 0.45, PORC, GALV), 0);
    m.append(&insulator(high, high + Vec3::X * 0.45, PORC, GALV), 0);
    // the bracket (horizontal tube) out over the wire and the stay tube up to the top hinge
    let bracket_end = Vec3::new(r + 0.35, 0.0, CONTACT + 0.35);
    m.append(&bar(0.03, low + Vec3::X * 0.45, bracket_end, GALV), 0);
    m.append(&bar(0.025, high + Vec3::X * 0.45, Vec3::new(r * 0.8, 0.0, CONTACT + 0.37), GALV), 0);
    // the messenger's clamp on top of the bracket, over the wire
    m.append(&bar(0.02, Vec3::new(r, 0.0, CONTACT + 0.37), Vec3::new(r, 0.0, MESSENGER), GALV), 0);
    m.append(&cube(Vec3::new(0.12, 0.08, 0.06), Vec3::new(r, 0.0, MESSENGER), GALV), 0);
    // the registration arm down to the contact wire, its steady arm holding the wire
    m.append(&bar(0.022, Vec3::new(r - 0.6, 0.0, CONTACT + 0.35), Vec3::new(r - 0.25, 0.0, CONTACT + 0.12), GALV), 0);
    m.append(&bar(0.014, Vec3::new(r - 0.25, 0.0, CONTACT + 0.12), Vec3::new(r, 0.0, CONTACT + 0.02), GALV), 0);
    m.append(&cube(Vec3::new(0.06, 0.05, 0.04), Vec3::new(r, 0.0, CONTACT + 0.02), COPPER), 0);
    source("Catenary mast (openMapper)", m, true)
}

/// A trolleybus pole whose bracket reaches out (+x) over the road, an insulated hanger under it
/// at each of `pairs` (m out from the pole) holding a pair of wires 0.6 m apart.
pub fn trolleybus_pole(pairs: &[f32]) -> Source {
    let pairs: Vec<f32> = pairs.iter().map(|x| x.clamp(0.8, 14.0)).collect();
    let far = pairs.iter().copied().fold(1.5, f32::max) + 0.5;
    let arm_z = TROLLEY + 0.65;
    let mut m = Mesh::default();
    m.append(&cube(Vec3::new(0.6, 0.6, 0.25), Vec3::new(0.0, 0.0, 0.08), CONCRETE), 0);
    m.append(&taper(0.16, 0.1, Vec3::new(0.0, 0.0, 0.2), Vec3::new(0.0, 0.0, arm_z + 1.4), PAINT), 0);
    m.append(&shape(Primitive::UvSphere { radius: 0.1, segments: 12, rings: 6 }, Mat4::from_translation(Vec3::new(0.0, 0.0, arm_z + 1.4)), PAINT), 0);
    // the bracket and its tie rod
    m.append(&bar(0.045, Vec3::new(0.0, 0.0, arm_z), Vec3::new(far, 0.0, arm_z - 0.1), PAINT), 0);
    m.append(&bar(0.018, Vec3::new(0.0, 0.0, arm_z + 1.3), Vec3::new(far * 0.7, 0.0, arm_z - 0.07), GALV), 0);
    for x in pairs {
        let top = arm_z - 0.1 * (x / far);
        // an insulated crossbar under the bracket, a clamp on each wire
        m.append(&insulator(Vec3::new(x, 0.0, top), Vec3::new(x, 0.0, TROLLEY + 0.25), PORC, GALV), 0);
        m.append(&bar(0.02, Vec3::new(x - 0.45, 0.0, TROLLEY + 0.25), Vec3::new(x + 0.45, 0.0, TROLLEY + 0.25), GALV), 0);
        for w in [x - 0.3, x + 0.3] {
            m.append(&insulator(Vec3::new(w, 0.0, TROLLEY + 0.25), Vec3::new(w, 0.0, TROLLEY + 0.04), PORC, GALV), 0);
            m.append(&cube(Vec3::new(0.08, 0.14, 0.05), Vec3::new(w, 0.0, TROLLEY + 0.02), COPPER), 0);
        }
    }
    source("Trolleybus pole (openMapper)", m, true)
}

/// A frog of the trolleybus wire, the wires coming in along -y and leaving along +y and to a
/// side: the cast housing over each wire, the guiding runners, the switch's motor box.
pub fn frog() -> Source {
    let mut m = Mesh::default();
    for w in [-0.3f32, 0.3] {
        m.append(&cube(Vec3::new(0.11, 0.9, 0.07), Vec3::new(w, 0.0, TROLLEY + 0.03), HOUSING), 0);
        // runners: straight on and off to the side
        m.append(&bar(0.012, Vec3::new(w, -0.6, TROLLEY), Vec3::new(w, 0.6, TROLLEY), COPPER), 0);
        m.append(&bar(0.012, Vec3::new(w, -0.1, TROLLEY), Vec3::new(w + 0.17, 0.6, TROLLEY), COPPER), 0);
    }
    m.append(&insulator(Vec3::new(-0.24, 0.0, TROLLEY + 0.06), Vec3::new(0.24, 0.0, TROLLEY + 0.06), PORC, GALV), 0);
    m.append(&cube(Vec3::new(0.25, 0.2, 0.18), Vec3::new(0.0, -0.3, TROLLEY + 0.2), HOUSING), 0);
    m.append(&bar(0.01, Vec3::new(0.0, -0.3, TROLLEY + 0.29), Vec3::new(0.0, -0.3, TROLLEY + 0.6), GALV), 0);
    source("Trolleybus frog (openMapper)", m, false)
}

/// A section insulator: a runner of insulating rods in each wire (one for the overhead
/// contact wire with `trolley` false).
pub fn section_insulator(trolley: bool) -> Source {
    let mut m = Mesh::default();
    let (z, wires): (f32, Vec<f32>) = if trolley { (TROLLEY, vec![-0.3, 0.3]) } else { (CONTACT, vec![0.0]) };
    for w in wires {
        m.append(&insulator(Vec3::new(w, -0.5, z + 0.02), Vec3::new(w, 0.5, z + 0.02), PORC, GALV), 0);
        for s in [-0.08f32, 0.08] {
            m.append(&bar(0.008, Vec3::new(w + s, -0.6, z), Vec3::new(w + s, 0.6, z), HOUSING), 0);
        }
    }
    source("Section insulator (openMapper)", m, false)
}

/// The object a stem names, or None.
pub fn of_stem(stem: &str) -> Option<Source> {
    let dm = |s: &str| s.parse::<f32>().ok().map(|v| v / 10.0);
    if let Some(r) = stem.strip_prefix("om_catenary_mast_r") {
        return Some(catenary_mast(dm(r)?));
    }
    if let Some(h) = stem.strip_prefix("om_trolleybus_pole_h") {
        let pairs: Option<Vec<f32>> = h.split('_').map(dm).collect();
        return Some(trolleybus_pole(&pairs?));
    }
    if let Some(wh) = stem.strip_prefix("om_tunnel_portal_w") {
        let (w, h) = wh.split_once("_h")?;
        return Some(portal(dm(w)?, dm(h)?));
    }
    match stem {
        "om_trolleybus_frog" => Some(frog()),
        "om_section_insulator_trolley" => Some(section_insulator(true)),
        "om_section_insulator" => Some(section_insulator(false)),
        _ => None,
    }
}

/// A tunnel's portal: a concrete headwall round a mouth `width` m wide and `dig::CLEAR` high,
/// `height` m from the road up to over the ground, its coping on top; the tunnel runs along
/// -y behind it.
pub fn portal(width: f32, height: f32) -> Source {
    let (w, h, clear) = (width.clamp(3.0, 30.0), height.clamp(5.0, 30.0), crate::dig::CLEAR as f32);
    let (side, thick) = (1.2, 0.8);
    let mut m = Mesh::default();
    for s in [-1.0f32, 1.0] {
        // the pillars beside the mouth, and a wing wall angled back into the bank
        m.append(&cube(Vec3::new(side, thick, h), Vec3::new(s * (w / 2.0 + side / 2.0), 0.0, h / 2.0), CONCRETE_TEX), 0);
        let wing = Mat4::from_translation(Vec3::new(s * (w / 2.0 + side + 1.6), 1.2, h * 0.4)) * Mat4::from_rotation_z(s * 0.6);
        m.append(&shape(Primitive::Cube { size: Vec3::new(4.0, 0.5, h * 0.8) }, wing, CONCRETE_TEX), 0);
    }
    // the lintel over the mouth and the coping along the top
    m.append(&cube(Vec3::new(w, thick, h - clear), Vec3::new(0.0, 0.0, clear + (h - clear) / 2.0), CONCRETE_TEX), 0);
    m.append(&cube(Vec3::new(w + 2.0 * side + 0.4, thick + 0.3, 0.3), Vec3::new(0.0, 0.0, h + 0.15), GALV), 0);
    source("Tunnel portal (openMapper)", m, true)
}

/// The stem of a portal `width` m wide, `height` m high (to 0.1 m).
pub fn portal_stem(width: f32, height: f32) -> String {
    format!("om_tunnel_portal_w{}_h{}", (width * 10.0).round() as i32, (height * 10.0).round() as i32)
}

/// The frog's stem.
pub fn frog_stem() -> &'static str {
    "om_trolleybus_frog"
}

/// The stem of a mast reaching `reach` m, of a pole with hangers `pairs` m out (to 0.1 m).
pub fn mast_stem(reach: f64) -> String {
    format!("om_catenary_mast_r{}", (reach * 10.0).round().clamp(15.0, 80.0) as i64)
}

pub fn pole_stem(pairs: &[f64]) -> String {
    let mut v: Vec<i64> = pairs.iter().map(|x| (x * 10.0).round().clamp(8.0, 140.0) as i64).collect();
    v.sort();
    v.dedup();
    format!("om_trolleybus_pole_h{}", v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("_"))
}

// ----- textures -----

fn noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263) ^ seed.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

/// A value 0..1 smooth over cells of `cell` pixels (128 square, repeating).
fn soft(x: u32, y: u32, cell: u32, seed: u32) -> f32 {
    let n = 128 / cell;
    let (fx, fy) = (x as f32 / cell as f32, y as f32 / cell as f32);
    let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
    let at = |i: u32, j: u32| noise(i % n, j % n, seed);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let a = at(x0, y0) + (at(x0 + 1, y0) - at(x0, y0)) * s(tx);
    let b = at(x0, y0 + 1) + (at(x0 + 1, y0 + 1) - at(x0, y0 + 1)) * s(tx);
    a + (b - a) * s(ty)
}

fn img(seed: u32, f: impl Fn(f32, f32, f32, f32) -> [f32; 3]) -> image::RgbImage {
    image::RgbImage::from_fn(128, 128, |x, y| {
        let fine = noise(x, y, seed);
        let blotch = 0.6 * soft(x, y, 8, seed + 1) + 0.4 * soft(x, y, 32, seed + 2);
        let c = f(x as f32 / 128.0, y as f32 / 128.0, fine, blotch);
        image::Rgb([c[0].clamp(0.0, 255.0) as u8, c[1].clamp(0.0, 255.0) as u8, c[2].clamp(0.0, 255.0) as u8])
    })
}

/// The overhead objects' textures (BMP, 128 pixels square).
pub fn textures() -> Vec<(&'static str, image::RgbImage)> {
    // galvanized steel: grey zinc with its spangle and darker weathered blotches
    let galvanized = img(31, |_, _, f, b| {
        let g = 150.0 + 40.0 * f - 45.0 * b;
        [g, g + 3.0, g + 6.0]
    });
    // painted steel (a dark green as many towns paint their poles), a little worn
    let painted = img(32, |_, _, f, b| {
        let worn = if b > 0.82 { 0.6 } else { 1.0 };
        [42.0 * worn + 10.0 * f + 30.0 * (1.0 - worn), 70.0 * worn + 10.0 * f + 30.0 * (1.0 - worn), 52.0 * worn + 8.0 * f + 30.0 * (1.0 - worn)]
    });
    // glazed brown porcelain with light catching its rings
    let porcelain = img(33, |_, v, f, _| {
        let ring = ((v * 8.0).fract() - 0.5).abs() * 2.0;
        let s = 0.75 + 0.35 * ring + 0.05 * f;
        [120.0 * s, 62.0 * s, 32.0 * s]
    });
    // copper gone dull brown, a hint of green
    let copper = img(34, |_, _, f, b| [120.0 + 30.0 * f - 20.0 * b, 78.0 + 20.0 * f + 10.0 * b, 52.0 + 10.0 * f + 15.0 * b]);
    // the frogs' cast housing: dark grey iron
    let housing = img(35, |_, _, f, b| {
        let g = 58.0 + 25.0 * f + 15.0 * b;
        [g, g, g + 4.0]
    });
    // cast concrete: the formwork's boards and a few stains
    let concrete = img(36, |_, v, f, b| {
        let board = if (v * 8.0).fract() < 0.03 { 0.82 } else { 1.0 };
        let g = (138.0 + 22.0 * f - 26.0 * b) * board;
        [g, g - 2.0, g - 6.0]
    });
    vec![("om_concrete.bmp", concrete), ("om_galvanized.bmp", galvanized), ("om_painted.bmp", painted), ("om_porcelain.bmp", porcelain), ("om_copper.bmp", copper), ("om_housing.bmp", housing)]
}

/// Make the overhead object `stem` in the content folder if it is not there; its file as the
/// map names it.
pub fn ensure(roots: &Roots, stem: &str) -> anyhow::Result<String> {
    let rel = super::kit::object_rel(stem);
    if omsi_cfg::resolve_path(&roots.content, &rel).is_file() {
        return Ok(rel);
    }
    let src = of_stem(stem).ok_or_else(|| anyhow::anyhow!("{stem}: no such overhead object"))?;
    save(roots, &rel, &src)?;
    Ok(rel)
}

/// Write an object with the textures it uses beside it.
pub fn save(roots: &Roots, rel: &str, src: &Source) -> anyhow::Result<()> {
    crate::editable::save(roots, rel, src)?;
    let (dir, _) = crate::editable::folder_of(roots, rel);
    let tex = dir.join("texture");
    std::fs::create_dir_all(&tex)?;
    let wanted: Vec<&str> = src.model.materials.iter().filter_map(|m| m.texture.as_deref()).collect();
    for (name, img) in textures().into_iter().filter(|(n, _)| wanted.contains(n)) {
        img.save_with_format(tex.join(name), image::ImageFormat::Bmp)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masts_and_poles_are_made_to_measure_with_their_textures() {
        let dir = std::env::temp_dir().join(format!("om-overhead-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let roots = Roots { omsi: dir.join("omsi"), content: dir.join("content") };
        assert_eq!(mast_stem(3.04), "om_catenary_mast_r30");
        assert_eq!(pole_stem(&[7.0, 3.5, 3.52]), "om_trolleybus_pole_h35_70");
        for stem in ["om_catenary_mast_r30", "om_trolleybus_pole_h35_70", "om_trolleybus_frog", "om_section_insulator_trolley"] {
            let rel = ensure(&roots, stem).unwrap();
            let path = omsi_cfg::resolve_path(&roots.content, &rel);
            let sco = omsi_scenery::sco::SceneryObject::load(&path).unwrap();
            assert!(!sco.model.meshes.is_empty(), "{stem}");
            assert!(path.parent().unwrap().join("texture").join("om_galvanized.bmp").is_file(), "{stem}");
        }
        // (the mast's steady arm ends where the wire hangs: 3 m out, 5.95 m up)
        let m = catenary_mast(3.0);
        let hi = m.model.parts[0].mesh.positions.iter().fold(f32::MIN, |a, v| a.max(v.x));
        assert!((3.0..3.5).contains(&hi), "{hi}");
        assert!(of_stem("om_nothing").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
