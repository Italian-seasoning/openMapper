//! Bringing new things in, by dropping them on the window or with the Import button:
//!
//! * a mod (a folder or a `.zip` laid out like OMSI 2: `Sceneryobjects`, `Splines`,
//!   `Texture`, ...) is merged into openOMSI's content folder;
//! * a folder of `.sco` or `.sli` files becomes a folder of `Sceneryobjects` / `Splines`;
//! * a 3D model (`.obj`, `.gltf` / `.glb`, `.fbx`, `.x`, `.o3d`) becomes a new scenery object: the model goes into
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
            "obj" | "x" | "o3d" | "gltf" | "glb" | "fbx" => import_model(path, content),
            "sco" | "sli" => {
                let dir = path.parent().ok_or_else(|| anyhow::anyhow!("no folder"))?;
                import_folder(dir, content)
            }
            _ => anyhow::bail!("{}", tr("openMapper cannot import this kind of file (use a folder, .zip, .obj, .gltf, .glb, .fbx, .x or .o3d)", "Этот тип файла не импортируется (подходят папка, .zip, .obj, .gltf, .glb, .fbx, .x, .o3d)")),
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

/// One mesh file of an imported object and what its materials are: the texture file name
/// (if any) and how it treats transparency, in the order of the mesh's material slots.
struct MeshOut {
    file: String,
    slots: Vec<(Option<String>, u8)>,
}

