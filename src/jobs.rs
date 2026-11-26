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
pub fn current(app: &AppInfo, portable_dir: &Path) -> Option<String> {
    if app.id == LAUNCHER.id {
        return Some(env!("CARGO_PKG_VERSION").to_string());
    }
    catalog::installed(app).map(|i| i.version).or_else(|| catalog::portable(app, portable_dir).map(|p| p.version))
}

/// The apps on this PC that have a newer version (the launcher too).
pub fn updates(releases: &Releases, portable_dir: &Path) -> Vec<(&'static str, String)> {
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

/// The downloads before they run: the launcher's own folder, not %TEMP% (programs run from %TEMP% look like droppers to
/// Defender – Behavior:Win32/Execution.A!ml, 2026-11-23).
fn temp() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("HeMi").join("HeMiLauncher").join("downloads")
}

/// A zip unpacked into `into` (inside the launcher – no tar.exe in the background).
pub fn unzip(file: &Path, into: &Path) -> bool {
    let Ok(f) = std::fs::File::open(file) else { return false };
    let Ok(mut z) = zip::ZipArchive::new(f) else { return false };
    unpack_entries(&mut z, into).is_ok()
}

/// The entries one by one instead of `ZipArchive::extract` (RUSTSEC-2026-0368 / 0429: its symbolic links could point out
/// of the folder, and a link with a huge declared size ended the process): only files and folders, only inside `into`
/// (`enclosed_name`), no links, at most 1 GB unpacked in all.
fn unpack_entries<R: std::io::Read + std::io::Seek>(z: &mut zip::ZipArchive<R>, into: &Path) -> std::io::Result<()> {
    const LIMIT: u64 = 1 << 30;
    let mut total = 0u64;
    for i in 0..z.len() {
        let mut entry = z.by_index(i)?;
        if entry.is_symlink() {
            continue;
        }
        let Some(rel) = entry.enclosed_name() else { continue };
        let out = into.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        total = total.saturating_add(entry.size());
        if total > LIMIT {
            return Err(std::io::Error::other("the zip unpacks to more than 1 GB"));
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut to = std::fs::File::create(&out)?;
        // never more than the declared size, whatever the entry really holds
        std::io::copy(&mut std::io::Read::take(&mut entry, LIMIT), &mut to)?;
    }
    Ok(())
}

#[cfg(test)]
mod unzip_tests {
    use std::io::Write;

    #[test]
    fn unpacks_files_and_leaves_out_escapes() {
        let dir = std::env::temp_dir().join("hemilauncher-unzip-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip_file = dir.join("t.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_file).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            w.add_directory("folder/", o).unwrap();
            w.start_file("folder/a.txt", o).unwrap();
            w.write_all(b"hello").unwrap();
            w.start_file("../escape.txt", o).unwrap();
            w.write_all(b"bad").unwrap();
            w.add_symlink("link", "C:\\Windows", o).unwrap();
            w.finish().unwrap();
        }
        let out = dir.join("out");
        assert!(super::unzip(&zip_file, &out));
        assert_eq!(std::fs::read_to_string(out.join("folder").join("a.txt")).unwrap(), "hello");
        assert!(!dir.join("escape.txt").exists());
        assert!(!out.join("link").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Downloads a file of the release into `to`, checked against SHA256SUMS.txt of the release (read first). The open file
/// returned keeps it unchanged until it is dropped – keep it while the file is used.
fn fetch(release: &Release, name: &str, to: &Path, progress: impl FnMut(f32)) -> Result<std::fs::File, String> {
    let url = release.asset(name).ok_or_else(|| format!("{name}: {}", crate::i18n::tr("not in the release", "ve vydání chybí")))?.to_string();
    let want = github::checksum(release, name)?;
    github::download(&url, to, name, &want, progress)
}

/// What an exit code of an Inno Setup installer means (0 is done). `new`: a new installation (with its pages).
fn installer_result(code: Option<i32>, new: bool) -> Result<(), String> {
    let tr = crate::i18n::tr;
    match code {
        Some(0) => Ok(()),
        // 1: closed at its first question (just for me / all users) – 1 is also "Setup failed to initialize", which a
        // new installation shows itself; 2: cancelled in the wizard, 5: cancelled while installing
        Some(1) if new => Err(CANCELLED.into()),
        Some(2) | Some(5) => Err(CANCELLED.into()),
        Some(8) => Err(tr("The installer needs Windows to restart first – restart and try again.", "Instalátor potřebuje nejdřív restartovat Windows – restartuj a zkus to znovu.").into()),
        Some(7) => Err(tr("The installer could not prepare the installation (is the app still running?).", "Instalátor nemohl připravit instalaci (neběží ještě aplikace?).").into()),
        c => Err(format!("{} {}", tr("The installer ended with code", "Instalátor skončil s kódem"), c.unwrap_or(-1))),
    }
}

#[cfg(test)]
mod installer_tests {
    use super::*;

    #[test]
    fn reads_the_exit_codes() {
        assert!(installer_result(Some(0), false).is_ok());
        assert_eq!(installer_result(Some(1), true).unwrap_err(), CANCELLED);
        assert_ne!(installer_result(Some(1), false).unwrap_err(), CANCELLED);
        assert_eq!(installer_result(Some(2), false).unwrap_err(), CANCELLED);
        assert_eq!(installer_result(Some(5), true).unwrap_err(), CANCELLED);
        assert_ne!(installer_result(Some(8), true).unwrap_err(), CANCELLED);
        assert!(installer_result(None, true).is_err());
    }
}

/// Downloads the installer of the app and runs it: with its pages (a new installation), or quietly (an update).
pub fn install(app: &AppInfo, release: &Release, installed: Option<&Installed>, progress: impl FnMut(f32)) -> Result<(), String> {
    let name = format!("{}-Setup.exe", app.id);
    let file = temp().join(&name);
    let lock = fetch(release, &name, &file, progress)?;
    let mut cmd = Command::new(&file);
    // the launcher keeps the app up to date: its installer does not ask about updates (HeMiDock); others ignore it
    cmd.arg("/LAUNCHER");
    if let Some(i) = installed {
        cmd.args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/UPDATE", if i.all_users { "/ALLUSERS" } else { "/CURRENTUSER" }]);
    }
    let status = cmd.status().map_err(|e| format!("{} ({e})", crate::i18n::tr("The installer could not be started", "Instalátor nešlo spustit")));
    drop(lock);
    // cleaning up: the downloaded installer (downloaded again next time)
    let _ = std::fs::remove_file(&file);
    // 0 done; the rest cancelled by the user (or administrator rights refused), or failed
    installer_result(status?.code(), installed.is_none())
}

/// The portable version of this PC's architecture (the x64 one when the release has no ARM64 one): the exe itself when the
/// release has it (a program of one file), else the zip (HeMiDock, and the releases before the exes).
fn portable_asset(app: &AppInfo, release: &Release) -> String {
    // a browser extension: one zip for every processor
    if app.kind == catalog::Kind::Extension {
        return format!("{}.zip", app.id);
    }
    // a plain exe (HeMiMizer): "<Id>.exe", x64 only – Windows on ARM runs it emulated
    if app.plain_exe {
        return format!("{}.exe", app.id);
    }
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
    unzip(file, into)
}

/// The portable version into `dir\<Id>`: unpacked next to it first, then swapped with the old one (which must not be
/// running); the version goes to a small file in the folder.
pub fn install_portable(app: &AppInfo, release: &Release, dir: &Path, progress: impl FnMut(f32)) -> Result<(), String> {
    let tr = crate::i18n::tr;
    let target = dir.join(app.id);
    let new = dir.join(format!("{}.new", app.id));
    let old = dir.join(format!("{}.old", app.id));
    // a folder of that name the launcher did not make (no version file – e.g. unpacked by hand) is never replaced
    if let Some(f) = [&target, &new, &old].into_iter().find(|f| f.exists() && !f.join(catalog::PORTABLE_VERSION).exists()) {
        return Err(format!(
            "{}: {}",
            f.display(),
            tr("the folder already exists and was not made by HeMi Launcher – move or rename it first", "složka už existuje a nevytvořil ji HeMi Launcher – nejdřív ji přesuň nebo přejmenuj")
        ));
    }
    let name = portable_asset(app, release);
    let zip = temp().join(&name);
    let lock = fetch(release, &name, &zip, progress)?;
    // cleaning up: a .new left by an earlier try (it has the version file, checked above)
    let _ = std::fs::remove_dir_all(&new);
    std::fs::create_dir_all(&new).map_err(|e| format!("{}: {e}", new.display()))?;
    let ok = unpack_portable(app, &zip, &new);
    drop(lock);
    // cleaning up: the download
    let _ = std::fs::remove_file(&zip);
    // the exe right inside the zip, or (older zips) in one folder inside it
    let root = if new.join(app.exe).exists() {
        Some(new.clone())
    } else {
        std::fs::read_dir(&new).ok().and_then(|r| r.flatten().map(|e| e.path()).find(|p| p.join(app.exe).exists()))
    };
    let Some(root) = root.filter(|_| ok) else {
        let _ = std::fs::remove_dir_all(&new);
        return Err(tr("The portable version could not be unpacked.", "Portable verzi nešlo rozbalit.").into());
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
        return Err(crate::i18n::tr("The folder was not made by HeMi Launcher.", "Složku nevytvořil HeMi Launcher.").into());
    }
    std::fs::remove_dir_all(&folder).map_err(|_| format!("{} – {}", app.name, crate::i18n::tr("close it first, it is running", "nejdřív ji zavři, běží")))
}

/// HeMi Dark into VLC: the skins unpacked into its skins folder and one set – the one chosen before, else blue in the
/// size of the display scaling. With VLC running the skins are unpacked only (it would write its old settings back).
pub fn install_vlc_skins(app: &AppInfo, release: &Release, progress: impl FnMut(f32)) -> Result<String, String> {
    let tr = crate::i18n::tr;
    let name = format!("{}-Skins.zip", app.id);
    let zip = temp().join(&name);
    let lock = fetch(release, &name, &zip, progress)?;
    let r = crate::vlc::unpack(&zip, &release.version);
    drop(lock);
    // cleaning up: the download
    let _ = std::fs::remove_file(&zip);
    r?;
    let (colours, sizes) = crate::vlc::choices();
    let (colour, size) = match crate::vlc::current() {
        Some((c, s)) if colours.contains(&c.as_str()) && sizes.contains(&s) => (c, s),
        Some((c, _)) if colours.contains(&c.as_str()) => (c, crate::vlc::size_for_screen(&sizes)),
        _ => ("blue".to_string(), crate::vlc::size_for_screen(&sizes)),
    };
    Ok(match crate::vlc::apply(&colour, size) {
        Ok(()) => format!("{} {} – {}", app.name, release.version, tr("set in VLC, start VLC", "nastaveno ve VLC, spusť VLC")),
        Err(e) => format!("{} {} – {}. {e}", app.name, release.version, tr("unpacked", "rozbaleno")),
    })
}

/// A program without an installer (HeMiMizer before its installer): the exe for this processor into the Downloads folder.
pub fn download_tool(app: &AppInfo, release: &Release, progress: impl FnMut(f32)) -> Result<PathBuf, String> {
    // the ARM64 exe when the release has one (HeMiMizer is x64 only – Windows on ARM runs it emulated)
    let arm = format!("{}-arm64.exe", app.id);
    let asset = if std::env::consts::ARCH == "aarch64" && release.asset(&arm).is_some() { arm } else { format!("{}.exe", app.id) };
    let file = crate::settings::downloads().join(format!("{}.exe", app.id));
    fetch(release, &asset, &file, progress)?;
    // only a help: the message names the file too
    let _ = Command::new(crate::settings::explorer()).arg(format!("/select,{}", file.display())).spawn();
    Ok(file)
}

/// The program and its arguments of an UninstallString: `"C:\…\unins000.exe" /SILENT` or a path without quotes.
fn split_command(s: &str) -> (String, Vec<String>) {
    let s = s.trim();
    let (exe, rest) = match s.strip_prefix('"').and_then(|r| r.split_once('"')) {
        Some((exe, rest)) => (exe, rest),
        // without quotes: the path up to ".exe", the rest arguments (a path may have spaces)
        None => match s.to_ascii_lowercase().find(".exe") {
            Some(i) => s.split_at(i + 4),
            None => (s, ""),
        },
    };
    (exe.trim().to_string(), rest.split_whitespace().map(String::from).collect())
}

#[cfg(test)]
mod uninstall_tests {
    #[test]
    fn splits_the_uninstall_string() {
        let (e, a) = super::split_command(r#""C:\Program Files\HeMi\HeMiPad\unins000.exe" /SILENT"#);
        assert_eq!(e, r"C:\Program Files\HeMi\HeMiPad\unins000.exe");
        assert_eq!(a, vec!["/SILENT"]);
        let (e, a) = super::split_command(r"C:\Program Files\HeMi\unins000.exe");
        assert_eq!(e, r"C:\Program Files\HeMi\unins000.exe");
        assert!(a.is_empty());
    }
}

/// The uninstaller of the app (with its question).
pub fn uninstall(i: &Installed) -> Result<(), String> {
    let tr = crate::i18n::tr;
    let (exe, args) = split_command(&i.uninstall);
    if exe.is_empty() {
        return Err(tr("No uninstaller found.", "Odinstalátor nebyl nalezen.").into());
    }
    let status = Command::new(exe).args(args).status().map_err(|e| format!("{} ({e})", tr("The uninstaller could not be started", "Odinstalátor nešlo spustit")))?;
    // 0 done, 2 the user said no / cancelled (nothing to tell); anything else failed
    match status.code() {
        Some(0) | Some(2) => Ok(()),
        c => Err(format!("{} {}", tr("The uninstaller ended with code", "Odinstalátor skončil s kódem"), c.unwrap_or(-1))),
    }
}

pub fn open(exe: &Path) -> Result<(), String> {
    /// ERROR_ELEVATION_REQUIRED: the program asks for administrator rights in its manifest (HeMiMizer)
    const ELEVATION_REQUIRED: i32 = 740;
    let dir = exe.parent().unwrap_or(Path::new("."));
    match Command::new(exe).current_dir(dir).spawn() {
        Ok(_) => Ok(()),
        // started through the shell instead: Windows asks for the rights (UAC) as on a double-click – on its own thread,
        // the window does not wait for the answer (No is no error; another failure goes to the log)
        Err(e) if e.raw_os_error() == Some(ELEVATION_REQUIRED) => {
            let (exe, dir) = (exe.to_path_buf(), dir.to_path_buf());
            std::thread::spawn(move || {
                if let Err(e) = shell_start(&exe, &dir)
                    && e != CANCELLED
                {
                    crate::log::write(e);
                }
            });
            Ok(())
        }
        Err(e) => Err(format!("{}: {} ({e})", exe.display(), crate::i18n::tr("could not be started", "nešlo spustit"))),
    }
}

/// A program started by the shell (ShellExecute "open"): its manifest decides about administrator rights.
fn shell_start(exe: &Path, dir: &Path) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &str| crate::reg::wide(s);
    let (verb, file, folder) = (wide("open"), wide(&exe.display().to_string()), wide(&dir.display().to_string()));
    // SAFETY: the three strings are NUL-ended and live until the call returns; the window and the parameters may be
    // null by the API.
    let r = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), folder.as_ptr(), SW_SHOWNORMAL) };
    // above 32 started; 5 (access denied) is also the answer when the user said No to UAC
    match r as isize {
        n if n > 32 => Ok(()),
        5 => Err(CANCELLED.into()),
        n => Err(format!("{}: {} ({n})", exe.display(), crate::i18n::tr("could not be started", "nešlo spustit"))),
    }
}

