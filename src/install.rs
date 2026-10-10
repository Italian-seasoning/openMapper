//! Where OMSI 2 and openOMSI live on this computer.
//!
//! openMapper reads the original installation (it ships no content of its own) and writes
//! maps and imported assets into openOMSI's content folder, which the game reads before the
//! installation. The original folder is never written to.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// openOMSI's data folder (`~/.openomsi`): the launcher's settings and, by default, the
/// content folder.
pub fn openomsi_data_dir() -> PathBuf {
    home().join(".openomsi")
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The bits of openOMSI's `launcher.json` the editor uses.
#[derive(Debug, Default, Deserialize)]
struct LauncherCfg {
    #[serde(default)]
    root: String,
    #[serde(default)]
    game: String,
}

fn launcher_cfg() -> LauncherCfg {
    std::fs::read_to_string(openomsi_data_dir().join("launcher.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// The editor's own settings (`~/.openomsi/openmapper.json`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Prefs {
    /// The OMSI 2 folder chosen by hand (empty: found by itself).
    #[serde(default)]
    pub omsi_root: String,
    /// "en" or "ru" (empty: the system's language).
    #[serde(default)]
    pub language: String,
    /// Maps opened lately, newest first (global.cfg paths).
    #[serde(default)]
    pub recent: Vec<String>,
    /// Show the welcome hints.
    #[serde(default)]
    pub hints_seen: bool,
}

impl Prefs {
    fn path() -> PathBuf {
        openomsi_data_dir().join("openmapper.json")
    }

    pub fn load() -> Prefs {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(openomsi_data_dir());
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Self::path(), s);
        }
    }

    pub fn push_recent(&mut self, cfg: &Path) {
        let s = cfg.to_string_lossy().to_string();
        self.recent.retain(|r| r != &s);
        self.recent.insert(0, s);
        self.recent.truncate(8);
        self.save();
    }
}

/// A complete OMSI 2 installation.
pub fn is_omsi_root(p: &Path) -> bool {
    p.is_dir() && omsi_cfg::missing_original_essentials(p).is_empty()
}

/// The OMSI 2 folder: the one chosen in openMapper, the one openOMSI's launcher uses, or
/// one found in the usual places (Steam libraries, next to the program).
pub fn find_omsi_root(prefs: &Prefs) -> Option<PathBuf> {
    let mut first: Vec<PathBuf> = Vec::new();
    if let Some(r) = std::env::var_os("OMSI_ROOT") {
        first.push(PathBuf::from(r));
    }
    if !prefs.omsi_root.is_empty() {
        first.push(PathBuf::from(&prefs.omsi_root));
    }
    let l = launcher_cfg();
    if !l.root.is_empty() {
        first.push(PathBuf::from(&l.root));
    }
    for c in &first {
        for g in omsi_cfg::install_search::root_guesses(c) {
            if is_omsi_root(&g) {
                return Some(g);
            }
        }
    }
    omsi_cfg::find_original_install(&first).filter(|p| is_omsi_root(p))
}

/// openOMSI's content folder: `$OMSI_CONTENT`, the `content` folder beside the game binary
/// openOMSI's launcher starts (when it can be written), else `~/.openomsi/content`.
pub fn content_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("OMSI_CONTENT") {
        return PathBuf::from(d);
    }
    let l = launcher_cfg();
    if !l.game.is_empty() {
        if let Some(dir) = Path::new(&l.game).parent() {
            let beside = if dir.ends_with("Contents/MacOS") {
                dir.parent().and_then(|p| p.parent()).and_then(|p| p.parent()).map(Path::to_path_buf)
            } else {
                Some(dir.to_path_buf())
            };
            if let Some(b) = beside {
                let cand = omsi_cfg::content_folder_of(&b);
                if cand.is_dir() && !omsi_cfg::is_programs_folder(&cand) && omsi_cfg::is_writable(&cand) {
                    return cand;
                }
            }
        }
    }
    openomsi_data_dir().join("content")
}

