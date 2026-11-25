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
    // SAFETY: the snapshot is checked, used only here and closed once; PROCESSENTRY32W is plain data (zeroed is valid)
    // with its dwSize set as the API wants; szExeFile is read only up to its NUL inside the array.
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
        return Err(tr("Close VLC first – it writes its settings when it ends.", "Nejdřív zavři VLC – při ukončení přepíše své nastavení.").into());
    }
    // the interface VLC had before HeMi Dark (none written = its default), given back by `remove`
    let before = std::fs::read(vlcrc()).ok().and_then(|b| String::from_utf8(b).ok()).and_then(|t| value(&t, "core", "intf"));
    if !before.as_deref().is_some_and(|v| v.starts_with("skins")) {
        let f = intf_file();
        std::fs::create_dir_all(f.parent().unwrap_or(&f)).map_err(|e| e.to_string())?;
        std::fs::write(&f, before.unwrap_or_default()).map_err(|e| e.to_string())?;
    }
    edit(|text| {
        let t = set(text, "core", "intf", "skins2,any");
        set(&t, "skins2", "skins2-last", &file.display().to_string())
    })
}

/// Where the interface of VLC before HeMi Dark is kept: %AppData%\HeMi\HeMiLauncher\vlc-intf.txt (empty: VLC had none
/// written, its default).
fn intf_file() -> PathBuf {
    crate::settings::data_dir().join("vlc-intf.txt")
}

/// VLC back to the interface it had before (the skin stays chosen, unused) and the skins removed.
pub fn remove() -> Result<(), String> {
    let tr = crate::i18n::tr;
    if vlc_running() {
        return Err(tr("Close VLC first – it writes its settings when it ends.", "Nejdřív zavři VLC – při ukončení přepíše své nastavení.").into());
    }
    if current().is_some() {
        // set by an older launcher (nothing kept): the normal interface of VLC
        let before = std::fs::read_to_string(intf_file()).unwrap_or_else(|_| "qt".into());
        let before = before.trim().to_string();
        edit(|text| if before.is_empty() { unset(text, "core", "intf") } else { set(text, "core", "intf", &before) })?;
    }
    // cleaning up: no longer needed
    let _ = std::fs::remove_file(intf_file());
    match std::fs::remove_dir_all(folder()) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

/// Changes vlcrc (made when VLC has never been started): its encoding and line ends kept. A vlcrc that is not UTF-8 is
/// left as it is (it would be damaged); the new one is written next to it first and then takes its place.
fn edit(change: impl Fn(&str) -> String) -> Result<(), String> {
    let path = vlcrc();
    let bytes = std::fs::read(&path).unwrap_or_default();
    let bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
    let text = std::str::from_utf8(if bom { &bytes[3..] } else { &bytes })
        .map_err(|_| format!("{}: {}", path.display(), crate::i18n::tr("not UTF-8 text – not changed", "není text v UTF-8 – nezměněno")))?;
    let crlf = text.contains("\r\n");
    let new = change(&text.replace("\r\n", "\n"));
    let new = if crlf { new.replace('\n', "\r\n") } else { new };
    let mut out = if bom { vec![0xEF, 0xBB, 0xBF] } else { vec![] };
    out.extend_from_slice(new.as_bytes());
    std::fs::create_dir_all(vlc_data()).map_err(|e| e.to_string())?;
    // a copy of the settings as they were, once – without it vlcrc is not changed
    let backup = vlc_data().join("vlcrc.before-hemidark");
    if !backup.exists() && path.exists() {
        std::fs::copy(&path, &backup).map_err(|e| format!("{}: {e}", backup.display()))?;
    }
    let tmp = vlc_data().join("vlcrc.hemilauncher-tmp");
    std::fs::write(&tmp, out).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        // cleaning up: the new text that could not take its place
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// The text without `key=…` in the section (VLC then uses its default).
fn unset(text: &str, section: &str, key: &str) -> String {
    let mut inside = false;
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| {
            let l = line.trim();
            if let Some(head) = l.strip_prefix('[') {
                inside = head.starts_with(&format!("{section}]"));
                return true;
            }
            !(inside && l.starts_with(&format!("{key}=")))
        })
        .collect();
    lines.join("\n") + "\n"
}

/// The value of a key in a section of vlcrc ("[core] # main program" starts the section "core").
fn value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;
    for line in text.lines() {
        let l = line.trim();
        if let Some(head) = l.strip_prefix('[') {
            inside = head.starts_with(&format!("{section}]"));
        } else if inside && let Some(v) = l.strip_prefix(&format!("{key}=")) {
            return Some(v.to_string());
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
        if let Some(head) = l.strip_prefix('[') {
            inside = head.starts_with(&format!("{section}]"));
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

    #[test]
    fn unsets_a_key_in_its_section_only() {
        let text = "[qt] # Qt\nintf=keep\n[core] # main\n#intf=\nintf=skins2,any\nother=1\n";
        let t = unset(text, "core", "intf");
        assert_eq!(value(&t, "core", "intf"), None);
        assert_eq!(value(&t, "qt", "intf").as_deref(), Some("keep"));
        assert!(t.contains("#intf=") && t.contains("other=1"));
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
