//! Original byte identities retained across imports and conversion.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Mutex;

const INDEX: &str = ".openmapper-provenance.json";
const LIMIT: u64 = 4 * 1024 * 1024;
static WRITER: Mutex<()> = Mutex::new(());

pub fn file_hash(path: &Path) -> io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct History {
    pub format_version: u32,
    /// Output byte digest -> original input byte digests. No machine paths are recorded.
    pub entries: BTreeMap<String, BTreeSet<String>>,
}

impl History {
    pub fn load(content: &Path) -> io::Result<Self> {
        let path = content.join(INDEX);
        if !path.exists() {
            return Ok(Self {
                format_version: 1,
                entries: BTreeMap::new(),
            });
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > LIMIT {
            return Err(io::Error::other("provenance index exceeds its size limit"));
        }
        let history: Self = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if history.format_version != 1 || history.entries.len() > 32768 {
            return Err(io::Error::other(
                "unsupported or oversized provenance index",
            ));
        }
        Ok(history)
    }
}

pub struct Source {
    hashes: BTreeSet<String>,
}

impl Source {
    /// This runs before any decoder or converter reads the input.
    pub fn capture(path: &Path) -> io::Result<Self> {
        if omsi_cfg::vfs::is_sealed(path) {
            return Err(io::Error::other(
                "protected playback assets cannot be imported for editing",
            ));
        }
        Ok(Self {
            hashes: BTreeSet::from([file_hash(path)?]),
        })
    }

    pub fn include(&mut self, path: &Path) -> io::Result<()> {
        if omsi_cfg::vfs::is_sealed(path) {
            return Err(io::Error::other(
                "protected playback assets cannot be converted",
            ));
        }
        self.hashes.insert(file_hash(path)?);
        Ok(())
    }

    pub fn record(&self, content: &Path, output: &Path) -> io::Result<()> {
        let _writer = WRITER
            .lock()
            .map_err(|_| io::Error::other("provenance index is busy"))?;
        let mut history = History::load(content)?;
        let mut stack = vec![output.to_path_buf()];
        while let Some(path) = stack.pop() {
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                return Err(io::Error::other(
                    "converted output contains a symbolic link",
                ));
            }
            if metadata.is_dir() {
                for entry in std::fs::read_dir(path)? {
                    stack.push(entry?.path());
                }
            } else if metadata.is_file() {
                history
                    .entries
                    .entry(file_hash(&path)?)
                    .or_default()
                    .extend(self.hashes.clone());
                if history.entries.len() > 32768 {
                    return Err(io::Error::other("provenance index has too many entries"));
                }
            }
        }
        let bytes = serde_json::to_vec(&history).map_err(io::Error::other)?;
        if bytes.len() as u64 > LIMIT {
            return Err(io::Error::other("provenance index exceeds its size limit"));
        }
        let target = content.join(INDEX);
        let temporary = content.join(format!("{INDEX}.{}.new", std::process::id()));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&temporary, &target)
        })();
        if temporary.exists() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    }
}