/// A model file becomes a scenery object of its own folder in `Sceneryobjects\Imported`:
/// one mesh for each piece the file has, with the transparency of each material.
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
    let mut meshes: Vec<MeshOut> = Vec::new();
    // the textures to copy from beside the model (not already written as pngs)
    let mut textures: Vec<String> = Vec::new();
    let mut copied = 1;
    if matches!(ext.as_str(), "obj" | "gltf" | "glb" | "fbx") {
        let (mut m, mut tex_names) = if ext == "obj" {
            obj_to_model(path)?
        } else {
            let m = if ext == "fbx" { crate::models::load_fbx(path)? } else { crate::models::load_gltf(path)? };
            // (its textures, read out of the file or from beside it, go in as pngs)
            let names = crate::models::save_textures(&m, &safe, &tex_dir)?;
            (m, names)
        };
        if ext == "obj" {
            // (a texture in a format OMSI does not read goes in as a png; the others are copied
            // whole, with their maps)
            for t in tex_names.iter_mut().flatten() {
                let name = Path::new(&t.replace('\\', "/")).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                let e = Path::new(&name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                let src = [src_dir.join(t.replace('\\', "/")), src_dir.join(&name)].into_iter().find(|p| p.is_file());
                if !name.is_empty() && !omsi_texture::EXTENSIONS.contains(&e.as_str()) {
                    if let Some(img) = crate::models::picture(None, src.as_deref()) {
                        let png = format!("{}.png", Path::new(&name).file_stem().unwrap().to_string_lossy());
                        img.save(tex_dir.join(&png))?;
                        *t = png;
                        continue;
                    }
                }
                *t = name.clone();
                textures.push(name);
            }
        }
        // copy what is beside the model first, then look at the pictures for transparency
        copy_textures(&textures, src_dir, &tex_dir, &mut copied)?;
        if ext == "obj" {
            for (mat, name) in m.mats.iter_mut().zip(&tex_names) {
                if let (0, Some(n)) = (mat.alpha, name) {
                    mat.alpha = crate::models::picture(None, Some(&tex_dir.join(n))).map(|i| crate::models::alpha_of(&i)).unwrap_or(0);
                }
            }
        }
        let many = m.parts.iter().filter(|p| !p.faces.is_empty()).count() > 1;
        let mut taken = std::collections::HashSet::new();
        for (i, part) in m.parts.iter().enumerate().filter(|(_, p)| !p.faces.is_empty()) {
            let (x, used) = crate::models::write_x(part, &m.mats, &tex_names);
            let file = if many {
                let clean: String = part.name.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
                let mut f = format!("{safe}_{}", if clean.is_empty() { format!("part{i}") } else { clean });
                while !taken.insert(f.to_lowercase()) {
                    f.push('_');
                }
                format!("{f}.x")
            } else {
                format!("{safe}.x")
            };
            std::fs::write(model_dir.join(&file), x)?;
            meshes.push(MeshOut { file, slots: used.iter().map(|u| (tex_names.get(*u).cloned().flatten(), m.mats.get(*u).map(|m| m.alpha).unwrap_or(0))).collect() });
        }
        if meshes.is_empty() {
            anyhow::bail!("{}", tr("The model has no triangles", "В модели нет треугольников"));
        }
    } else {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        std::fs::copy(path, model_dir.join(&name))?;
        let tex: Vec<String> = omsi_o3d::load_mesh(path).map(|m| m.materials.iter().map(|m| m.texture.clone()).filter(|t| !t.is_empty()).collect()).unwrap_or_default();
        copy_textures(&tex, src_dir, &tex_dir, &mut copied)?;
        // (a texture with transparency gets alpha testing: leaves, fences, windows)
        let slots = tex
            .iter()
            .map(|t| {
                let file = Path::new(&t.replace('\\', "/")).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                let alpha = crate::models::picture(None, Some(&tex_dir.join(&file))).map(|i| crate::models::alpha_of(&i)).unwrap_or(0);
                (Some(file), alpha)
            })
            .collect();
        meshes.push(MeshOut { file: name, slots });
    }
    let mut sco = String::new();
    let _ = write!(sco, "[friendlyname]\n{stem}\n\n[groups]\n1\nImported\n\n");
    for m in &meshes {
        let _ = write!(sco, "[mesh]\n{}\n\n", m.file);
        let mut seen: HashMap<String, i32> = HashMap::new();
        for (tex, alpha) in &m.slots {
            let Some(t) = tex else { continue };
            // (a material is named by its texture and the how-many-th use of it)
            let n = seen.entry(t.to_lowercase()).or_insert(-1);
            *n += 1;
            if *alpha > 0 {
                let _ = write!(sco, "[matl]\n{t}\n{n}\n[matl_alpha]\n{alpha}\n\n");
            }
        }
    }
    let sco_path = dir.join(format!("{safe}.sco"));
    std::fs::write(&sco_path, omsi_cfg::encode_text(&sco.replace('\n', "\r\n")))?;
    let rel = format!("Sceneryobjects\\Imported\\{safe}\\{safe}.sco");
    Ok(Report { copied, objects: vec![rel], message: format!("{} {stem}", tr("Model imported:", "Модель импортирована:")) })
}

