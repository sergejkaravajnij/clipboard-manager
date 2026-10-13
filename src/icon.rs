use std::ptr::null;
use windows_sys::Win32::{System::LibraryLoader::GetModuleHandleW, UI::WindowsAndMessaging::*};

pub fn load(small: bool) -> HICON {
    load_resource(101, small)
}

pub fn tray() -> HICON {
    load_resource(102, true)
}

fn load_resource(id: usize, small: bool) -> HICON {
    unsafe {
        let (x, y) = if small {
            (SM_CXSMICON, SM_CYSMICON)
        } else {
            (SM_CXICON, SM_CYICON)
        };
        // LR_SHARED icons belong to the module and must not be destroyed by the app.
        LoadImageW(
            GetModuleHandleW(null()),
            id as *const u16,
            IMAGE_ICON,
            GetSystemMetrics(x),
            GetSystemMetrics(y),
            LR_SHARED,
        ) as HICON
    }
}
