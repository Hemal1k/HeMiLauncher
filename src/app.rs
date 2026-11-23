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
}

enum Action {
    /// The theme or the language changed.
    SetLook(crate::look::Look),
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
    /// the notes of a new version unfolded or folded in its card
    ToggleNotes(&'static str),
    /// HeMi Dark: this colour and size set in VLC
    VlcSkin(String, u32),
}

struct Toast {
    text: String,
    tone: Tone,
    until: Instant,
}

pub struct LauncherApp {
    c: Ctx,
    /// the section "Other" (HeMi Dark) unfolded – folded at every start
    other_open: std::cell::Cell<bool>,
    now: Sender<()>,
    events: Receiver<Event>,
    page: Page,
    p: Palette,
    source: theme::Source,
    theme_checked: Instant,
    hwnd: isize,
    /// The theme and the language chosen in the settings.
    look: crate::look::Look,
    mica: bool,
    icons: HashMap<&'static str, egui::TextureHandle>,
    /// What is installed (read from the registry again every few seconds and after each job).
    installed: HashMap<&'static str, Option<Installed>>,
    /// The portable versions in the chosen folder.
    portable: HashMap<&'static str, Option<catalog::Portable>>,
    /// HeMi Dark: what there is to choose and what VLC uses now (read with the installed apps)
    vlc_choices: (Vec<&'static str>, Vec<u32>),
    vlc_now: Option<(String, u32)>,
    installed_read: Instant,
    autostart: bool,
    toast: Option<Toast>,
    /// Frames left in which the window is hidden (a start with Windows).
    hide_frames: u8,
    /// the cards whose notes of the new version are unfolded
    notes_open: std::collections::HashSet<&'static str>,
}

impl LauncherApp {
    pub fn new(cc: &eframe::CreationContext<'_>, hidden: bool) -> Self {
        let look = crate::look::Look::load();
        crate::i18n::set(look.language);
        let source = theme::read_source(look.theme);
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
        // the GitHub mark (white, tinted when drawn)
        let img = egui::ColorImage::from_rgba_unmultiplied([64, 64], include_bytes!("../assets/github-64.rgba"));
        icons.insert("github", cc.egui_ctx.load_texture("github", img, egui::TextureOptions::LINEAR));
        let mut app = LauncherApp {
            other_open: std::cell::Cell::new(false),
            c,
            now,
            events,
            page: Page::Apps,
            p,
            source,
            theme_checked: Instant::now(),
            hwnd,
            look,
            mica,
            icons,
            installed: HashMap::new(),
            portable: HashMap::new(),
            vlc_choices: (Vec::new(), Vec::new()),
            vlc_now: None,
            installed_read: Instant::now(),
            autostart: settings::autostart(),
            toast: None,
            hide_frames: if hidden { 3 } else { 0 },
            notes_open: Default::default(),
        };
        app.read_installed();
        app
    }

