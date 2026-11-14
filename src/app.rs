//! The window of HeMi Launcher: the HeMi apps in cards (install, update, open, what's new, uninstall), the new version of
//! the launcher itself on top, the settings and About – laid out like HeMiPix and HeMiUnlocker (a large title, small
//! semibold section headers, cards with a frame). The work runs on other threads (watch.rs); the window only shows it.

use crate::catalog::{self, APPS, AppInfo, Installed, Kind, LAUNCHER};
use crate::i18n::{self, tr};
use crate::settings::{self, Settings};
use crate::theme::{self, Palette, glyph};
use crate::tray::{self, Event};
use crate::watch::{self, Ctx, Phase, State};
use crate::widgets::{Btn, Tone, card, info_bar, progress, row_layout, section, toggle};
use eframe::egui::{self, Align, Layout, Margin, RichText, vec2};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

const CONTACT: &str = "https://github.com/Hemal1k/HeMiLauncher/issues";
const RELEASES: &str = "https://github.com/Hemal1k/HeMiLauncher/releases";
const PROJECT: &str = "https://github.com/Hemal1k/HeMiLauncher";
const SITE: &str = "https://hemal1k.github.io";

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Apps,
    Notes(&'static str),
    Settings,
    About,
}

enum Action {
    Go(Page),
    Check,
    Install(&'static str),
    UpdateAll,
    UpdateSelf,
    Open(&'static str),
    Uninstall(&'static str),
    InstallPortable(&'static str),
    RemovePortable(&'static str),
    PickPortableDir,
    ClearPortableDir,
    Url(String),
    Licences,
    Set(Settings),
    Autostart(bool),
    DismissToast,
}

struct Toast {
    text: String,
    tone: Tone,
    until: Instant,
}

pub struct LauncherApp {
    c: Ctx,
    now: Sender<()>,
    events: Receiver<Event>,
    page: Page,
    p: Palette,
    source: theme::Source,
    theme_checked: Instant,
    hwnd: isize,
    mica: bool,
    icons: HashMap<&'static str, egui::TextureHandle>,
    /// What is installed (read from the registry again every few seconds and after each job).
    installed: HashMap<&'static str, Option<Installed>>,
    /// The portable versions in the chosen folder.
    portable: HashMap<&'static str, Option<catalog::Portable>>,
    installed_read: Instant,
    autostart: bool,
    toast: Option<Toast>,
    /// Frames left in which the window is hidden (a start with Windows).
    hide_frames: u8,
}

impl LauncherApp {
    pub fn new(cc: &eframe::CreationContext<'_>, hidden: bool) -> Self {
        let source = theme::read_source();
        let p = theme::palette(&source);
        theme::install_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx, &p);
        let hwnd = match cc.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(w)) => w.hwnd.get(),
            _ => 0,
        };
        let mica = theme::style_window(hwnd, &p, true);
        let c = Ctx { state: Arc::new(Mutex::new(State::default())), settings: Arc::new(Mutex::new(Settings::load())), egui: cc.egui_ctx.clone() };
        let (events_tx, events) = channel();
        tray::start(hwnd, cc.egui_ctx.clone(), events_tx, &watch::tip(0));
        let (now, now_rx) = channel();
        watch::start(c.clone(), now_rx);
        let mut icons = HashMap::new();
        for a in APPS.iter().chain(std::iter::once(&LAUNCHER)) {
            let img = egui::ColorImage::from_rgba_unmultiplied([64, 64], a.icon);
            icons.insert(a.id, cc.egui_ctx.load_texture(a.id, img, egui::TextureOptions::LINEAR));
        }
        let mut app = LauncherApp {
            c,
            now,
            events,
            page: Page::Apps,
            p,
            source,
            theme_checked: Instant::now(),
            hwnd,
            mica,
            icons,
            installed: HashMap::new(),
            portable: HashMap::new(),
            installed_read: Instant::now(),
            autostart: settings::autostart(),
            toast: None,
            hide_frames: if hidden { 3 } else { 0 },
        };
        app.read_installed();
        app
    }

    fn read_installed(&mut self) {
        self.installed = APPS.iter().map(|a| (a.id, catalog::installed(a))).collect();
        let dir = self.c.settings.lock().unwrap().portable();
        self.portable = APPS.iter().map(|a| (a.id, dir.as_deref().and_then(|d| catalog::portable(a, d)))).collect();
        self.installed_read = Instant::now();
    }

    fn toast(&mut self, text: impl Into<String>, tone: Tone) {
        self.toast = Some(Toast { text: text.into(), tone, until: Instant::now() + Duration::from_secs(6) });
    }

    /// Light / dark or the accent colour changed (in Windows or in HeMiDock): the new colours at once.
    fn follow_theme(&mut self, ctx: &egui::Context) {
        if self.theme_checked.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.theme_checked = Instant::now();
        let source = theme::read_source();
        if source != self.source {
            self.source = source;
            self.p = theme::palette(&source);
            theme::apply(ctx, &self.p);
            self.mica = theme::style_window(self.hwnd, &self.p, true);
        }
    }

    fn backdrop(&self) -> egui::Color32 {
        if self.mica { egui::Color32::TRANSPARENT } else { self.p.bg }
    }

    fn poll(&mut self) {
        while let Ok(e) = self.events.try_recv() {
            match e {
                Event::Open => {}
                Event::Check => {
                    let _ = self.now.send(());
                }
                Event::Quit => {}
            }
        }
        let messages: Vec<(String, bool)> = std::mem::take(&mut self.c.state.lock().unwrap().messages);
        for (text, ok) in messages {
            self.toast(text, if ok { Tone::Success } else { Tone::Critical });
            self.read_installed();
        }
        if self.installed_read.elapsed() > Duration::from_secs(3) {
            self.read_installed();
        }
        if self.toast.as_ref().is_some_and(|t| Instant::now() > t.until) {
            self.toast = None;
        }
    }

    // ------------------------------------------------------------------ the pages

    fn header(&self, ui: &mut egui::Ui, st: &State, acts: &mut Vec<Action>) {
        let p = self.p;
        ui.horizontal(|ui| {
            ui.set_min_height(40.0);
            if self.page != Page::Apps {
                if Btn::new("").icon(glyph::BACK).subtle().tip(tr("Back", "Zpět")).show(ui, &p).clicked() {
                    acts.push(Action::Go(Page::Apps));
                }
                ui.add_space(4.0);
            }
            let title = match self.page {
                Page::Apps => "HeMi Launcher".to_string(),
                Page::Notes(id) => format!("{} {}", tr("What's new –", "Co je nového –"), name_of(id)),
                Page::Settings => tr("Settings", "Nastavení").into(),
                Page::About => tr("About", "O aplikaci").into(),
            };
            ui.label(RichText::new(title).font(theme::display(28.0)).color(p.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.page != Page::Apps {
                    return;
                }
                if Btn::new("").icon(glyph::INFO).subtle().tip(tr("About", "O aplikaci")).show(ui, &p).clicked() {
                    acts.push(Action::Go(Page::About));
                }
                if Btn::new("").icon(glyph::SETTINGS).subtle().tip(tr("Settings", "Nastavení")).show(ui, &p).clicked() {
                    acts.push(Action::Go(Page::Settings));
                }
                let check = Btn::new("").icon(glyph::REFRESH).subtle().enabled(!st.checking).tip(tr("Check for updates", "Zkontrolovat aktualizace"));
                if check.show(ui, &p).clicked() {
                    acts.push(Action::Check);
                }
            });
        });
    }

    /// The state of the updates in one line, with "Update all".
    fn summary(&self, ui: &mut egui::Ui, st: &State, acts: &mut Vec<Action>) {
        let p = self.p;
        let ups = self.app_updates(st);
        card(ui, &p, |ui| {
            row_layout(ui, 44.0, |ui| {
                let (g, color, text) = if st.checking {
                    (glyph::REFRESH, p.accent_text, tr("Checking for updates…", "Hledám aktualizace…").to_string())
                } else if !ups.is_empty() {
                    (glyph::DOWNLOAD, p.accent_text, updates_text(ups.len()))
                } else if let Some(e) = &st.check_error {
                    (glyph::WARNING, p.caution, format!("{} – {}", tr("Could not check for updates", "Aktualizace se nepodařilo zjistit"), short(e)))
                } else if st.checked.is_some() {
                    (glyph::COMPLETED, p.success, tr("Everything is up to date", "Vše je aktuální").to_string())
                } else {
                    (glyph::REFRESH, p.text2, tr("Not checked yet", "Zatím nezkontrolováno").to_string())
                };
                if st.checking {
                    let (r, _) = ui.allocate_exact_size(vec2(32.0, 32.0), egui::Sense::hover());
                    egui::Spinner::new().size(18.0).color(color).paint_at(ui, egui::Rect::from_center_size(r.center(), vec2(18.0, 18.0)));
                } else {
                    crate::widgets::glyph_box(ui, g, 18.0, color);
                }
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(RichText::new(text).size(14.0).color(p.text));
                    if let Some(t) = st.checked {
                        ui.label(RichText::new(ago(t)).size(12.0).color(p.text2));
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let busy = !st.busy.is_empty();
                    if ups.len() > 1 && Btn::new(tr("Update all", "Aktualizovat vše")).accent().enabled(!busy).show(ui, &p).clicked() {
                        acts.push(Action::UpdateAll);
                    }
                });
            });
        });
    }

    /// The version on this PC: installed, else portable.
    fn current(&self, id: &str) -> Option<String> {
        match (self.installed.get(id), self.portable.get(id)) {
            (Some(Some(i)), _) => Some(i.version.clone()),
            (_, Some(Some(p))) => Some(p.version.clone()),
            _ => None,
        }
    }

    /// The apps on this PC (installed or portable) with a newer version (not the launcher).
    fn app_updates(&self, st: &State) -> Vec<&'static str> {
        APPS.iter()
            .filter(|a| {
                let Some(v) = self.current(a.id) else { return false };
                st.releases.get(a.id).is_some_and(|r| catalog::newer(&r.version, &v))
            })
            .map(|a| a.id)
            .collect()
    }

