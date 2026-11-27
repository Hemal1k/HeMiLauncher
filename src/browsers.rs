//! The Chromium browsers on this PC, for HeMiTube: the launcher opens a short guide in the one the user picks – the
//! address of its extensions page and the folder of the extension to copy. The browsers open no internal page
//! (chrome://extensions…) from the command line and let no program switch on the developer mode or load an extension, so
//! these two clicks stay the user's.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Browser {
    pub name: &'static str,
    pub exe: PathBuf,
    /// "chrome", "edge", "brave"… – its extensions page is `<scheme>://extensions`
    pub scheme: &'static str,
}

/// The browsers found: App Paths of Windows (for all users first – only administrators write there –, then for the
/// user), else their usual folders.
pub fn found() -> Vec<Browser> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default();
    let pf = std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    let pf86 = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
    let known: [(&str, &str, &str, Vec<PathBuf>); 5] = [
        ("Google Chrome", "chrome", "chrome.exe", vec![pf.join(r"Google\Chrome\Application\chrome.exe"), local.join(r"Google\Chrome\Application\chrome.exe")]),
        ("Microsoft Edge", "edge", "msedge.exe", vec![pf86.join(r"Microsoft\Edge\Application\msedge.exe"), pf.join(r"Microsoft\Edge\Application\msedge.exe")]),
        ("Brave", "brave", "brave.exe", vec![local.join(r"BraveSoftware\Brave-Browser\Application\brave.exe"), pf.join(r"BraveSoftware\Brave-Browser\Application\brave.exe")]),
        ("Vivaldi", "vivaldi", "vivaldi.exe", vec![local.join(r"Vivaldi\Application\vivaldi.exe"), pf.join(r"Vivaldi\Application\vivaldi.exe")]),
        ("Opera", "opera", "opera.exe", vec![local.join(r"Programs\Opera\opera.exe"), pf.join(r"Opera\opera.exe")]),
    ];
    let mut out = Vec::new();
    for (name, scheme, exe, paths) in known {
        let key = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
        let registered = [true, false].into_iter().filter_map(|m| crate::reg::read(m, &key, "")).map(|p| PathBuf::from(p.trim_matches('"'))).find(|p| p.exists());
        if let Some(exe) = registered.or_else(|| paths.into_iter().find(|p| p.exists())) {
            out.push(Browser { name, exe, scheme });
        }
    }
    out
}

/// The folders of the data of every Chromium browser of the user (Chrome and its Beta/Dev/Canary, Edge, Brave, Vivaldi,
/// Opera and Opera GX, Chromium, Yandex, Thorium, Arc…): a folder with the browser's `Local State`, up to three levels
/// under %LocalAppData% and %AppData% (Arc: in its package). Their profiles (`Default`, `Profile 1`…) are inside; Opera's
/// is a profile itself. Found once a run.
fn data_dirs() -> &'static [PathBuf] {
    static DIRS: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    DIRS.get_or_init(|| {
        fn walk(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
            if dir.join("Local State").is_file() {
                out.push(dir.to_path_buf());
                return;
            }
            if depth == 0 {
                return;
            }
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                // real folders only (no junctions like "Application Data")
                if e.file_type().map(|t| t.is_dir() && !t.is_symlink()).unwrap_or(false) {
                    walk(&e.path(), depth - 1, out);
                }
            }
        }
        let mut out = Vec::new();
        for var in ["LOCALAPPDATA", "APPDATA"] {
            if let Some(base) = std::env::var_os(var).map(PathBuf::from) {
                walk(&base, 3, &mut out);
            }
        }
        // browsers from the Store / MSIX (Arc): Packages\<package>\LocalCache\Local\<browser>\User Data
        if let Some(pk) = std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("Packages")) {
            for e in std::fs::read_dir(pk).into_iter().flatten().flatten() {
                walk(&e.path().join(r"LocalCache\Local"), 2, &mut out);
            }
        }
        out
    })
}

