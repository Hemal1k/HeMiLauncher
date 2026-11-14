//! Reading and writing strings in the registry (HKCU, or HKLM in its 64-bit view).

use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_WRITE, REG_SZ, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegCloseKey,
    RegCreateKeyExW, RegDeleteKeyValueW, RegGetValueW, RegSetValueExW,
};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A string value; `machine` = HKLM (64-bit view), else HKCU. None when it is not there (or empty).
pub fn read(machine: bool, key: &str, name: &str) -> Option<String> {
    let (k, n) = (wide(key), wide(name));
    let root = if machine { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER };
    let mut buf = vec![0u16; 4096];
    let mut size = (buf.len() * 2) as u32;
    let flags = RRF_RT_REG_SZ | if machine { RRF_SUBKEY_WOW6464KEY } else { 0 };
    let r = unsafe { RegGetValueW(root, k.as_ptr(), n.as_ptr(), flags, null_mut(), buf.as_mut_ptr() as _, &mut size) };
    if r != ERROR_SUCCESS {
        return None;
    }
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let s = String::from_utf16_lossy(&buf[..end]);
    (!s.is_empty()).then_some(s)
}

/// A string value of the user.
pub fn write_user(key: &str, name: &str, value: &str) -> bool {
    let (k, n, v) = (wide(key), wide(name), wide(value));
    unsafe {
        let mut hkey: HKEY = null_mut();
        if RegCreateKeyExW(HKEY_CURRENT_USER, k.as_ptr(), 0, null(), 0, KEY_WRITE, null(), &mut hkey, null_mut()) != ERROR_SUCCESS {
            return false;
        }
        let r = RegSetValueExW(hkey, n.as_ptr(), 0, REG_SZ, v.as_ptr() as *const u8, (v.len() * 2) as u32);
        RegCloseKey(hkey);
        r == ERROR_SUCCESS
    }
}

pub fn delete_user(key: &str, name: &str) {
    let (k, n) = (wide(key), wide(name));
    unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr()) };
}
