//! Bringing new things in, by dropping them on the window or with the Import button:
//!
//! * a mod (a folder or a `.zip` laid out like OMSI 2: `Sceneryobjects`, `Splines`,
//!   `Texture`, ...) is merged into openOMSI's content folder;
//! * a folder of `.sco` or `.sli` files becomes a folder of `Sceneryobjects` / `Splines`;
//! * a 3D model (`.obj`, `.x`, `.o3d`) becomes a new scenery object: the model goes into
//!   `model\`, its textures and their PBR maps (`_nn`, `_orm`, ...) into `texture\`, and a
//!   `.sco` is written for it. A Wavefront `.obj` (what Blender and most tools export) is
//!   turned into a DirectX `.x` that OMSI 2 and openOMSI both read.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::i18n::tr;

pub struct Report {
    /// Files copied.
    #[allow(dead_code)]
    pub copied: usize,
    pub objects: Vec<String>,
    pub message: String,
}

const CONTENT_FOLDERS: [&str; 6] = ["sceneryobjects", "splines", "texture", "maps", "vehicles", "fonts"];

/// Import whatever `path` is into the content folder.
pub fn import(path: &Path, content: &Path) -> anyhow::Result<Report> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if path.is_file() {
        return match ext.as_str() {
            "zip" => import_zip(path, content),
            "obj" | "x" | "o3d" => import_model(path, content),
            "sco" | "sli" => {
                let dir = path.parent().ok_or_else(|| anyhow::anyhow!("no folder"))?;
                import_folder(dir, content)
            }
            _ => anyhow::bail!("{}", tr("openMapper cannot import this kind of file (use a folder, .zip, .obj, .x or .o3d)", "Этот тип файла не импортируется (подходят папка, .zip, .obj, .x, .o3d)")),
        };
    }
    import_folder(path, content)
}

fn is_content_root(dir: &Path) -> bool {
    std::fs::read_dir(dir).map(|rd| rd.flatten().any(|e| e.path().is_dir() && CONTENT_FOLDERS.contains(&e.file_name().to_string_lossy().to_lowercase().as_str()))).unwrap_or(false)
}

/// The folder inside `dir` that is laid out like OMSI 2 (archives often wrap it).
fn find_content_root(dir: &Path, depth: usize) -> Option<PathBuf> {
    if is_content_root(dir) {
        return Some(dir.to_path_buf());
    }
    if depth == 0 {
        return None;
    }
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).find_map(|p| find_content_root(&p, depth - 1))
}

fn import_folder(dir: &Path, content: &Path) -> anyhow::Result<Report> {
    if let Some(root) = find_content_root(dir, 3) {
        let mut copied = 0;
        for e in std::fs::read_dir(&root)?.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if e.path().is_dir() && CONTENT_FOLDERS.contains(&name.to_lowercase().as_str()) {
                // into the content folder's own spelling of it
                let target = existing_child(content, &name).unwrap_or_else(|| content.join(&name));
                copied += copy_tree(&e.path(), &target)?;
            }
        }
        return Ok(Report { copied, objects: Vec::new(), message: format!("{} {copied}", tr("Mod installed, files:", "Мод установлен, файлов:")) });
    }
    let has = |ext: &str| walk_has(dir, ext);
    let name = dir.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "Imported".into());
    if has("sco") {
        let target = existing_child(content, "Sceneryobjects").unwrap_or_else(|| content.join("Sceneryobjects")).join(&name);
        let copied = copy_tree(dir, &target)?;
        return Ok(Report { copied, objects: Vec::new(), message: format!("{} Sceneryobjects\\{name}", tr("Objects added to", "Объекты добавлены в")) });
    }
    if has("sli") {
        let target = existing_child(content, "Splines").unwrap_or_else(|| content.join("Splines")).join(&name);
        let copied = copy_tree(dir, &target)?;
        return Ok(Report { copied, objects: Vec::new(), message: format!("{} Splines\\{name}", tr("Road types added to", "Типы дорог добавлены в")) });
    }
    // a folder of models: each becomes an object
    let mut objects = Vec::new();
    for e in std::fs::read_dir(dir)?.flatten() {
        let p = e.path();
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if matches!(ext.as_str(), "obj" | "x" | "o3d") {
            let r = import_model(&p, content)?;
            objects.extend(r.objects);
        }
    }
    if objects.is_empty() {
        anyhow::bail!("{}", tr("Nothing to import was found in this folder", "В этой папке нечего импортировать"));
    }
    let n = objects.len();
    Ok(Report { copied: n, objects, message: format!("{} {n}", tr("Models imported:", "Импортировано моделей:")) })
}