/// The launcher updates itself: installed – its installer quietly (it starts the launcher again); portable – the new
/// exe in place of this one (this one renamed to .old, removed at the next start). The caller quits then. `show`: the
/// update was clicked in the window – the new launcher opens its window; else (in the background) it starts in the tray.
pub fn update_self(release: &Release, show: bool, progress: impl FnMut(f32)) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let here = exe.parent().map(PathBuf::from).unwrap_or_default();
    match catalog::installed(&LAUNCHER).filter(|i| i.folder.eq(&here)) {
        Some(i) => {
            let name = "HeMiLauncher-Setup.exe";
            let file = temp().join(name);
            // kept unchanged until the installer runs (the process ends right after)
            let _lock = fetch(release, name, &file, progress)?;
            Command::new(&file)
                .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/UPDATE", if i.all_users { "/ALLUSERS" } else { "/CURRENTUSER" }])
                // the installer starts the new launcher with its window, not hidden in the tray
                .args(show.then_some("/OPEN=1"))
                .spawn()
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        None => {
            let name = portable_asset(&LAUNCHER, release);
            let zip = temp().join(&name);
            let lock = fetch(release, &name, &zip, progress)?;
            let out = temp().join("portable");
            // cleaning up: what an earlier try left
            let _ = std::fs::remove_dir_all(&out);
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            let ok = unpack_portable(&LAUNCHER, &zip, &out);
            drop(lock);
            let new = out.join("HeMiLauncher.exe");
            if !ok || !new.exists() {
                return Err(crate::i18n::tr("The new version could not be unpacked.", "Novou verzi nešlo rozbalit.").into());
            }
            let old = exe.with_extension("exe.old");
            let _ = std::fs::remove_file(&old);
            std::fs::rename(&exe, &old).map_err(|e| e.to_string())?;
            if let Err(e) = std::fs::copy(&new, &exe) {
                let _ = std::fs::rename(&old, &exe);
                return Err(e.to_string());
            }
            Command::new(&exe).args((!show).then_some(crate::settings::TRAY)).spawn().map_err(|e| e.to_string())?;
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
