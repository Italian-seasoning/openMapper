//! 3D models of other tools read into one shape and written as a DirectX `.x` text mesh
//! that OMSI 2 and openOMSI read: glTF / GLB (Blender's and most tools' modern export) and
//! FBX (3ds Max, Maya, SketchUp, game asset stores). Positions come in OMSI's frame (x
//! right, y up, z forward, left-handed), with every node's transform applied; materials
//! keep their colour, their colour texture, and their normal and occlusion / roughness /
//! metalness maps, which become the `_nn` / `_orm` maps openOMSI finds beside a texture.

use std::fmt::Write as _;
use std::path::Path;

use glam::{Mat4, Vec3};

use crate::i18n::tr;

/// A material as it comes out of a model.
#[derive(Default)]
pub struct Mat {
    pub color: [f32; 4],
    pub base: Option<image::RgbaImage>,
    /// The normal map and whether it is OpenGL-style (green up).
    pub normal: Option<(image::RgbaImage, bool)>,
    /// Occlusion, roughness, metalness in red, green, blue.
    pub orm: Option<image::RgbaImage>,
}

/// A model in OMSI's frame: vertices (position, normal, uv with v down), triangles with
/// their material.
#[derive(Default)]
pub struct Model {
    pub verts: Vec<([f32; 3], [f32; 3], [f32; 2])>,
    pub faces: Vec<([u32; 3], usize)>,
    pub mats: Vec<Mat>,
}

impl Model {
    /// Add a triangle given in a right-handed y-up frame (glTF, FBX converted, OBJ): z turns
    /// round into OMSI's left-handed one, and with it the winding.
    fn push_rh(&mut self, v: [(Vec3, Vec3, [f32; 2]); 3], mat: usize) {
        let base = self.verts.len() as u32;
        for (p, n, uv) in v {
            self.verts.push(([p.x, p.y, -p.z], [n.x, n.y, -n.z], uv));
        }
        self.faces.push(([base, base + 2, base + 1], mat));
    }
}

/// Read a picture out of memory or a file into RGBA, whatever OMSI-supported or common
/// format it is in (png, jpg, bmp, tga, dds, tiff, webp).
pub fn picture(bytes: Option<&[u8]>, file: Option<&Path>) -> Option<image::RgbaImage> {
    if let Some(b) = bytes.filter(|b| !b.is_empty()) {
        if let Ok(i) = image::load_from_memory(b) {
            return Some(i.to_rgba8());
        }
    }
    let f = file?;
    if !f.is_file() {
        return None;
    }
    if let Ok(i) = image::open(f) {
        return Some(i.to_rgba8());
    }
    let i = omsi_texture::decode_file(f).ok()?;
    image::RgbaImage::from_raw(i.width, i.height, i.rgba)
}

/// Occlusion, roughness, metalness packed into one picture of the largest's size (any may
/// be missing: a value stands in for it).
fn pack_orm(ao: Option<(&image::RgbaImage, usize)>, rough: (Option<(&image::RgbaImage, usize)>, f32), metal: (Option<(&image::RgbaImage, usize)>, f32)) -> image::RgbaImage {
    let size = [ao.map(|a| a.0), rough.0.map(|r| r.0), metal.0.map(|m| m.0)].into_iter().flatten().map(|i| i.dimensions()).max_by_key(|d| d.0 * d.1).unwrap_or((4, 4));
    let fit = |i: &image::RgbaImage| if i.dimensions() == size { i.clone() } else { image::imageops::resize(i, size.0, size.1, image::imageops::FilterType::Triangle) };
    let (ao, r, m) = (ao.map(|(i, c)| (fit(i), c)), rough.0.map(|(i, c)| (fit(i), c)), metal.0.map(|(i, c)| (fit(i), c)));
    image::RgbaImage::from_fn(size.0, size.1, |x, y| {
        let ch = |src: &Option<(image::RgbaImage, usize)>, k: f32| src.as_ref().map(|(i, c)| (i.get_pixel(x, y)[*c] as f32 * k).round() as u8).unwrap_or((k * 255.0).round() as u8);
        image::Rgba([ch(&ao, 1.0), ch(&r, rough.1), ch(&m, metal.1), 255])
    })
}

