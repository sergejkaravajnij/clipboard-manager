#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod clipboard;
mod image;
mod startup;
mod store;
#[cfg(test)]
mod windows_tests;

use clipboard::wide;
use std::{
    path::PathBuf,
    ptr::{null, null_mut},
};
use store::{PAGE_SIZE, Result, Row, Store};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{DataExchange::*, LibraryLoader::*, Threading::*},
    UI::{Controls::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};

const CLASS: &str = "RustClipboardManagerWindow";
const TRAY: u32 = WM_APP + 1;
const ACTION: u32 = WM_APP + 2;
const SHOW: usize = 1;
const REFRESH: usize = 2;
const SELECT: usize = 3;
const COPY: usize = 4;
const RESIZE: usize = 5;
const CAPTURE: usize = 6;
const SEARCH: usize = 7;
const MENU: usize = 8;
const HIDE: usize = 9;
const NEXT: usize = 10;
const PREV: usize = 11;
const EXIT: usize = 12;
const STARTUP: usize = 13;
const SEARCH_ID: usize = 101;
const LIST_ID: usize = 102;
const COPY_ID: usize = 103;
const NEXT_ID: usize = 104;
const PREV_ID: usize = 105;
const TIMER_CAPTURE: usize = 1;
const TIMER_SEARCH: usize = 2;

struct App {
    hwnd: HWND,
    search: HWND,
    list: HWND,
    preview: HWND,
    image_window: HWND,
    image_state: std::rc::Rc<image::ImageState>,
    status: HWND,
    copy: HWND,
    next: HWND,
    prev: HWND,
    store: Store,
    rows: Vec<Row>,
    filter: String,
    offset: usize,
    font: HFONT,
    tray: NOTIFYICONDATAW,
    taskbar_created: u32,
    own_sequence: u32,
    last_sequence: u32,
    retries: u32,
    visible: bool,
    db_path: PathBuf,
}

fn data_path() -> Result<PathBuf> {
    if let Some(p) = std::env::args()
        .skip(1)
        .find_map(|s| s.strip_prefix("--data-dir=").map(PathBuf::from))
    {
        return Ok(p);
    }
    Ok(
        PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA отсутствует")?)
            .join("RustClipboardManager"),
    )
}

fn log_error(path: &std::path::Path, message: &str) {
    use std::io::Write;
    let log = path.with_file_name("errors.log");
    if std::fs::metadata(&log).is_ok_and(|m| m.len() > 1024 * 1024) {
        let _ = std::fs::remove_file(&log);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        let _ = writeln!(file, "{now}: {message}");
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_CLOSE => {
                PostMessageW(hwnd, ACTION, HIDE, 0);
                return 0;
            }
            WM_SIZE => {
                PostMessageW(hwnd, ACTION, RESIZE, 0);
                return 0;
            }
            WM_GETMINMAXINFO => {
                let info = &mut *(l as *mut MINMAXINFO);
                info.ptMinTrackSize = POINT { x: 480, y: 380 };
                return 0;
            }
            WM_CLIPBOARDUPDATE => {
                PostMessageW(hwnd, ACTION, CAPTURE, 0);
                return 0;
            }
            WM_TIMER => {
                KillTimer(hwnd, w);
                PostMessageW(
                    hwnd,
                    ACTION,
                    if w == TIMER_SEARCH { REFRESH } else { CAPTURE },
                    0,
                );
                return 0;
            }
            WM_COMMAND => {
                let id = w & 0xffff;
                let notification = (w >> 16) & 0xffff;
                let action = match (id, notification) {
                    (SEARCH_ID, n) if n == EN_CHANGE as usize => SEARCH,
                    (LIST_ID, n) if n == LBN_SELCHANGE as usize => SELECT,
                    (LIST_ID, n) if n == LBN_DBLCLK as usize => COPY,
                    (COPY_ID, _) => COPY,
                    (NEXT_ID, _) => NEXT,
                    (PREV_ID, _) => PREV,
                    _ => return DefWindowProcW(hwnd, msg, w, l),
                };
                PostMessageW(hwnd, ACTION, action, 0);
                return 0;
            }
            TRAY => {
                let event = (l as u32) & 0xffff;
                if event == WM_LBUTTONUP || event == NIN_SELECT || event == (NIN_SELECT | 1) {
                    PostMessageW(hwnd, ACTION, SHOW, 0);
                }
                if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                    PostMessageW(hwnd, ACTION, MENU, 0);
                }
                return 0;
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                return 0;
            }
            WM_DESTROYCLIPBOARD => {
                clipboard::release_private_handles();
                return 0;
            }
            ACTION => {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
                if !ptr.is_null() {
                    (*ptr).action(w);
                }
                return 0;
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, w, l)
    }
}

