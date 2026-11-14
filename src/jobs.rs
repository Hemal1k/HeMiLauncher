//! What the launcher does with the apps, each on its own thread: looking for new versions, installing (the installer
//! of the app, with its own pages), updating (the installer quietly – for all users it asks for administrator rights
//! itself), the portable versions in a folder of the user's choice, uninstalling, downloading the skins of HeMi Dark –
//! and updating HeMi Launcher itself. Every download is checked against SHA256SUMS.txt of its release first.

use crate::catalog::{self, APPS, AppInfo, Installed, LAUNCHER};
use crate::github::{self, Release};
use std::path::{Path, PathBuf};
use std::process::Command;

/// A job the user cancelled (no error to show).
pub const CANCELLED: &str = "cancelled";

pub type Releases = Vec<(&'static str, Result<Release, String>)>;

/// The latest releases of every app and of the launcher.
pub fn check() -> Releases {
    APPS.iter().chain(std::iter::once(&LAUNCHER)).map(|a| (a.id, github::latest(a.id))).collect()
}

/// The version of an app on this PC: installed, else portable (in `portable_dir`).
pub fn current(app: &AppInfo, portable_dir: Option<&Path>) -> Option<String> {
    if app.id == LAUNCHER.id {
        return Some(env!("CARGO_PKG_VERSION").to_string());
    }
    catalog::installed(app).map(|i| i.version).or_else(|| portable_dir.and_then(|d| catalog::portable(app, d)).map(|p| p.version))
}

/// The apps on this PC that have a newer version (the launcher too).
pub fn updates(releases: &Releases, portable_dir: Option<&Path>) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    for (id, r) in releases {
        let Ok(r) = r else { continue };
        let Some(app) = find(id) else { continue };
        if let Some(v) = current(app, portable_dir)
            && catalog::newer(&r.version, &v)
        {
            out.push((*id, r.version.clone()));
        }
    }
    out
}

pub fn find(id: &str) -> Option<&'static AppInfo> {
    APPS.iter().chain(std::iter::once(&LAUNCHER)).find(|a| a.id == id)
}

fn temp() -> PathBuf {
    std::env::temp_dir().join("HeMiLauncher")
}

/// Downloads a file of the release into `to` and checks it.
fn fetch(release: &Release, name: &str, to: &Path, progress: impl FnMut(f32)) -> Result<(), String> {
    let url = release.asset(name).ok_or_else(|| format!("{name}: {}", crate::i18n::tr("not in the release", "ve vydání chybí")))?.to_string();
    github::download(&url, to, progress)?;
    github::verify(release, to, name)
}

/// Downloads the installer of the app and runs it: with its pages (a new installation), or quietly (an update).
pub fn install(app: &AppInfo, release: &Release, installed: Option<&Installed>, progress: impl FnMut(f32)) -> Result<(), String> {
    let name = format!("{}-Setup.exe", app.id);
    let file = temp().join(&name);
    fetch(release, &name, &file, progress)?;
    let mut cmd = Command::new(&file);
    if let Some(i) = installed {
        cmd.args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/UPDATE", if i.all_users { "/ALLUSERS" } else { "/CURRENTUSER" }]);
    }
    let status = cmd.status().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&file);
    // 0 done; the rest cancelled by the user (or administrator rights refused), or failed
    match status.code() {
        Some(0) => Ok(()),
        // 1: closed at its first question (just for me / all users), 2: cancelled in the wizard, 5: cancelled while installing
        Some(1) | Some(2) | Some(5) if installed.is_none() => Err(CANCELLED.into()),
        Some(2) | Some(5) => Err(CANCELLED.into()),
        c => Err(format!("{} {}", crate::i18n::tr("The installer ended with", "Instalátor skončil s kódem"), c.unwrap_or(-1))),
    }
}

/// The portable zip of this PC's architecture (the x64 one when the release has no ARM64 zip).
fn portable_zip(app: &AppInfo, release: &Release) -> String {
    let arm = format!("{}-Portable-arm64.zip", app.id);
    if cfg!(target_arch = "aarch64") && release.asset(&arm).is_some() { arm } else { format!("{}-Portable.zip", app.id) }
}

