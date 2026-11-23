//! HeMi Dark in VLC: the skins unpacked into the skins folder of VLC (`%APPDATA%\vlc\skins\HeMiDark`), the one the user
//! picks (a colour and a size) set in the settings of VLC (`vlcrc`: the skinnable interface and its last skin), and the
//! normal interface back when HeMi Dark is removed. VLC writes its settings when it ends, so it must be closed meanwhile.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// The display scaling of Windows in % (the interface writes it every frame) – the size picked at the first install.
pub static SYSTEM_SCALE: AtomicU32 = AtomicU32::new(100);

/// The colours of the skins in the order of HeMi Dark (key, English, Czech).
pub const COLOURS: &[(&str, &str, &str)] = &[
    ("blue", "Blue", "Modrá"),
    ("teal", "Teal", "Tyrkysová"),
    ("green", "Green", "Zelená"),
    ("yellow", "Yellow", "Žlutá"),
    ("orange", "Orange", "Oranžová"),
    ("red", "Red", "Červená"),
    ("pink", "Pink", "Růžová"),
    ("purple", "Purple", "Fialová"),
    ("graphite", "Graphite", "Grafitová"),
];

fn vlc_data() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default().join("vlc")
}

/// Where the launcher keeps the skins.
pub fn folder() -> PathBuf {
    vlc_data().join("skins").join("HeMiDark")
}

fn vlcrc() -> PathBuf {
    vlc_data().join("vlcrc")
}

/// The skins on this PC: their version (the launcher's version file), None when not installed by the launcher.
pub fn installed() -> Option<String> {
    std::fs::read_to_string(folder().join(crate::catalog::PORTABLE_VERSION)).ok().map(|v| v.trim().to_string())
}

/// The skin file of a colour and size: `<folder>\<colour>\HeMi-Dark-<colour>-<size>.vlt`.
fn skin(colour: &str, size: u32) -> PathBuf {
    folder().join(colour).join(format!("HeMi-Dark-{colour}-{size}.vlt"))
}

/// What there is to choose: the colours (in the order above) and the sizes found in the folder.
pub fn choices() -> (Vec<&'static str>, Vec<u32>) {
    let colours: Vec<&str> = COLOURS.iter().map(|c| c.0).filter(|c| folder().join(c).is_dir()).collect();
    let mut sizes: Vec<u32> = colours
        .first()
        .and_then(|c| std::fs::read_dir(folder().join(c)).ok())
        .map(|r| {
            r.flatten()
                .filter_map(|e| e.file_name().to_string_lossy().rsplit_once('-').and_then(|(_, s)| s.strip_suffix(".vlt").and_then(|n| n.parse().ok())))
                .collect()
        })
        .unwrap_or_default();
    sizes.sort_unstable();
    (colours, sizes)
}

/// The HeMi Dark skin VLC uses now (colour, size) – None when it uses another skin or its normal interface.
pub fn current() -> Option<(String, u32)> {
    let text = std::fs::read_to_string(vlcrc()).ok()?;
    let on = value(&text, "core", "intf").is_some_and(|v| v.starts_with("skins"));
    let last = value(&text, "skins2", "skins2-last")?;
    let name = Path::new(&last).file_stem()?.to_string_lossy().to_string();
    let rest = name.strip_prefix("HeMi-Dark-")?;
    let (colour, size) = rest.rsplit_once('-')?;
    on.then(|| (colour.to_string(), size.parse().unwrap_or(100)))
}

/// The size nearest to the display scaling.
pub fn size_for_screen(sizes: &[u32]) -> u32 {
    let want = SYSTEM_SCALE.load(Ordering::Relaxed);
    sizes.iter().copied().min_by_key(|s| s.abs_diff(want)).unwrap_or(100)
}

/// VLC running: the list of processes of Windows (no tasklist.exe in the background).
fn vlc_running() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut ok = Process32FirstW(snap, &mut e) != 0;
        while ok {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            if String::from_utf16_lossy(&e.szExeFile[..len]).eq_ignore_ascii_case("vlc.exe") {
                found = true;
                break;
            }
            ok = Process32NextW(snap, &mut e) != 0;
        }
        CloseHandle(snap);
        found
    }
}

/// The skins from the downloaded zip (a folder "HeMi Dark" with a folder per colour) into the skins folder of VLC.
pub fn unpack(zip: &Path, version: &str) -> Result<(), String> {
    let tr = crate::i18n::tr;
    let stage = std::env::temp_dir().join("HeMiLauncher-HeMiDark");
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    let ok = crate::jobs::unzip(zip, &stage);
    // the folder with the colours: "HeMi Dark" inside the zip (or the zip itself)
    let root = std::fs::read_dir(&stage).ok().and_then(|r| r.flatten().map(|e| e.path()).find(|p| p.join("blue").is_dir())).unwrap_or(stage.clone());
    if !ok || !root.join("blue").is_dir() {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(tr("The skins could not be unpacked.", "Vzhledy nešlo rozbalit.").into());
    }
    let target = folder();
    let _ = std::fs::remove_dir_all(&target);
    std::fs::create_dir_all(target.parent().unwrap_or(&target)).map_err(|e| e.to_string())?;
    copy_dir(&root, &target).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::write(target.join(crate::catalog::PORTABLE_VERSION), version).map_err(|e| e.to_string())
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()))?;
        } else {
            std::fs::copy(&p, to.join(e.file_name()))?;
        }
    }
    Ok(())
}