    /// A new version of the launcher itself.
    fn launcher_bar(&self, ui: &mut egui::Ui, st: &State, acts: &mut Vec<Action>) {
        let p = self.p;
        let Some(r) = st.releases.get(LAUNCHER.id) else { return };
        if !catalog::newer(&r.version, env!("CARGO_PKG_VERSION")) {
            return;
        }
        egui::Frame::new()
            .fill(p.info_bg)
            .stroke(egui::Stroke::new(1.0, p.card_stroke))
            .corner_radius(crate::widgets::CARD_RADIUS)
            .inner_margin(Margin { left: 16, right: 14, top: 10, bottom: 10 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                row_layout(ui, 44.0, |ui| {
                    self.icon(ui, LAUNCHER.id, 32.0);
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(RichText::new(format!("{} {}", tr("HeMi Launcher – a new version", "HeMi Launcher – nová verze"), r.version)).font(theme::semibold(14.0)).color(p.text));
                        ui.label(RichText::new(tr("It restarts after the update.", "Po aktualizaci se znovu spustí.")).size(12.0).color(p.text2));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(phase) = st.busy.get(LAUNCHER.id) {
                            phase_ui(ui, &p, phase);
                        } else {
                            if Btn::new(tr("Update", "Aktualizovat")).accent().show(ui, &p).clicked() {
                                acts.push(Action::UpdateSelf);
                            }
                            if Btn::new("").icon(glyph::NOTES).subtle().tip(tr("What's new", "Co je nového")).show(ui, &p).clicked() {
                                acts.push(Action::Go(Page::Notes(LAUNCHER.id)));
                            }
                        }
                    });
                });
            });
        ui.add_space(8.0);
    }

    fn icon(&self, ui: &mut egui::Ui, id: &str, size: f32) {
        if let Some(t) = self.icons.get(id) {
            ui.add(egui::Image::new(t).fit_to_exact_size(vec2(size, size)));
        }
    }

    fn apps_page(&self, ui: &mut egui::Ui, st: &State, acts: &mut Vec<Action>) {
        self.launcher_bar(ui, st, acts);
        self.summary(ui, st, acts);
        ui.add_space(14.0);
        section(ui, &self.p, tr("Apps", "Aplikace"));
        ui.spacing_mut().item_spacing.y = 4.0;
        for a in APPS {
            self.app_row(ui, a, st, acts);
        }
        ui.add_space(14.0);
        let p = self.p;
        card(ui, &p, |ui| {
            row_layout(ui, 36.0, |ui| {
                ui.label(RichText::new(tr("All apps also on the web", "Všechny aplikace i na webu")).size(14.0).color(p.text));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if Btn::new(tr("Open", "Otevřít")).tip(SITE).show(ui, &p).clicked() {
                        acts.push(Action::Url(SITE.into()));
                    }
                });
            });
        });
    }

    fn app_row(&self, ui: &mut egui::Ui, a: &'static AppInfo, st: &State, acts: &mut Vec<Action>) {
        let p = self.p;
        let installed = self.installed.get(a.id).cloned().flatten();
        // the portable version counts only when the app is not installed
        let portable = if installed.is_none() { self.portable.get(a.id).cloned().flatten() } else { None };
        let have = installed.as_ref().map(|i| i.version.clone()).or_else(|| portable.as_ref().map(|p| p.version.clone()));
        let portable_on = self.c.settings.lock().unwrap().portable().is_some();
        let release = st.releases.get(a.id);
        let update = match (&have, release) {
            (Some(v), Some(r)) => catalog::newer(&r.version, v),
            _ => false,
        };
        card(ui, &p, |ui| {
            row_layout(ui, 52.0, |ui| {
                self.icon(ui, a.id, 40.0);
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(phase) = st.busy.get(a.id) {
                        phase_ui(ui, &p, phase);
                    } else {
                        // the main button
                        if have.is_some() && update {
                            if Btn::new(tr("Update", "Aktualizovat")).accent().show(ui, &p).clicked() {
                                acts.push(if portable.is_some() { Action::InstallPortable(a.id) } else { Action::Install(a.id) });
                            }
                        } else if have.is_some() {
                            if Btn::new(tr("Open", "Otevřít")).show(ui, &p).clicked() {
                                acts.push(Action::Open(a.id));
                            }
                        } else if a.kind == Kind::Files {
                            let b = Btn::new(tr("Download", "Stáhnout")).icon(glyph::DOWNLOAD).enabled(release.is_some());
                            if b.tip(tr("The skins into the Downloads folder", "Vzhledy do složky Stažené soubory")).show(ui, &p).clicked() {
                                acts.push(Action::Install(a.id));
                            }
                        } else {
                            if Btn::new(tr("Install", "Nainstalovat")).accent().enabled(release.is_some()).show(ui, &p).clicked() {
                                acts.push(Action::Install(a.id));
                            }
                            // the portable version into the chosen folder (only with a folder in the settings)
                            if portable_on {
                                let tip = tr("The portable version into your folder", "Portable verze do tvé složky");
                                if Btn::new("Portable").enabled(release.is_some()).tip(tip).show(ui, &p).clicked() {
                                    acts.push(Action::InstallPortable(a.id));
                                }
                            }
                        }
                        // the smaller ones
                        if installed.is_some()
                            && Btn::new("").icon(glyph::DELETE).subtle().tip(tr("Uninstall", "Odinstalovat")).show(ui, &p).clicked()
                        {
                            acts.push(Action::Uninstall(a.id));
                        }
                        if portable.is_some()
                            && Btn::new("").icon(glyph::DELETE).subtle().tip(tr("Remove the portable version", "Odstranit portable verzi")).show(ui, &p).clicked()
                        {
                            acts.push(Action::RemovePortable(a.id));
                        }
                        if Btn::new("").icon(glyph::GLOBE).subtle().tip(tr("Project on GitHub", "Projekt na GitHubu")).show(ui, &p).clicked() {
                            acts.push(Action::Url(format!("https://github.com/Hemal1k/{}", a.id)));
                        }
                        if release.is_some() && Btn::new("").icon(glyph::NOTES).subtle().tip(tr("What's new", "Co je nového")).show(ui, &p).clicked() {
                            acts.push(Action::Go(Page::Notes(a.id)));
                        }
                    }
                    ui.add_space(8.0);
                    // the versions
                    let (text, color) = match (&have, release) {
                        (Some(v), Some(r)) if update => (format!("{v} → {}", r.version), p.accent_text),
                        (Some(v), _) => (v.clone(), p.text2),
                        (None, Some(r)) => (r.version.clone(), p.text3),
                        (None, None) => (String::new(), p.text3),
                    };
                    ui.label(RichText::new(text).size(13.0).color(color));
                    ui.add_space(8.0);
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(a.name).font(theme::semibold(15.0)).color(p.text));
                                let who = match (&installed, &portable) {
                                    (Some(i), _) if i.all_users => Some(tr("all users", "všichni uživatelé")),
                                    (Some(_), _) => Some(tr("installed", "nainstalováno")),
                                    (None, Some(_)) => Some("portable"),
                                    _ => None,
                                };
                                if let Some(who) = who {
                                    ui.label(RichText::new(who).size(11.0).color(p.text3));
                                }
                            });
                            let (line, color) = match st.errors.get(a.id) {
                                Some(e) => (short(e), p.critical),
                                None => (a.line().to_string(), p.text2),
                            };
                            ui.add(egui::Label::new(RichText::new(line).size(12.0).color(color)).truncate());
                        });
                    });
                });
            });
        });
    }

    fn notes_page(&self, ui: &mut egui::Ui, id: &'static str, st: &State, acts: &mut Vec<Action>) {
        let p = self.p;
        let Some(r) = st.releases.get(id) else {
            ui.label(RichText::new(tr("Not known yet.", "Zatím neznámé.")).color(p.text2));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} {}", tr("Version", "Verze"), r.version)).font(theme::semibold(14.0)).color(p.text));
            ui.label(RichText::new(&r.date).size(13.0).color(p.text2));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if Btn::new(tr("On GitHub", "Na GitHubu")).icon(glyph::GLOBE).show(ui, &p).clicked() {
                    acts.push(Action::Url(r.page.clone()));
                }
            });
        });
        ui.add_space(8.0);
        card(ui, &p, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for line in r.notes.lines() {
                let t = line.trim_end();
                if let Some(h) = t.strip_prefix('#') {
                    ui.add_space(4.0);
                    ui.label(RichText::new(h.trim_start_matches('#').trim()).font(theme::semibold(14.0)).color(p.text));
                } else if let Some(item) = t.trim_start().strip_prefix("- ").or_else(|| t.trim_start().strip_prefix("* ")) {
                    let indent = (t.len() - t.trim_start().len()) as f32 * 6.0;
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(indent);
                        ui.label(RichText::new(format!("•  {}", plain(item))).size(13.0).color(p.text));
                    });
                } else if !t.trim().is_empty() {
                    ui.label(RichText::new(plain(t)).size(13.0).color(p.text));
                }
            }
        });
    }

    fn settings_page(&self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let s = self.c.settings.lock().unwrap().clone();
        ui.spacing_mut().item_spacing.y = 4.0;
        let row = |ui: &mut egui::Ui, title: &str, detail: &str, on: bool| -> bool {
            let mut clicked = false;
            card(ui, &p, |ui| {
                row_layout(ui, 48.0, |ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        ui.label(RichText::new(title).size(14.0).color(p.text));
                        if !detail.is_empty() {
                            ui.label(RichText::new(detail).size(12.0).color(p.text2));
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(if on { tr("On", "Zapnuto") } else { tr("Off", "Vypnuto") }).size(14.0).color(p.text));
                        clicked = toggle(ui, &p, on).clicked();
                    });
                });
            });
            clicked
        };
        section(ui, &p, tr("Updates", "Aktualizace"));
        if row(ui, tr("Check for updates automatically", "Hledat aktualizace automaticky"), tr("After the start and every 4 hours", "Po spuštění a každé 4 hodiny"), s.auto_check) {
            acts.push(Action::Set(Settings { auto_check: !s.auto_check, ..s.clone() }));
        }
        if row(ui, tr("Install updates by itself", "Instalovat aktualizace samo"), tr("Quietly in the background", "Potichu na pozadí"), s.auto_install) {
            acts.push(Action::Set(Settings { auto_install: !s.auto_install, ..s.clone() }));
        }
        if row(ui, tr("Notify about new versions", "Upozornit na nové verze"), tr("A notification of Windows", "Oznámení Windows"), s.notify) {
            acts.push(Action::Set(Settings { notify: !s.notify, ..s.clone() }));
        }
        ui.add_space(14.0);
        section(ui, &p, tr("Start", "Spouštění"));
        if row(ui, tr("Start with Windows", "Spouštět s Windows"), tr("Hidden in the notification area", "Skrytě v oznamovací oblasti"), self.autostart) {
            acts.push(Action::Autostart(!self.autostart));
        }
        if row(ui, tr("Closing hides it in the notification area", "Zavřením se skryje do oznamovací oblasti"), tr("It keeps checking for updates", "Dál hlídá aktualizace"), s.tray_on_close) {
            acts.push(Action::Set(Settings { tray_on_close: !s.tray_on_close, ..s.clone() }));
        }
        ui.add_space(14.0);
        section(ui, &p, tr("Portable versions", "Portable verze"));
        card(ui, &p, |ui| {
            row_layout(ui, 48.0, |ui| {
                // the Downloads folder unless another one is chosen
                let detail = s.portable().map(|d| d.display().to_string()).unwrap_or_default();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !s.portable_dir.trim().is_empty() && Btn::new(tr("Downloads", "Stažené soubory")).tip(tr("Back to the Downloads folder", "Zpět do složky Stažené soubory")).show(ui, &p).clicked() {
                        acts.push(Action::ClearPortableDir);
                    }
                    if Btn::new(tr("Choose…", "Vybrat…")).show(ui, &p).clicked() {
                        acts.push(Action::PickPortableDir);
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(RichText::new(tr("Folder for portable versions", "Složka pro portable verze")).size(14.0).color(p.text));
                            ui.add(egui::Label::new(RichText::new(detail).size(12.0).color(p.text2)).truncate());
                        });
                    });
                });
            });
        });
    }

    fn about_page(&self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        section(ui, &p, "HeMi Launcher");
        ui.spacing_mut().item_spacing.y = 4.0;
        let value = |ui: &mut egui::Ui, label: &str, value: &str| {
            card(ui, &p, |ui| {
                row_layout(ui, 44.0, |ui| {
                    ui.label(RichText::new(label).size(14.0).color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(value).size(14.0).color(p.text2));
                    });
                });
            });
        };
        value(ui, tr("Author", "Autor"), "Richard Hemala");
        value(ui, tr("Version", "Verze"), &format!("Beta {}", env!("CARGO_PKG_VERSION")));
        value(ui, tr("Build date", "Datum sestavení"), &i18n::build_date());
        let open = |ui: &mut egui::Ui, title: &str, tip: &str, act: Action, acts: &mut Vec<Action>| {
            card(ui, &p, |ui| {
                row_layout(ui, 44.0, |ui| {
                    ui.label(RichText::new(title).size(14.0).color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if Btn::new(tr("Open", "Otevřít")).tip(tip).show(ui, &p).clicked() {
                            acts.push(act);
                        }
                    });
                });
            });
        };
        open(ui, tr("Project on GitHub", "Projekt na GitHubu"), PROJECT, Action::Url(PROJECT.into()), acts);
        open(ui, tr("What's new", "Co je nového"), RELEASES, Action::Url(RELEASES.into()), acts);
        open(ui, tr("Report a problem", "Nahlásit problém"), CONTACT, Action::Url(CONTACT.into()), acts);
        open(ui, tr("Licences", "Licence"), "", Action::Licences, acts);
    }

    fn act(&mut self, acts: Vec<Action>) {
        for a in acts {
            match a {
                Action::Go(page) => self.page = page,
                Action::Check => {
                    let _ = self.now.send(());
                }
                Action::Install(id) => self.c.spawn(move |c| c.install(id, false)),
                Action::UpdateAll => {
                    let st = self.c.state.lock().unwrap();
                    // (id, only the portable version is on this PC)
                    let ids: Vec<(&'static str, bool)> =
                        self.app_updates(&st).into_iter().map(|id| (id, !matches!(self.installed.get(id), Some(Some(_))))).collect();
                    drop(st);
                    self.c.spawn(move |c| {
                        for (id, portable) in ids {
                            if portable {
                                c.install_portable(id, false);
                            } else {
                                c.install(id, false);
                            }
                        }
                    });
                }
                Action::UpdateSelf => self.c.spawn(|c| c.update_self()),
                Action::Open(id) => {
                    if let Some(app) = crate::jobs::find(id) {
                        let exe = match (self.installed.get(id), self.portable.get(id)) {
                            (Some(Some(i)), _) => Some(i.exe(app)),
                            (_, Some(Some(p))) => Some(p.exe(app)),
                            _ => None,
                        };
                        if let Some(exe) = exe {
                            crate::jobs::open(&exe);
                        }
                    }
                }
                Action::InstallPortable(id) => self.c.spawn(move |c| c.install_portable(id, false)),
                Action::RemovePortable(id) => self.c.spawn(move |c| c.remove_portable(id)),
                Action::PickPortableDir => {
                    let start = self.c.settings.lock().unwrap().portable();
                    let mut d = rfd::FileDialog::new().set_title(tr("Folder for portable versions", "Složka pro portable verze"));
                    if let Some(s) = start.filter(|s| s.is_dir()) {
                        d = d.set_directory(s);
                    }
                    if let Some(dir) = d.pick_folder() {
                        let s = Settings { portable_dir: dir.display().to_string(), ..self.c.settings.lock().unwrap().clone() };
                        s.save();
                        *self.c.settings.lock().unwrap() = s;
                        self.read_installed();
                        self.c.spawn(|c| {
                            c.refresh_updates();
                        });
                    }
                }
                Action::ClearPortableDir => {
                    let s = Settings { portable_dir: String::new(), ..self.c.settings.lock().unwrap().clone() };
                    s.save();
                    *self.c.settings.lock().unwrap() = s;
                    self.read_installed();
                    self.c.spawn(|c| {
                        c.refresh_updates();
                    });
                }
                Action::Uninstall(id) => self.c.spawn(move |c| c.uninstall(id)),
                Action::Url(u) => shell_open(&u),
                Action::Licences => {
                    let file = std::env::temp_dir().join("HeMiLauncher-ThirdPartyNotices.txt");
                    match std::fs::write(&file, include_str!("../assets/ThirdPartyNotices.txt")) {
                        Ok(()) => shell_open(&file.display().to_string()),
                        Err(e) => self.toast(e.to_string(), Tone::Critical),
                    }
                }
                Action::Set(s) => {
                    s.save();
                    let turned_on = s.auto_check && !self.c.settings.lock().unwrap().auto_check;
                    *self.c.settings.lock().unwrap() = s;
                    if turned_on {
                        let _ = self.now.send(());
                    }
                }
                Action::Autostart(on) => {
                    settings::set_autostart(on);
                    self.autostart = settings::autostart();
                }
                Action::DismissToast => self.toast = None,
            }
        }
    }
}