/// The portable version into `dir\<Id>`: unpacked next to it first, then swapped with the old one (which must not be
/// running); the version goes to a small file in the folder.
pub fn install_portable(app: &AppInfo, release: &Release, dir: &Path, progress: impl FnMut(f32)) -> Result<(), String> {
    let tr = crate::i18n::tr;
    let name = portable_zip(app, release);
    let zip = temp().join(&name);
    fetch(release, &name, &zip, progress)?;
    let target = dir.join(app.id);
    let new = dir.join(format!("{}.new", app.id));
    let old = dir.join(format!("{}.old", app.id));
    let _ = std::fs::remove_dir_all(&new);
    std::fs::create_dir_all(&new).map_err(|e| format!("{}: {e}", new.display()))?;
    let ok = Command::new("tar.exe").arg("-xf").arg(&zip).arg("-C").arg(&new).status().is_ok_and(|s| s.success());
    let _ = std::fs::remove_file(&zip);
    // the exe right inside the zip, or (older zips) in one folder inside it
    let root = if new.join(app.exe).exists() {
        Some(new.clone())
    } else {
        std::fs::read_dir(&new).ok().and_then(|r| r.flatten().map(|e| e.path()).find(|p| p.join(app.exe).exists()))
    };
    let Some(root) = root.filter(|_| ok) else {
        let _ = std::fs::remove_dir_all(&new);
        return Err(tr("The zip could not be unpacked.", "Zip nešel rozbalit.").into());
    };
    std::fs::write(root.join(catalog::PORTABLE_VERSION), &release.version).map_err(|e| e.to_string())?;
    if target.exists() {
        let _ = std::fs::remove_dir_all(&old);
        if std::fs::rename(&target, &old).is_err() {
            let _ = std::fs::remove_dir_all(&new);
            return Err(format!("{} – {}", app.name, tr("close it first, it is running", "nejdřív ji zavři, běží")));
        }
    }
    if let Err(e) = std::fs::rename(&root, &target) {
        let _ = std::fs::rename(&old, &target);
        let _ = std::fs::remove_dir_all(&new);
        return Err(e.to_string());
    }
    let _ = std::fs::remove_dir_all(&new);
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Removes the portable version (only a folder the launcher made – it has the version file).
pub fn remove_portable(app: &AppInfo, dir: &Path) -> Result<(), String> {
    let folder = dir.join(app.id);
    if !folder.join(catalog::PORTABLE_VERSION).exists() {
        return Err("?".into());
    }
    std::fs::remove_dir_all(&folder).map_err(|_| format!("{} – {}", app.name, crate::i18n::tr("close it first, it is running", "nejdřív ji zavři, běží")))
}

/// HeMi Dark: the skins zip into the Downloads folder, shown in File Explorer.
pub fn download_files(app: &AppInfo, release: &Release, progress: impl FnMut(f32)) -> Result<PathBuf, String> {
    let name = format!("{}-Skins.zip", app.id);
    let downloads = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default().join("Downloads");
    let file = downloads.join(&name);
    fetch(release, &name, &file, progress)?;
    let _ = Command::new("explorer.exe").arg(format!("/select,{}", file.display())).spawn();
    Ok(file)
}

/// The uninstaller of the app (with its question).
pub fn uninstall(i: &Installed) -> Result<(), String> {
    let exe = i.uninstall.trim().trim_matches('"').to_string();
    if exe.is_empty() {
        return Err("No uninstaller.".into());
    }
    Command::new(exe).status().map(|_| ()).map_err(|e| e.to_string())
}

pub fn open(exe: &Path) {
    let _ = Command::new(exe).current_dir(exe.parent().unwrap_or(Path::new("."))).spawn();
}

/// The launcher updates itself: installed – its installer quietly (it starts the launcher again); portable – the new
/// exe in place of this one (this one renamed to .old, removed at the next start). The caller quits then.
pub fn update_self(release: &Release, progress: impl FnMut(f32)) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let here = exe.parent().map(PathBuf::from).unwrap_or_default();
    match catalog::installed(&LAUNCHER).filter(|i| i.folder.eq(&here)) {
        Some(i) => {
            let name = "HeMiLauncher-Setup.exe";
            let file = temp().join(name);
            fetch(release, name, &file, progress)?;
            Command::new(&file)
                .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/UPDATE", if i.all_users { "/ALLUSERS" } else { "/CURRENTUSER" }])
                .spawn()
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        None => {
            let name = portable_zip(&LAUNCHER, release);
            let zip = temp().join(&name);
            fetch(release, &name, &zip, progress)?;
            let out = temp().join("portable");
            let _ = std::fs::remove_dir_all(&out);
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            // tar of Windows unpacks zips too
            let ok = Command::new("tar.exe").arg("-xf").arg(&zip).arg("-C").arg(&out).status().is_ok_and(|s| s.success());
            let new = out.join("HeMiLauncher.exe");
            if !ok || !new.exists() {
                return Err("The new version could not be unpacked.".into());
            }
            let old = exe.with_extension("exe.old");
            let _ = std::fs::remove_file(&old);
            std::fs::rename(&exe, &old).map_err(|e| e.to_string())?;
            if let Err(e) = std::fs::copy(&new, &exe) {
                let _ = std::fs::rename(&old, &exe);
                return Err(e.to_string());
            }
            Command::new(&exe).spawn().map_err(|e| e.to_string())?;
            Ok(())
        }
    }
}

/// What an earlier update of the portable launcher left.
pub fn clean_old() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("exe.old"));
    }
}