unsafe fn control(hwnd: HWND, class: &str, text: &str, style: u32, id: usize, font: HFONT) -> HWND {
    unsafe {
        let child = CreateWindowExW(
            0,
            wide(class).as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            0,
            0,
            hwnd,
            id as HMENU,
            GetModuleHandleW(null()),
            null(),
        );
        SendMessageW(child, WM_SETFONT, font as usize, 1);
        child
    }
}

fn window_text(hwnd: HWND) -> String {
    unsafe {
        let length = GetWindowTextLengthW(hwnd);
        let mut text = vec![0u16; length as usize + 1];
        let n = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
        String::from_utf16_lossy(&text[..n as usize])
    }
}

impl App {
    unsafe fn create(store: Store, db_path: PathBuf) -> Result<Box<Self>> {
        unsafe {
            let instance = GetModuleHandleW(null());
            let class_name = wide(CLASS);
            let class = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance,
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
                lpszClassName: class_name.as_ptr(),
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                return Err("Не удалось зарегистрировать окно".into());
            }
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                wide(CLASS).as_ptr(),
                wide("Буфер обмена — история").as_ptr(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                860,
                620,
                null_mut(),
                null_mut(),
                instance,
                null(),
            );
            if hwnd.is_null() {
                return Err("Не удалось создать окно".into());
            }
            let font = CreateFontW(
                -17,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                CLEARTYPE_QUALITY as u32,
                0,
                wide("Segoe UI").as_ptr(),
            );
            let search = control(
                hwnd,
                "EDIT",
                "",
                WS_BORDER | ES_AUTOHSCROLL as u32 | WS_TABSTOP,
                SEARCH_ID,
                font,
            );
            SendMessageW(
                search,
                EM_SETCUEBANNER,
                1,
                wide("Начните печатать для поиска…").as_ptr() as isize,
            );
            let list = control(
                hwnd,
                "LISTBOX",
                "",
                WS_BORDER
                    | WS_VSCROLL
                    | WS_HSCROLL
                    | WS_TABSTOP
                    | LBS_NOTIFY as u32
                    | LBS_NOINTEGRALHEIGHT as u32,
                LIST_ID,
                font,
            );
            let preview = control(
                hwnd,
                "EDIT",
                "",
                WS_BORDER
                    | WS_VSCROLL
                    | ES_MULTILINE as u32
                    | ES_READONLY as u32
                    | ES_AUTOVSCROLL as u32,
                106,
                font,
            );
            let image_window = control(hwnd, "STATIC", "", WS_BORDER, 108, font);
            let image_state = std::rc::Rc::new(std::cell::RefCell::new(None));
            SetWindowSubclass(
                image_window,
                Some(image::paint),
                1,
                std::rc::Rc::as_ptr(&image_state) as usize,
            );
            ShowWindow(image_window, SW_HIDE);
            let status = control(hwnd, "STATIC", "", 0, 107, font);
            let copy = control(
                hwnd,
                "BUTTON",
                "Скопировать · Enter",
                WS_TABSTOP,
                COPY_ID,
                font,
            );
            let prev = control(hwnd, "BUTTON", "← Новее", WS_TABSTOP, PREV_ID, font);
            let next = control(hwnd, "BUTTON", "Старее →", WS_TABSTOP, NEXT_ID, font);
            if [search, list, preview, status, copy, prev, next]
                .iter()
                .any(|h| h.is_null())
            {
                DestroyWindow(hwnd);
                return Err("Не удалось создать элементы окна".into());
            }
            let mut tray = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: 1,
                uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
                uCallbackMessage: TRAY,
                hIcon: LoadIconW(null_mut(), IDI_APPLICATION),
                ..Default::default()
            };
            let tip = wide("Буфер обмена — нажмите для истории");
            tray.szTip[..tip.len()].copy_from_slice(&tip);
            if Shell_NotifyIconW(NIM_ADD, &tray) == 0 {
                DestroyWindow(hwnd);
                return Err("Не удалось добавить значок в трей".into());
            }
            let mut app = Box::new(Self {
                hwnd,
                search,
                list,
                preview,
                image_window,
                image_state,
                status,
                copy,
                next,
                prev,
                store,
                rows: Vec::new(),
                filter: String::new(),
                offset: 0,
                font,
                tray,
                taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
                own_sequence: 0,
                last_sequence: 0,
                retries: 0,
                visible: false,
                db_path,
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, app.as_mut() as *mut Self as isize);
            if AddClipboardFormatListener(hwnd) == 0 {
                return Err("Не удалось подписаться на изменения буфера".into());
            }
            PostMessageW(hwnd, ACTION, CAPTURE, 0);
            Ok(app)
        }
    }

    fn report(&self, message: &str) {
        log_error(&self.db_path, message);
        unsafe {
            SetWindowTextW(self.status, wide(message).as_ptr());
        }
    }

    unsafe fn layout(&self) {
        unsafe {
            let mut rect = RECT::default();
            GetClientRect(self.hwnd, &mut rect);
            let width = rect.right.max(380);
            let height = rect.bottom.max(300);
            MoveWindow(self.search, 12, 12, width - 24, 32, 1);
            let list_height = ((height - 140) * 55 / 100).max(80);
            MoveWindow(self.list, 12, 54, width - 24, list_height, 1);
            let image_visible = self.image_state.borrow().is_some();
            let preview_width = if image_visible {
                (width - 36) / 2
            } else {
                width - 24
            };
            MoveWindow(
                self.preview,
                12,
                64 + list_height,
                preview_width,
                (height - 130 - list_height).max(50),
                1,
            );
            MoveWindow(
                self.image_window,
                24 + preview_width,
                64 + list_height,
                width - preview_width - 36,
                (height - 130 - list_height).max(50),
                1,
            );
            ShowWindow(
                self.image_window,
                if image_visible { SW_SHOW } else { SW_HIDE },
            );
            MoveWindow(self.status, 12, height - 56, width - 24, 20, 1);
            MoveWindow(self.prev, 12, height - 32, 105, 28, 1);
            MoveWindow(self.next, 125, height - 32, 105, 28, 1);
            MoveWindow(self.copy, width - 217, height - 32, 205, 28, 1);
        }
    }

    unsafe fn refresh(&mut self) {
        match self.store.list(&self.filter, self.offset) {
            Ok(rows) => unsafe {
                SendMessageW(self.list, WM_SETREDRAW, 0, 0);
                SendMessageW(self.list, LB_RESETCONTENT, 0, 0);
                for row in &rows {
                    SendMessageW(
                        self.list,
                        LB_ADDSTRING,
                        0,
                        wide(&row.label).as_ptr() as isize,
                    );
                }
                self.rows = rows;
                SendMessageW(self.list, LB_SETCURSEL, 0, 0);
                SendMessageW(self.list, WM_SETREDRAW, 1, 0);
                InvalidateRect(self.list, null(), 1);
                EnableWindow(self.prev, (self.offset > 0) as i32);
                EnableWindow(self.next, (self.rows.len() == PAGE_SIZE) as i32);
                EnableWindow(self.copy, (!self.rows.is_empty()) as i32);
                self.selection();
                let total = self.store.count().unwrap_or_default();
                let label = if self.rows.is_empty() {
                    format!("Ничего не найдено · Всего записей: {total}")
                } else {
                    format!(
                        "{}–{} · Всего записей: {total} · Двойной щелчок / Enter — скопировать",
                        self.offset + 1,
                        self.offset + self.rows.len()
                    )
                };
                SetWindowTextW(self.status, wide(&label).as_ptr());
            },
            Err(e) => self.report(&e.to_string()),
        }
    }

    unsafe fn selection(&self) {
        unsafe {
            let index = SendMessageW(self.list, LB_GETCURSEL, 0, 0);
            let text = self
                .rows
                .get(index as usize)
                .and_then(|row| self.store.preview(row.id).ok())
                .unwrap_or_default();
            SetWindowTextW(self.preview, wide(&text).as_ptr());
            let image = self
                .rows
                .get(index as usize)
                .and_then(|row| self.store.image(row.id).ok().flatten())
                .and_then(image::Image::from_dib);
            *self.image_state.borrow_mut() = image;
            self.layout();
            image::invalidate(self.image_window);
        }
    }

    unsafe fn action(&mut self, action: usize) {
        unsafe {
            match action {
                SHOW => {
                    self.visible = true;
                    self.offset = 0;
                    self.filter.clear();
                    SetWindowTextW(self.search, wide("").as_ptr());
                    self.refresh();
                    self.layout();
                    ShowWindow(self.hwnd, SW_RESTORE);
                    SetForegroundWindow(self.hwnd);
                    SetFocus(self.search);
                }
                HIDE => {
                    self.visible = false;
                    ShowWindow(self.hwnd, SW_HIDE);
                    SendMessageW(self.list, LB_RESETCONTENT, 0, 0);
                    SetWindowTextW(self.preview, wide("").as_ptr());
                    self.rows.clear();
                    self.rows.shrink_to_fit();
                    *self.image_state.borrow_mut() = None;
                    let _ = self.store.0.execute_batch("PRAGMA shrink_memory;");
                }
                REFRESH => {
                    if self.visible {
                        self.refresh();
                    }
                }
                SELECT => self.selection(),
                RESIZE => self.layout(),
                SEARCH => {
                    let filter = window_text(self.search);
                    if filter != self.filter {
                        self.filter = filter;
                        self.offset = 0;
                        SetTimer(self.hwnd, TIMER_SEARCH, 70, None);
                    }
                }
                NEXT => {
                    if self.rows.len() == PAGE_SIZE {
                        self.offset += PAGE_SIZE;
                        self.refresh();
                    }
                }
                PREV => {
                    self.offset = self.offset.saturating_sub(PAGE_SIZE);
                    self.refresh();
                }
                COPY => {
                    let selected = SendMessageW(self.list, LB_GETCURSEL, 0, 0);
                    if let Some(row) = self.rows.get(selected as usize) {
                        match clipboard::restore(&self.store, row.id, self.hwnd) {
                            Ok(sequence) => {
                                self.own_sequence = sequence;
                                self.last_sequence = sequence;
                                self.action(HIDE);
                            }
                            Err(e) => self.report(&e.to_string()),
                        }
                    }
                }
                CAPTURE => {
                    let sequence = GetClipboardSequenceNumber();
                    if sequence == self.own_sequence || sequence == self.last_sequence {
                        return;
                    }
                    match clipboard::capture(&mut self.store, self.hwnd) {
                        Ok(_) => {
                            self.last_sequence = GetClipboardSequenceNumber();
                            self.retries = 0;
                            if self.visible {
                                self.offset = 0;
                                self.refresh();
                            }
                        }
                        Err(e) => {
                            self.retries += 1;
                            if self.retries <= 20 {
                                SetTimer(self.hwnd, TIMER_CAPTURE, 100, None);
                            } else {
                                self.last_sequence = sequence;
                                self.retries = 0;
                                self.report(&e.to_string());
                            }
                        }
                    }
                }
                MENU => {
                    let menu = CreatePopupMenu();
                    AppendMenuW(menu, MF_STRING, SHOW, wide("История буфера").as_ptr());
                    AppendMenuW(
                        menu,
                        MF_STRING
                            | if startup::enabled() {
                                MF_CHECKED
                            } else {
                                MF_UNCHECKED
                            },
                        STARTUP,
                        wide("Запускать с Windows").as_ptr(),
                    );
                    AppendMenuW(menu, MF_SEPARATOR, 0, null());
                    AppendMenuW(menu, MF_STRING, EXIT, wide("Выход").as_ptr());
                    let mut p = POINT::default();
                    GetCursorPos(&mut p);
                    SetForegroundWindow(self.hwnd);
                    let command = TrackPopupMenu(
                        menu,
                        TPM_RETURNCMD | TPM_RIGHTBUTTON,
                        p.x,
                        p.y,
                        0,
                        self.hwnd,
                        null(),
                    );
                    DestroyMenu(menu);
                    if command > 0 {
                        PostMessageW(self.hwnd, ACTION, command as usize, 0);
                    }
                    PostMessageW(self.hwnd, WM_NULL, 0, 0);
                }
                EXIT => {
                    DestroyWindow(self.hwnd);
                }
                STARTUP => {
                    if let Err(e) = startup::set(!startup::enabled()) {
                        self.report(&e.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    unsafe fn key(&self, msg: &MSG) -> bool {
        unsafe {
            if !self.visible || msg.message != WM_KEYDOWN {
                return false;
            }
            let key = msg.wParam as u16;
            if key == VK_ESCAPE {
                PostMessageW(self.hwnd, ACTION, HIDE, 0);
                return true;
            }
            if key == VK_RETURN {
                PostMessageW(self.hwnd, ACTION, COPY, 0);
                return true;
            }
            if key == VK_DOWN || key == VK_UP {
                let current = SendMessageW(self.list, LB_GETCURSEL, 0, 0);
                let last = self.rows.len().saturating_sub(1) as isize;
                let next = if key == VK_DOWN {
                    (current + 1).min(last)
                } else {
                    (current - 1).max(0)
                };
                SendMessageW(self.list, LB_SETCURSEL, next as usize, 0);
                PostMessageW(self.hwnd, ACTION, SELECT, 0);
                return true;
            }
            if key == VK_F && GetKeyState(VK_CONTROL as i32) < 0 {
                SetFocus(self.search);
                return true;
            }
            // Typing while a row or preview has focus starts filtering immediately.
            if msg.hwnd != self.search
                && key != VK_TAB
                && key != VK_CONTROL
                && key != VK_SHIFT
                && key != VK_MENU
                && GetKeyState(VK_CONTROL as i32) >= 0
                && GetKeyState(VK_MENU as i32) >= 0
            {
                SetFocus(self.search);
                let mut forwarded = *msg;
                forwarded.hwnd = self.search;
                TranslateMessage(&forwarded);
                DispatchMessageW(&forwarded);
                return true;
            }
            false
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        unsafe {
            RemoveClipboardFormatListener(self.hwnd);
            Shell_NotifyIconW(NIM_DELETE, &self.tray);
            DeleteObject(self.font);
        }
    }
}

fn run() -> Result<()> {
    unsafe {
        SetProcessDPIAware();
    }
    let dir = data_path()?;
    std::fs::create_dir_all(&dir)?;
    let db = dir.join("history.sqlite");
    let mutex = unsafe { CreateMutexW(null(), 0, wide("Local\\RustClipboardManager.v1").as_ptr()) };
    if mutex.is_null() {
        return Err("Не удалось создать блокировку приложения".into());
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let existing = FindWindowW(wide(CLASS).as_ptr(), null());
            if !existing.is_null() {
                PostMessageW(existing, ACTION, SHOW, 0);
            }
            CloseHandle(mutex);
        }
        return Ok(());
    }
    let store = Store::open(&db)?;
    let app = unsafe { App::create(store, db)? };
    let mut msg = MSG::default();
    unsafe {
        loop {
            let result = GetMessageW(&mut msg, null_mut(), 0, 0);
            if result == 0 {
                break;
            }
            if result < 0 {
                return Err("Ошибка цикла сообщений".into());
            }
            if msg.message == app.taskbar_created {
                Shell_NotifyIconW(NIM_ADD, &app.tray);
            }
            let keyboard = matches!(
                msg.message,
                WM_KEYDOWN | WM_SYSKEYDOWN | WM_CHAR | WM_SYSCHAR
            );
            if !app.key(&msg) && (!keyboard || IsDialogMessageW(app.hwnd, &msg) == 0) {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        CloseHandle(mutex);
    }
    drop(app);
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        if let Ok(dir) = data_path() {
            log_error(&dir.join("history.sqlite"), &e.to_string());
        }
        unsafe {
            MessageBoxW(
                null_mut(),
                wide(&e.to_string()).as_ptr(),
                wide("Менеджер буфера обмена").as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}
