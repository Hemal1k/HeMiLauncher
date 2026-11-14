//! The icon of HeMi Launcher in the notification area (also in the one of HeMiDock, which hosts the icons of the apps):
//! a click opens the window, the right click offers Open / Check for updates / Quit. Its own thread with a hidden window
//! for the messages of the icon; the window of the app is shown and hidden from here directly (a hidden egui window
//! does not run its loop).

use crate::i18n::tr;
use crate::reg::wide;
use eframe::egui;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Open,
    Check,
    Quit,
}

const CALLBACK: u32 = WM_APP + 1;
/// From another start of the launcher: the window up (`wparam` 1: and look for updates).
const WAKE: u32 = WM_APP + 2;
/// The argument the other HeMi apps start the launcher with: look for updates now.
pub const CHECK: &str = "--check";
static TRAY: AtomicIsize = AtomicIsize::new(0);
static MAIN: AtomicIsize = AtomicIsize::new(0);
/// Quit from the menu: the window really closes (not into the notification area).
pub static QUITTING: AtomicBool = AtomicBool::new(false);
static EVENTS: OnceLock<Mutex<Sender<Event>>> = OnceLock::new();
static CTX: OnceLock<egui::Context> = OnceLock::new();
static TIP: Mutex<String> = Mutex::new(String::new());

fn send(e: Event) {
    if let Some(tx) = EVENTS.get() {
        let _ = tx.lock().unwrap().send(e);
    }
    if let Some(ctx) = CTX.get() {
        ctx.request_repaint();
    }
}

/// The window of the app back on the screen, in front.
pub fn show_main() {
    let h = MAIN.load(Ordering::Relaxed) as HWND;
    if h.is_null() {
        return;
    }
    unsafe {
        ShowWindow(h, if IsIconic(h) != 0 { SW_RESTORE } else { SW_SHOW });
        SetForegroundWindow(h);
    }
    if let Some(ctx) = CTX.get() {
        ctx.request_repaint();
    }
}

/// The window of the app hidden (the icon stays).
pub fn hide_main() {
    let h = MAIN.load(Ordering::Relaxed) as HWND;
    if !h.is_null() {
        unsafe { ShowWindow(h, SW_HIDE) };
    }
}

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid
}

fn add_icon(hwnd: HWND) {
    let mut nid = data(hwnd);
    nid.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE;
    nid.uCallbackMessage = CALLBACK;
    unsafe {
        let icon = LoadIconW(GetModuleHandleW(null()), 1 as _);
        nid.hIcon = if icon.is_null() { LoadIconW(null_mut(), IDI_APPLICATION) } else { icon };
        copy(&mut nid.szTip, &TIP.lock().unwrap());
        Shell_NotifyIconW(NIM_ADD, &nid);
    }
}

/// The tooltip of the icon (e.g. how many updates there are).
pub fn set_tip(text: &str) {
    *TIP.lock().unwrap() = text.to_string();
    let h = TRAY.load(Ordering::Relaxed) as HWND;
    if h.is_null() {
        return;
    }
    let mut nid = data(h);
    nid.uFlags = NIF_TIP;
    copy(&mut nid.szTip, text);
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
}

pub fn remove() {
    let h = TRAY.load(Ordering::Relaxed) as HWND;
    if !h.is_null() {
        let nid = data(h);
        unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
    }
}

fn copy<const N: usize>(to: &mut [u16; N], text: &str) {
    let w: Vec<u16> = text.encode_utf16().take(N - 1).collect();
    to[..w.len()].copy_from_slice(&w);
    to[w.len()] = 0;
}

static TASKBAR_CREATED: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if msg == CALLBACK {
            match lparam as u32 {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                    show_main();
                    send(Event::Open);
                }
                WM_RBUTTONUP | WM_CONTEXTMENU => menu(hwnd),
                _ => {}
            }
            return 0;
        }
        if msg == WAKE {
            show_main();
            send(Event::Open);
            if wparam == 1 {
                send(Event::Check);
            }
            return 0;
        }
        // File Explorer (or HeMiDock) started again: the icon is put back
        if msg as isize == TASKBAR_CREATED.load(Ordering::Relaxed) && msg != 0 {
            add_icon(hwnd);
            return 0;
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

unsafe fn menu(hwnd: HWND) {
    unsafe {
        let m = CreatePopupMenu();
        let items = [(1usize, tr("Open HeMi Launcher", "Otevřít HeMi Launcher")), (2, tr("Check for updates", "Zkontrolovat aktualizace"))];
        for (id, text) in items {
            let t = wide(text);
            AppendMenuW(m, MF_STRING, id, t.as_ptr());
        }
        AppendMenuW(m, MF_SEPARATOR, 0, null());
        let q = wide(tr("Quit", "Ukončit"));
        AppendMenuW(m, MF_STRING, 3, q.as_ptr());
        SetMenuDefaultItem(m, 1, 0);
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        // the menu closes when clicked elsewhere only with the window in front
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, null());
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(m);
        match cmd {
            1 => {
                show_main();
                send(Event::Open);
            }
            2 => send(Event::Check),
            3 => {
                QUITTING.store(true, Ordering::Relaxed);
                send(Event::Quit);
                if let Some(ctx) = CTX.get() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                show_main();
            }
            _ => {}
        }
    }
}

/// True when this is the first launcher of the user (the mutex stays for the life of the process).
pub fn first() -> bool {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name = wide("Local\\HeMiLauncher.Window");
    unsafe {
        let handle = CreateMutexW(null(), 0, name.as_ptr());
        !handle.is_null() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// Wakes the running launcher (through the window of its icon – it finds also the hidden one).
pub fn wake(check: bool) {
    let class = wide("HeMiLauncherTray");
    unsafe {
        let h = FindWindowW(class.as_ptr(), null());
        if !h.is_null() {
            // the running one may bring its window to the front
            AllowSetForegroundWindow(u32::MAX);
            PostMessageW(h, WAKE, check as usize, 0);
        }
    }
}

/// Starts the icon on its own thread.
pub fn start(main_hwnd: isize, ctx: egui::Context, events: Sender<Event>, tip: &str) {
    MAIN.store(main_hwnd, Ordering::Relaxed);
    let _ = CTX.set(ctx);
    let _ = EVENTS.set(Mutex::new(events));
    *TIP.lock().unwrap() = tip.to_string();
    std::thread::spawn(|| unsafe {
        let instance = GetModuleHandleW(null());
        let class = wide("HeMiLauncherTray");
        let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: instance, lpszClassName: class.as_ptr(), ..std::mem::zeroed() };
        RegisterClassW(&wc);
        let name = wide("HeMiLauncherTray");
        // a hidden top-level window (not message-only): it gets "TaskbarCreated" when the notification area starts again
        let hwnd = CreateWindowExW(0, class.as_ptr(), name.as_ptr(), 0, 0, 0, 0, 0, null_mut(), null_mut(), instance, null());
        TRAY.store(hwnd as isize, Ordering::Relaxed);
        let tc = wide("TaskbarCreated");
        TASKBAR_CREATED.store(RegisterWindowMessageW(tc.as_ptr()) as isize, Ordering::Relaxed);
        add_icon(hwnd);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}
