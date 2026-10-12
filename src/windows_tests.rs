use super::*;
use rusqlite::params;
use std::time::{Duration, Instant};

struct RestoreClipboard {
    store: Store,
    original: Option<i64>,
    hwnd: HWND,
}
impl Drop for RestoreClipboard {
    fn drop(&mut self) {
        if let Some(id) = self.original {
            if let Err(e) = clipboard::restore(&self.store, id, self.hwnd) {
                eprintln!("Clipboard restore failed: {e}");
            }
        } else {
            unsafe {
                if OpenClipboard(self.hwnd) != 0 {
                    EmptyClipboard();
                    CloseClipboard();
                }
            }
        }
    }
}

fn pump(app: &App, duration: Duration) {
    let start = Instant::now();
    unsafe {
        while start.elapsed() < duration {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message != WM_QUIT && !app.key(&msg) {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn utf16(text: &str) -> Vec<u8> {
    wide(text).iter().flat_map(|w| w.to_le_bytes()).collect()
}

#[test]
#[ignore = "Uses the real Windows clipboard; restores its previous contents"]
fn windows_clipboard_tray_search_restore_and_restart() -> Result<()> {
    unsafe {
        SetProcessDPIAware();
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let directory = std::env::temp_dir().join(format!("clipboard-runtime-test-{stamp}"));
    std::fs::create_dir_all(&directory)?;
    let db = directory.join("history.sqlite");
    let mut app = unsafe { App::create(Store::open(&db)?, db.clone())? };
    let mut backup = Store::open(&directory.join("backup.sqlite"))?;
    let original = clipboard::capture(&mut backup, app.hwnd)?;
    let restore = RestoreClipboard {
        store: backup,
        original,
        hwnd: app.hwnd,
    };
    let html = unsafe { RegisterClipboardFormatW(wide("HTML Format").as_ptr()) };
    let rtf = unsafe { RegisterClipboardFormatW(wide("Rich Text Format").as_ptr()) };
    let custom =
        unsafe { RegisterClipboardFormatW(wide("RustClipboardManager.Test.Binary").as_ptr()) };
    let text = utf16("Привет — clipboard test 100%_\\\r\nВторая строка");
    let html_bytes = b"Version:0.9\r\n<html><body><b>test</b></body></html>\0".to_vec();
    let rtf_bytes = b"{\\rtf1\\ansi test} \0".to_vec();
    let custom_bytes = vec![0, 255, 1, 0, 42, 128];
    clipboard::set_test_formats(
        app.hwnd,
        &[
            (13, text.clone()),
            (html, html_bytes.clone()),
            (rtf, rtf_bytes.clone()),
            (custom, custom_bytes.clone()),
        ],
    )?;
    pump(&app, Duration::from_millis(250));
    assert_eq!(
        app.store.count()?,
        1,
        "Clipboard listener must save exactly once"
    );
    let row = app.store.list("ПРИВЕТ", 0)?.remove(0);
    for (id, expected) in [
        (13, &text),
        (html, &html_bytes),
        (rtf, &rtf_bytes),
        (custom, &custom_bytes),
    ] {
        let saved = app
            .store
            .format_keys(row.id)?
            .into_iter()
            .map(|k| app.store.format(row.id, k).unwrap())
            .find(|f| f.id == id)
            .unwrap();
        assert_eq!(saved.data, *expected);
    }
    unsafe {
        PostMessageW(app.hwnd, TRAY, 1, WM_LBUTTONUP as isize);
    }
    pump(&app, Duration::from_millis(100));
    assert!(app.visible);
    assert_ne!(unsafe { IsWindowVisible(app.hwnd) }, 0);
    assert_eq!(unsafe { SendMessageW(app.list, LB_GETCOUNT, 0, 0) }, 1);
    // Type without a search shortcut, while the list has focus.
    unsafe {
        SetFocus(app.list);
    }
    let mut key = MSG {
        hwnd: app.list,
        message: WM_KEYDOWN,
        wParam: VK_F as usize,
        ..Default::default()
    };
    assert!(unsafe { app.key(&key) });
    assert_eq!(unsafe { GetFocus() }, app.search);
    pump(&app, Duration::from_millis(100));
    unsafe {
        SetWindowTextW(app.search, wide("").as_ptr());
    }
    for c in "ПРИВЕТ".encode_utf16() {
        unsafe {
            SendMessageW(app.search, WM_CHAR, c as usize, 0);
        }
    }
    pump(&app, Duration::from_millis(200));
    assert_eq!(app.filter, "ПРИВЕТ");
    assert_eq!(app.rows.len(), 1);
    key.hwnd = app.search;
    key.wParam = VK_RETURN as usize;
    assert!(unsafe { app.key(&key) });
    pump(&app, Duration::from_millis(150));
    assert!(!app.visible);
    for (id, expected) in [
        (13, &text),
        (html, &html_bytes),
        (rtf, &rtf_bytes),
        (custom, &custom_bytes),
    ] {
        assert_eq!(clipboard::current_bytes(app.hwnd, id)?, *expected);
    }
    assert_eq!(
        app.store.count()?,
        1,
        "Restoring history must not create a duplicate"
    );
    println!(
        "PASS text / HTML / RTF / registered binary exact roundtrip; tray click; typing; Enter; self-copy suppression"
    );

    // A valid CF_HDROP payload: DROPFILES header followed by a double-NUL UTF-16 file list.
    let mut files = Vec::new();
    for value in [20u32, 0, 0, 0, 1] {
        files.extend_from_slice(&value.to_le_bytes());
    }
    files.extend(utf16("C:\\tools\\clipboard-manager\\Cargo.toml"));
    files.extend_from_slice(&[0, 0]);
    clipboard::set_test_formats(app.hwnd, &[(15, files.clone())])?;
    pump(&app, Duration::from_millis(150));
    let file_id = app.store.list("cargo.toml", 0)?.remove(0).id;
    app.own_sequence = clipboard::restore(&app.store, file_id, app.hwnd)?;
    pump(&app, Duration::from_millis(100));
    assert_eq!(clipboard::current_bytes(app.hwnd, 15)?, files);
    println!("PASS file list exact roundtrip and filename search");

    // Create a 2x2, 32-bit DIB with four distinguishable pixels.
    let mut dib = vec![0u8; 56];
    dib[0..4].copy_from_slice(&40u32.to_le_bytes());
    dib[4..8].copy_from_slice(&2i32.to_le_bytes());
    dib[8..12].copy_from_slice(&2i32.to_le_bytes());
    dib[12..14].copy_from_slice(&1u16.to_le_bytes());
    dib[14..16].copy_from_slice(&32u16.to_le_bytes());
    dib[20..24].copy_from_slice(&16u32.to_le_bytes());
    dib[40..].copy_from_slice(&[
        255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
    ]);
    clipboard::set_test_formats(app.hwnd, &[(8, dib.clone())])?;
    pump(&app, Duration::from_millis(150));
    let image_id = app.store.list("Изображение", 0)?.remove(0).id;
    app.own_sequence = clipboard::restore(&app.store, image_id, app.hwnd)?;
    pump(&app, Duration::from_millis(100));
    assert_eq!(clipboard::current_bytes(app.hwnd, 8)?, dib);
    println!("PASS image exact pixel roundtrip");
    unsafe {
        app.action(SHOW);
    }
    pump(&app, Duration::from_millis(150));
    assert!(app.image_state.borrow().is_some());
    save_screenshot(app.hwnd)?;

    // CF_BITMAP is an OS handle; Windows must synthesize DIB for durable storage.
    unsafe {
        let pixels = [
            255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        let bitmap = CreateBitmap(2, 2, 1, 32, pixels.as_ptr().cast());
        assert!(!bitmap.is_null());
        assert_ne!(OpenClipboard(app.hwnd), 0);
        assert_ne!(EmptyClipboard(), 0);
        assert!(!SetClipboardData(2, bitmap).is_null());
        CloseClipboard();
    }
    let synthesized = clipboard::current_bytes(app.hwnd, 8)?;
    pump(&app, Duration::from_millis(150));
    let bitmap_id = app.store.list("Изображение", 0)?.remove(0).id;
    app.own_sequence = clipboard::restore(&app.store, bitmap_id, app.hwnd)?;
    pump(&app, Duration::from_millis(100));
    assert_eq!(clipboard::current_bytes(app.hwnd, 8)?, synthesized);

    unsafe {
        let dc = CreateEnhMetaFileW(null_mut(), null(), null(), null());
        assert!(!dc.is_null());
        MoveToEx(dc, 0, 0, null_mut());
        LineTo(dc, 100, 100);
        let meta = CloseEnhMetaFile(dc);
        assert!(!meta.is_null());
        assert_ne!(OpenClipboard(app.hwnd), 0);
        assert_ne!(EmptyClipboard(), 0);
        assert!(!SetClipboardData(14, meta).is_null());
        CloseClipboard();
    }
    let metafile = clipboard::current_bytes(app.hwnd, 14)?;
    pump(&app, Duration::from_millis(150));
    let meta_id = app.store.list("Enhanced metafile", 0)?.remove(0).id;
    app.own_sequence = clipboard::restore(&app.store, meta_id, app.hwnd)?;
    pump(&app, Duration::from_millis(100));
    assert_eq!(clipboard::current_bytes(app.hwnd, 14)?, metafile);

    clipboard::set_test_formats(app.hwnd, &[(0x200, vec![7, 1, 2, 3, 0])])?;
    pump(&app, Duration::from_millis(150));
    let private_id = app.store.list("Binary", 0)?.remove(0).id;
    app.own_sequence = clipboard::restore(&app.store, private_id, app.hwnd)?;
    pump(&app, Duration::from_millis(100));
    assert_eq!(
        clipboard::current_bytes(app.hwnd, 0x200)?,
        vec![7, 1, 2, 3, 0]
    );
    println!("PASS bitmap-only conversion, enhanced metafile and private-memory format roundtrip");

    let before = app.store.count()?;
    clipboard::set_test_formats(app.hwnd, &[(13, utf16("clipboard lock retry"))])?;
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let locker = std::thread::spawn(move || {
        let opened = unsafe { OpenClipboard(null_mut()) } != 0;
        ready_tx.send(opened).unwrap();
        let _ = release_rx.recv();
        if opened {
            unsafe {
                CloseClipboard();
            }
        }
    });
    assert!(ready_rx.recv()?);
    unsafe {
        app.action(CAPTURE);
    }
    assert!(
        app.retries > 0,
        "Clipboard contention must schedule retries"
    );
    release_tx.send(())?;
    locker.join().unwrap();
    pump(&app, Duration::from_millis(200));
    assert_eq!(app.store.count()?, before + 1);
    assert_eq!(app.retries, 0);
    println!("PASS busy clipboard retry without losing the entry");

    // Accumulated history stays on disk; the UI loads only one page.
    for n in 0..220 {
        app.store.0.execute(
            "INSERT INTO entries(created,title,search,preview) VALUES(0,?,?,?)",
            params![
                format!("page {n}"),
                format!("page {n}"),
                format!("page {n}")
            ],
        )?;
    }
    unsafe {
        app.action(SHOW);
    }
    pump(&app, Duration::from_millis(100));
    assert_eq!(app.rows.len(), 100);
    unsafe {
        app.action(NEXT);
    }
    assert_eq!(app.offset, 100);
    assert_eq!(app.rows.len(), 100);
    unsafe {
        app.action(NEXT);
    }
    assert_eq!(app.offset, 200);
    assert!(app.rows.len() < 100);
    unsafe {
        app.action(PREV);
    }
    assert_eq!(app.offset, 100);
    let persisted = app.store.count()?;
    let reopened = Store::open(&db)?;
    assert_eq!(reopened.count()?, persisted);
    assert_eq!(reopened.format(image_id, 0)?.data, dib);
    println!("PASS disk persistence after reopen; paged history");
    // Restore the user's clipboard before destroying its temporary owner window.
    drop(restore);
    unsafe {
        DestroyWindow(app.hwnd);
    }
    drop(app);
    drop(reopened);
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

fn save_screenshot(hwnd: HWND) -> Result<()> {
    unsafe {
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect);
        let dc = GetDC(hwnd);
        let mem = CreateCompatibleDC(dc);
        let bitmap = CreateCompatibleBitmap(dc, rect.right, rect.bottom);
        let old = SelectObject(mem, bitmap);
        BitBlt(mem, 0, 0, rect.right, rect.bottom, dc, 0, 0, SRCCOPY);
        SelectObject(mem, old);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: 40,
                biWidth: rect.right,
                biHeight: rect.bottom,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bytes = vec![0u8; (rect.right * rect.bottom * 4) as usize];
        let lines = GetDIBits(
            mem,
            bitmap,
            0,
            rect.bottom as u32,
            bytes.as_mut_ptr().cast(),
            &mut info,
            DIB_RGB_COLORS,
        );
        DeleteObject(bitmap);
        DeleteDC(mem);
        ReleaseDC(hwnd, dc);
        assert_eq!(lines, rect.bottom);
        let size = 54 + bytes.len() as u32;
        let mut bmp = Vec::new();
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&size.to_le_bytes());
        bmp.extend_from_slice(&[0; 4]);
        bmp.extend_from_slice(&54u32.to_le_bytes());
        bmp.extend_from_slice(std::slice::from_raw_parts(
            (&info.bmiHeader as *const BITMAPINFOHEADER).cast(),
            40,
        ));
        bmp.extend(bytes);
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("history.bmp"), bmp)?;
    }
    Ok(())
}