// ---------------------------------------------------------------------------- glTF

pub fn load_gltf(path: &Path) -> anyhow::Result<Model> {
    let (doc, buffers, images) = gltf::import(path)?;
    let mut m = Model::default();
    let rgba = |i: usize| -> Option<image::RgbaImage> {
        let d = images.get(i)?;
        let px: Vec<u8> = match d.format {
            gltf::image::Format::R8G8B8A8 => d.pixels.clone(),
            gltf::image::Format::R8G8B8 => d.pixels.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
            gltf::image::Format::R8G8 => d.pixels.chunks(2).flat_map(|c| [c[0], c[1], 0, 255]).collect(),
            gltf::image::Format::R8 => d.pixels.iter().flat_map(|c| [*c, *c, *c, 255]).collect(),
            _ => return None,
        };
        image::RgbaImage::from_raw(d.width, d.height, px)
    };
    // materials (the last one for primitives without any)
    for mat in doc.materials() {
        let pbr = mat.pbr_metallic_roughness();
        let base = pbr.base_color_texture().and_then(|t| rgba(t.texture().source().index()));
        let normal = mat.normal_texture().and_then(|t| rgba(t.texture().source().index())).map(|i| (i, true));
        let mr = pbr.metallic_roughness_texture().and_then(|t| rgba(t.texture().source().index()));
        let occ = mat.occlusion_texture().and_then(|t| rgba(t.texture().source().index()));
        // (only with a colour texture: the maps go beside it by its name)
        let orm = base.as_ref().map(|_| pack_orm(occ.as_ref().map(|i| (i, 0)), (mr.as_ref().map(|i| (i, 1)), pbr.roughness_factor()), (mr.as_ref().map(|i| (i, 2)), pbr.metallic_factor())));
        m.mats.push(Mat { color: pbr.base_color_factor(), base, normal, orm });
    }
    let default_mat = m.mats.len();
    m.mats.push(Mat { color: [0.8, 0.8, 0.8, 1.0], ..Default::default() });
    let scene = doc.default_scene().or_else(|| doc.scenes().next()).ok_or_else(|| anyhow::anyhow!("{}", tr("The file has no scene", "В файле нет сцены")))?;
    let mut stack: Vec<(gltf::Node, Mat4)> = scene.nodes().map(|n| (n, Mat4::IDENTITY)).collect();
    while let Some((node, parent)) = stack.pop() {
        let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        let nm = Mat4::from_mat3(glam::Mat3::from_mat4(world).inverse().transpose());
        if let Some(mesh) = node.mesh() {
            for prim in mesh.primitives().filter(|p| p.mode() == gltf::mesh::Mode::Triangles) {
                let r = prim.reader(|b| buffers.get(b.index()).map(|d| &d.0[..]));
                let Some(pos) = r.read_positions().map(|p| p.collect::<Vec<_>>()) else { continue };
                let nor: Vec<[f32; 3]> = r.read_normals().map(|n| n.collect()).unwrap_or_default();
                let uv: Vec<[f32; 2]> = r.read_tex_coords(0).map(|t| t.into_f32().collect()).unwrap_or_default();
                let idx: Vec<u32> = r.read_indices().map(|i| i.into_u32().collect()).unwrap_or_else(|| (0..pos.len() as u32).collect());
                let mat = prim.material().index().unwrap_or(default_mat);
                for t in idx.chunks_exact(3) {
                    let corner = |i: u32| {
                        let i = i as usize;
                        let p = world.transform_point3(Vec3::from(pos[i]));
                        let n = nor.get(i).map(|n| nm.transform_vector3(Vec3::from(*n)).normalize_or_zero()).unwrap_or(Vec3::Y);
                        // (glTF's v already runs down the picture, as DirectX's)
                        (p, n, uv.get(i).copied().unwrap_or([0.0, 0.0]))
                    };
                    m.push_rh([corner(t[0]), corner(t[1]), corner(t[2])], mat);
                }
            }
        }
        stack.extend(node.children().map(|c| (c, world)));
    }
    Ok(m)
}

