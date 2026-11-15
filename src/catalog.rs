//! The HeMi apps the launcher knows: their GitHub projects, icons and how they are installed – and what of them is on
//! this PC (the uninstall entries the installers write, for the user or for all users).

use crate::reg;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// An app with an installer ("<Id>-Setup.exe", uninstall entry "<Id>_is1").
    App,
    /// Files without an installer (HeMi Dark: skins for VLC) – downloaded into the Downloads folder.
    Files,
}

#[derive(Clone, Copy, Debug)]
pub struct AppInfo {
    /// The name of the GitHub project, of the installer and of the uninstall entry.
    pub id: &'static str,
    pub name: &'static str,
    pub exe: &'static str,
    pub kind: Kind,
    /// What it is, in a few words (English, Czech).
    pub line: (&'static str, &'static str),
    pub icon: &'static [u8],
}

impl AppInfo {
    pub fn line(&self) -> &'static str {
        crate::i18n::tr(self.line.0, self.line.1)
    }
}

pub const APPS: &[AppInfo] = &[
    AppInfo {
        id: "HeMiDock",
        name: "HeMiDock",
        exe: "HeMiDock.exe",
        kind: Kind::App,
        line: ("A dock, Start menu, control center and window manager", "Dock, nabídka Start, ovládací centrum a správce oken"),
        icon: include_bytes!("../assets/apps/hemidock.rgba"),
    },
    AppInfo {
        id: "HeMiPix",
        name: "HeMiPix",
        exe: "HeMiPix.exe",
        kind: Kind::App,
        line: ("Converts photo formats and makes photos smaller", "Převod formátů fotek a zmenšení jejich velikosti"),
        icon: include_bytes!("../assets/apps/hemipix.rgba"),
    },
    AppInfo {
        id: "HeMiUnlocker",
        name: "HeMiUnlocker",
        exe: "HeMiUnlocker.exe",
        kind: Kind::App,
        line: ("Finds the program that holds a file – closes it, unlocks the file or takes ownership", "Zjistí, který program drží soubor – zavře ho, soubor odemkne nebo převezme vlastnictví"),
        icon: include_bytes!("../assets/apps/hemiunlocker.rgba"),
    },
    AppInfo {
        id: "HeMiPad",
        name: "HeMiPad",
        exe: "HeMiPad.exe",
        kind: Kind::App,
        line: ("A simple notepad", "Jednoduchý poznámkový blok"),
        icon: include_bytes!("../assets/apps/hemipad.rgba"),
    },
    AppInfo {
        id: "HeMiDark",
        name: "HeMi Dark",
        exe: "",
        kind: Kind::Files,
        line: ("A modern dark skin for VLC with support for high DPI", "Moderní tmavý vzhled pro VLC s podporou vyššího DPI"),
        icon: include_bytes!("../assets/apps/hemidark.rgba"),
    },
];

/// The launcher itself (updates itself the same way).
pub const LAUNCHER: AppInfo = AppInfo {
    id: "HeMiLauncher",
    name: "HeMi Launcher",
    exe: "HeMiLauncher.exe",
    kind: Kind::App,
    line: ("Installs and updates the HeMi apps", "Instaluje a aktualizuje aplikace HeMi"),
    icon: include_bytes!("../assets/apps/hemilauncher.rgba"),
};

/// An app on this PC, as its installer left it.
#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub version: String,
    pub folder: PathBuf,
    /// Installed for all users (Program Files): its update asks for administrator rights.
    pub all_users: bool,
    pub uninstall: String,
}

impl Installed {
    pub fn exe(&self, app: &AppInfo) -> PathBuf {
        self.folder.join(app.exe)
    }
}

/// What the uninstall entry of the app says (for the user first, then for all users).
pub fn installed(app: &AppInfo) -> Option<Installed> {
    if app.kind != Kind::App {
        return None;
    }
    let key = format!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{}_is1", app.id);
    for all_users in [false, true] {
        let Some(version) = reg::read(all_users, &key, "DisplayVersion") else { continue };
        let folder = PathBuf::from(reg::read(all_users, &key, "InstallLocation").unwrap_or_default().trim_end_matches('\\'));
        if !folder.join(app.exe).exists() {
            continue;
        }
        let uninstall = reg::read(all_users, &key, "UninstallString").unwrap_or_default();
        return Some(Installed { version, folder, all_users, uninstall });
    }
    None
}

/// "1.6.1" as numbers, for comparing versions.
pub fn version_of(text: &str) -> Vec<u32> {
    text.trim().trim_start_matches(['v', 'V']).split('.').map(|p| p.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)).collect()
}

pub fn newer(latest: &str, installed: &str) -> bool {
    version_of(latest) > version_of(installed)
}

/// The file the launcher leaves in a portable folder: the version it unpacked there.
pub const PORTABLE_VERSION: &str = "hemilauncher-version.txt";

/// A portable version in the launcher's folder (`dir\<Id>`).
#[derive(Clone, Debug, PartialEq)]
pub struct Portable {
    pub version: String,
    pub folder: PathBuf,
}

impl Portable {
    pub fn exe(&self, app: &AppInfo) -> PathBuf {
        self.folder.join(app.exe)
    }
}

pub fn portable(app: &AppInfo, dir: &std::path::Path) -> Option<Portable> {
    if app.kind != Kind::App {
        return None;
    }
    let folder = dir.join(app.id);
    let version = std::fs::read_to_string(folder.join(PORTABLE_VERSION)).ok()?.trim().to_string();
    folder.join(app.exe).exists().then_some(Portable { version, folder })
}