/// Sets how a material of an imported object treats transparency (0 opaque, 1 cut out, 2
/// blended): the `[matl]` block of its texture and use number in the `n`-th mesh of the `.sco`
/// gets its `[matl_alpha]`, or loses it.
pub fn set_alpha(sco: &Path, mesh: usize, texture: &str, index: i32, alpha: u8) -> anyhow::Result<()> {
    let text = omsi_cfg::decode_text(&std::fs::read(sco)?);
    let mut lines: Vec<String> = text.lines().map(|l| l.trim_end().to_string()).collect();
    let is = |l: &str, key: &str| l.trim().eq_ignore_ascii_case(key);
    // the n-th mesh's lines: from its [mesh] to the next one
    let starts: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| is(l, "[mesh]")).map(|(i, _)| i).collect();
    let from = *starts.get(mesh).ok_or_else(|| anyhow::anyhow!("no such mesh"))?;
    let to = starts.get(mesh + 1).copied().unwrap_or(lines.len());
    // its [matl] block: the line after [matl] names the texture, the next its use number
    let found = (from..to).find(|&i| is(&lines[i], "[matl]") && lines.get(i + 1).map(|t| t.trim().eq_ignore_ascii_case(texture)).unwrap_or(false) && lines.get(i + 2).and_then(|n| n.trim().parse::<i32>().ok()) == Some(index));
    match found {
        Some(i) => {
            // the block ends at the next [matl] or [mesh] or at the end
            let end = (i + 3..to).find(|&j| is(&lines[j], "[matl]") || is(&lines[j], "[matl_change]")).unwrap_or(to);
            let a = (i + 3..end).find(|&j| is(&lines[j], "[matl_alpha]"));
            match (a, alpha) {
                (Some(j), 0) => {
                    lines.drain(j..(j + 2).min(lines.len()));
                }
                (Some(j), v) => lines[j + 1] = v.to_string(),
                (None, 0) => {}
                (None, v) => {
                    lines.insert(i + 3, v.to_string());
                    lines.insert(i + 3, "[matl_alpha]".into());
                }
            }
        }
        None if alpha > 0 => {
            let mut at = to;
            while at > from + 1 && lines[at - 1].trim().is_empty() {
                at -= 1;
            }
            for (k, l) in ["", "[matl]", texture, &index.to_string(), "[matl_alpha]", &alpha.to_string()].iter().enumerate() {
                lines.insert(at + k, l.to_string());
            }
        }
        None => {}
    }
    std::fs::write(sco, omsi_cfg::encode_text(&(lines.join("\r\n") + "\r\n")))?;
    Ok(())
}

/// The textures a model names, from beside it or a texture folder next to it, with their
/// PBR maps, into the object's own texture folder.
fn copy_textures(names: &[String], src_dir: &Path, tex_dir: &Path, copied: &mut usize) -> anyhow::Result<()> {
    let mut seen = std::collections::HashSet::new();
    for t in names {
        let name = Path::new(&t.replace('\\', "/")).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        if name.is_empty() || !seen.insert(name.to_lowercase()) {
            continue;
        }
        let cands = [src_dir.join(t.replace('\\', "/")), src_dir.join(&name), src_dir.join("texture").join(&name), src_dir.join("textures").join(&name), src_dir.join("Texture").join(&name)];
        if let Some(src) = cands.iter().find(|p| p.is_file()) {
            std::fs::copy(src, tex_dir.join(&name))?;
            *copied += 1;
            for p in pbr_siblings(src) {
                std::fs::copy(&p, tex_dir.join(p.file_name().unwrap()))?;
                *copied += 1;
            }
        }
    }
    Ok(())
}

