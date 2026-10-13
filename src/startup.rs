use crate::{clipboard::wide, store::Result};
use std::ptr::{null, null_mut};
use windows_sys::Win32::System::Registry::*;

const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const NAME: &str = "ClipboardManager";

pub fn enabled() -> bool {
    unsafe {
        let mut key = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN).as_ptr(), 0, KEY_READ, &mut key) != 0 {
            return false;
        }
        let result = RegQueryValueExW(
            key,
            wide(NAME).as_ptr(),
            null(),
            null_mut(),
            null_mut(),
            null_mut(),
        );
        RegCloseKey(key);
        result == 0
    }
}

pub fn set(enabled: bool) -> Result<()> {
    unsafe {
        let mut key = null_mut();
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN).as_ptr(),
            0,
            null(),
            0,
            KEY_SET_VALUE,
            null(),
            &mut key,
            null_mut(),
        );
        if status != 0 {
            return Err(format!("Не удалось открыть автозапуск: {status}").into());
        }
        let result = if enabled {
            let exe = std::env::current_exe()?;
            let value = wide(&format!("\"{}\"", exe.display()));
            RegSetValueExW(
                key,
                wide(NAME).as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        } else {
            RegDeleteValueW(key, wide(NAME).as_ptr())
        };
        RegCloseKey(key);
        if result != 0 && result != 2 {
            return Err(format!("Не удалось изменить автозапуск: {result}").into());
        }
    }
    Ok(())
}
