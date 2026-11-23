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
pub fn check(by_user: bool) -> Releases {
    APPS.iter().chain(std::iter::once(&LAUNCHER)).map(|a| (a.id, github::latest(a.id, by_user))).collect()
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
    // the launcher keeps the app up to date: its installer does not ask about updates (HeMiDock); others ignore it
    cmd.arg("/LAUNCHER");
    if let Some(i) = installed {
        cmd.args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/UPDATE", if i.all_users { "/ALLUSERS" } else { "/CURRENTUSER" }]);
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

/// The portable version of this PC's architecture (the x64 one when the release has no ARM64 one): the exe itself when the
/// release has it (a program of one file), else the zip (HeMiDock, and the releases before the exes).
fn portable_asset(app: &AppInfo, release: &Release) -> String {
    let arm = cfg!(target_arch = "aarch64");
    for ext in ["exe", "zip"] {
        let names = if arm { vec![format!("{}-Portable-arm64.{ext}", app.id), format!("{}-Portable.{ext}", app.id)] } else { vec![format!("{}-Portable.{ext}", app.id)] };
        if let Some(n) = names.into_iter().find(|n| release.asset(n).is_some()) {
            return n;
        }
    }
    format!("{}-Portable.zip", app.id)
}

/// The downloaded portable version into the empty folder `into`: the exe copied under the name of the app, a zip unpacked.
fn unpack_portable(app: &AppInfo, file: &Path, into: &Path) -> bool {
    if file.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) {
        return std::fs::copy(file, into.join(app.exe)).is_ok();
    }
    // tar of Windows unpacks zips too
    Command::new("tar.exe").arg("-xf").arg(file).arg("-C").arg(into).status().is_ok_and(|s| s.success())
}

/// The portable version into `dir\<Id>`: unpacked next to it first, then swapped with the old one (which must not be
/// running); the version goes to a small file in the folder.
pub fn install_portable(app: &AppInfo, release: &Release, dir: &Path, progress: impl FnMut(f32)) -> Result<(), String> {
    let tr = crate::i18n::tr;
    let name = portable_asset(app, release);
    let zip = temp().join(&name);
    fetch(release, &name, &zip, progress)?;
    let target = dir.join(app.id);
    let new = dir.join(format!("{}.new", app.id));
    let old = dir.join(format!("{}.old", app.id));
    let _ = std::fs::remove_dir_all(&new);
    std::fs::create_dir_all(&new).map_err(|e| format!("{}: {e}", new.display()))?;
    let ok = unpack_portable(app, &zip, &new);
    let _ = std::fs::remove_file(&zip);
    // the exe right inside the zip, or (older zips) in one folder inside it
    let root = if new.join(app.exe).exists() {
        Some(new.clone())
    } else {
        std::fs::read_dir(&new).ok().and_then(|r| r.flatten().map(|e| e.path()).find(|p| p.join(app.exe).exists()))
    };
    let Some(root) = root.filter(|_| ok) else {
        let _ = std::fs::remove_dir_all(&new);
        return Err(tr("The portable version could not be unpacked.", "Portable verzi nešlo připravit.").into());
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
    let downloads = crate::settings::downloads();
    let file = downloads.join(&name);
    fetch(release, &name, &file, progress)?;
    let _ = Command::new("explorer.exe").arg(format!("/select,{}", file.display())).spawn();
    Ok(file)
}

/// A program without an installer (HeMiMizer): the exe for this processor into the Downloads folder.
pub fn download_tool(app: &AppInfo, release: &Release, progress: impl FnMut(f32)) -> Result<PathBuf, String> {
    // the ARM64 exe when the release has one (HeMiMizer is x64 only – Windows on ARM runs it emulated)
    let arm = format!("{}-arm64.exe", app.id);
    let asset = if std::env::consts::ARCH == "aarch64" && release.asset(&arm).is_some() { arm } else { format!("{}.exe", app.id) };
    let file = crate::settings::downloads().join(format!("{}.exe", app.id));
    fetch(release, &asset, &file, progress)?;
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
                .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/UPDATE", if i.all_users { "/ALLUSERS" } else { "/CURRENTUSER" }])
                .spawn()
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        None => {
            let name = portable_asset(&LAUNCHER, release);
            let zip = temp().join(&name);
            fetch(release, &name, &zip, progress)?;
            let out = temp().join("portable");
            let _ = std::fs::remove_dir_all(&out);
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            let ok = unpack_portable(&LAUNCHER, &zip, &out);
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