/// Whether a browser profile has loaded the unpacked extension from `folder`: the browser keeps its path in
/// `Secure Preferences` (or `Preferences`) of the profile and drops it when the extension is removed.
fn loaded_now(folder: &Path) -> bool {
    // the path as JSON writes it: "C:\\Users\\…\\HeMiTube"
    let needle = format!("\"path\":\"{}\"", folder.display().to_string().replace('\\', "\\\\")).to_lowercase();
    let has = |dir: &Path| {
        ["Secure Preferences", "Preferences"]
            .iter()
            .any(|f| std::fs::read(dir.join(f)).map(|b| String::from_utf8_lossy(&b).to_lowercase().contains(&needle)).unwrap_or(false))
    };
    // the profiles: the folder itself (Opera), "Default", "Profile 1"…
    let profile = |e: &std::fs::DirEntry| {
        let n = e.file_name().to_string_lossy().to_string();
        e.file_type().map(|t| t.is_dir()).unwrap_or(false) && (n == "Default" || n.starts_with("Profile "))
    };
    data_dirs().iter().any(|d| has(d) || std::fs::read_dir(d).map(|rd| rd.flatten().any(|e| profile(&e) && has(&e.path()))).unwrap_or(false))
}

/// Finds the data folders ahead (a quarter of a second), so the first frame does not wait for it.
pub fn warm() {
    data_dirs();
}

/// [`loaded_now`], read again at most every few seconds (the cards are drawn every frame).
pub fn loaded(folder: &Path) -> bool {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static CACHE: Mutex<Option<(Instant, PathBuf, bool)>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, f, v)) = c.as_ref()
        && f == folder
        && at.elapsed() < Duration::from_secs(4)
    {
        return *v;
    }
    let v = loaded_now(folder);
    *c = Some((Instant::now(), folder.to_path_buf(), v));
    v
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// What the extensions page of the browser calls its switch and button, and where the switch is – (English, Czech).
struct Words {
    dev: (&'static str, &'static str),
    dev_where: (&'static str, &'static str),
    load: (&'static str, &'static str),
}

fn words(scheme: &str) -> Words {
    match scheme {
        "edge" => Words {
            dev: ("Developer mode", "Režim vývojáře"),
            dev_where: ("in the panel on the left (in a narrow window under ☰ at the top left)", "v panelu vlevo (v úzkém okně pod ☰ vlevo nahoře)"),
            load: ("Load unpacked", "Načíst rozbalené"),
        },
        "opera" => Words {
            dev: ("Developer mode", "Režim vývojáře"),
            dev_where: ("a switch at the top right of the page", "přepínač vpravo nahoře na stránce"),
            load: ("Load unpacked", "Načíst rozbalené"),
        },
        // Chrome, Brave, Vivaldi: the page of Chromium
        _ => Words {
            dev: ("Developer mode", "Režim pro vývojáře"),
            dev_where: ("a switch at the top right of the page", "přepínač vpravo nahoře na stránce"),
            load: ("Load unpacked", "Načíst rozbalené"),
        },
    }
}