/// A Wavefront `.obj` in OMSI's frame (x right, y up, z forward, left-handed; the `.obj` is
/// right-handed with y up), a piece for each of its objects and groups, and each material's
/// texture as the `.mtl` names it.
pub fn obj_to_model(path: &Path) -> anyhow::Result<(crate::models::Model, Vec<Option<String>>)> {
    use crate::models::{Mat, Model, Part};
    let text = std::fs::read_to_string(path)?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut v: Vec<[f32; 3]> = Vec::new();
    let mut vt: Vec<[f32; 2]> = Vec::new();
    let mut vn: Vec<[f32; 3]> = Vec::new();
    let mut mtl: HashMap<String, MtlMat> = HashMap::new();
    let mut mat_order: Vec<String> = Vec::new();
    let mut current = String::from("default");
    let mut model = Model::default();
    // (each piece has its own vertices: the first of a name takes the name, a second of the
    // same name goes on in the first)
    let mut index: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut part: Option<usize> = None;
    let new_piece = |model: &mut Model, name: &str, index: &mut HashMap<(i64, i64, i64), u32>| -> usize {
        index.clear();
        model.parts.push(Part { name: name.to_string(), ..Default::default() });
        model.parts.len() - 1
    };
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
            Some("o") | Some("g") => {
                // (a group with no faces yet is reused for the next name)
                let name = it.collect::<Vec<_>>().join(" ");
                match part {
                    Some(i) if model.parts[i].faces.is_empty() => model.parts[i].name = name,
                    _ => part = Some(new_piece(&mut model, &name, &mut index)),
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
                let pi = match part {
                    Some(i) => i,
                    None => {
                        let i = new_piece(&mut model, "model", &mut index);
                        part = Some(i);
                        i
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
                    let verts = &mut model.parts[pi].verts;
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
                    model.parts[pi].faces.push(([poly[0], poly[k + 1], poly[k]], mat));
                }
            }
            _ => {}
        }
    }
    model.parts.retain(|p| !p.faces.is_empty());
    if model.parts.is_empty() {
        anyhow::bail!("{}", tr("The .obj file has no faces", "В файле .obj нет полигонов"));
    }
    if mat_order.is_empty() {
        mat_order.push("default".into());
    }
    // flat normals where the file had none
    if vn.is_empty() {
        for p in &mut model.parts {
            let mut acc = vec![glam::Vec3::ZERO; p.verts.len()];
            for (f, _) in &p.faces {
                let a = glam::Vec3::from(p.verts[f[0] as usize].0);
                let b = glam::Vec3::from(p.verts[f[1] as usize].0);
                let c = glam::Vec3::from(p.verts[f[2] as usize].0);
                let n = (b - a).cross(c - a);
                for i in f {
                    acc[*i as usize] -= n;
                }
            }
            for (v, n) in p.verts.iter_mut().zip(acc) {
                v.1 = n.normalize_or_zero().to_array();
            }
        }
    }
    let mut textures = Vec::new();
    for m in &mat_order {
        let e = mtl.get(m).cloned().unwrap_or(MtlMat { kd: [0.8, 0.8, 0.8], tex: None, opacity: 1.0, mask: None });
        // (a map_d picture gives the transparency of the colour picture)
        let alpha = if e.opacity < 0.98 { 2 } else { 0 };
        model.mats.push(Mat { color: [e.kd[0], e.kd[1], e.kd[2], e.opacity.clamp(0.0, 1.0)], alpha, ..Default::default() });
        textures.push(e.tex.or(e.mask));
    }
    Ok((model, textures))
}

#[derive(Clone)]
struct MtlMat {
    kd: [f32; 3],
    tex: Option<String>,
    opacity: f32,
    /// `map_d`: used for the texture where no `map_Kd` is.
    mask: Option<String>,
}

