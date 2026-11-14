// HeMi Launcher â€“ installs the HeMi apps and keeps them up to date (and itself).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod catalog;
mod github;
mod i18n;
mod jobs;
mod notify;
mod reg;
mod settings;
mod theme;
mod tray;
mod watch;
mod widgets;

use eframe::egui;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // started with Windows: hidden in the notification area
    let hidden = args.iter().any(|a| a == settings::TRAY);
    // started by another HeMi app ("look for updates now")
    let check = args.iter().any(|a| a == tray::CHECK);

    // the launcher runs already: its window comes up (and looks for updates when asked), this one ends
    if !tray::first() {
        tray::wake(check);
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
            // three cards side by side and the two rows of them without scrolling
            .with_inner_size([1010.0, 600.0])
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
