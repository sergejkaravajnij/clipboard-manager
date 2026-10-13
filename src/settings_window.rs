use crate::{
    clipboard::wide,
    control,
    i18n::Language,
    preferences::{Hotkey, Preferences},
    store::Result,
};
use std::ptr::{null, null_mut};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Controls::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

pub const LANGUAGE_ID: usize = 201;
pub const HOTKEY_ID: usize = 202;
const SAVE_ID: usize = 203;
const CANCEL_ID: usize = 204;
const CLEAR_ID: usize = 205;
const WINDOWS_ID: usize = 206;

unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_CLOSE {
            PostMessageW(
                GetWindow(hwnd, GW_OWNER),
                crate::ACTION,
                crate::CANCEL_SETTINGS,
                0,
            );
            return 0;
        }
        if msg == WM_COMMAND {
            let parent = GetWindow(hwnd, GW_OWNER);
            match w & 0xffff {
                SAVE_ID => {
                    PostMessageW(parent, crate::ACTION, crate::SAVE_SETTINGS, 0);
                    return 0;
                }
                CANCEL_ID => {
                    PostMessageW(parent, crate::ACTION, crate::CANCEL_SETTINGS, 0);
                    return 0;
                }
                CLEAR_ID => {
                    SendMessageW(GetDlgItem(hwnd, HOTKEY_ID as i32), HKM_SETHOTKEY, 0, 0);
                    SendMessageW(
                        GetDlgItem(hwnd, WINDOWS_ID as i32),
                        BM_SETCHECK,
                        BST_UNCHECKED as usize,
                        0,
                    );
                    return 0;
                }
                LANGUAGE_ID if (w >> 16) & 0xffff == CBN_SELCHANGE as usize => {
                    PostMessageW(parent, crate::ACTION, crate::PREVIEW_LANGUAGE, 0);
                    return 0;
                }
                _ => {}
            }
        }
        DefWindowProcW(hwnd, msg, w, l)
    }
}

pub struct SettingsWindow {
    pub hwnd: HWND,
    pub language: HWND,
    pub hotkey: HWND,
    windows: HWND,
    language_label: HWND,
    hotkey_label: HWND,
    hint: HWND,
    error: HWND,
    save: HWND,
    cancel: HWND,
    clear: HWND,
    retention: HWND,
    retention_label: HWND,
}