/// Writes the guide next to the extension and opens it in the browser – in English and Czech (a switch at the top, the
/// language of the launcher first), with the names the extensions page of this browser uses.
pub fn open_guide(b: &Browser, folder: &Path) -> Result<(), String> {
    let page = format!("{}://extensions", b.scheme);
    let dir = folder.display().to_string();
    let w = words(b.scheme);
    // one language of the guide: (lang, texts)
    let part = |cz: bool| {
        let t = |en: &str, cs: &str| esc(if cz { cs } else { en });
        let (dev, dev_where, load) = if cz { (w.dev.1, w.dev_where.1, w.load.1) } else { (w.dev.0, w.dev_where.0, w.load.0) };
        let lang = if cz { "cs" } else { "en" };
        format!(
            r#"<section lang="{lang}">
<h1>{title}</h1><p class="lead">{lead}</p>
<ol>
<li>{s1}<div class="copy"><input readonly value="{page}"><button onclick="c(this)">{copy}</button></div></li>
<li>{s2}</li>
<li>{s3}<div class="copy"><input readonly value="{dir}"><button onclick="c(this)">{copy}</button></div></li>
</ol>
<p class="lead">{after}</p>
</section>"#,
            title = t("Add HeMiTube to your browser", "Přidej HeMiTube do prohlížeče"),
            lead = esc(&format!("{} – {}", b.name, if cz { "tři kroky – prohlížeč nedovolí žádnému programu udělat je za tebe." } else { "three steps – the browser won't let any program do them for you." })),
            s1 = t("Copy this address, paste it into the address bar and press Enter:", "Zkopíruj tuto adresu, vlož ji do adresního řádku a stiskni Enter:"),
            s2 = esc(&if cz { format!("Zapni {dev} ({dev_where}).") } else { format!("Turn on {dev} ({dev_where}).") }),
            s3 = esc(&if cz {
                format!("Klikni na {load} a vyber tuto složku (vlož ji do adresního řádku dialogu):")
            } else {
                format!("Click {load} and choose this folder (paste it into the address bar of the dialog):")
            }),
            after = t(
                "HeMi Launcher updates the extension in this folder; after an update, click ⟳ next to HeMiTube on the extensions page.",
                "HeMi Launcher rozšíření v této složce aktualizuje; po aktualizaci klikni na stránce rozšíření u HeMiTube na ⟳.",
            ),
            copy = t("Copy", "Kopírovat"),
            page = esc(&page),
            dir = esc(&dir),
        )
    };
    let first = if crate::i18n::cz() { "cs" } else { "en" };
    let html = format!(
        r#"<!doctype html><html lang="{first}"><head><meta charset="utf-8"><title>HeMiTube</title>
<style>
:root{{color-scheme:light dark;font-family:"Segoe UI Variable Text","Segoe UI",system-ui,sans-serif}}
body{{max-width:640px;margin:32px auto 48px;padding:0 20px;line-height:1.5}}
nav{{display:flex;justify-content:flex-end;gap:4px;margin-bottom:16px}}
nav button{{background:transparent;color:inherit;border:1px solid #8886;padding:4px 12px}}
nav button.on{{background:#4cc2ff;color:#000;border-color:#4cc2ff}}
h1{{font-size:26px;margin:0 0 6px}} p.lead{{opacity:.75;margin:0 0 26px}}
ol{{padding-left:22px}} li{{margin:0 0 22px}}
.copy{{display:flex;gap:8px;margin-top:8px}}
input{{flex:1;font:14px Consolas,monospace;padding:8px 10px;border-radius:6px;border:1px solid #8886;background:transparent;color:inherit}}
button{{font:inherit;padding:8px 16px;border-radius:6px;border:0;background:#4cc2ff;color:#000;cursor:pointer}}
section{{display:none}} section.on{{display:block}}
</style></head><body>
<nav><button data-l="en" onclick="l('en')">English</button><button data-l="cs" onclick="l('cs')">Čeština</button></nav>
{en}
{cs}
<script>
function c(b){{var i=b.previousElementSibling;i.select();try{{document.execCommand('copy')}}catch(e){{}};b.textContent='✓'}}
function l(x){{document.documentElement.lang=x;document.querySelectorAll('section,nav button').forEach(function(e){{e.classList.toggle('on',(e.lang||e.dataset.l)===x)}})}}
l('{first}');
</script>
</body></html>"#,
        en = part(false),
        cs = part(true),
    );
    let file = folder.parent().unwrap_or(folder).join("HeMiTube-setup.html");
    std::fs::write(&file, html).map_err(|e| e.to_string())?;
    std::process::Command::new(&b.exe).arg(&file).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod live {
    /// By hand only: `cargo test --release browsers_live -- --ignored` – lists the browsers and opens the guide in the first.
    #[test]
    #[ignore]
    fn browsers_live() {
        let all = super::found();
        println!("{:?}", all.iter().map(|b| (b.name, b.exe.display().to_string())).collect::<Vec<_>>());
        let folder = crate::catalog::extension_base().join("HeMiTube");
        super::open_guide(&all[0], &folder).unwrap();
    }

    /// By hand only: `cargo test --release loaded_live -- --ignored --nocapture` – the data folders found and whether HeMiTube is loaded.
    #[test]
    #[ignore]
    fn loaded_live() {
        let t = std::time::Instant::now();
        println!("{:#?} ({:?})", super::data_dirs(), t.elapsed());
        let folder = crate::catalog::extension_base().join("HeMiTube");
        let t = std::time::Instant::now();
        println!("{} loaded: {} ({:?})", folder.display(), super::loaded_now(&folder), t.elapsed());
    }
}
