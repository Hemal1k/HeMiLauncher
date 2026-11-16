//! The state all threads of the launcher share (the window reads it at each frame) and the watcher: it looks for new
//! versions after the start and every few hours – also with the window hidden, when egui does not run –, writes
//! updates.txt for the other HeMi apps, tells about new versions with a notification and installs them when asked.

use crate::catalog::{self, LAUNCHER};
use crate::github::Release;
use crate::i18n::tr;
use crate::jobs::{self, Releases};
use crate::settings::Settings;
use eframe::egui;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// How often the watcher looks.
const EVERY: Duration = Duration::from_secs(4 * 60 * 60);

#[derive(Clone, Debug)]
pub enum Phase {
    /// Downloading (0–1, or below 0 when the size is not known).
    Download(f32),
    /// The installer (or the uninstaller) runs.
    Install,
}

#[derive(Default)]
pub struct State {
    pub releases: HashMap<&'static str, Release>,
    /// What could not be read (no internet, GitHub limits…).
    pub check_error: Option<String>,
    pub checking: bool,
    pub checked: Option<SystemTime>,
    pub busy: HashMap<&'static str, Phase>,
    /// The last error of an app (shown in its row until the next try).
    pub errors: HashMap<&'static str, String>,
    /// Short messages for the window (shown, then cleared).
    pub messages: Vec<(String, bool)>,
    /// The launcher updated itself: the process ends.
    pub quit: bool,
    /// The updates last told about by a notification (not again for the same versions).
    pub told: Vec<(&'static str, String)>,
}

pub type Shared = Arc<Mutex<State>>;

pub struct Ctx {
    pub state: Shared,
    pub settings: Arc<Mutex<Settings>>,
    pub egui: egui::Context,
}

impl Clone for Ctx {
    fn clone(&self) -> Self {
        Ctx { state: self.state.clone(), settings: self.settings.clone(), egui: self.egui.clone() }
    }
}

impl Ctx {
    pub fn repaint(&self) {
        self.egui.request_repaint();
    }

    fn message(&self, text: String, ok: bool) {
        self.state.lock().unwrap().messages.push((text, ok));
        self.repaint();
    }

    /// The updates, by what is installed now (also to updates.txt and the tooltip of the icon).
    pub fn refresh_updates(&self) -> Vec<(&'static str, String)> {
        let releases: Releases = {
            let st = self.state.lock().unwrap();
            st.releases.iter().map(|(id, r)| (*id, Ok(r.clone()))).collect()
        };
        let dir = self.settings.lock().unwrap().portable();
        let ups = jobs::updates(&releases, dir.as_deref());
        crate::notify::write_updates(&ups);
        crate::tray::set_tip(&tip(ups.len()));
        ups
    }

    /// Looks for new versions now.
    pub fn check(&self, by_user: bool) {
        {
            let mut st = self.state.lock().unwrap();
            if st.checking {
                return;
            }
            st.checking = true;
        }
        self.repaint();
        let all = jobs::check(by_user);
        let mut failed = None;
        {
            let mut st = self.state.lock().unwrap();
            for (id, r) in all {
                match r {
                    Ok(r) => {
                        st.releases.insert(id, r);
                    }
                    // the launcher without a release yet (or GitHub not reachable for it alone) is no error for the apps
                    Err(e) if id != LAUNCHER.id => failed = Some(e),
                    Err(_) => {}
                }
            }
            st.check_error = failed.clone();
            st.checking = false;
            st.checked = Some(SystemTime::now());
        }
        let ups = self.refresh_updates();
        self.repaint();
        if by_user {
            return;
        }
        let s = self.settings.lock().unwrap().clone();
        // only versions not told about yet
        let new = {
            let mut st = self.state.lock().unwrap();
            let new: Vec<_> = ups.iter().filter(|u| !st.told.contains(u)).cloned().collect();
            st.told = ups.clone();
            new
        };
        if s.auto_install {
            // the apps first, the launcher last (it ends then)
            for (id, _) in ups.iter().filter(|(id, _)| *id != LAUNCHER.id) {
                // installed by its installer: quietly with it; else the portable version
                match jobs::find(id) {
                    Some(app) if catalog::installed(app).is_none() => self.install_portable(id, true),
                    _ => self.install(id, true),
                }
            }
            if ups.iter().any(|(id, _)| *id == LAUNCHER.id) {
                self.update_self();
            }
        } else if s.notify && !new.is_empty() {
            let names: Vec<&str> = new.iter().filter_map(|(id, _)| jobs::find(id)).map(|a| a.name).collect();
            let title = if new.len() == 1 { tr("A new version", "Nová verze").to_string() } else { format!("{} {}", new.len(), tr("new versions", "nových verzí")) };
            crate::notify::toast(&title, &names.join(", "));
        }
    }

    /// Installs or updates an app with its installer (on this thread). `quiet`: an update in the background, no
    /// message on success.
    pub fn install(&self, id: &'static str, quiet: bool) {
        self.job(id, quiet, false);
    }

    /// The portable version into the chosen folder (new or updated).
    pub fn install_portable(&self, id: &'static str, quiet: bool) {
        self.job(id, quiet, true);
    }

