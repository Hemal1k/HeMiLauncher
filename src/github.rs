//! The releases of the HeMi apps on GitHub (the only server the launcher talks to): the latest version, its notes and
//! its files – and downloading a file with its progress. HTTPS through the TLS of Windows.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::OnceLock;

const OWNER: &str = "Hemal1k";

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// "1.6.1" (the tag without its "v").
    pub version: String,
    pub notes: String,
    pub date: String,
    pub page: String,
    pub assets: Vec<(String, String)>,
}

impl Release {
    /// The download link of a file of the release.
    pub fn asset(&self, name: &str) -> Option<&str> {
        self.assets.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, u)| u.as_str())
    }
}

/// Where the files of the releases come from: the API (and the download links in it) only under this address. GitHub
/// sends the downloads on to its own servers (release-assets.githubusercontent.com) – over HTTPS only.
const TRUSTED: &str = "https://github.com/Hemal1k/";

fn config() -> ureq::config::ConfigBuilder<ureq::typestate::AgentScope> {
    let tls = ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        // no redirect to plain http
        .https_only(true)
        .timeout_connect(Some(std::time::Duration::from_secs(30)))
        .user_agent("HeMiLauncher")
}

/// The questions to the API and SHA256SUMS.txt: small answers, 30 seconds in all.
fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| config().timeout_global(Some(std::time::Duration::from_secs(30))).build().into())
}

/// The downloads: no limit for the whole (a big installer on a slow line takes long), only for each wait for data.
fn download_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        config()
            .timeout_recv_response(Some(std::time::Duration::from_secs(60)))
            .timeout_per_read(Some(std::time::Duration::from_secs(60)))
            .build()
            .into()
    })
}

/// The last answer of GitHub for a project: %LocalAppData%\HeMi\HeMiLauncher\cache\<project>.json, its ETag on the
/// first line. GitHub allows 60 questions an hour without an account (a 304 "unchanged" answer counts too).
fn cache_file(project: &str) -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_default().join("HeMi").join("HeMiLauncher").join("cache").join(format!("{project}.json"))
}

/// The latest release of a project (not pre-releases). GitHub counts every question without an account – also "has it
/// changed?" answered by 304 – so an answer younger than 30 minutes is used without asking (by_user, the Refresh
/// button: 1 minute).
pub fn latest(project: &str, by_user: bool) -> Result<Release, String> {
    let tr = crate::i18n::tr;
    let url = format!("https://api.github.com/repos/{OWNER}/{project}/releases/latest");
    let cached = std::fs::read_to_string(cache_file(project)).ok().and_then(|t| t.split_once('\n').map(|(e, b)| (e.to_string(), b.to_string())));
    let age = std::fs::metadata(cache_file(project)).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
    let fresh = std::time::Duration::from_secs(if by_user { 60 } else { 30 * 60 });
    if let (Some((_, body)), Some(age)) = (&cached, age)
        && age < fresh
    {
        return parse(body);
    }
    let mut req = agent().get(&url).header("Accept", "application/vnd.github+json");
    if let Some((etag, _)) = &cached {
        req = req.header("If-None-Match", etag.as_str());
    }
    let text = match req.call() {
        Ok(resp) if resp.status() == 304 => {
            // unchanged: the answer counts as new (the file gets a new time); the cache is only a saving, a failed write
            // just asks GitHub again next time
            let f = cache_file(project);
            if let Some((etag, body)) = &cached {
                let _ = std::fs::write(&f, format!("{etag}\n{body}"));
            }
            cached.map(|(_, b)| b).ok_or("304")?
        }
        Ok(mut resp) => {
            let etag = resp.headers().get("etag").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            let body = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
            if !etag.is_empty() {
                // the cache: a failed write only costs another question later
                let f = cache_file(project);
                let _ = std::fs::create_dir_all(f.parent().unwrap_or(Path::new(".")));
                let _ = std::fs::write(f, format!("{etag}\n{body}"));
            }
            body
        }
        // the limit of GitHub reached: the last known release is still good enough
        Err(ureq::Error::StatusCode(403 | 429)) => match cached {
            Some((_, b)) => b,
            None => return Err(tr("GitHub limit reached – try again in an hour", "Byl dosažen limit GitHubu – zkus to znovu za hodinu").into()),
        },
        Err(ureq::Error::StatusCode(404)) => return Err(tr("No release yet", "Zatím žádné vydání").into()),
        Err(e) => match cached {
            Some((_, b)) if matches!(e, ureq::Error::StatusCode(_)) => b,
            _ => return Err(tr("GitHub could not be reached", "GitHub není dostupný").into()),
        },
    };
    parse(&text)
}

/// A release from the answer of GitHub.
fn parse(text: &str) -> Result<Release, String> {
    let j: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let s = |k: &str| j[k].as_str().unwrap_or("").to_string();
    // only links to the projects of the HeMi apps (a link elsewhere – or a path to a program – is left out)
    let trusted = |u: &str| u.starts_with(TRUSTED).then(|| u.to_string());
    let assets = j["assets"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| Some((x["name"].as_str()?.to_string(), trusted(x["browser_download_url"].as_str()?)?))).collect())
        .unwrap_or_default();
    Ok(Release {
        version: s("tag_name").trim_start_matches(['v', 'V']).to_string(),
        notes: s("body"),
        date: s("published_at").chars().take(10).collect(),
        page: trusted(&s("html_url")).unwrap_or_default(),
        assets,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The checksum of a file in the text of SHA256SUMS.txt ("<hash>  <name>", or "<hash> *<name>" in binary mode; the
/// name in any case) – lowercase.
fn sum_in(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hash, file) = l.trim().split_once(char::is_whitespace)?;
        file.trim().trim_start_matches('*').eq_ignore_ascii_case(name).then(|| hash.trim().to_lowercase())
    })
}

