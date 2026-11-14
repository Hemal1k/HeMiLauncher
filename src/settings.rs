//! What HeMi Launcher remembers (%AppData%\HeMiLauncher\settings.ini), the start with Windows (the Run value of the user)
//! and the place the other HeMi apps find it by (HKCU\Software\HeMi\HeMiLauncher, value Path).

use crate::reg;
use std::path::PathBuf;

#[derive(Clone, PartialEq, Debug)]
pub struct Settings {
    /// Look for new versions after the start and every few hours.
    pub auto_check: bool,
    /// Install them by itself (in the background, quietly).
    pub auto_install: bool,
    /// A notification of Windows when there are new versions.
    pub notify: bool,
    /// The × of the window hides it into the notification area (the launcher keeps watching).
    pub tray_on_close: bool,
    /// Where the portable versions go (empty: the launcher does not manage portable versions).
    pub portable_dir: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { auto_check: true, auto_install: false, notify: true, tray_on_close: true, portable_dir: String::new() }
    }
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default().join("HeMi").join("HeMiLauncher")
}

fn file() -> PathBuf {
    data_dir().join("settings.ini")
}

impl Settings {
    /// The folder of the portable versions, when one is chosen.
    pub fn portable(&self) -> Option<PathBuf> {
        (!self.portable_dir.trim().is_empty()).then(|| PathBuf::from(self.portable_dir.trim()))
    }

    pub fn load() -> Settings {
        let mut s = Settings::default();
        let Ok(text) = std::fs::read_to_string(file()) else { return s };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let b = v.trim() == "1";
            match k.trim() {
                "AutoCheck" => s.auto_check = b,
                "AutoInstall" => s.auto_install = b,
                "Notify" => s.notify = b,
                "TrayOnClose" => s.tray_on_close = b,
                "PortableDir" => s.portable_dir = v.trim().to_string(),
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let b = |v: bool| if v { "1" } else { "0" };
        let text = format!(
            "AutoCheck={}\nAutoInstall={}\nNotify={}\nTrayOnClose={}\nPortableDir={}\n",
            b(self.auto_check),
            b(self.auto_install),
            b(self.notify),
            b(self.tray_on_close),
            self.portable_dir
        );
        let _ = std::fs::create_dir_all(data_dir());
        let _ = std::fs::write(file(), text);
    }
}

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "HeMiLauncher";
/// Started with Windows: hidden in the notification area, looking for updates.
pub const TRAY: &str = "--tray";

pub fn autostart() -> bool {
    reg::read(false, RUN, VALUE).is_some()
}

pub fn set_autostart(on: bool) {
    match (on, std::env::current_exe()) {
        (true, Ok(exe)) => {
            reg::write_user(RUN, VALUE, &format!("\"{}\" {TRAY}", exe.display()));
        }
        _ => reg::delete_user(RUN, VALUE),
    }
}

/// Moved (another folder, an update of the portable one): the start with Windows points to this exe again.
pub fn repair_autostart() {
    if autostart() {
        set_autostart(true);
    }
}

/// Where the other HeMi apps find the launcher: they leave their updates to it then.
pub fn register() {
    if let Ok(exe) = std::env::current_exe() {
        reg::write_user(r"Software\HeMi\HeMiLauncher", "Path", &exe.display().to_string());
    }
}