// ---------------------------------------------------------------------------- FBX

pub fn load_fbx(path: &Path) -> anyhow::Result<Model> {
    let opts = ufbx::LoadOpts {
        target_axes: ufbx::CoordinateAxes::right_handed_y_up(),
        target_unit_meters: 1.0,
        space_conversion: ufbx::SpaceConversion::ModifyGeometry,
        generate_missing_normals: true,
        ..Default::default()
    };
    let scene = ufbx::load_file(&path.to_string_lossy(), opts).map_err(|e| anyhow::anyhow!("FBX: {}", e.description.as_ref() as &str))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut m = Model::default();
    let mut ids: Vec<u32> = Vec::new();
    let tex_of = |t: &Option<ufbx::Ref<ufbx::Texture>>| -> Option<image::RgbaImage> {
        let t = t.as_ref()?;
        let files = [t.absolute_filename.as_ref() as &str, t.filename.as_ref(), t.relative_filename.as_ref()];
        let file = files.iter().filter(|f| !f.is_empty()).map(|f| Path::new(&f.replace('\\', "/")).to_path_buf()).flat_map(|p| [p.clone(), dir.join(&p), dir.join(p.file_name().unwrap_or_default())]).find(|p| p.is_file());
        picture(Some(&t.content), file.as_deref())
    };
    let mut material = |mat: Option<&ufbx::Material>, m: &mut Model| -> usize {
        let id = mat.map(|x| x.element.element_id).unwrap_or(u32::MAX);
        if let Some(i) = ids.iter().position(|x| *x == id) {
            return i;
        }
        ids.push(id);
        let Some(x) = mat else {
            m.mats.push(Mat { color: [0.8, 0.8, 0.8, 1.0], ..Default::default() });
            return m.mats.len() - 1;
        };
        let (p, f) = (&x.pbr, &x.fbx);
        let c = if p.base_color.has_value { p.base_color.value_vec4 } else { f.diffuse_color.value_vec4 };
        let base = tex_of(&p.base_color.texture).or_else(|| tex_of(&f.diffuse_color.texture));
        let normal = tex_of(&p.normal_map.texture).or_else(|| tex_of(&f.normal_map.texture)).map(|i| (i, true));
        let (rough, metal, ao) = (tex_of(&p.roughness.texture), tex_of(&p.metalness.texture), tex_of(&p.ambient_occlusion.texture));
        let rf = if p.roughness.has_value { p.roughness.value_vec4.x as f32 } else { 0.8 };
        let mf = if p.metalness.has_value { p.metalness.value_vec4.x as f32 } else { 0.0 };
        let orm = (base.is_some() && (rough.is_some() || metal.is_some() || ao.is_some())).then(|| pack_orm(ao.as_ref().map(|i| (i, 0)), (rough.as_ref().map(|i| (i, 0)), if rough.is_some() { 1.0 } else { rf }), (metal.as_ref().map(|i| (i, 0)), if metal.is_some() { 1.0 } else { mf })));
        m.mats.push(Mat { color: [c.x as f32, c.y as f32, c.z as f32, 1.0], base, normal, orm });
        m.mats.len() - 1
    };
    let mut tri = vec![0u32; 64];
    for node in scene.nodes.iter() {
        let Some(mesh) = node.mesh.as_ref() else { continue };
        let g = &node.geometry_to_world;
        let nmat = ufbx::matrix_for_normals(g);
        tri.resize(mesh.max_face_triangles * 3, 0);
        for (fi, face) in mesh.faces.iter().enumerate() {
            let n = mesh.triangulate_face(&mut tri, *face) as usize;
            let mi = mesh.face_material.get(fi).copied().unwrap_or(0) as usize;
            let mat = node.materials.get(mi).or_else(|| mesh.materials.get(mi)).map(|r| &**r);
            let mat = material(mat, &mut m);
            for t in tri[..n * 3].chunks_exact(3) {
                let corner = |i: u32| {
                    let i = i as usize;
                    let p = ufbx::transform_position(g, mesh.vertex_position[i]);
                    let nrm = if mesh.vertex_normal.exists { ufbx::transform_direction(&nmat, mesh.vertex_normal[i]) } else { ufbx::Vec3 { x: 0.0, y: 1.0, z: 0.0 } };
                    let uv = if mesh.vertex_uv.exists { mesh.vertex_uv[i] } else { ufbx::Vec2 { x: 0.0, y: 0.0 } };
                    // (FBX's v runs up the picture, DirectX's down)
                    (Vec3::new(p.x as f32, p.y as f32, p.z as f32), Vec3::new(nrm.x as f32, nrm.y as f32, nrm.z as f32).normalize_or_zero(), [uv.x as f32, 1.0 - uv.y as f32])
                };
                m.push_rh([corner(t[0]), corner(t[1]), corner(t[2])], mat);
            }
        }
    }
    if m.faces.is_empty() {
        anyhow::bail!("{}", tr("The FBX file has no meshes", "В файле FBX нет мешей"));
    }
    Ok(m)
}

