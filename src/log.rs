//! The log of HeMi Launcher (Settings → Logging, off unless switched on): what it checks, downloads, installs and what
//! failed, in %AppData%\HeMi\HeMiLauncher\log.txt. No personal data: the folders of the user are written as
//! %USERPROFILE%, %AppData% and %LocalAppData%, the user name in other paths as <user> (the launcher has no tokens or
//! passwords). At 1 MB the file becomes log.old.txt (the one before is removed) and a new one starts.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(false);
/// One writer at a time (the jobs run on their own threads).
static WRITING: Mutex<()> = Mutex::new(());
const LIMIT: u64 = 1024 * 1024;

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
    if std::fs::metadata(&f).is_ok_and(|m| m.len() >= LIMIT) {
        let old = f.with_file_name("log.old.txt");
        // cleaning up: the log before the last one
        let _ = std::fs::remove_file(&old);
        let _ = std::fs::rename(&f, &old);
    }
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(&f) {
        let _ = writeln!(out, "{} {}", now(), scrub(text.as_ref()));
    }
}

/// The time in UTC ("2026-11-26 14:03:05Z") – without a library for dates.
fn now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (y, m, d) = civil(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z", t / 3600, t / 60 % 60, t % 60)
}

/// The date of a day counted from 1970-01-01 (the algorithm of Howard Hinnant).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
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
    fn counts_the_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(11_017), (2000, 3, 1));
        assert_eq!(civil(20_418), (2025, 11, 26));
    }

    #[test]
    fn leaves_out_the_user() {
        assert_eq!(replace_ci(r"C:\USERS\Jan\a.txt – č", r"c:\users\jan", "%USERPROFILE%"), r"%USERPROFILE%\a.txt – č");
        assert_eq!(replace_ci("žluťoučký", "x", "y"), "žluťoučký");
        let s = scrub("a\r\nb");
        assert_eq!(s, "a  b");
    }
}