/// The checksum SHA256SUMS.txt of the release gives for a file of it (read before the download).
pub fn checksum(release: &Release, name: &str) -> Result<String, String> {
    let tr = crate::i18n::tr;
    let url = release.asset("SHA256SUMS.txt").ok_or_else(|| tr("The release has no SHA256SUMS.txt – not installed.", "Vydání nemá SHA256SUMS.txt – neinstaluji.").to_string())?;
    sum_in(&text(url)?, name).ok_or_else(|| format!("{name}: {}", tr("not in SHA256SUMS.txt", "chybí v SHA256SUMS.txt")))
}

fn mismatch(name: &str) -> String {
    format!("{name}: {}", crate::i18n::tr("the checksum does not match (damaged or changed download)", "kontrolní součet nesedí (poškozené nebo změněné stažení)"))
}

/// Downloads `url` into `to` and checks it against `want` (SHA-256): the checksum is counted while downloading into a
/// temporary file, which takes the place of `to` only when it matches (a bad download never replaces a good file). The
/// file is then opened so that nobody can change or delete it, and checked once more: the open file returned keeps it
/// so until it is dropped – after the installer has started, the zip is unpacked or the exe copied. `progress` gets the
/// share done (0–1, or -1 when the size is not known).
pub fn download(url: &str, to: &Path, name: &str, want: &str, mut progress: impl FnMut(f32)) -> Result<std::fs::File, String> {
    use sha2::{Digest, Sha256};
    let response = download_agent().get(url).call().map_err(|e| format!("{} ({e})", crate::i18n::tr("The download failed", "Stažení se nepovedlo")))?;
    let total = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    let mut body = response.into_body();
    let mut reader = body.as_reader();
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = to.with_extension("part");
    let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    let copied = (|| -> Result<(), String> {
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("{} ({e})", crate::i18n::tr("The download failed", "Stažení se nepovedlo")))?;
            if n == 0 {
                return Ok(());
            }
            file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            h.update(&buf[..n]);
            done += n as u64;
            progress(total.map_or(-1.0, |t| done as f32 / t.max(1) as f32));
        }
    })();
    drop(file);
    // a broken or wrong download goes, the file it was meant to replace stays
    let checked = copied.and_then(|()| if hex(&h.finalize()) == want { Ok(()) } else { Err(mismatch(name)) });
    if let Err(e) = checked {
        // cleaning up: the temporary file only
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, to).map_err(|e| e.to_string())?;
    let locked = lock(to).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    std::io::copy(&mut &locked, &mut h).map_err(|e| e.to_string())?;
    if hex(&h.finalize()) != want {
        drop(locked);
        // cleaning up: the changed file is not used
        let _ = std::fs::remove_file(to);
        return Err(mismatch(name));
    }
    Ok(locked)
}

/// The file opened for reading, shared for reading only: while it is open no other process can write, rename or delete
/// it – starting it, copying or unpacking it (all reading) still work.
fn lock(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x1;
    std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(path)
}

/// A small text file of a release (SHA256SUMS.txt).
pub fn text(url: &str) -> Result<String, String> {
    let failed = |e: ureq::Error| format!("{} ({e})", crate::i18n::tr("SHA256SUMS.txt could not be downloaded", "SHA256SUMS.txt nešlo stáhnout"));
    agent().get(url).call().map_err(failed)?.body_mut().read_to_string().map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_sums_in_any_form() {
        let sums = "ABCDEF  HeMiDock-Setup.exe\nfedcba *HeMiPad-Portable.exe\n\n0123 other.zip\n";
        assert_eq!(sum_in(sums, "hemidock-setup.exe").as_deref(), Some("abcdef"));
        assert_eq!(sum_in(sums, "HeMiPad-Portable.exe").as_deref(), Some("fedcba"));
        assert_eq!(sum_in(sums, "missing.exe"), None);
        assert_eq!(hex(&[0x00, 0xab, 0x10]), "00ab10");
    }

    #[test]
    fn keeps_only_links_to_the_hemi_projects() {
        let json = r#"{"tag_name":"v1.2.3","body":"x","published_at":"2026-11-23T19:08:48Z","html_url":"C:\\Windows\\notepad.exe",
            "assets":[{"name":"a.exe","browser_download_url":"https://github.com/Hemal1k/HeMiPad/releases/download/v1.2.3/a.exe"},
                      {"name":"b.exe","browser_download_url":"http://example.com/b.exe"}]}"#;
        let r = parse(json).unwrap();
        assert_eq!(r.version, "1.2.3");
        assert_eq!(r.date, "2026-11-23");
        assert_eq!(r.page, "");
        assert!(r.asset("A.EXE").is_some());
        assert!(r.asset("b.exe").is_none());
    }
}
