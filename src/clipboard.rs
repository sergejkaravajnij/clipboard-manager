use crate::store::{Format, PREVIEW_CHARS, Result, Store};
use rusqlite::params;
use std::ptr::null_mut;
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{DataExchange::*, Memory::*},
    UI::Shell::DragQueryFileW,
};

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

struct Clipboard;
impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}

fn open(owner: HWND) -> Result<Clipboard> {
    if unsafe { OpenClipboard(owner) } == 0 {
        return Err("Буфер обмена занят другим приложением".into());
    }
    Ok(Clipboard)
}

pub fn format_name(id: u32) -> String {
    if id >= 0xc000 {
        let mut name = [0u16; 256];
        let len = unsafe { GetClipboardFormatNameW(id, name.as_mut_ptr(), name.len() as i32) };
        if len > 0 {
            return String::from_utf16_lossy(&name[..len as usize]);
        }
    }
    match id {
        1 => "Text",
        2 => "Bitmap",
        3 => "Metafile",
        7 => "OEM text",
        8 => "DIB image",
        9 => "Palette",
        13 => "Unicode text",
        14 => "Enhanced metafile",
        15 => "Files",
        17 => "DIBV5 image",
        _ => "Binary",
    }
    .into()
}

thread_local! {
    // Windows does not free CF_PRIVATEFIRST..CF_PRIVATELAST allocations.
    static PRIVATE_HANDLES: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn release_private_handles() {
    PRIVATE_HANDLES.with(|handles| {
        for h in handles.borrow_mut().drain(..) {
            unsafe {
                GlobalFree(h as HGLOBAL);
            }
        }
    });
}

unsafe fn read_format(id: u32) -> Result<Option<Format>> {
    // These formats contain application-owned objects, not serializable global memory.
    if id == 0x80 {
        return Err("Owner-display: данные принадлежат окну исходного приложения".into());
    }
    if id == 2 {
        return Ok(None);
    } // Windows also exposes a serializable DIB/DIBV5.
    let name = format_name(id);
    if [
        "FileContents",
        "FileGroupDescriptor",
        "FileGroupDescriptorW",
    ]
    .contains(&name.as_str())
    {
        return Err("Виртуальные файлы не входят в поддерживаемые форматы".into());
    }
    if name == "DataObject" || name == "Ole Private Data" {
        return Err(format!("{name}: ссылки на объекты процесса не переносятся в историю").into());
    }
    let handle = unsafe { GetClipboardData(id) };
    if handle.is_null() {
        return Err(format!("Не удалось прочитать формат {}", format_name(id)).into());
    }
    let (kind, data) = if id == 14 {
        let size = unsafe { GetEnhMetaFileBits(handle, 0, null_mut()) };
        if size == 0 {
            return Err("Не удалось прочитать метафайл".into());
        }
        let mut bytes = vec![0; size as usize];
        if unsafe { GetEnhMetaFileBits(handle, size, bytes.as_mut_ptr()) } != size {
            return Err("Ошибка чтения метафайла".into());
        }
        (1, bytes)
    } else if id == 3 {
        let p = unsafe { GlobalLock(handle) } as *const METAFILEPICT;
        if p.is_null() {
            return Err("Не удалось прочитать METAFILEPICT".into());
        }
        let meta = unsafe { *p };
        unsafe {
            GlobalUnlock(handle);
        }
        let size = unsafe { GetMetaFileBitsEx(meta.hMF, 0, null_mut()) };
        let mut bytes = Vec::with_capacity(12 + size as usize);
        bytes.extend_from_slice(&meta.mm.to_le_bytes());
        bytes.extend_from_slice(&meta.xExt.to_le_bytes());
        bytes.extend_from_slice(&meta.yExt.to_le_bytes());
        bytes.resize(12 + size as usize, 0);
        if size == 0
            || unsafe { GetMetaFileBitsEx(meta.hMF, size, bytes[12..].as_mut_ptr().cast()) } != size
        {
            return Err("Ошибка чтения METAFILEPICT".into());
        }
        (2, bytes)
    } else if id == 9 {
        let count = unsafe { GetPaletteEntries(handle, 0, 0, null_mut()) };
        if count == 0 || count > 65535 {
            return Err("Ошибка чтения палитры".into());
        }
        let mut entries = vec![PALETTEENTRY::default(); count as usize];
        if unsafe { GetPaletteEntries(handle, 0, count, entries.as_mut_ptr()) } != count {
            return Err("Ошибка чтения палитры".into());
        }
        let mut bytes = Vec::with_capacity(4 + count as usize * 4);
        bytes.extend_from_slice(&0x300u16.to_le_bytes());
        bytes.extend_from_slice(&(count as u16).to_le_bytes());
        for e in entries {
            bytes.extend_from_slice(&[e.peRed, e.peGreen, e.peBlue, e.peFlags]);
        }
        (3, bytes)
    } else {
        let size = unsafe { GlobalSize(handle) };
        if size == 0 {
            return Err(
                format!("Формат {} не содержит переносимых данных", format_name(id)).into(),
            );
        }
        let p = unsafe { GlobalLock(handle) };
        if p.is_null() {
            return Err("Не удалось заблокировать данные буфера".into());
        }
        let bytes = unsafe { std::slice::from_raw_parts(p.cast::<u8>(), size).to_vec() };
        unsafe {
            GlobalUnlock(handle);
        }
        (0, bytes)
    };
    Ok(Some(Format {
        id,
        name: format_name(id),
        kind,
        data,
    }))
}

pub fn capture(store: &mut Store, owner: HWND) -> Result<Option<i64>> {
    let _clipboard = open(owner)?;
    let tx = store.0.transaction()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;
    tx.execute("INSERT INTO entries(created) VALUES(?)", [now])?;
    let entry = tx.last_insert_rowid();
    let mut id = 0;
    let mut ordinal = 0;
    let mut bytes = 0i64;
    let mut text = None;
    let mut files = None;
    let mut image = None;
    let mut names = Vec::new();
    let mut warnings = Vec::new();
    loop {
        id = unsafe { EnumClipboardFormats(id) };
        if id == 0 {
            break;
        }
        let format = match unsafe { read_format(id) } {
            Ok(Some(f)) => f,
            Ok(None) => continue,
            Err(e) => {
                warnings.push(e.to_string());
                continue;
            }
        };
        if id == 13 {
            let words = format
                .data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .take_while(|w| *w != 0);
            text = Some(
                char::decode_utf16(words)
                    .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
                    .collect::<String>(),
            );
        }
        if id == 15 {
            let hdrop = unsafe { GetClipboardData(15) };
            let count = unsafe { DragQueryFileW(hdrop, u32::MAX, null_mut(), 0) };
            let mut paths = Vec::new();
            for i in 0..count {
                let len = unsafe { DragQueryFileW(hdrop, i, null_mut(), 0) };
                let mut path = vec![0u16; len as usize + 1];
                unsafe {
                    DragQueryFileW(hdrop, i, path.as_mut_ptr(), len + 1);
                }
                paths.push(String::from_utf16_lossy(&path[..len as usize]));
            }
            files = Some(paths.join("\r\n"));
        }
        if (id == 8 || id == 17) && format.data.len() >= 12 {
            let width = i32::from_le_bytes(format.data[4..8].try_into()?);
            let height = i32::from_le_bytes(format.data[8..12].try_into()?).unsigned_abs();
            image = Some(format!("Изображение {width} × {height}"));
        }
        bytes += format.data.len() as i64;
        names.push(format.name.clone());
        tx.execute(
            "INSERT INTO formats VALUES(?,?,?,?,?,?)",
            params![
                entry,
                ordinal,
                format.id,
                format.name,
                format.kind,
                format.data
            ],
        )?;
        ordinal += 1;
    }
    drop(_clipboard);
    if ordinal == 0 && warnings.is_empty() {
        return Ok(None);
    }
    let generated = text.is_none() && files.is_none();
    let content = text
        .or(files)
        .unwrap_or_else(|| image.unwrap_or_else(|| format!("Данные: {}", names.join(", "))));
    let title: String = content
        .chars()
        .take(180)
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    let preview: String = content.chars().take(PREVIEW_CHARS).collect();
    let mut preview = format!("{preview}\r\n\r\nФорматы: {}", names.join(", "));
    if !warnings.is_empty() {
        preview.push_str(&format!("\r\nНе сохранено: {}", warnings.join("; ")));
    }
    tx.execute(
        "UPDATE entries SET title=?, search=?, preview=?, bytes=?, formats=? WHERE id=?",
        params![
            title,
            if generated {
                format!(
                    "{} {}",
                    content.to_lowercase(),
                    crate::i18n::Language::English
                        .generated(&content)
                        .to_lowercase()
                )
            } else {
                content.to_lowercase()
            },
            preview,
            bytes,
            ordinal,
            entry
        ],
    )?;
    tx.commit()?;
    Ok(Some(entry))
}

unsafe fn make_handle(format: &Format) -> Result<HANDLE> {
    let bytes = &format.data;
    let h = match format.kind {
        1 => unsafe { SetEnhMetaFileBits(bytes.len() as u32, bytes.as_ptr()) },
        2 => {
            if bytes.len() < 12 {
                return Err("Повреждённый метафайл".into());
            }
            let meta =
                unsafe { SetMetaFileBitsEx((bytes.len() - 12) as u32, bytes[12..].as_ptr()) };
            if meta.is_null() {
                return Err("Ошибка восстановления метафайла".into());
            }
            let h = unsafe { GlobalAlloc(GMEM_MOVEABLE, std::mem::size_of::<METAFILEPICT>()) };
            let p = unsafe { GlobalLock(h) } as *mut METAFILEPICT;
            if p.is_null() {
                unsafe {
                    DeleteMetaFile(meta);
                    GlobalFree(h);
                }
                return Err("Недостаточно памяти".into());
            }
            unsafe {
                *p = METAFILEPICT {
                    mm: i32::from_le_bytes(bytes[0..4].try_into()?),
                    xExt: i32::from_le_bytes(bytes[4..8].try_into()?),
                    yExt: i32::from_le_bytes(bytes[8..12].try_into()?),
                    hMF: meta,
                };
                GlobalUnlock(h);
            }
            h
        }
        3 => {
            // Vec<u32> provides the alignment required by LOGPALETTE.
            let mut aligned = vec![0u32; bytes.len().div_ceil(4)];
            unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    aligned.as_mut_ptr().cast(),
                    bytes.len(),
                );
                CreatePalette(aligned.as_ptr().cast())
            }
        }
        _ => {
            let h = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) };
            let p = unsafe { GlobalLock(h) };
            if p.is_null() {
                unsafe {
                    GlobalFree(h);
                }
                return Err("Недостаточно памяти".into());
            }
            unsafe {
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), p.cast(), bytes.len());
                GlobalUnlock(h);
            }
            h
        }
    };
    if h.is_null() {
        return Err("Ошибка восстановления формата буфера".into());
    }
    Ok(h)
}

