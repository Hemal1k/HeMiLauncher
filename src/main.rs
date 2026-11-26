// HeMi Launcher – installs the HeMi apps and keeps them up to date (and itself).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod browsers;
mod catalog;
mod github;
mod i18n;
mod jobs;
mod log;
mod look;
mod notify;
mod reg;
mod settings;
mod theme;
mod tray;
mod vlc;
mod watch;
mod widgets;

use eframe::egui;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // the installer after an installation or a move to another folder: the start with Windows and the path the other
    // HeMi apps know point to this exe
    if args.iter().any(|a| a == "--repair") {
        settings::register();
        settings::fix_autostart();
        return Ok(());
    }
    // the installer: "Start with Windows" chosen – only written, the launcher does not start before Finish (it started
    // during the installation, 2026-11-19)
    if args.iter().any(|a| a == "--set-autostart") {
        settings::set_autostart(true);
        return Ok(());
    }
    // started with Windows: hidden in the notification area
    let hidden = args.iter().any(|a| a == settings::TRAY);
    // started by another HeMi app ("look for updates now")
    let check = args.iter().any(|a| a == tray::CHECK);

    // started with Windows although switched off in Task Manager (the task does not know it)
    if hidden && !args.iter().any(|a| a == "--autostart") && settings::startup_switched_off() {
        return Ok(());
    }
    // the launcher runs already: its window comes up (and looks for updates when asked), this one ends; the second
    // start at sign-in (the task and the Run value both start it) ends quietly
    if !tray::first() {
        if !hidden {
            tray::wake(check);
        }
        return Ok(());
    }
    jobs::clean_old();
    settings::register();
    // the installer: "Start with Windows" chosen
    if args.iter().any(|a| a == "--autostart") {
        settings::set_autostart(true);
    }
    settings::repair_autostart();

    let icon = egui::IconData { rgba: include_bytes!("../assets/HeMiLauncher-64.rgba").to_vec(), width: 64, height: 64 };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("HeMi Launcher")
            .with_app_id("HeMiLauncher")
            // everything at once (the user, 2026-11-24): three cards side by side, wide enough for all their buttons, and
            // all three rows of them (3 × 3) without scrolling; the height is then fitted to them after the first frames
            // (app.rs, fit_height) – 800 left a big gap under them
            .with_inner_size([1180.0, 720.0])
            .with_min_inner_size([640.0, 480.0])
            .with_icon(icon)
            .with_visible(!hidden)
            // transparent, so the Mica of Windows 11 shows around the cards
            .with_transparent(true),
        centered: true,
        ..Default::default()
    };
    eframe::run_native("HeMiLauncher", options, Box::new(move |cc| Ok(Box::new(app::LauncherApp::new(cc, hidden)))))
}