fn walk_has(dir: &Path, ext: &str) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else { return false };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if walk_has(&p, ext) {
                return true;
            }
        } else if p.extension().map(|x| x.to_string_lossy().eq_ignore_ascii_case(ext)).unwrap_or(false) {
            return true;
        }
    }
    false
}

fn existing_child(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.is_dir() && p.file_name().map(|f| f.to_string_lossy().eq_ignore_ascii_case(name)).unwrap_or(false))
}

/// Copy a folder over (files that are there already are replaced).
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(dst)?;
    let mut n = 0;
    for e in std::fs::read_dir(src)?.flatten() {
        let from = e.path();
        let name = e.file_name();
        // (a folder of another spelling at the destination is used)
        if from.is_dir() {
            let to = existing_child(dst, &name.to_string_lossy()).unwrap_or_else(|| dst.join(&name));
            n += copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, dst.join(&name))?;
            n += 1;
        }
    }
    Ok(n)
}

fn import_zip(path: &Path, content: &Path) -> anyhow::Result<Report> {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "mod".into());
    let tmp = std::env::temp_dir().join(format!("openmapper-import-{}-{stem}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        // (an entry that would land outside the folder is skipped)
        let Some(rel) = f.enclosed_name() else { continue };
        let rel = PathBuf::from(rel.to_string_lossy().replace('\\', "/"));
        let out = tmp.join(&rel);
        if f.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        std::fs::write(&out, buf)?;
    }
    // a single folder of objects or models inside: import that folder by its name
    let r = import_folder(&tmp, content).or_else(|e| {
        let only: Vec<PathBuf> = std::fs::read_dir(&tmp).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
        if only.len() == 1 {
            import_folder(&only[0], content)
        } else {
            Err(e)
        }
    });
    let _ = std::fs::remove_dir_all(&tmp);
    r
}

/// The PBR maps beside a texture (`bus.png` > `bus_nn.png`, `bus_orm.png`, ...).
fn pbr_siblings(tex: &Path) -> Vec<PathBuf> {
    let Some(dir) = tex.parent() else { return Vec::new() };
    let Some(stem) = tex.file_stem().map(|s| s.to_string_lossy().to_lowercase()) else { return Vec::new() };
    const SUFFIXES: [&str; 20] = ["_nn", "_normal", "_nrm", "_nn_gl", "_normal_gl", "_nrm_gl", "_rr", "_rough", "_roughness", "_gg", "_gloss", "_glossiness", "_mm", "_metal", "_metallic", "_metalness", "_aa", "_ao", "_orm", "_mra"];
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| {
            let s = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            s.starts_with(&stem) && SUFFIXES.iter().any(|suf| s == format!("{stem}{suf}")) || s == format!("{stem}_arm") || s == format!("{stem}_occlusion")
        })
        .collect()
}

/// A model file becomes a scenery object of its own folder in `Sceneryobjects\Imported`.
fn import_model(path: &Path, content: &Path) -> anyhow::Result<Report> {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "model".into());
    let safe: String = stem.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    let sco_root = existing_child(content, "Sceneryobjects").unwrap_or_else(|| content.join("Sceneryobjects"));
    let dir = sco_root.join("Imported").join(&safe);
    let model_dir = dir.join("model");
    let tex_dir = dir.join("texture");
    std::fs::create_dir_all(&model_dir)?;
    std::fs::create_dir_all(&tex_dir)?;
    let src_dir = path.parent().unwrap_or(Path::new("."));
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let (mesh_name, textures): (String, Vec<String>) = if ext == "obj" {
        let (x, tex) = obj_to_x(path)?;
        let name = format!("{safe}.x");
        std::fs::write(model_dir.join(&name), x)?;
        (name, tex)
    } else {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        std::fs::copy(path, model_dir.join(&name))?;
        let tex = omsi_o3d::load_mesh(path).map(|m| m.materials.iter().map(|m| m.texture.clone()).filter(|t| !t.is_empty()).collect()).unwrap_or_default();
        (name, tex)
    };
    // the textures it names, from beside the model or a texture folder next to it
    let mut copied = 1;
    let mut seen = std::collections::HashSet::new();
    for t in &textures {
        let name = Path::new(&t.replace('\\', "/")).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        if name.is_empty() || !seen.insert(name.to_lowercase()) {
            continue;
        }
        let cands = [src_dir.join(t.replace('\\', "/")), src_dir.join(&name), src_dir.join("texture").join(&name), src_dir.join("textures").join(&name), src_dir.join("Texture").join(&name)];
        if let Some(src) = cands.iter().find(|p| p.is_file()) {
            std::fs::copy(src, tex_dir.join(&name))?;
            copied += 1;
            for p in pbr_siblings(src) {
                std::fs::copy(&p, tex_dir.join(p.file_name().unwrap()))?;
                copied += 1;
            }
        }
    }
    let mut sco = String::new();
    let _ = write!(sco, "[friendlyname]\n{stem}\n\n[groups]\n1\nImported\n\n[mesh]\n{mesh_name}\n\n");
    for t in &seen {
        // (a texture with transparency gets alpha testing: leaves, fences, windows)
        let has_alpha = std::fs::read_dir(&tex_dir).ok().and_then(|rd| rd.flatten().map(|e| e.path()).find(|p| p.file_name().map(|f| f.to_string_lossy().to_lowercase() == *t).unwrap_or(false))).and_then(|p| omsi_texture::decode_file(&p).ok()).map(|img| img.has_alpha && img.rgba.chunks(4).any(|c| c[3] < 250)).unwrap_or(false);
        if has_alpha {
            let _ = write!(sco, "[matl]\n{t}\n0\n\n[matl_alpha]\n1\n\n");
        }
    }
    let sco_path = dir.join(format!("{safe}.sco"));
    std::fs::write(&sco_path, omsi_cfg::encode_text(&sco.replace('\n', "\r\n")))?;
    let rel = format!("Sceneryobjects\\Imported\\{safe}\\{safe}.sco");
    Ok(Report { copied, objects: vec![rel], message: format!("{} {stem}", tr("Model imported:", "Модель импортирована:")) })
}

