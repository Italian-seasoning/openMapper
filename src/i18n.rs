//! Two languages, written side by side where the text is used: `tr("Save", "Сохранить")`.

use std::sync::atomic::{AtomicU8, Ordering};

static LANG: AtomicU8 = AtomicU8::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ru,
}

pub fn set(lang: Lang) {
    LANG.store(lang as u8, Ordering::Relaxed);
}

pub fn get() -> Lang {
    if LANG.load(Ordering::Relaxed) == 1 { Lang::Ru } else { Lang::En }
}

pub fn code(l: Lang) -> &'static str {
    match l {
        Lang::En => "en",
        Lang::Ru => "ru",
    }
}

/// The language a settings value names, else the system's.
pub fn from_code(c: &str) -> Lang {
    match c {
        "ru" => Lang::Ru,
        "en" => Lang::En,
        _ => {
            let sys = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|v| std::env::var(v).ok()).find(|v| !v.is_empty()).unwrap_or_default();
            let mac = if sys.is_empty() && cfg!(target_os = "macos") {
                std::process::Command::new("defaults").args(["read", "-g", "AppleLanguages"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default()
            } else {
                String::new()
            };
            let s = format!("{sys}{mac}").to_lowercase();
            let first = s.split(|c: char| !c.is_ascii_alphabetic()).find(|w| w.len() == 2).unwrap_or("");
            if first == "ru" || first == "uk" || first == "be" { Lang::Ru } else { Lang::En }
        }
    }
}

/// The text in the current language.
pub fn tr(en: &'static str, ru: &'static str) -> &'static str {
    match get() {
        Lang::En => en,
        Lang::Ru => ru,
    }
}