impl SettingsWindow {
    pub unsafe fn create(owner: HWND, font: HFONT) -> Result<Self> {
        unsafe {
            let common = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_HOTKEY_CLASS | ICC_STANDARD_CLASSES,
            };
            if InitCommonControlsEx(&common) == 0 {
                return Err("Could not initialize settings controls".into());
            }
            let name = wide("ClipboardManagerSettings");
            let class = WNDCLASSW {
                lpfnWndProc: Some(procedure),
                hInstance: GetModuleHandleW(null()),
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                hIcon: crate::icon::load(false),
                hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
                lpszClassName: name.as_ptr(),
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
                return Err("Could not register settings window".into());
            }
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                name.as_ptr(),
                wide("Settings").as_ptr(),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                540,
                440,
                owner,
                null_mut(),
                GetModuleHandleW(null()),
                null(),
            );
            if hwnd.is_null() {
                return Err("Could not create settings window".into());
            }
            SendMessageW(
                hwnd,
                WM_SETICON,
                ICON_SMALL as usize,
                crate::icon::load(true) as isize,
            );
            let language_label = control(hwnd, "STATIC", "", 0, 207, font);
            let language = control(
                hwnd,
                "COMBOBOX",
                "",
                CBS_DROPDOWNLIST as u32 | WS_TABSTOP | WS_VSCROLL,
                LANGUAGE_ID,
                font,
            );
            SendMessageW(language, CB_ADDSTRING, 0, wide("English").as_ptr() as isize);
            SendMessageW(language, CB_ADDSTRING, 0, wide("Русский").as_ptr() as isize);
            let hotkey_label = control(hwnd, "STATIC", "", 0, 208, font);
            let hotkey = control(hwnd, "msctls_hotkey32", "", WS_TABSTOP, HOTKEY_ID, font);
            let windows = control(
                hwnd,
                "BUTTON",
                "Windows key",
                BS_AUTOCHECKBOX as u32 | WS_TABSTOP,
                WINDOWS_ID,
                font,
            );
            let hint = control(hwnd, "STATIC", "", 0, 209, font);
            let error = control(hwnd, "STATIC", "", 0, 210, font);
            let save = control(
                hwnd,
                "BUTTON",
                "",
                BS_DEFPUSHBUTTON as u32 | WS_TABSTOP,
                SAVE_ID,
                font,
            );
            let cancel = control(hwnd, "BUTTON", "", WS_TABSTOP, CANCEL_ID, font);
            let clear = control(hwnd, "BUTTON", "", WS_TABSTOP, CLEAR_ID, font);
            let retention_label = control(hwnd, "STATIC", "", 0, 211, font);
            let retention = control(
                hwnd,
                "EDIT",
                "",
                WS_TABSTOP | WS_BORDER | ES_NUMBER as u32,
                212,
                font,
            );
            SendMessageW(retention, EM_SETLIMITTEXT, 5, 0);
            if [
                language_label,
                language,
                hotkey_label,
                hotkey,
                windows,
                hint,
                error,
                save,
                cancel,
                clear,
                retention,
                retention_label,
            ]
            .iter()
            .any(|h| h.is_null())
            {
                DestroyWindow(hwnd);
                return Err("Could not create settings controls".into());
            }
            for (h, x, y, width, height) in [
                (language_label, 18, 14, 484, 24),
                (language, 18, 40, 484, 200),
                (hotkey_label, 18, 83, 484, 24),
                (hotkey, 18, 109, 354, 30),
                (clear, 390, 109, 112, 30),
                (windows, 18, 149, 250, 26),
                (hint, 18, 183, 484, 46),
                (retention_label, 18, 231, 484, 24),
                (retention, 18, 258, 100, 28),
                (error, 18, 295, 484, 44),
                (save, 282, 352, 100, 28),
                (cancel, 390, 352, 112, 28),
            ] {
                MoveWindow(h, x, y, width, height, 1);
            }
            Ok(Self {
                hwnd,
                language,
                hotkey,
                windows,
                language_label,
                hotkey_label,
                hint,
                error,
                save,
                cancel,
                clear,
                retention,
                retention_label,
            })
        }
    }

    pub fn selected_language(&self) -> Language {
        if unsafe { SendMessageW(self.language, CB_GETCURSEL, 0, 0) } == 1 {
            Language::Russian
        } else {
            Language::English
        }
    }
    pub fn candidate(&self) -> Result<Preferences> {
        let retention_days = crate::window_text(self.retention)
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|days| *days <= 36500)
            .ok_or_else(|| {
                self.selected_language()
                    .choose(
                        "Enter 0–36500 retention days (0 disables cleanup).",
                        "Введите срок 0–36500 дней (0 отключает очистку).",
                    )
                    .to_string()
            })?;
        Ok(Preferences {
            retention_days,
            language: self.selected_language(),
            hotkey: Hotkey::from_control(
                unsafe { SendMessageW(self.hotkey, HKM_GETHOTKEY, 0, 0) } as usize,
                unsafe { SendMessageW(self.windows, BM_GETCHECK, 0, 0) } == BST_CHECKED as isize,
            ),
        })
    }
    pub fn translate(&self, language: Language) {
        unsafe {
            for (h, en, ru) in [
                (self.hwnd, "Settings", "Настройки"),
                (self.language_label, "Interface language", "Язык интерфейса"),
                (
                    self.hotkey_label,
                    "Open clipboard history",
                    "Открыть историю буфера",
                ),
                (
                    self.windows,
                    "Include Windows key",
                    "Добавить клавишу Windows",
                ),
                (self.clear, "Clear", "Очистить"),
                (self.save, "Save", "Сохранить"),
                (self.cancel, "Cancel", "Отмена"),
                (
                    self.retention_label,
                    "Delete entries older than (days, 0 = keep forever)",
                    "Удалять записи старше (дней, 0 = хранить всегда)",
                ),
                (
                    self.hint,
                    "Click the shortcut field and press a combination. Clear disables it.\r\nWindows may reserve some combinations.",
                    "Нажмите сочетание в поле. «Очистить» отключает хоткей.\r\nЧасть сочетаний зарезервирована Windows.",
                ),
            ] {
                SetWindowTextW(h, wide(language.choose(en, ru)).as_ptr());
            }
        }
    }
    pub fn error(&self, text: &str) {
        unsafe {
            SetWindowTextW(self.error, wide(text).as_ptr());
        }
    }
    pub fn show(&self, p: Preferences) {
        unsafe {
            SendMessageW(
                self.language,
                CB_SETCURSEL,
                if p.language == Language::English {
                    0
                } else {
                    1
                },
                0,
            );
            SendMessageW(self.hotkey, HKM_SETHOTKEY, p.hotkey.control_value(), 0);
            SendMessageW(
                self.windows,
                BM_SETCHECK,
                if p.hotkey.modifiers & MOD_WIN != 0 {
                    BST_CHECKED
                } else {
                    BST_UNCHECKED
                } as usize,
                0,
            );
            self.translate(p.language);
            SetWindowTextW(self.retention, wide(&p.retention_days.to_string()).as_ptr());
            self.error("");
            ShowWindow(self.hwnd, SW_RESTORE);
            SetForegroundWindow(self.hwnd);
            SetFocus(self.language);
        }
    }
    pub fn contains(&self, hwnd: HWND) -> bool {
        hwnd == self.hwnd || unsafe { IsChild(self.hwnd, hwnd) } != 0
    }
    pub fn key(&self, msg: &MSG) -> bool {
        if !self.contains(msg.hwnd) || msg.message != WM_KEYDOWN {
            return false;
        }
        unsafe {
            if msg.wParam == VK_ESCAPE as usize {
                PostMessageW(
                    GetWindow(self.hwnd, GW_OWNER),
                    crate::ACTION,
                    crate::CANCEL_SETTINGS,
                    0,
                );
                return true;
            }
            if msg.wParam == VK_RETURN as usize {
                PostMessageW(
                    GetWindow(self.hwnd, GW_OWNER),
                    crate::ACTION,
                    crate::SAVE_SETTINGS,
                    0,
                );
                return true;
            }
        }
        false
    }
}
impl Drop for SettingsWindow {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(self.hwnd) != 0 {
                DestroyWindow(self.hwnd);
            }
        }
    }
}
