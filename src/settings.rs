//! What HeMi Launcher remembers (%AppData%\HeMi\HeMiLauncher\settings.ini), the start with Windows (the Run value of the user)
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
    /// Where the portable versions go (empty: the Downloads folder).
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
    /// Where the portable versions go: the folder chosen in the settings, else the Downloads folder.
    pub fn portable(&self) -> PathBuf {
        if self.portable_dir.trim().is_empty() { downloads() } else { PathBuf::from(self.portable_dir.trim()) }
    }

    pub fn load() -> Settings {
        let mut s = Settings::read_file();
        s.take_setup_choice();
        s
    }

    /// What was chosen in the installer (Updates: look for them, install them): it writes it to
    /// HKCU\Software\HeMi\HeMiLauncher instead of starting the launcher; taken into the settings once and removed.
    fn take_setup_choice(&mut self) {
        const KEY: &str = r"Software\HeMi\HeMiLauncher";
        let check = reg::read(false, KEY, "SetupAutoCheck");
        let install = reg::read(false, KEY, "SetupAutoInstall");
        if check.is_none() && install.is_none() {
            return;
        }
        if let Some(v) = check {
            self.auto_check = v.trim() == "1";
        }
        if let Some(v) = install {
            self.auto_install = v.trim() == "1";
        }
        // at the start, without a window yet: not saved, the choice is still used now (and saved with the next change)
        let _ = self.save();
        reg::delete_user(KEY, "SetupAutoCheck");
        reg::delete_user(KEY, "SetupAutoInstall");
    }

    fn read_file() -> Settings {
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

    /// Writes the settings; the error is for the window to show.
    pub fn save(&self) -> Result<(), String> {
        let b = |v: bool| if v { "1" } else { "0" };
        let text = format!(
            "AutoCheck={}\nAutoInstall={}\nNotify={}\nTrayOnClose={}\nPortableDir={}\n",
            b(self.auto_check),
            b(self.auto_install),
            b(self.notify),
            b(self.tray_on_close),
            self.portable_dir
        );
        let failed = |e: std::io::Error| format!("{} ({e})", crate::i18n::tr("The settings could not be saved", "Nastavení nešlo uložit"));
        std::fs::create_dir_all(data_dir()).map_err(failed)?;
        std::fs::write(file(), text).map_err(failed)
    }
}

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE: &str = "HeMiLauncher";
/// Started with Windows: hidden in the notification area, looking for updates.
pub const TRAY: &str = "--tray";

pub fn autostart() -> bool {
    reg::read(false, RUN, VALUE).is_some()
}

/// The start with Windows is two things: the Run value (it shows in Task Manager → Startup apps, where it can be switched
/// off) and a task at sign-in – Windows starts the Run values only some time after the sign-in, the task at once.
/// False when switching it on did not fully work (the Run value or the task could not be written).
pub fn set_autostart(on: bool) -> bool {
    match (on, std::env::current_exe()) {
        (true, Ok(exe)) => {
            let run = reg::write_user(RUN, VALUE, &format!("\"{}\" {TRAY}", exe.display()));
            register_task(&exe) && run
        }
        (true, Err(_)) => false,
        _ => {
            reg::delete_user(RUN, VALUE);
            delete_task();
            true
        }
    }
}