/// A Wavefront `.obj` as a DirectX `.x` text mesh in OMSI's frame (x right, y up, z forward,
/// left-handed; the `.obj` is right-handed with y up), and the textures its materials use.
pub fn obj_to_x(path: &Path) -> anyhow::Result<(String, Vec<String>)> {
    let text = std::fs::read_to_string(path)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut v: Vec<[f32; 3]> = Vec::new();
    let mut vt: Vec<[f32; 2]> = Vec::new();
    let mut vn: Vec<[f32; 3]> = Vec::new();
    // materials of the .mtl: name -> (diffuse colour, texture)
    let mut mtl: HashMap<String, ([f32; 3], Option<String>)> = HashMap::new();
    let mut mat_order: Vec<String> = Vec::new();
    let mut current = String::from("default");
    // unique vertices and faces
    let mut verts: Vec<([f32; 3], [f32; 3], [f32; 2])> = Vec::new();
    let mut index: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut faces: Vec<([u32; 3], usize)> = Vec::new();
    let fix = |i: i64, n: usize| -> i64 { if i < 0 { n as i64 + i } else { i - 1 } };
    for line in text.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("v") => {
                let p: Vec<f32> = it.take(3).filter_map(|x| x.parse().ok()).collect();
                if p.len() == 3 {
                    v.push([p[0], p[1], p[2]]);
                }
            }
            Some("vt") => {
                let p: Vec<f32> = it.take(2).filter_map(|x| x.parse().ok()).collect();
                vt.push([p.first().copied().unwrap_or(0.0), p.get(1).copied().unwrap_or(0.0)]);
            }
            Some("vn") => {
                let p: Vec<f32> = it.take(3).filter_map(|x| x.parse().ok()).collect();
                if p.len() == 3 {
                    vn.push([p[0], p[1], p[2]]);
                }
            }
            Some("mtllib") => {
                let name = line.trim_start()[6..].trim();
                if let Ok(t) = std::fs::read_to_string(dir.join(name)) {
                    parse_mtl(&t, &mut mtl);
                }
            }
            Some("usemtl") => {
                current = it.collect::<Vec<_>>().join(" ");
            }
            Some("f") => {
                let mat = match mat_order.iter().position(|m| *m == current) {
                    Some(i) => i,
                    None => {
                        mat_order.push(current.clone());
                        mat_order.len() - 1
                    }
                };
                let mut poly: Vec<u32> = Vec::new();
                for c in it {
                    let mut parts = c.split('/');
                    let vi = parts.next().and_then(|x| x.parse::<i64>().ok()).map(|i| fix(i, v.len())).unwrap_or(-1);
                    let ti = parts.next().and_then(|x| x.parse::<i64>().ok()).map(|i| fix(i, vt.len())).unwrap_or(-1);
                    let ni = parts.next().and_then(|x| x.parse::<i64>().ok()).map(|i| fix(i, vn.len())).unwrap_or(-1);
                    if vi < 0 || vi as usize >= v.len() {
                        continue;
                    }
                    let id = *index.entry((vi, ti, ni)).or_insert_with(|| {
                        let p = v[vi as usize];
                        let n = if ni >= 0 { vn.get(ni as usize).copied().unwrap_or([0.0, 1.0, 0.0]) } else { [0.0, 1.0, 0.0] };
                        let t = if ti >= 0 { vt.get(ti as usize).copied().unwrap_or([0.0, 0.0]) } else { [0.0, 0.0] };
                        // right-handed y-up > OMSI: z turns round; v runs down
                        verts.push(([p[0], p[1], -p[2]], [n[0], n[1], -n[2]], [t[0], 1.0 - t[1]]));
                        (verts.len() - 1) as u32
                    });
                    poly.push(id);
                }
                for k in 1..poly.len().saturating_sub(1) {
                    // (the mirrored axis turns the winding round)
                    faces.push(([poly[0], poly[k + 1], poly[k]], mat));
                }
            }
            _ => {}
        }
    }
    if faces.is_empty() {
        anyhow::bail!("{}", tr("The .obj file has no faces", "В файле .obj нет полигонов"));
    }
    if mat_order.is_empty() {
        mat_order.push("default".into());
    }
    // flat normals where the file had none
    if vn.is_empty() {
        let mut acc = vec![glam::Vec3::ZERO; verts.len()];
        for (f, _) in &faces {
            let a = glam::Vec3::from(verts[f[0] as usize].0);
            let b = glam::Vec3::from(verts[f[1] as usize].0);
            let c = glam::Vec3::from(verts[f[2] as usize].0);
            let n = (b - a).cross(c - a);
            for i in f {
                acc[*i as usize] -= n;
            }
        }
        for (v, n) in verts.iter_mut().zip(acc) {
            v.1 = n.normalize_or_zero().to_array();
        }
    }
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
    let _ = writeln!(x, " MeshMaterialList {{\n  {};\n  {};", mat_order.len(), faces.len());
    for (i, (_, m)) in faces.iter().enumerate() {
        let _ = writeln!(x, "  {}{}", m, if i + 1 == faces.len() { ";" } else { "," });
    }
    let mut textures = Vec::new();
    for m in &mat_order {
        let (kd, tex) = mtl.get(m).cloned().unwrap_or(([0.8, 0.8, 0.8], None));
        let _ = writeln!(x, "  Material {{\n   {:.4};{:.4};{:.4};1.0;;\n   10.0;\n   0.1;0.1;0.1;;\n   0.0;0.0;0.0;;", kd[0], kd[1], kd[2]);
        if let Some(t) = tex {
            let name = Path::new(&t.replace('\\', "/")).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or(t.clone());
            let _ = writeln!(x, "   TextureFilename {{\n    \"{name}\";\n   }}");
            textures.push(t);
        }
        x.push_str("  }\n");
    }
    x.push_str(" }\n}\n");
    Ok((x, textures))
}