fn parse_mtl(text: &str, out: &mut HashMap<String, MtlMat>) {
    let mut cur: Option<String> = None;
    for line in text.lines() {
        let l = line.trim();
        let mut it = l.split_whitespace();
        let word = it.next();
        // (options like -bm come before the file name, which is last)
        let last = l.split_whitespace().last().unwrap_or("").to_string();
        let Some(e) = cur.as_ref().and_then(|c| out.get_mut(c)) else {
            if word == Some("newmtl") {
                let n = it.collect::<Vec<_>>().join(" ");
                out.insert(n.clone(), MtlMat { kd: [0.8; 3], tex: None, opacity: 1.0, mask: None });
                cur = Some(n);
            }
            continue;
        };
        match word {
            Some("newmtl") => {
                let n = it.collect::<Vec<_>>().join(" ");
                out.insert(n.clone(), MtlMat { kd: [0.8; 3], tex: None, opacity: 1.0, mask: None });
                cur = Some(n);
            }
            Some("Kd") => {
                let p: Vec<f32> = it.take(3).filter_map(|x| x.parse().ok()).collect();
                if p.len() == 3 {
                    e.kd = [p[0], p[1], p[2]];
                }
            }
            Some("d") => e.opacity = it.next().and_then(|x| x.parse().ok()).unwrap_or(1.0),
            Some("Tr") => e.opacity = 1.0 - it.next().and_then(|x| x.parse::<f32>().ok()).unwrap_or(0.0),
            Some("map_Kd") => e.tex = Some(last),
            Some("map_d") => e.mask = Some(last),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_obj_becomes_pieces_openomsi_reads() {
        let dir = std::env::temp_dir().join(format!("om-obj-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("box.mtl"), "newmtl red\nKd 1 0 0\nmap_Kd wall.png\n").unwrap();
        std::fs::write(dir.join("box.obj"), "mtllib box.mtl\no a\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nusemtl red\nf 1/1 2/2 3/3 4/4\no b\nf 1/1 2/2 3/3\n").unwrap();
        let (m, tex) = super::obj_to_model(&dir.join("box.obj")).unwrap();
        assert_eq!(tex, vec![Some("wall.png".to_string())]);
        assert_eq!(m.parts.len(), 2);
        assert_eq!(m.parts[0].name, "a");
        let (x, used) = crate::models::write_x(&m.parts[0], &m.mats, &tex);
        assert_eq!(used, vec![0]);
        std::fs::write(dir.join("box.x"), &x).unwrap();
        let back = omsi_o3d::load_mesh(&dir.join("box.x")).unwrap();
        assert_eq!(back.vertices.len(), 4);
        assert_eq!(back.triangles.len(), 2);
        assert_eq!(back.materials[0].texture, "wall.png");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_materials_alpha_is_set_and_taken_away_in_the_sco() {
        let dir = std::env::temp_dir().join(format!("om-alpha-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sco = dir.join("a.sco");
        std::fs::write(&sco, omsi_cfg::encode_text("[mesh]\r\nfirst.x\r\n\r\n[mesh]\r\nsecond.x\r\n\r\n[matl]\r\nleaf.png\r\n0\r\n[matl_alpha]\r\n1\r\n\r\n")).unwrap();
        let read = || omsi_cfg::decode_text(&std::fs::read(&sco).unwrap());
        super::set_alpha(&sco, 1, "leaf.png", 0, 2).unwrap();
        assert!(read().contains("[matl_alpha]\r\n2"), "{}", read());
        super::set_alpha(&sco, 1, "leaf.png", 0, 0).unwrap();
        assert!(!read().contains("matl_alpha"), "{}", read());
        super::set_alpha(&sco, 0, "glass.png", 0, 2).unwrap();
        let t = read();
        assert!(t.find("glass.png").unwrap() < t.find("second.x").unwrap() && t.contains("[matl_alpha]\r\n2"), "{t}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A glb-like model with two nodes, one of them a see-through leaf texture: two meshes, and
    /// the leaf's `[matl_alpha]` in the `.sco`.
    #[test]
    fn a_model_keeps_its_pieces_and_the_alpha_of_its_materials() {
        let dir = std::env::temp_dir().join(format!("om-pieces-{}", std::process::id()));
        let content = dir.join("content");
        std::fs::create_dir_all(&content).unwrap();
        let mut leaf = image::RgbaImage::from_pixel(4, 4, image::Rgba([20, 200, 20, 255]));
        leaf.put_pixel(0, 0, image::Rgba([0, 0, 0, 0]));
        leaf.save(dir.join("leaf.png")).unwrap();
        std::fs::write(dir.join("two.mtl"), "newmtl solid\nKd 1 1 1\nnewmtl leaf\nKd 1 1 1\nmap_Kd leaf.png\n").unwrap();
        std::fs::write(dir.join("two.obj"), "mtllib two.mtl\no wall\nv 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nusemtl solid\nf 1/1 2/2 3/3\no leaves\nusemtl leaf\nf 1/1 2/2 3/3\n").unwrap();
        super::import(&dir.join("two.obj"), &content).unwrap();
        let folder = content.join("Sceneryobjects").join("Imported").join("two");
        let sco = String::from_utf8_lossy(&std::fs::read(folder.join("two.sco")).unwrap()).replace('\0', "");
        assert_eq!(sco.matches("[mesh]").count(), 2, "{sco}");
        assert!(sco.contains("[matl]\r\nleaf.png\r\n0\r\n[matl_alpha]\r\n1"), "{sco}");
        assert!(folder.join("model").join("two_wall.x").is_file() && folder.join("model").join("two_leaves.x").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
