//! The HeMi apps the launcher knows: their GitHub projects, icons and how they are installed – and what of them is on
//! this PC (the uninstall entries the installers write, for the user or for all users).

use crate::reg;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// An app with an installer ("<Id>-Setup.exe", uninstall entry "<Id>_is1").
    App,
    /// Files without an installer (HeMi Dark: skins for VLC) – unpacked into the skins folder of VLC and set there
    /// (vlc.rs); the colour and the size are picked in its card.
    Files,
    /// One program without an installer, run once – "<Id>.exe" (on ARM "<Id>-arm64.exe" when the release has it) into
    /// the Downloads folder. No app is one any more: only HeMiMizer before its installer (a release without
    /// "HeMiMizer-Setup.exe", see `AppInfo::kind_for`).
    Tool,
    /// A browser extension (HeMiTube): "<Id>.zip" unpacked into %AppData%\HeMi\<Id> (with
    /// the version file), loaded once in the browser as an unpacked extension; an update unpacks the new one in place.
    Extension,
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
    /// The program released as plain "<Id>.exe" for x64 only (HeMiMizer): that is its portable version (no
    /// "-Portable", no "-arm64" – Windows on ARM runs the x64 one), and a release without "<Id>-Setup.exe" (the ones
    /// before its installer) has only this exe – the card then just downloads it (`Kind::Tool`).
    pub plain_exe: bool,
}

impl AppInfo {
    pub fn line(&self) -> &'static str {
        crate::i18n::tr(self.line.0, self.line.1)
    }

    /// How the app is installed with this release: an app with a plain exe whose release has no installer yet is only
    /// downloaded (`Kind::Tool`); not known yet (no release read) – as its kind says.
    pub fn kind_for(&self, release: Option<&crate::github::Release>) -> Kind {
        match release {
            Some(r) if self.plain_exe && r.asset(&format!("{}-Setup.exe", self.id)).is_none() => Kind::Tool,
            _ => self.kind,
        }
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
        plain_exe: false,
    },
    AppInfo {
        id: "HeMiPix",
        name: "HeMiPix",
        exe: "HeMiPix.exe",
        kind: Kind::App,
        line: ("Converts photo formats and makes photos smaller", "Převod formátů fotek a zmenšení jejich velikosti"),
        icon: include_bytes!("../assets/apps/hemipix.rgba"),
        plain_exe: false,
    },
    AppInfo {
        id: "HeMiUnlocker",
        name: "HeMiUnlocker",
        exe: "HeMiUnlocker.exe",
        kind: Kind::App,
        line: ("Finds the program that holds a file – closes it, unlocks the file or takes ownership", "Zjistí, který program drží soubor – zavře ho, soubor odemkne nebo převezme vlastnictví"),
        icon: include_bytes!("../assets/apps/hemiunlocker.rgba"),
        plain_exe: false,
    },
    AppInfo {
        id: "HeMiPad",
        name: "HeMiPad",
        exe: "HeMiPad.exe",
        kind: Kind::App,
        line: ("A simple notepad", "Jednoduchý poznámkový blok"),
        icon: include_bytes!("../assets/apps/hemipad.rgba"),
        plain_exe: false,
    },
    AppInfo {
        id: "HeMiTube",
        name: "HeMiTube",
        // what makes the folder an extension (checked like the exe of a portable app)
        exe: "manifest.json",
        kind: Kind::Extension,
        line: ("A calmer YouTube: theater mode, dislikes, quality, codecs, no Shorts", "Klidnější YouTube: režim kina, dislajky, kvalita, kodeky, bez Shorts"),
        icon: include_bytes!("../assets/apps/hemitube.rgba"),
        plain_exe: false,
    },
    AppInfo {
        id: "HeMiDark",
        name: "HeMi Dark",
        exe: "",
        kind: Kind::Files,
        line: ("A modern dark skin for VLC with support for high DPI", "Moderní tmavý vzhled pro VLC s podporou vyššího DPI"),
        icon: include_bytes!("../assets/apps/hemidark.rgba"),
        plain_exe: false,
    },
    AppInfo {
        id: "HeMiMizer",
        name: "HeMiMizer",
        exe: "HeMiMizer.exe",
        // an installer ("HeMiMizer-Setup.exe", entry "HeMiMizer_is1") and the portable "HeMiMizer.exe", x64 only; the
        // releases before the installer only downloaded (kind_for)
        kind: Kind::App,
        line: ("Gets a fresh Windows ready: drivers, programs, settings – only what you tick", "Připraví čerstvé Windows: ovladače, programy, nastavení – jen to, co zaškrtneš"),
        icon: include_bytes!("../assets/apps/hemimizer.rgba"),
        plain_exe: true,
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
    plain_exe: false,
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

/// `latest` is a newer version than `installed` ("1.4" and "1.4.0" are the same).
pub fn newer(latest: &str, installed: &str) -> bool {
    let (mut a, mut b) = (version_of(latest), version_of(installed));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn compares_versions() {
        assert!(!newer("1.4.0", "1.4"));
        assert!(!newer("1.4", "1.4.0"));
        assert!(newer("1.4.1", "1.4"));
        assert!(newer("1.10.0", "1.9.9"));
        assert!(newer("v2.0", "1.99.99"));
        assert!(!newer("1.4.3", "1.4.4"));
    }

    #[test]
    fn hemimizer_without_its_installer_is_only_downloaded() {
        use super::{APPS, Kind};
        let release = |names: &[&str]| crate::github::Release {
            version: "1.0.0".into(),
            notes: String::new(),
            date: String::new(),
            page: String::new(),
            assets: names.iter().map(|n| (n.to_string(), format!("https://github.com/Hemal1k/x/{n}"))).collect(),
        };
        let mizer = APPS.iter().find(|a| a.id == "HeMiMizer").unwrap();
        assert_eq!(mizer.kind_for(Some(&release(&["HeMiMizer.exe"]))), Kind::Tool);
        assert_eq!(mizer.kind_for(Some(&release(&["HeMiMizer-Setup.exe", "HeMiMizer.exe"]))), Kind::App);
        assert_eq!(mizer.kind_for(None), Kind::App);
        let pad = APPS.iter().find(|a| a.id == "HeMiPad").unwrap();
        assert_eq!(pad.kind_for(Some(&release(&["HeMiPad-Portable.exe"]))), Kind::App);
    }
}

/// The file the launcher leaves in a portable folder: the version it unpacked there.
pub const PORTABLE_VERSION: &str = "hemilauncher-version.txt";

/// A portable version: in the launcher's folder (`dir\<Id>`), or a program of one file saved right in the chosen folder
/// (`dir\<App>.exe`, `single`).
#[derive(Clone, Debug, PartialEq)]
pub struct Portable {
    pub version: String,
    pub folder: PathBuf,
    pub single: bool,
}

impl Portable {
    pub fn exe(&self, app: &AppInfo) -> PathBuf {
        self.folder.join(app.exe)
    }
}

/// Where a browser extension is unpacked: its own folder in %AppData%\HeMi (`<base>\<Id>`) – not the folder of the
/// portable versions (the user, 2026-11-23).
pub fn extension_base() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default().join("HeMi")
}