impl eframe::App for LauncherApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll();
        // started hidden: eframe shows the window after its first frames, so it is hidden again then
        if self.hide_frames > 0 {
            self.hide_frames -= 1;
            tray::hide_main();
            ctx.request_repaint();
        }
        self.follow_theme(&ctx);
        ctx.request_repaint_after(Duration::from_millis(1000));

        // the × hides the window into the notification area (when set so); Quit of the icon really closes it
        if ctx.input(|i| i.viewport().close_requested()) {
            let s = self.c.settings.lock().unwrap().clone();
            if s.tray_on_close && !tray::QUITTING.load(std::sync::atomic::Ordering::Relaxed) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                tray::hide_main();
                self.page = Page::Apps;
            } else {
                tray::remove();
            }
        }

        // a copy of the state for this frame (the threads keep working meanwhile)
        let st = {
            let s = self.c.state.lock().unwrap();
            State {
                releases: s.releases.clone(),
                check_error: s.check_error.clone(),
                checking: s.checking,
                checked: s.checked,
                busy: s.busy.clone(),
                errors: s.errors.clone(),
                ..Default::default()
            }
        };
        if !st.busy.is_empty() || st.checking {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        let bg = self.backdrop();
        let mut acts = Vec::new();
        egui::Panel::top("header")
            .frame(egui::Frame::new().fill(bg).inner_margin(Margin { left: 28, right: 28, top: 14, bottom: 14 }))
            .show_separator_line(false)
            .show(ui, |ui| self.header(ui, &st, &mut acts));
        if let Some(t) = &self.toast {
            let (text, tone) = (t.text.clone(), t.tone);
            egui::Panel::bottom("toast")
                .frame(egui::Frame::new().fill(bg).inner_margin(Margin { left: 28, right: 28, top: 6, bottom: 14 }))
                .show_separator_line(false)
                .show(ui, |ui| {
                    if info_bar(ui, &self.p, tone, &text, Some(())) {
                        acts.push(Action::DismissToast);
                    }
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(bg).inner_margin(Margin { left: 28, right: 28, top: 0, bottom: 4 }))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    egui::Frame::new().inner_margin(Margin { left: 0, right: 8, top: 0, bottom: 12 }).show(ui, |ui| match self.page {
                        Page::Apps => self.apps_page(ui, &st, &mut acts),
                        Page::Notes(id) => self.notes_page(ui, id, &st, &mut acts),
                        Page::Settings => self.settings_page(ui, &mut acts),
                        Page::About => self.about_page(ui, &mut acts),
                    });
                });
            });
        self.act(acts);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.backdrop().to_normalized_gamma_f32()
    }
}

