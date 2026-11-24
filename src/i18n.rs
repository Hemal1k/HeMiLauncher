//! Languages: English by default, Czech when the display language of Windows is Czech.

use std::sync::OnceLock;
use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;

/// The language chosen in the settings: 0 = as Windows, 1 = English, 2 = Czech.
static CHOICE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn set(choice: u8) {
    CHOICE.store(choice.min(2), std::sync::atomic::Ordering::Relaxed);
}

/// True when the app speaks Czech: chosen in the settings, else as the display language of Windows (primary language 0x05).
pub fn cz() -> bool {
    match CHOICE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => return false,
        2 => return true,
        _ => {}
    }
    static CZ: OnceLock<bool> = OnceLock::new();
    *CZ.get_or_init(|| {
        // Debug builds only: HEMILAUNCHER_LANG=en / cs to test the other language on this PC
        if cfg!(debug_assertions) {
            match std::env::var("HEMILAUNCHER_LANG").as_deref() {
                Ok("en") => return false,
                Ok("cs") => return true,
                _ => {}
            }
        }
        (unsafe { GetUserDefaultUILanguage() } & 0x3FF) == 0x05
    })
}

/// The text in the language of the app.
pub fn tr<'a>(en: &'a str, cs: &'a str) -> &'a str {
    if cz() { cs } else { en }
}


/// The form of a word for a count, by the Czech rule: 1 → `one`, 2–4 → `few`, else `many` (English: `few` = `many`).
pub fn plural<'a>(n: usize, one: &'a str, few: &'a str, many: &'a str) -> &'a str {
    match n {
        1 => one,
        2..=4 => few,
        _ => many,
    }
}

/// A share in %: "42 %" in Czech, "42%" in English.
pub fn percent(n: impl std::fmt::Display) -> String {
    if cz() { format!("{n} %") } else { format!("{n}%") }
}

/// A date from GitHub ("2026-11-23"): as it is in English, "23. 11. 2026" in Czech.
pub fn date(iso: &str) -> String {
    let parts: Vec<u32> = iso.split('-').filter_map(|p| p.parse().ok()).collect();
    match parts[..] {
        [y, m, d] if cz() => format!("{d}. {m}. {y}"),
        _ => iso.to_string(),
    }
}