unsafe fn free_handle(kind: i32, h: HANDLE) {
    unsafe {
        match kind {
            1 => {
                DeleteEnhMetaFile(h);
            }
            2 => {
                let p = GlobalLock(h) as *const METAFILEPICT;
                if !p.is_null() {
                    DeleteMetaFile((*p).hMF);
                    GlobalUnlock(h);
                }
                GlobalFree(h);
            }
            3 => {
                DeleteObject(h);
            }
            _ => {
                GlobalFree(h);
            }
        }
    }
}

pub fn restore(store: &Store, entry: i64, owner: HWND) -> Result<u32> {
    let keys = store.format_keys(entry)?;
    if keys.is_empty() {
        return Err("Запись пуста".into());
    }
    // Construct one selected item's OS objects before touching the live clipboard.
    // Raw payloads are read from disk one at a time, never for the entire history.
    struct Pending {
        id: u32,
        kind: i32,
        handle: HANDLE,
    }
    impl Drop for Pending {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe {
                    free_handle(self.kind, self.handle);
                }
            }
        }
    }
    let mut pending = Vec::new();
    for key in keys {
        let f = store.format(entry, key)?;
        if f.data.is_empty() {
            return Err("Повреждённая запись".into());
        }
        let id = if f.id >= 0xc000 {
            unsafe { RegisterClipboardFormatW(wide(&f.name).as_ptr()) }
        } else {
            f.id
        };
        if id == 0 {
            return Err(format!("Не удалось зарегистрировать {}", f.name).into());
        }
        pending.push(Pending {
            id,
            kind: f.kind,
            handle: unsafe { make_handle(&f)? },
        });
    }
    let _clipboard = open(owner)?;
    if unsafe { EmptyClipboard() } == 0 {
        return Err("Не удалось очистить буфер обмена".into());
    }
    for item in &mut pending {
        if unsafe { SetClipboardData(item.id, item.handle) }.is_null() {
            return Err(format!("Не удалось восстановить {}", format_name(item.id)).into());
        }
        if (0x200..=0x2ff).contains(&item.id) {
            PRIVATE_HANDLES.with(|handles| handles.borrow_mut().push(item.handle as usize));
        }
        item.handle = null_mut(); // Ownership has moved to Windows / PRIVATE_HANDLES.
    }
    drop(_clipboard);
    Ok(unsafe { GetClipboardSequenceNumber() })
}

#[cfg(test)]
pub fn set_test_formats(owner: HWND, formats: &[(u32, Vec<u8>)]) -> Result<()> {
    let _clipboard = open(owner)?;
    if unsafe { EmptyClipboard() } == 0 {
        return Err("EmptyClipboard failed".into());
    }
    for (id, data) in formats {
        let format = Format {
            id: *id,
            name: format_name(*id),
            kind: 0,
            data: data.clone(),
        };
        let h = unsafe { make_handle(&format)? };
        if unsafe { SetClipboardData(*id, h) }.is_null() {
            unsafe {
                free_handle(0, h);
            }
            return Err("SetClipboardData failed".into());
        }
        if (0x200..=0x2ff).contains(id) {
            PRIVATE_HANDLES.with(|handles| handles.borrow_mut().push(h as usize));
        }
    }
    Ok(())
}

#[cfg(test)]
pub fn current_bytes(owner: HWND, id: u32) -> Result<Vec<u8>> {
    let _clipboard = open(owner)?;
    Ok(unsafe { read_format(id)? }
        .ok_or("No serializable data")?
        .data)
}