/// Sets the skin in VLC (the skinnable interface, this skin as the last one).
pub fn apply(colour: &str, size: u32) -> Result<(), String> {
    let tr = crate::i18n::tr;
    let file = skin(colour, size);
    if !file.exists() {
        return Err(format!("{} {}", tr("Missing skin", "Chybí vzhled"), file.display()));
    }
    if vlc_running() {
        return Err(tr("Close VLC first – it writes its settings when it ends.", "Nejdřív zavři VLC – při ukončení přepisuje svoje nastavení.").into());
    }
    edit(|text| {
        let t = set(text, "core", "intf", "skins2,any");
        set(&t, "skins2", "skins2-last", &file.display().to_string())
    })
}

/// VLC back to its normal interface (the skin stays chosen, unused) and the skins removed.
pub fn remove() -> Result<(), String> {
    let tr = crate::i18n::tr;
    if vlc_running() {
        return Err(tr("Close VLC first – it writes its settings when it ends.", "Nejdřív zavři VLC – při ukončení přepisuje svoje nastavení.").into());
    }
    if current().is_some() {
        edit(|text| set(text, "core", "intf", "qt"))?;
    }
    std::fs::remove_dir_all(folder()).map_err(|e| e.to_string())
}

/// Changes vlcrc (made when VLC has never been started): its encoding and line ends kept.
fn edit(change: impl Fn(&str) -> String) -> Result<(), String> {
    let path = vlcrc();
    let bytes = std::fs::read(&path).unwrap_or_default();
    let bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
    let text = String::from_utf8_lossy(if bom { &bytes[3..] } else { &bytes }).to_string();
    let crlf = text.contains("\r\n");
    let new = change(&text.replace("\r\n", "\n"));
    let new = if crlf { new.replace('\n', "\r\n") } else { new };
    let mut out = if bom { vec![0xEF, 0xBB, 0xBF] } else { vec![] };
    out.extend_from_slice(new.as_bytes());
    std::fs::create_dir_all(vlc_data()).map_err(|e| e.to_string())?;
    // a copy of the settings as they were, once
    let backup = vlc_data().join("vlcrc.before-hemidark");
    if !backup.exists() && path.exists() {
        let _ = std::fs::copy(&path, &backup);
    }
    std::fs::write(&path, out).map_err(|e| e.to_string())
}

/// The value of a key in a section of vlcrc ("[core] # main program" starts the section "core").
fn value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            inside = l[1..].starts_with(&format!("{section}]"));
        } else if inside {
            if let Some(v) = l.strip_prefix(&format!("{key}=")) {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// The text with `key=value` in the section: the key (or its commented default "#key=") replaced, else added after the
/// heading; a missing section is added at the end.
fn set(text: &str, section: &str, key: &str, val: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let mut inside = false;
    let mut heading = None;
    let mut commented = None;
    for (i, line) in lines.iter().enumerate() {
        let l = line.trim();
        if l.starts_with('[') {
            inside = l[1..].starts_with(&format!("{section}]"));
            if inside {
                heading = Some(i);
            }
        } else if inside {
            if l.starts_with(&format!("{key}=")) {
                lines[i] = format!("{key}={val}");
                return lines.join("\n") + "\n";
            }
            if commented.is_none() && l.starts_with(&format!("#{key}=")) {
                commented = Some(i);
            }
        }
    }
    match (commented, heading) {
        (Some(i), _) => lines.insert(i + 1, format!("{key}={val}")),
        (None, Some(h)) => lines.insert(h + 1, format!("{key}={val}")),
        (None, None) => {
            lines.push(format!("[{section}]"));
            lines.push(format!("{key}={val}"));
        }
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_keys_in_their_sections() {
        let text = "[skins2] # Skins\n#skins2-last=\n\n[core] # main\n#intf=\nintf=qt\n";
        let t = set(text, "core", "intf", "skins2,any");
        let t = set(&t, "skins2", "skins2-last", r"C:\x\HeMi-Dark-blue-125.vlt");
        assert_eq!(value(&t, "core", "intf").as_deref(), Some("skins2,any"));
        assert_eq!(value(&t, "skins2", "skins2-last").as_deref(), Some(r"C:\x\HeMi-Dark-blue-125.vlt"));
        let fresh = set("", "core", "intf", "skins2,any");
        assert_eq!(value(&fresh, "core", "intf").as_deref(), Some("skins2,any"));
    }
}

#[cfg(test)]
mod live {
    /// By hand only: `HEMIDARK_ZIP=<zip> cargo test --release vlc_live -- --ignored` – unpacks into VLC and sets the skin
    /// chosen before (or blue) on this PC.
    #[test]
    #[ignore]
    fn vlc_live() {
        let zip = std::env::var("HEMIDARK_ZIP").expect("HEMIDARK_ZIP");
        super::unpack(std::path::Path::new(&zip), "test").unwrap();
        let (colours, sizes) = super::choices();
        println!("colours {colours:?} sizes {sizes:?} now {:?}", super::current());
        let (c, s) = super::current().unwrap_or(("blue".into(), 100));
        super::apply(&c, if sizes.contains(&s) { s } else { 100 }).unwrap();
        println!("after {:?}", super::current());
    }
}