/// Switched off in Task Manager → Startup apps: Windows marks the Run value (the first byte odd = off); the task then
/// starts nothing either.
pub fn startup_switched_off() -> bool {
    reg::first_byte_user(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run", VALUE).is_some_and(|b| b & 1 == 1)
}

/// One task per user (the tasks of all users share one folder).
fn task_name() -> String {
    format!("HeMiLauncher ({})", std::env::var("USERNAME").unwrap_or_default())
}

fn schtasks(args: &[&str]) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("schtasks.exe").args(args).creation_flags(CREATE_NO_WINDOW).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The task: at the sign-in of this user, as this user without raised rights, normal priority, also on battery,
/// never ended after a time. False when schtasks did not make it.
fn register_task(exe: &std::path::Path) -> bool {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let user = format!("{}\\{}", std::env::var("USERDOMAIN").unwrap_or_default(), std::env::var("USERNAME").unwrap_or_default());
    let exe_s = exe.display().to_string();
    let dir = exe.parent().map(|d| d.display().to_string()).unwrap_or_default();
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>HeMi Launcher at sign-in</Description></RegistrationInfo>
  <Triggers><LogonTrigger><Enabled>true</Enabled><UserId>{u}</UserId></LogonTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{u}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>false</AllowHardTerminate>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>5</Priority>
  </Settings>
  <Actions Context="Author"><Exec><Command>"{e}"</Command><Arguments>{TRAY}</Arguments><WorkingDirectory>{d}</WorkingDirectory></Exec></Actions>
</Task>"#,
        u = esc(&user),
        e = esc(&exe_s),
        d = esc(&dir)
    );
    // schtasks wants the file in UTF-16 with its mark
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(|c| c.to_le_bytes()));
    let file = std::env::temp_dir().join("hemilauncher-task.xml");
    if std::fs::write(&file, bytes).is_err() {
        return false;
    }
    let name = task_name();
    let made = schtasks(&["/Create", "/TN", &name, "/XML", &file.display().to_string(), "/F"]).is_some();
    // cleaning up: the temporary file of the task
    let _ = std::fs::remove_file(&file);
    made
}

fn delete_task() {
    let name = task_name();
    // fails also when there is no task (nothing to tell)
    let _ = schtasks(&["/Delete", "/TN", &name, "/F"]);
}

/// The task there and pointing to this exe (asked by schtasks – only on the thread of the start, never on the window's).
fn task_ok(exe: &std::path::Path) -> bool {
    let name = task_name();
    schtasks(&["/Query", "/TN", &name, "/XML"]).is_some_and(|x| x.to_lowercase().contains(&exe.display().to_string().to_lowercase()))
}

/// Moved (another folder, an update of the portable one) or from a version without the task: the start with Windows
/// points to this exe again and has its task. Off the window's thread (schtasks takes a moment).
pub fn repair_autostart() {
    if autostart() {
        std::thread::spawn(fix_autostart);
    }
}

/// The work of `repair_autostart` on this thread (also `--repair` of the installer, which waits for it).
pub fn fix_autostart() {
    if !autostart() {
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        if reg::read(false, RUN, VALUE).is_none_or(|v| !v.to_lowercase().contains(&exe.display().to_string().to_lowercase())) {
            reg::write_user(RUN, VALUE, &format!("\"{}\" {TRAY}", exe.display()));
        }
        // in the background, nobody to tell: tried again at the next start
        if !task_ok(&exe) {
            let _ = register_task(&exe);
        }
    }
}

/// Where the other HeMi apps find the launcher: they leave their updates to it then.
pub fn register() {
    if let Ok(exe) = std::env::current_exe() {
        reg::write_user(r"Software\HeMi\HeMiLauncher", "Path", &exe.display().to_string());
    }
}

/// The Downloads folder of the user (a moved one too).
pub fn downloads() -> PathBuf {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath};
    // SAFETY: SHGetKnownFolderPath writes a NUL-ended string it allocated into `p` (only read when it succeeded and `p`
    // is not null); it is read up to its NUL, copied, and freed once with CoTaskMemFree as the API wants.
    unsafe {
        let mut p: windows_sys::core::PWSTR = std::ptr::null_mut();
        if SHGetKnownFolderPath(&FOLDERID_Downloads, 0, std::ptr::null_mut(), &mut p) == 0 && !p.is_null() {
            let len = (0..).take_while(|&i| *p.add(i) != 0).count();
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
            CoTaskMemFree(p as _);
            return PathBuf::from(s);
        }
    }
    std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default().join("Downloads")
}