/// The openOMSI game to test maps with: of every copy found (beside openMapper, the one
/// openOMSI's launcher names and the program beside it, the macOS app), the newest build -
/// an old app left in /Applications must not win over the current one.
pub fn find_game() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) { &["openomsi.exe", "omsi.exe"] } else { &["openomsi", "omsi"] };
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.to_path_buf());
        }
    }
    let l = launcher_cfg();
    let mut cands: Vec<PathBuf> = Vec::new();
    if !l.game.is_empty() {
        cands.push(PathBuf::from(&l.game));
        if let Some(dir) = Path::new(&l.game).parent() {
            dirs.push(dir.to_path_buf());
        }
    }
    for d in &dirs {
        for n in names {
            cands.push(d.join(n));
        }
    }
    #[cfg(target_os = "macos")]
    {
        cands.push(PathBuf::from("/Applications/openOMSI.app/Contents/MacOS/openomsi"));
        cands.push(home().join("Applications/openOMSI.app/Contents/MacOS/openomsi"));
    }
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = cands
        .into_iter()
        .filter(|p| p.is_file() && p.file_stem().map(|s| s.to_string_lossy() != "openmapper").unwrap_or(false))
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    let game = found.into_iter().next().map(|f| f.1);
    if let Some(g) = &game {
        log::info!("openOMSI for test drives: {}", g.display());
    }
    game
}

/// Both content roots in the order the game searches them: the content folder (mods and the
/// maps made here), then the installation.
pub struct Roots {
    pub omsi: PathBuf,
    pub content: PathBuf,
}

impl Roots {
    pub fn new(omsi: PathBuf) -> Roots {
        let content = content_dir();
        let _ = omsi_cfg::ensure_content_layout(&content);
        omsi_cfg::add_content_root(content.clone());
        omsi_cfg::add_content_root(omsi.clone());
        Roots { omsi, content }
    }

    /// Content roots, the content folder first.
    pub fn all(&self) -> [&Path; 2] {
        [&self.content, &self.omsi]
    }

    /// A path relative to whichever root has it (the content folder first).
    pub fn resolve(&self, rel: &str) -> PathBuf {
        let c = omsi_cfg::resolve_path(&self.content, rel);
        if omsi_cfg::vfs::exists(&c) {
            return c;
        }
        omsi_cfg::resolve_path(&self.omsi, rel)
    }
}

/// A map found on this computer.
#[derive(Debug, Clone)]
pub struct MapEntry {
    pub name: String,
    pub cfg: PathBuf,
    /// Made or copied by openMapper / installed as a mod (in the content folder).
    pub in_content: bool,
}

/// Every map of both roots (a map in the content folder hides the installation's copy).
pub fn list_maps(roots: &Roots) -> Vec<MapEntry> {
    let mut out: Vec<MapEntry> = Vec::new();
    for (i, root) in roots.all().iter().enumerate() {
        let dir = root.join("maps");
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let cfg = p.join("global.cfg");
            if !cfg.is_file() {
                continue;
            }
            let folder = e.file_name().to_string_lossy().to_string();
            if out.iter().any(|m| m.cfg.parent().and_then(|d| d.file_name()).map(|f| f.to_string_lossy().eq_ignore_ascii_case(&folder)).unwrap_or(false)) {
                continue;
            }
            let name = friendly_name(&cfg).unwrap_or_else(|| folder.clone());
            out.push(MapEntry { name, cfg, in_content: i == 0 });
        }
    }
    out.sort_by_key(|m| m.name.to_lowercase());
    out
}

fn friendly_name(cfg: &Path) -> Option<String> {
    let f = omsi_cfg::CfgFile::read(cfg).ok()?;
    let mut r = f.reader();
    let mut name = None;
    while let Some(k) = r.next_keyword() {
        match k.as_str() {
            "friendlyname" => return Some(r.str().trim().to_string()).filter(|s| !s.is_empty()),
            "name" => name = Some(r.str().trim().to_string()).filter(|s| !s.is_empty()),
            _ => {}
        }
    }
    name
}