    fn job(&self, id: &'static str, quiet: bool, portable: bool) {
        let Some(app) = jobs::find(id) else { return };
        let release = self.state.lock().unwrap().releases.get(id).cloned();
        let Some(release) = release else { return };
        {
            let mut st = self.state.lock().unwrap();
            if st.busy.contains_key(id) {
                return;
            }
            st.busy.insert(id, Phase::Download(0.0));
            st.errors.remove(id);
        }
        self.repaint();
        let progress = |f: f32| {
            let mut st = self.state.lock().unwrap();
            // the installer itself after the download
            st.busy.insert(id, if f >= 1.0 { Phase::Install } else { Phase::Download(f) });
            drop(st);
            self.repaint();
        };
        let dir = self.settings.lock().unwrap().portable();
        let before = catalog::installed(app);
        let had = dir.as_deref().and_then(|d| catalog::portable(app, d));
        let result = match app.kind {
            catalog::Kind::App if portable => match &dir {
                Some(d) => jobs::install_portable(app, &release, d, progress).map(|()| {
                    if had.is_some() {
                        format!("{} {} {} (portable)", app.name, tr("updated to", "aktualizován na"), release.version)
                    } else {
                        format!("{} – {} {}", app.name, tr("portable version in", "portable verze v"), d.join(app.id).display())
                    }
                }),
                None => Err(tr("Choose a folder for portable versions in the settings.", "V nastavení vyber složku pro portable verze.").into()),
            },
            catalog::Kind::App => jobs::install(app, &release, before.as_ref(), progress).map(|()| {
                if before.is_some() {
                    format!("{} {} {}", app.name, tr("updated to", "aktualizován na"), release.version)
                } else {
                    format!("{} {}", app.name, tr("installed", "nainstalován"))
                }
            }),
            catalog::Kind::Tool => jobs::download_tool(app, &release, progress).map(|f| format!("{} – {}", app.name, f.display())),
            catalog::Kind::Files => jobs::download_files(app, &release, progress).map(|_| format!("{} – {}", app.name, tr("downloaded into Downloads", "staženo do složky Stažené soubory"))),
        };
        self.state.lock().unwrap().busy.remove(id);
        match result {
            Ok(text) => {
                if !quiet {
                    self.message(text, true);
                } else if self.settings.lock().unwrap().notify {
                    crate::notify::toast(&text, "");
                }
            }
            Err(e) if e == jobs::CANCELLED => {}
            Err(e) => {
                self.state.lock().unwrap().errors.insert(id, e.clone());
                self.message(format!("{}: {e}", app.name), false);
            }
        }
        self.refresh_updates();
        self.repaint();
    }

    /// Removes the portable version of an app.
    pub fn remove_portable(&self, id: &'static str) {
        let Some(app) = jobs::find(id) else { return };
        let Some(dir) = self.settings.lock().unwrap().portable() else { return };
        if let Err(e) = jobs::remove_portable(app, &dir) {
            self.message(format!("{}: {e}", app.name), false);
        }
        self.refresh_updates();
        self.repaint();
    }

    pub fn uninstall(&self, id: &'static str) {
        let Some(app) = jobs::find(id) else { return };
        let Some(i) = catalog::installed(app) else { return };
        self.state.lock().unwrap().busy.insert(id, Phase::Install);
        self.repaint();
        let r = jobs::uninstall(&i);
        self.state.lock().unwrap().busy.remove(id);
        if let Err(e) = r {
            self.message(format!("{}: {e}", app.name), false);
        }
        self.refresh_updates();
        self.repaint();
    }

    /// The new version of the launcher; this one ends when it is on the way.
    pub fn update_self(&self) {
        let release = self.state.lock().unwrap().releases.get(LAUNCHER.id).cloned();
        let Some(release) = release else { return };
        self.state.lock().unwrap().busy.insert(LAUNCHER.id, Phase::Download(0.0));
        self.repaint();
        let r = jobs::update_self(&release, |f| {
            self.state.lock().unwrap().busy.insert(LAUNCHER.id, if f >= 1.0 { Phase::Install } else { Phase::Download(f) });
            self.repaint();
        });
        let mut st = self.state.lock().unwrap();
        st.busy.remove(LAUNCHER.id);
        match r {
            Ok(()) => {
                st.quit = true;
                drop(st);
                crate::tray::remove();
                std::process::exit(0);
            }
            Err(e) => {
                st.errors.insert(LAUNCHER.id, e.clone());
                st.messages.push((format!("HeMi Launcher: {e}"), false));
            }
        }
        drop(st);
        self.repaint();
    }

    /// Runs `f` on its own thread.
    pub fn spawn(&self, f: impl FnOnce(&Ctx) + Send + 'static) {
        let c = self.clone();
        std::thread::spawn(move || f(&c));
    }
}

/// The text of the icon in the notification area.
pub fn tip(updates: usize) -> String {
    match updates {
        0 => "HeMi Launcher".into(),
        1 => format!("HeMi Launcher – {}", tr("1 update", "1 aktualizace")),
        n => format!("HeMi Launcher – {n} {}", if crate::i18n::cz() && n < 5 { "aktualizace" } else { tr("updates", "aktualizací") }),
    }
}

/// The watcher: looks after the start (when allowed) and every few hours; `now` asks it to look at once.
pub fn start(c: Ctx, now: Receiver<()>) {
    std::thread::spawn(move || {
        // the network after the start of Windows needs a moment
        let mut next = Instant::now() + Duration::from_secs(5);
        loop {
            let wait = next.saturating_duration_since(Instant::now());
            match now.recv_timeout(wait) {
                Ok(()) => {
                    c.check(true);
                    next = Instant::now() + EVERY;
                }
                Err(RecvTimeoutError::Timeout) => {
                    if c.settings.lock().unwrap().auto_check {
                        c.check(false);
                    }
                    next = Instant::now() + EVERY;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}
