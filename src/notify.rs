//! A notification of Windows under the name and icon of HeMi Launcher (its own app id), and the state the other HeMi apps
//! read: %LocalAppData%\HeMi\HeMiLauncher\updates.txt – one line "<id> <new version>" per app with an update (HeMiDock shows
//! them as a status icon that opens the launcher).

use std::path::PathBuf;

const APP_ID: &str = "HeMi.HeMiLauncher";
/// Started from a click on its notification: the window comes up.
pub const SHOW: &str = "--show";

fn local_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default().join("HeMi").join("HeMiLauncher")
}

pub fn toast(title: &str, text: &str) -> bool {
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
    use windows::core::HSTRING;
    let icon = local_dir().join("HeMiLauncher.png");
    if !icon.exists() {
        let _ = std::fs::create_dir_all(local_dir());
        let _ = std::fs::write(&icon, include_bytes!("../assets/HeMiLauncher.png"));
    }
    let key = format!(r"Software\Classes\AppUserModelId\{APP_ID}");
    crate::reg::write_user(&key, "DisplayName", "HeMi Launcher");
    crate::reg::write_user(&key, "IconUri", &icon.display().to_string());
    // a click on the notification (also later in the notification centre) opens the link hemilauncher: – it starts
    // the launcher, which brings up the window of the running one
    if let Ok(exe) = std::env::current_exe() {
        let proto = r"Software\Classes\hemilauncher";
        crate::reg::write_user(proto, "", "URL:HeMi Launcher");
        crate::reg::write_user(proto, "URL Protocol", "");
        crate::reg::write_user(&format!(r"{proto}\shell\open\command"), "", &format!("\"{}\" {SHOW}", exe.display()));
    }
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    let xml = format!(
        r#"<toast activationType="protocol" launch="hemilauncher:updates"><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
        esc(title),
        esc(text)
    );
    (|| -> windows::core::Result<()> {
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml))?;
        let note = ToastNotification::CreateToastNotification(&doc)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?.Show(&note)
    })()
    .is_ok()
}

/// The apps with an update, for the other HeMi apps (empty: all up to date).
pub fn write_updates(updates: &[(&str, String)]) {
    let _ = std::fs::create_dir_all(local_dir());
    let text: String = updates.iter().map(|(id, v)| format!("{id} {v}\n")).collect();
    let _ = std::fs::write(local_dir().join("updates.txt"), text);
}