fn parse_mtl(text: &str, out: &mut HashMap<String, ([f32; 3], Option<String>)>) {
    let mut cur: Option<String> = None;
    for line in text.lines() {
        let l = line.trim();
        let mut it = l.split_whitespace();
        match it.next() {
            Some("newmtl") => {
                let n = it.collect::<Vec<_>>().join(" ");
                out.insert(n.clone(), ([0.8; 3], None));
                cur = Some(n);
            }
            Some("Kd") => {
                let p: Vec<f32> = it.take(3).filter_map(|x| x.parse().ok()).collect();
                if let (Some(c), 3) = (&cur, p.len()) {
                    if let Some(e) = out.get_mut(c) {
                        e.0 = [p[0], p[1], p[2]];
                    }
                }
            }
            Some("map_Kd") => {
                // (options like -bm come before the file name, which is last)
                let name = l.split_whitespace().last().unwrap_or("").to_string();
                if let Some(c) = &cur {
                    if let Some(e) = out.get_mut(c) {
                        e.1 = Some(name);
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_obj_becomes_an_x_openomsi_reads() {
        let dir = std::env::temp_dir().join(format!("om-obj-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("box.mtl"), "newmtl red\nKd 1 0 0\nmap_Kd wall.png\n").unwrap();
        std::fs::write(dir.join("box.obj"), "mtllib box.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nusemtl red\nf 1/1 2/2 3/3 4/4\n").unwrap();
        let (x, tex) = super::obj_to_x(&dir.join("box.obj")).unwrap();
        assert_eq!(tex, vec!["wall.png".to_string()]);
        std::fs::write(dir.join("box.x"), &x).unwrap();
        let m = omsi_o3d::load_mesh(&dir.join("box.x")).unwrap();
        assert_eq!(m.vertices.len(), 4);
        assert_eq!(m.triangles.len(), 2);
        assert_eq!(m.materials[0].texture, "wall.png");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