// ---------------------------------------------------------------------- helpers

/// Downloading (with its share) or installing, in place of the buttons.
fn phase_ui(ui: &mut egui::Ui, p: &Palette, phase: &Phase) {
    match phase {
        Phase::Download(f) => {
            if *f >= 0.0 {
                ui.label(RichText::new(format!("{:.0} %", f * 100.0)).size(13.0).color(p.text2));
                ui.add_space(4.0);
                progress(ui, p, *f, 120.0);
            } else {
                ui.add(egui::Spinner::new().size(16.0).color(p.accent_text));
            }
            ui.label(RichText::new(tr("Downloading", "Stahuji")).size(13.0).color(p.accent_text));
        }
        Phase::Install => {
            ui.add(egui::Spinner::new().size(16.0).color(p.accent_text));
            ui.label(RichText::new(tr("Installing…", "Instaluji…")).size(13.0).color(p.accent_text));
        }
    }
}

fn name_of(id: &str) -> &'static str {
    crate::jobs::find(id).map(|a| a.name).unwrap_or("")
}

fn updates_text(n: usize) -> String {
    if i18n::cz() {
        match n {
            1 => "1 aktualizace k dispozici".into(),
            _ => format!("{n} aktualizace k dispozici"),
        }
    } else if n == 1 {
        "1 update available".into()
    } else {
        format!("{n} updates available")
    }
}

