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

/// The browsers found: App Paths of Windows (for the user, then for all), else their usual folders.
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
        let registered = [false, true].into_iter().filter_map(|m| crate::reg::read(m, &key, "")).map(|p| PathBuf::from(p.trim_matches('"'))).find(|p| p.exists());
        if let Some(exe) = registered.or_else(|| paths.into_iter().find(|p| p.exists())) {
            out.push(Browser { name, exe, scheme });
        }
    }
    out
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Writes the guide next to the extension and opens it in the browser.
pub fn open_guide(b: &Browser, folder: &Path) -> Result<(), String> {
    let tr = crate::i18n::tr;
    let page = format!("{}://extensions", b.scheme);
    let dir = folder.display().to_string();
    let html = format!(
        r#"<!doctype html><html lang="{lang}"><head><meta charset="utf-8"><title>HeMiTube – {title}</title>
<style>
:root{{color-scheme:light dark;font-family:"Segoe UI Variable Text","Segoe UI",system-ui,sans-serif}}
body{{max-width:640px;margin:48px auto;padding:0 20px;line-height:1.5}}
h1{{font-size:26px;margin:0 0 6px}} p.lead{{opacity:.75;margin:0 0 26px}}
ol{{padding-left:22px}} li{{margin:0 0 22px}}
.copy{{display:flex;gap:8px;margin-top:8px}}
input{{flex:1;font:14px Consolas,monospace;padding:8px 10px;border-radius:6px;border:1px solid #8886;background:transparent;color:inherit}}
button{{font:inherit;padding:8px 16px;border-radius:6px;border:0;background:#4cc2ff;color:#000;cursor:pointer}}
</style></head><body>
<h1>{title}</h1><p class="lead">{lead}</p>
<ol>
<li>{s1}<div class="copy"><input id="a" readonly value="{page}"><button onclick="c('a',this)">{copy}</button></div></li>
<li>{s2}</li>
<li>{s3}<div class="copy"><input id="b" readonly value="{dir}"><button onclick="c('b',this)">{copy}</button></div></li>
</ol>
<p class="lead">{after}</p>
<script>function c(id,b){{var i=document.getElementById(id);i.select();try{{document.execCommand('copy')}}catch(e){{}};b.textContent='✓'}}</script>
</body></html>"#,
        lang = if crate::i18n::cz() { "cs" } else { "en" },
        title = esc(tr("Add HeMiTube to your browser", "Přidej HeMiTube do prohlížeče")),
        lead = esc(&format!("{} – {}", b.name, tr("three steps, the browser lets no program do them for you.", "tři kroky, prohlížeč je za tebe žádnému programu udělat nedovolí."))),
        s1 = esc(tr("Copy this address, paste it into the address bar and press Enter:", "Zkopíruj tuto adresu, vlož ji do adresního řádku a stiskni Enter:")),
        s2 = esc(tr("Turn on Developer mode (a switch at the top right of the page).", "Zapni Režim pro vývojáře (přepínač vpravo nahoře na stránce).")),
        s3 = esc(tr("Click Load unpacked and choose this folder (paste it into the address line of the dialog):", "Klikni na Načíst rozbalené a vyber tuto složku (vlož ji do adresního řádku dialogu):")),
        after = esc(tr("HeMi Launcher updates the extension in this folder; after an update click ⟳ at HeMiTube on the extensions page.", "HeMi Launcher rozšíření v této složce aktualizuje; po aktualizaci klikni na stránce rozšíření u HeMiTube na ⟳.")),
        copy = esc(tr("Copy", "Kopírovat")),
        page = esc(&page),
        dir = esc(&dir),
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
}