    fn read_installed(&mut self) {
        self.installed = APPS.iter().map(|a| (a.id, catalog::installed(a))).collect();
        let dir = self.c.settings.lock().unwrap().portable();
        self.portable = APPS.iter().map(|a| (a.id, dir.as_deref().and_then(|d| catalog::portable(a, d)))).collect();
        self.vlc_choices = crate::vlc::choices();
        self.vlc_now = crate::vlc::current();
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
        let source = theme::read_source(self.look.theme);
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
                if Btn::new("").icon(glyph::BACK).subtle().show(ui, &p).clicked() {
                    acts.push(Action::Go(Page::Apps));
                }
                ui.add_space(4.0);
            }
            let title = match self.page {
                Page::Apps => "HeMi Launcher".to_string(),
                Page::Notes(id) => format!("{} {}", tr("What's new –", "Co je nového –"), name_of(id)),
                Page::Settings => tr("Settings", "Nastavení").into(),
            };
            ui.label(RichText::new(title).font(theme::display(28.0)).color(p.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.page != Page::Apps {
                    return;
                }
                // the settings and About on one page
                if Btn::new("").icon(glyph::SETTINGS).subtle().show(ui, &p).clicked() {
                    acts.push(Action::Go(Page::Settings));
                }
                // all HeMi apps on the web
                if self.web_button(ui) {
                    acts.push(Action::Url(SITE.into()));
                }
                let check = Btn::new("").icon(glyph::REFRESH).subtle().enabled(!st.checking).tip(tr("Check for updates", "Zkontrolovat aktualizace"));
                if check.show(ui, &p).clicked() {
                    acts.push(Action::Check);
                }
                // when it last looked (the state of each app is in its card)
                if st.checking {
                    ui.label(RichText::new(tr("Checking…", "Hledám…")).size(12.0).color(p.text2));
                    ui.add(egui::Spinner::new().size(14.0).color(p.accent_text));
                } else if let Some(t) = st.checked {
                    ui.label(RichText::new(ago(t)).size(12.0).color(p.text3));
                }
                // always there; it works when something has a new version
                let ups = self.app_updates(st);
                ui.add_space(8.0);
                let tip = if ups.is_empty() { tr("Everything is up to date", "Vše je aktuální") } else { tr("Installs every new version", "Nainstaluje všechny nové verze") };
                let all = Btn::new(tr("Update all", "Aktualizovat vše")).accent().enabled(!ups.is_empty() && st.busy.is_empty()).tip(tip);
                if all.show(ui, &p).clicked() {
                    acts.push(Action::UpdateAll);
                }
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
                        }
                    });
                });
                ui.add_space(4.0);
                self.notes_fold(ui, LAUNCHER.id, r, acts);
            });
        ui.add_space(8.0);
    }

    /// A subtle button with the GitHub mark (the system icon font has none), the size of the other icon buttons.
    fn github_button(&self, ui: &mut egui::Ui) -> bool {
        let p = self.p;
        let resp = Btn::new("").icon(' ').subtle().tip(tr("Project on GitHub", "Projekt na GitHubu")).show(ui, &p);
        if let Some(t) = self.icons.get("github") {
            let r = egui::Rect::from_center_size(resp.rect.center(), vec2(16.0, 16.0));
            ui.painter().image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), p.text);
        }
        resp.clicked()
    }

    /// The website: its own icon (the "H" in a rounded square with the dot), drawn in the colour of the text like the
    /// other icons of the bar.
    fn web_button(&self, ui: &mut egui::Ui) -> bool {
        let p = self.p;
        let resp = Btn::new("").icon(' ').subtle().tip("hemal1k.github.io").show(ui, &p);
        let r = egui::Rect::from_center_size(resp.rect.center(), vec2(16.0, 16.0));
        let at = |x: f32, y: f32| egui::pos2(r.min.x + x / 4.0, r.min.y + y / 4.0); // from the 64 × 64 of the website icon
        let painter = ui.painter();
        let line = egui::Stroke::new(1.3, p.text);
        painter.rect_stroke(r.shrink(0.65), 4.0, line, egui::StrokeKind::Middle);
        let h = egui::Stroke::new(1.7, p.text);
        painter.line_segment([at(21.0, 17.0), at(21.0, 47.0)], h);
        painter.line_segment([at(43.0, 17.0), at(43.0, 47.0)], h);
        painter.line_segment([at(21.0, 32.0), at(43.0, 32.0)], h);
        painter.circle_filled(at(51.0, 13.0), 1.2, p.text);
        resp.clicked()
    }

    fn icon(&self, ui: &mut egui::Ui, id: &str, size: f32) {
        if let Some(t) = self.icons.get(id) {
            ui.add(egui::Image::new(t).fit_to_exact_size(vec2(size, size)));
        }
    }

    fn apps_page(&self, ui: &mut egui::Ui, st: &State, acts: &mut Vec<Action>) {
        self.launcher_bar(ui, st, acts);
        // only when the check failed (no internet, GitHub limits…)
        if let Some(e) = st.check_error.as_ref().filter(|_| !st.checking) {
            info_bar(ui, &self.p, Tone::Caution, &format!("{} – {}", tr("Could not check for updates", "Aktualizace se nepodařilo zjistit"), short(e)), None);
            ui.add_space(4.0);
        }
        ui.add_space(14.0);
        // the apps, then what is not an app (HeMi Dark: skins for VLC) under "Other" (the user, 2026-11-19)
        let apps: Vec<&'static AppInfo> = APPS.iter().filter(|a| a.kind != Kind::Files).collect();
        let other: Vec<&'static AppInfo> = APPS.iter().filter(|a| a.kind == Kind::Files).collect();
        section(ui, &self.p, tr("Apps", "Aplikace"));
        self.cards(ui, &apps, st, acts);
        if !other.is_empty() {
            ui.add_space(4.0);
            // folded at the start, a click on the heading unfolds it (the user, 2026-11-19)
            let open = self.other_open.get();
            let head = ui
                .horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let chevron = if open { glyph::CHEVRON_DOWN } else { glyph::CHEVRON_RIGHT };
                    ui.label(RichText::new(chevron.to_string()).font(theme::icon(11.0)).color(self.p.text2));
                    ui.label(RichText::new(tr("Other", "Ostatní")).font(theme::semibold(14.0)).color(self.p.text));
                })
                .response
                .interact(egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if head.clicked() {
                self.other_open.set(!open);
            }
            ui.add_space(4.0);
            if open {
                self.cards(ui, &other, st, acts);
            }
        }
    }

    /// Cards in a grid, as on the website: as many columns as fit (at least 300 px each).
    fn cards(&self, ui: &mut egui::Ui, list: &[&'static AppInfo], st: &State, acts: &mut Vec<Action>) {
        let gap = 10.0;
        let w = ui.available_width();
        let cols = (((w + gap) / (300.0 + gap)).floor() as usize).clamp(1, 3);
        let cw = (w - gap * (cols - 1) as f32) / cols as f32;
        for chunk in list.chunks(cols) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for a in chunk {
                    ui.allocate_ui_with_layout(vec2(cw, 0.0), Layout::top_down(Align::Min), |ui| {
                        ui.set_width(cw);
                        self.app_card(ui, a, st, acts);
                    });
                }
            });
            ui.add_space(gap);
        }
    }

    /// One app as a card of the website: icon, name and what it is on this PC, the version, what it does, and the
    /// buttons (install / portable / update / open, what's new, GitHub, uninstall).
    fn app_card(&self, ui: &mut egui::Ui, a: &'static AppInfo, st: &State, acts: &mut Vec<Action>) {
        let p = self.p;
        let installed = self.installed.get(a.id).cloned().flatten();
        // the portable version counts only when the app is not installed
        let portable = if installed.is_none() { self.portable.get(a.id).cloned().flatten() } else { None };
        let have = installed.as_ref().map(|i| i.version.clone()).or_else(|| portable.as_ref().map(|p| p.version.clone()));
        let release = st.releases.get(a.id);
        let update = match (&have, release) {
            (Some(v), Some(r)) => catalog::newer(&r.version, v),
            _ => false,
        };
        let frame = egui::Frame::new()
            .fill(p.card)
            .stroke(egui::Stroke::new(1.0, if update { p.accent } else { p.card_stroke }))
            .corner_radius(crate::widgets::CARD_RADIUS)
            .inner_margin(Margin { left: 18, right: 16, top: 16, bottom: 14 });
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            // the head: icon, name, where it is on this PC; the version on the right
            ui.horizontal(|ui| {
                self.icon(ui, a.id, 36.0);
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(RichText::new(a.name).font(theme::semibold(16.0)).color(p.text));
                    let who = match (&installed, &portable) {
                        (Some(i), _) if i.all_users => tr("Installed for all users", "Nainstalováno pro všechny"),
                        (Some(_), _) => tr("Installed", "Nainstalováno"),
                        (None, Some(_)) if a.kind == Kind::Files => tr("In VLC", "Ve VLC"),
                        (None, Some(_)) if a.kind == Kind::Extension => tr("Unpacked", "Rozbaleno"),
                        (None, Some(_)) => "Portable",
                        _ if a.kind == Kind::Extension => tr("Browser extension", "Rozšíření prohlížeče"),
                        _ if a.kind == Kind::Files => tr("Skins for VLC", "Vzhledy pro VLC"),
                        _ if a.kind == Kind::Tool => tr("One program, no installation", "Jeden program, bez instalace"),
                        _ => tr("Not installed", "Nenainstalováno"),
                    };
                    // the state of its updates, right in the card
                    let (state, color) = if have.is_some() && update {
                        (tr("A new version", "Nová verze"), p.accent_text)
                    } else if have.is_some() && release.is_some() {
                        (tr("up to date", "aktuální"), p.success)
                    } else {
                        ("", p.text3)
                    };
                    // one label of two colours ("Installed · up to date")
                    let mut job = egui::text::LayoutJob::default();
                    let fmt = |c| egui::TextFormat { font_id: theme::body(12.0), color: c, ..Default::default() };
                    job.append(who, 0.0, fmt(p.text3));
                    if !state.is_empty() {
                        job.append(" · ", 0.0, fmt(p.text3));
                        job.append(state, 0.0, fmt(color));
                    }
                    ui.label(job);
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (text, color) = match (&have, release) {
                        (Some(v), Some(r)) if update => (format!("{v} → {}", r.version), p.accent_text),
                        (Some(v), _) => (v.clone(), p.text2),
                        (None, Some(r)) => (r.version.clone(), p.text3),
                        (None, None) => (String::new(), p.text3),
                    };
                    ui.label(RichText::new(text).size(13.0).color(color));
                });
            });
            ui.add_space(8.0);
            // what it does (or the last error), always two lines high so the cards of a row line up
            let (line, color) = match st.errors.get(a.id) {
                Some(e) => (short(e), p.critical),
                None => (a.line().to_string(), p.text2),
            };
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), egui::Sense::hover());
            let galley = ui.painter().layout(line, theme::body(13.0), color, r.width());
            ui.painter().galley(r.min, galley, color);
            // a new version: what is new in it, unfolded right in the card
            if let (true, Some(rel)) = (have.is_some() && update, release) {
                ui.add_space(4.0);
                self.notes_fold(ui, a.id, rel, acts);
            }
            ui.add_space(10.0);
            // the buttons
            ui.horizontal(|ui| {
                ui.set_min_height(32.0);
                if let Some(phase) = st.busy.get(a.id) {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| phase_ui(ui, &p, phase));
                    return;
                }
                if have.is_some() && update {
                    if Btn::new(tr("Update", "Aktualizovat")).accent().show(ui, &p).clicked() {
                        acts.push(if portable.is_some() { Action::InstallPortable(a.id) } else { Action::Install(a.id) });
                    }
                } else if have.is_some() && a.kind == Kind::Extension {
                    let tip = tr("The folder of the extension – the browser loads it as an unpacked extension", "Složka rozšíření – prohlížeč ji načte jako rozbalené rozšíření");
                    if Btn::new(tr("Show folder", "Zobrazit složku")).tip(tip).show(ui, &p).clicked() {
                        acts.push(Action::Open(a.id));
                    }
                } else if have.is_some() {
                    if Btn::new(tr("Open", "Otevřít")).show(ui, &p).clicked() {
                        acts.push(Action::Open(a.id));
                    }
                } else if a.kind == Kind::Extension {
                    let tip = tr(
                        "Unpacks the extension into the folder of the portable versions; the browser then loads it (Load unpacked)",
                        "Rozbalí rozšíření do složky portable verzí; prohlížeč ho pak načte (Načíst rozbalené)",
                    );
                    if Btn::new(tr("Download", "Stáhnout")).accent().enabled(release.is_some()).tip(tip).show(ui, &p).clicked() {
                        acts.push(Action::Install(a.id));
                    }
                } else if have.is_some() && a.kind == Kind::Files {
                    // the skin to use: a colour and a size, set in VLC at once
                    let (colours, sizes) = (&self.vlc_choices.0, &self.vlc_choices.1);
                    let (now_c, now_s) = self.vlc_now.clone().unwrap_or_default();
                    let names: Vec<&str> = colours
                        .iter()
                        .map(|k| crate::vlc::COLOURS.iter().find(|c| c.0 == *k).map(|c| tr(c.1, c.2)).unwrap_or(k))
                        .collect();
                    let labels: Vec<String> = sizes.iter().map(|s| format!("{s} %")).collect();
                    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                    let ci = colours.iter().position(|c| *c == now_c).unwrap_or(usize::MAX);
                    let si = sizes.iter().position(|s| *s == now_s).unwrap_or(usize::MAX);
                    if let Some(i) = crate::widgets::dropdown(ui, &p, ci, &names, 150.0) {
                        let size = if now_s == 0 { crate::vlc::size_for_screen(sizes) } else { now_s };
                        acts.push(Action::VlcSkin(colours[i].to_string(), size));
                    }
                    if let Some(i) = crate::widgets::dropdown(ui, &p, si, &label_refs, 96.0) {
                        let colour = if now_c.is_empty() { "blue".to_string() } else { now_c.clone() };
                        acts.push(Action::VlcSkin(colour, sizes[i]));
                    }
                } else if a.kind == Kind::Files {
                    let tip = tr("Unpacks the skins into VLC and turns HeMi Dark on – then pick its colour and size here", "Rozbalí vzhledy do VLC a zapne HeMi Dark – barvu a velikost pak vybereš tady");
                    if Btn::new(tr("Install to VLC", "Nainstalovat do VLC")).accent().enabled(release.is_some()).tip(tip).show(ui, &p).clicked() {
                        acts.push(Action::Install(a.id));
                    }
                } else if a.kind == Kind::Tool {
                    let b = Btn::new(tr("Download", "Stáhnout")).accent().enabled(release.is_some());
                    let tip = tr("The program into the Downloads folder – best copied to a USB stick", "Program do složky Stažené soubory – nejlépe zkopírovat na USB disk");
                    if b.tip(tip).show(ui, &p).clicked() {
                        acts.push(Action::Install(a.id));
                    }
                } else {
                    if Btn::new(tr("Install", "Nainstalovat")).accent().enabled(release.is_some()).show(ui, &p).clicked() {
                        acts.push(Action::Install(a.id));
                    }
                    let tip = tr(
                        "The portable version into the Downloads folder (or the folder from the settings)",
                        "Portable verze do Stažených souborů (nebo do složky z nastavení)",
                    );
                    if Btn::new("Portable").enabled(release.is_some()).tip(tip).show(ui, &p).clicked() {
                        acts.push(Action::InstallPortable(a.id));
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if installed.is_some() && Btn::new("").icon(glyph::DELETE).subtle().tip(tr("Uninstall", "Odinstalovat")).show(ui, &p).clicked() {
                        acts.push(Action::Uninstall(a.id));
                    }
                    if portable.is_some()
                        && Btn::new("").icon(glyph::DELETE).subtle().tip(match a.kind {
                            Kind::Files => tr("Remove from VLC (its normal look back)", "Odebrat z VLC (vrátí jeho běžný vzhled)"),
                            Kind::Extension => tr("Remove the unpacked extension (remove it in the browser too)", "Odstranit rozbalené rozšíření (odeber ho i v prohlížeči)"),
                            _ => tr("Remove the portable version", "Odstranit portable verzi"),
                        })
                        .show(ui, &p)
                        .clicked()
                    {
                        acts.push(Action::RemovePortable(a.id));
                    }
                    if self.github_button(ui) {
                        acts.push(Action::Url(format!("https://github.com/Hemal1k/{}", a.id)));
                    }
                    if release.is_some() && Btn::new("").icon(glyph::NOTES).subtle().tip(tr("What's new", "Co je nového")).show(ui, &p).clicked() {
                        acts.push(Action::Go(Page::Notes(a.id)));
                    }
                });
            });
        });
    }

    /// Under an update: "What's new in X" that unfolds the notes of the release right there.
    fn notes_fold(&self, ui: &mut egui::Ui, id: &'static str, r: &crate::github::Release, acts: &mut Vec<Action>) {
        let p = self.p;
        let open = self.notes_open.contains(id);
        let resp = ui
            .horizontal(|ui| {
                let arrow = if open { glyph::CHEVRON_DOWN } else { glyph::CHEVRON_RIGHT };
                ui.label(RichText::new(arrow.to_string()).font(theme::icon(10.0)).color(p.accent_text));
                ui.label(RichText::new(format!("{} {}", tr("What's new in", "Co je nového ve verzi"), r.version)).size(13.0).color(p.accent_text));
            })
            .response
            .interact(egui::Sense::click());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            acts.push(Action::ToggleNotes(id));
        }
        if open {
            ui.add_space(4.0);
            egui::Frame::new().fill(p.bg).corner_radius(6).inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical().id_salt(("notes", id)).max_height(220.0).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    self.notes_body(ui, &r.notes);
                });
            });
        }
    }

    /// The notes of a release (Markdown: headings, bullets, text).
    fn notes_body(&self, ui: &mut egui::Ui, notes: &str) {
        let p = self.p;
        for line in notes.lines() {
            let t = line.trim_end();
            if let Some(h) = t.strip_prefix('#') {
                ui.add_space(4.0);
                ui.label(RichText::new(h.trim_start_matches('#').trim()).font(theme::semibold(14.0)).color(p.text));
            } else if let Some(item) = t.trim_start().strip_prefix("- ").or_else(|| t.trim_start().strip_prefix("* ")) {
                let indent = (t.len() - t.trim_start().len()) as f32 * 6.0;
                // the bullet, and the text wrapped next to it
                ui.horizontal_top(|ui| {
                    ui.add_space(indent);
                    ui.label(RichText::new("•").size(13.0).color(p.text2));
                    ui.add(egui::Label::new(RichText::new(plain(item)).size(13.0).color(p.text)).wrap());
                });
            } else if !t.trim().is_empty() {
                ui.label(RichText::new(plain(t)).size(13.0).color(p.text));
            }
        }
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
            self.notes_body(ui, &r.notes);
        });
    }

    fn settings_page(&self, ui: &mut egui::Ui, acts: &mut Vec<Action>) {
        let p = self.p;
        let s = self.c.settings.lock().unwrap().clone();
        ui.spacing_mut().item_spacing.y = 4.0;
        // a title and the switch, the height of the other rows (no descriptions – the user, 2026-11-15)
        let row = |ui: &mut egui::Ui, title: &str, _detail: &str, on: bool| -> bool {
            let mut clicked = false;
            card(ui, &p, |ui| {
                row_layout(ui, 44.0, |ui| {
                    ui.label(RichText::new(title).size(14.0).color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(if on { tr("On", "Zapnuto") } else { tr("Off", "Vypnuto") }).size(14.0).color(p.text));
                        clicked = toggle(ui, &p, on).clicked();
                    });
                });
            });
            clicked
        };
        // the same at the top of every HeMi app: theme and language
        section(ui, &p, tr("Appearance", "Vzhled"));
        for (title, current, items, which) in [
            (tr("Theme", "Motiv"), self.look.theme, [tr("Automatic", "Automaticky"), tr("Light", "Světlý"), tr("Dark", "Tmavý")], 0),
            (tr("Language", "Jazyk"), self.look.language, [tr("Automatic", "Automaticky"), "English", "Čeština"], 1),
        ] {
            card(ui, &p, |ui| {
                row_layout(ui, 44.0, |ui| {
                    ui.label(RichText::new(title).size(14.0).color(p.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(i) = crate::widgets::dropdown(ui, &p, current as usize, &items, 220.0) {
                            let mut l = self.look;
                            if which == 0 {
                                l.theme = i as u8;
                            } else {
                                l.language = i as u8;
                            }
                            acts.push(Action::SetLook(l));
                        }
                    });
                });
            });
        }
        ui.add_space(14.0);        section(ui, &p, tr("Updates", "Aktualizace"));
        if row(ui, tr("Check for updates automatically", "Automaticky vyhledat aktualizace"), "", s.auto_check) {
            acts.push(Action::Set(Settings { auto_check: !s.auto_check, ..s.clone() }));
        }
        if row(ui, tr("Install updates in the background", "Instalovat aktualizace na pozadí"), "", s.auto_install) {
            acts.push(Action::Set(Settings { auto_install: !s.auto_install, ..s.clone() }));
        }
        if row(ui, tr("Notify about new versions", "Upozornit na nové verze"), "", s.notify) {
            acts.push(Action::Set(Settings { notify: !s.notify, ..s.clone() }));
        }
        ui.add_space(14.0);
        section(ui, &p, tr("Start", "Spouštění"));
        if row(ui, tr("Start with Windows", "Spouštět s Windows"), "", self.autostart) {
            acts.push(Action::Autostart(!self.autostart));
        }
        if row(ui, tr("Closing hides it in the notification area", "Zavřením se skryje do oznamovací oblasti"), "", s.tray_on_close) {
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
        section(ui, &p, tr("About", "O aplikaci"));
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
        value(ui, tr("Version", "Verze"), env!("CARGO_PKG_VERSION"));
        // the links side by side in one card (they wrap when the window is narrow)
        let links: Vec<(&str, &str, Action)> = vec![
            (tr("Project on GitHub", "Projekt na GitHubu"), PROJECT, Action::Url(PROJECT.into())),
            (tr("What's new", "Co je nového"), RELEASES, Action::Url(RELEASES.into())),
            (tr("Report a problem", "Nahlásit problém"), CONTACT, Action::Url(CONTACT.into())),
            (tr("Licences", "Licence"), "", Action::Licences),
        ];
        ui.add_space(10.0);
        section(ui, &p, tr("Links", "Odkazy"));
        let items: Vec<(&str, &str)> = links.iter().map(|(t, tip, _)| (*t, *tip)).collect();
        if let Some(i) = card(ui, &p, |ui| crate::widgets::button_row(ui, &p, &items))
            && let Some((_, _, act)) = links.into_iter().nth(i)
        {
            acts.push(act);
        }
    }

    fn act(&mut self, acts: Vec<Action>) {
        for a in acts {
            match a {
                Action::SetLook(l) => {
                    l.save();
                    crate::i18n::set(l.language);
                    self.look = l;
                    // the new colours at once, not after the next look at the theme
                    self.theme_checked = Instant::now().checked_sub(Duration::from_secs(5)).unwrap_or_else(Instant::now);
                }
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
                Action::VlcSkin(colour, size) => {
                    match crate::vlc::apply(&colour, size) {
                        Ok(()) => self.toast(tr("Set in VLC – start (or restart) VLC", "Nastaveno ve VLC – spusť (nebo restartuj) VLC"), Tone::Success),
                        Err(e) => self.toast(e, Tone::Caution),
                    }
                    self.vlc_now = crate::vlc::current();
                }
                Action::Open(id) => {
                    if let Some(app) = crate::jobs::find(id) {
                        let exe = match (self.installed.get(id), self.portable.get(id)) {
                            (Some(Some(i)), _) => Some(i.exe(app)),
                            (_, Some(Some(p))) => Some(p.exe(app)),
                            _ => None,
                        };
                        if let Some(exe) = exe {
                            if app.kind == Kind::Extension {
                                // the folder the browser loads, not the manifest
                                let _ = std::process::Command::new("explorer.exe").arg(exe.parent().unwrap_or(&exe)).spawn();
                            } else {
                                crate::jobs::open(&exe);
                            }
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
                Action::ToggleNotes(id) => {
                    if !self.notes_open.remove(id) {
                        self.notes_open.insert(id);
                    }
                }
            }
        }
    }
}

impl eframe::App for LauncherApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // the display scaling, for the size of the VLC skin
        crate::vlc::SYSTEM_SCALE.store((ui.ctx().pixels_per_point() * 100.0).round() as u32, std::sync::atomic::Ordering::Relaxed);
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
                        Page::Settings => {
                            self.settings_page(ui, &mut acts);
                            ui.add_space(14.0);
                            self.about_page(ui, &mut acts);
                        }
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