/// "Checked just now / 5 min ago / 2 h ago".
fn ago(t: SystemTime) -> String {
    let s = SystemTime::now().duration_since(t).map(|d| d.as_secs()).unwrap_or(0);
    let amount = match s {
        0..60 => return tr("Checked just now", "Zkontrolováno právě teď").into(),
        60..3600 => format!("{} min", s / 60),
        _ => format!("{} h", s / 3600),
    };
    if i18n::cz() { format!("Zkontrolováno před {amount}") } else { format!("Checked {amount} ago") }
}

/// The first line of an error, not too long.
fn short(e: &str) -> String {
    let line = e.lines().next().unwrap_or("");
    if line.chars().count() > 90 { format!("{}…", line.chars().take(90).collect::<String>()) } else { line.to_string() }
}

/// Markdown of the release notes as plain text: no ** and `, links as their text.
fn plain(t: &str) -> String {
    let mut s = t.replace("**", "").replace('`', "");
    // [text](url) → text
    while let (Some(a), Some(b)) = (s.find("]("), s.find('[')) {
        let Some(c) = s[a..].find(')') else { break };
        if b > a {
            break;
        }
        s = format!("{}{}{}", &s[..b], &s[b + 1..a], &s[a + c + 1..]);
    }
    s
}

fn shell_open(target: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let (verb, t) = (crate::reg::wide("open"), crate::reg::wide(target));
    unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), t.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
}
