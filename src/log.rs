//! The log of HeMi Launcher (Settings → Logging, off unless switched on): what it checks, downloads, installs and what
//! failed, in %AppData%\HeMi\HeMiLauncher\log.txt. No personal data: the folders of the user are written as
//! %USERPROFILE%, %AppData% and %LocalAppData%, the user name in other paths as <user> (the launcher has no tokens or
//! passwords). At most 1 MB: then the older half goes (log.old.txt of 1.6.0 is removed).
//! The same in all HeMi apps: %AppData%\HeMi\<app>\log.txt, the local time, at most 1 MB (then the older half goes), the
//! folders of the user written as %USERPROFILE%, %AppData% and %LocalAppData%.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(false);
/// One writer at a time (the jobs run on their own threads).
static WRITING: Mutex<()> = Mutex::new(());
const LIMIT: usize = 1024 * 1024;

pub fn file() -> PathBuf {
    crate::settings::data_dir().join("log.txt")
}

/// Switched on or off (from the settings); the change itself is written too.
pub fn enable(on: bool) {
    if ON.load(Ordering::SeqCst) == on {
        return;
    }
    if !on {
        write("Logging off");
    }
    ON.store(on, Ordering::SeqCst);
    if on {
        write(format!("Logging on – HeMi Launcher {}", env!("CARGO_PKG_VERSION")));
    }
}

/// One line into the log (only when it is on); a failed write is left out quietly.
pub fn write(text: impl AsRef<str>) {
    if !ON.load(Ordering::SeqCst) {
        return;
    }
    let _one = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    let f = file();
    // 1.6.0 kept the log before as log.old.txt
    let _ = std::fs::remove_file(f.with_file_name("log.old.txt"));
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let entry = format!("{} {}\r\n", now(), scrub(text.as_ref()));
    if std::fs::metadata(&f).is_ok_and(|m| m.len() as usize + entry.len() > LIMIT)
        && let Ok(old) = std::fs::read(&f)
    {
        let _ = std::fs::write(&f, newer_half(&old));
    }
    if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(&f) {
        let _ = out.write_all(entry.as_bytes());
    }
}

/// The newer half of the log, from the start of a line.
fn newer_half(log: &[u8]) -> &[u8] {
    let keep = LIMIT / 2;
    if log.len() <= keep {
        return log;
    }
    let tail = &log[log.len() - keep..];
    tail.iter().position(|&b| b == b'\n').map_or(&[][..], |i| &tail[i + 1..])
}

/// "2026-11-26 14:05:09" in the local time.
fn now() -> String {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: GetLocalTime only fills the plain struct it is given.
    let t = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// One line without the folders and the name of the user.
fn scrub(text: &str) -> String {
    let mut s = text.replace(['\r', '\n'], " ");
    // the longer folders first (both AppData folders are inside the profile)
    for (var, name) in [("LOCALAPPDATA", "%LocalAppData%"), ("APPDATA", "%AppData%"), ("USERPROFILE", "%USERPROFILE%")] {
        if let Some(v) = std::env::var_os(var).map(|v| v.to_string_lossy().trim_end_matches('\\').to_string())
            && v.len() > 3
        {
            s = replace_ci(&s, &v, name);
        }
    }
    // the name only as a folder of Users (a name like "HeMi" is also a part of the names of the apps)
    if let Ok(user) = std::env::var("USERNAME")
        && !user.is_empty()
    {
        s = replace_ci(&s, &format!(r"\Users\{user}"), r"\Users\<user>");
    }
    s
}

/// `needle` replaced in any case of its ASCII letters (paths of Windows).
fn replace_ci(hay: &str, needle: &str, with: &str) -> String {
    let mut out = String::with_capacity(hay.len());
    let mut i = 0;
    while i < hay.len() {
        if let Some(part) = hay.get(i..i + needle.len())
            && part.eq_ignore_ascii_case(needle)
        {
            out.push_str(with);
            i += needle.len();
            continue;
        }
        let Some(c) = hay[i..].chars().next() else { break };
        out.push(c);
        i += c.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newer_half_starts_a_line() {
        let line = "2026-11-26 14:05:09 a line of the log\r\n";
        let old = line.repeat(LIMIT / line.len() + 10);
        let kept = newer_half(old.as_bytes());
        assert!(kept.len() <= LIMIT / 2 && kept.starts_with(b"2026"));
        assert_eq!(newer_half(b"short\n"), b"short\n");
    }

    #[test]
    fn leaves_out_the_user() {
        assert_eq!(replace_ci(r"C:\USERS\Jan\a.txt – č", r"c:\users\jan", "%USERPROFILE%"), r"%USERPROFILE%\a.txt – č");
        assert_eq!(replace_ci("žluťoučký", "x", "y"), "žluťoučký");
        let s = scrub("a\r\nb");
        assert_eq!(s, "a  b");
    }
}