pub fn portable(app: &AppInfo, dir: &std::path::Path) -> Option<Portable> {
    let base = extension_base();
    let dir = if app.kind == Kind::Extension { base.as_path() } else { dir };
    // HeMi Dark: the skins in the folder of VLC
    if app.kind == Kind::Files {
        return crate::vlc::installed().map(|version| Portable { version, folder: crate::vlc::folder(), single: false });
    }
    if app.kind != Kind::App && app.kind != Kind::Extension {
        return None;
    }
    let folder = dir.join(app.id);
    if let Ok(v) = std::fs::read_to_string(folder.join(PORTABLE_VERSION))
        && folder.join(app.exe).exists()
    {
        return Some(Portable { version: v.trim().to_string(), folder, single: false });
    }
    // a program of one file: the exe itself, its version from the exe (the HeMi apps all have it)
    let exe = dir.join(app.exe);
    if app.kind == Kind::App && exe.is_file() {
        return file_version(&exe).map(|version| Portable { version, folder: dir.to_path_buf(), single: true });
    }
    None
}

/// The version an exe carries ("1.6.1" of its file version 1.6.1.0); None when it has none.
pub fn file_version(exe: &std::path::Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW};
    let wide: Vec<u16> = exe.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is NUL-ended; the buffer is as big as GetFileVersionInfoSizeW asked for; VerQueryValueW points `p`
    // into that buffer (alive while read) and gives its length, read only when it holds a whole VS_FIXEDFILEINFO.
    unsafe {
        let size = GetFileVersionInfoSizeW(wide.as_ptr(), std::ptr::null_mut());
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide.as_ptr(), 0, size, buf.as_mut_ptr().cast()) == 0 {
            return None;
        }
        let root: [u16; 2] = [b'\\' as u16, 0];
        let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if VerQueryValueW(buf.as_ptr().cast(), root.as_ptr(), &mut p, &mut len) == 0 || p.is_null() || (len as usize) < size_of::<VS_FIXEDFILEINFO>() {
            return None;
        }
        let fi = &*(p as *const VS_FIXEDFILEINFO);
        if fi.dwSignature != 0xFEEF_04BD {
            return None;
        }
        Some(format!("{}.{}.{}", fi.dwFileVersionMS >> 16, fi.dwFileVersionMS & 0xFFFF, fi.dwFileVersionLS >> 16))
    }
}

#[cfg(test)]
mod version_live {
    /// By hand only: `cargo test --release file_version_live -- --ignored --nocapture` – the versions of the installed HeMi exes.
    #[test]
    #[ignore]
    fn file_version_live() {
        let base = std::path::PathBuf::from(std::env::var("LOCALAPPDATA").unwrap()).join(r"Programs\HeMi");
        for e in std::fs::read_dir(base).unwrap().flatten() {
            let exe = e.path().join(format!("{}.exe", e.file_name().to_string_lossy()));
            println!("{} → {:?}", exe.display(), super::file_version(&exe));
        }
    }
}
