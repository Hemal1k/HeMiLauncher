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

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        let tls = ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).build();
        ureq::Agent::config_builder()
            .tls_config(tls)
            .timeout_global(Some(std::time::Duration::from_secs(600)))
            .user_agent("HeMiLauncher")
            .build()
            .into()
    })
}

/// The last answer of GitHub for a project: %LocalAppData%\HeMi\HeMiLauncher\cache\<project>.json, its ETag on the
/// first line. GitHub allows 60 questions an hour without an account; "has it changed?" answered by 304 does not count.
fn cache_file(project: &str) -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from).unwrap_or_default().join("HeMi").join("HeMiLauncher").join("cache").join(format!("{project}.json"))
}

/// The latest release of a project (not pre-releases). GitHub counts every question without an account – also "has it
/// changed?" answered by 304 – so an answer younger than 30 minutes is used without asking (y_user, the Refresh
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
            // unchanged: the answer counts as new (the file gets a new time)
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
                let f = cache_file(project);
                let _ = std::fs::create_dir_all(f.parent().unwrap_or(Path::new(".")));
                let _ = std::fs::write(f, format!("{etag}\n{body}"));
            }
            body
        }
        // the limit of GitHub reached: the last known release is still good enough
        Err(ureq::Error::StatusCode(403 | 429)) => match cached {
            Some((_, b)) => b,
            None => return Err(tr("GitHub limit reached – try again in an hour", "Dosažen limit GitHubu – zkus to znovu za hodinu").into()),
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
    let assets = j["assets"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| Some((x["name"].as_str()?.to_string(), x["browser_download_url"].as_str()?.to_string()))).collect())
        .unwrap_or_default();
    Ok(Release {
        version: s("tag_name").trim_start_matches(['v', 'V']).to_string(),
        notes: s("body"),
        date: s("published_at").chars().take(10).collect(),
        page: s("html_url"),
        assets,
    })
}

/// Downloads `url` into `to` (through a temporary file); `progress` gets the share done (0–1, or -1 when the size is not
/// known).
pub fn download(url: &str, to: &Path, mut progress: impl FnMut(f32)) -> Result<(), String> {
    let response = agent().get(url).call().map_err(|e| e.to_string())?;
    let total = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    let mut body = response.into_body();
    let mut reader = body.as_reader();
    if let Some(dir) = to.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = to.with_extension("part");
    let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        progress(total.map_or(-1.0, |t| done as f32 / t.max(1) as f32));
    }
    drop(file);
    std::fs::rename(&tmp, to).map_err(|e| e.to_string())
}

/// A small text file of a release (SHA256SUMS.txt).
pub fn text(url: &str) -> Result<String, String> {
    agent().get(url).call().map_err(|e| e.to_string())?.body_mut().read_to_string().map_err(|e| e.to_string())
}

/// Checks a downloaded file against SHA256SUMS.txt of its release; a file that does not match is deleted.
pub fn verify(release: &Release, file: &Path, name: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let tr = crate::i18n::tr;
    let url = release.asset("SHA256SUMS.txt").ok_or_else(|| tr("The release has no SHA256SUMS.txt – not installed.", "Vydání nemá SHA256SUMS.txt – neinstaluji.").to_string())?;
    let sums = text(url)?;
    let want = sums
        .lines()
        .find_map(|l| {
            let (hash, file) = l.trim().split_once(char::is_whitespace)?;
            file.trim().trim_start_matches('*').eq_ignore_ascii_case(name).then(|| hash.trim().to_lowercase())
        })
        .ok_or_else(|| format!("{name}: {}", tr("not in SHA256SUMS.txt", "chybí v SHA256SUMS.txt")))?;
    let mut f = std::fs::File::open(file).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).map_err(|e| e.to_string())?;
    let have: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if have != want {
        drop(f);
        let _ = std::fs::remove_file(file);
        return Err(format!("{name}: {}", tr("the checksum does not match (damaged or changed download)", "kontrolní součet nesedí (poškozené nebo změněné stažení)")));
    }
    Ok(())
}