// ---------------------------------------------------------------------------- writing

/// Write the textures of the model's materials as `<stem>_<n>.png` with their
/// `_nn` / `_nn_gl` and `_orm` maps beside them; the colour texture's name per material.
pub fn save_textures(m: &Model, stem: &str, dir: &Path) -> anyhow::Result<Vec<Option<String>>> {
    let mut names = Vec::new();
    for (i, mat) in m.mats.iter().enumerate() {
        let Some(base) = &mat.base else {
            names.push(None);
            continue;
        };
        let name = format!("{stem}_{i}");
        base.save(dir.join(format!("{name}.png")))?;
        if let Some((n, gl)) = &mat.normal {
            n.save(dir.join(format!("{name}_nn{}.png", if *gl { "_gl" } else { "" })))?;
        }
        if let Some(orm) = &mat.orm {
            orm.save(dir.join(format!("{name}_orm.png")))?;
        }
        names.push(Some(format!("{name}.png")));
    }
    Ok(names)
}

/// The model as a DirectX `.x` text mesh; `textures`: each material's texture file name.
pub fn write_x(m: &Model, textures: &[Option<String>]) -> String {
    let verts = &m.verts;
    let faces = &m.faces;
    let mut x = String::from("xof 0302txt 0032\n\nMesh {\n");
    let _ = writeln!(x, " {};", verts.len());
    for (i, (p, _, _)) in verts.iter().enumerate() {
        let _ = writeln!(x, " {:.6};{:.6};{:.6};{}", p[0], p[1], p[2], if i + 1 == verts.len() { ";" } else { "," });
    }
    let _ = writeln!(x, " {};", faces.len());
    for (i, (f, _)) in faces.iter().enumerate() {
        let _ = writeln!(x, " 3;{},{},{};{}", f[0], f[1], f[2], if i + 1 == faces.len() { ";" } else { "," });
    }
    let _ = writeln!(x, " MeshNormals {{\n  {};", verts.len());
    for (i, (_, n, _)) in verts.iter().enumerate() {
        let _ = writeln!(x, "  {:.6};{:.6};{:.6};{}", n[0], n[1], n[2], if i + 1 == verts.len() { ";" } else { "," });
    }
    let _ = writeln!(x, "  {};", faces.len());
    for (i, (f, _)) in faces.iter().enumerate() {
        let _ = writeln!(x, "  3;{},{},{};{}", f[0], f[1], f[2], if i + 1 == faces.len() { ";" } else { "," });
    }
    x.push_str(" }\n");
    let _ = writeln!(x, " MeshTextureCoords {{\n  {};", verts.len());
    for (i, (_, _, t)) in verts.iter().enumerate() {
        let _ = writeln!(x, "  {:.6};{:.6};{}", t[0], t[1], if i + 1 == verts.len() { ";" } else { "," });
    }
    x.push_str(" }\n");
    let n = m.mats.len().max(1);
    let _ = writeln!(x, " MeshMaterialList {{\n  {};\n  {};", n, faces.len());
    for (i, (_, mat)) in faces.iter().enumerate() {
        let _ = writeln!(x, "  {}{}", mat, if i + 1 == faces.len() { ";" } else { "," });
    }
    for i in 0..n {
        // (a coloured material shows its colour; one with a texture shows the texture as it is)
        let c = m.mats.get(i).map(|mat| if mat.base.is_some() { [1.0, 1.0, 1.0, mat.color[3]] } else { mat.color }).unwrap_or([0.8, 0.8, 0.8, 1.0]);
        let _ = writeln!(x, "  Material {{\n   {:.4};{:.4};{:.4};{:.4};;\n   10.0;\n   0.1;0.1;0.1;;\n   0.0;0.0;0.0;;", c[0], c[1], c[2], c[3]);
        if let Some(Some(t)) = textures.get(i) {
            let _ = writeln!(x, "   TextureFilename {{\n    \"{t}\";\n   }}");
        }
        x.push_str("  }\n");
    }
    x.push_str(" }\n}\n");
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A glTF of one triangle with a 2 × 2 texture, embedded as data URIs.
    fn triangle_gltf(dir: &Path) -> std::path::PathBuf {
        use base64::Engine;
        let pos: [f32; 9] = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let uv: [f32; 6] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
        let mut bin: Vec<u8> = pos.iter().chain(uv.iter()).flat_map(|f| f.to_le_bytes()).collect();
        bin.extend([0u16, 1, 2].iter().flat_map(|i| i.to_le_bytes()));
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 30, 30, 255])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
            "nodes":[{{"mesh":0,"translation":[10,0,0]}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0,"TEXCOORD_0":1}},"indices":2,"material":0}}]}}],
            "materials":[{{"pbrMetallicRoughness":{{"baseColorTexture":{{"index":0}},"roughnessFactor":0.5,"metallicFactor":0.0}}}}],
            "textures":[{{"source":0}}],"images":[{{"uri":"data:image/png;base64,{}"}}],
            "buffers":[{{"byteLength":{},"uri":"data:application/octet-stream;base64,{}"}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":36}},{{"buffer":0,"byteOffset":36,"byteLength":24}},{{"buffer":0,"byteOffset":60,"byteLength":6}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}},{{"bufferView":1,"componentType":5126,"count":3,"type":"VEC2"}},{{"bufferView":2,"componentType":5123,"count":3,"type":"SCALAR"}}]}}"#,
            b64.encode(&png),
            bin.len(),
            b64.encode(&bin)
        );
        let p = dir.join("tri.gltf");
        std::fs::write(&p, json).unwrap();
        p
    }

    #[test]
    fn a_gltf_comes_in_moved_by_its_node_with_its_texture_and_maps() {
        let dir = std::env::temp_dir().join(format!("om-gltf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let m = load_gltf(&triangle_gltf(&dir)).unwrap();
        assert_eq!(m.faces.len(), 1);
        // the node's translation is in, z turned round (none here), winding reversed
        assert!((m.verts[0].0[0] - 10.0).abs() < 1e-6 && (m.verts[1].0[0] - 11.0).abs() < 1e-6);
        assert_eq!(m.faces[0].0, [0, 2, 1]);
        let names = save_textures(&m, "tri", &dir).unwrap();
        assert_eq!(names[0].as_deref(), Some("tri_0.png"));
        assert!(dir.join("tri_0.png").is_file() && dir.join("tri_0_orm.png").is_file());
        // the roughness factor in the packed map's green
        let orm = image::open(dir.join("tri_0_orm.png")).unwrap().to_rgba8();
        assert_eq!(orm.get_pixel(0, 0)[1], 128);
        let x = write_x(&m, &names);
        assert!(x.contains("\"tri_0.png\""));
        // and openOMSI's own reader takes the mesh
        std::fs::write(dir.join("tri.x"), &x).unwrap();
        let back = omsi_o3d::load_mesh(&dir.join("tri.x")).unwrap();
        assert_eq!(back.materials.len(), 2);
        assert_eq!(back.materials[0].texture, "tri_0.png");
        let _ = std::fs::remove_dir_all(dir);
    }

}
