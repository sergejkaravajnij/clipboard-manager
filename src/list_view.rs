use crate::{clipboard::wide, image::Image};
use std::cell::RefCell;
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    UI::{Controls::*, WindowsAndMessaging::*},
};

pub const THUMBNAIL_WIDTH: i32 = 64;
pub struct Visual {
    pub label: String,
    pub has_image: bool,
    pub copied: bool,
    pub thumbnail: Option<Image>,
}
pub type State = RefCell<Vec<Visual>>;

unsafe fn state(hwnd: HWND) -> Option<&'static State> {
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const State;
    unsafe { pointer.as_ref() }
}

pub unsafe fn measure(hwnd: HWND, item: &mut MEASUREITEMSTRUCT) {
    let image = unsafe { state(hwnd) }
        .and_then(|s| s.borrow().get(item.itemID as usize).map(|r| r.has_image))
        .unwrap_or(false);
    item.itemHeight = if image { 60 } else { 26 };
}

pub unsafe fn draw(item: &DRAWITEMSTRUCT) -> bool {
    unsafe {
        let Some(state) = state(item.hwndItem) else {
            return false;
        };
        let rows = state.borrow();
        let font = SendMessageW(item.hwndItem, WM_GETFONT, 0, 0) as HFONT;
        let old_font = if font.is_null() {
            std::ptr::null_mut()
        } else {
            SelectObject(item.hDC, font)
        };
        let selected = item.itemState & ODS_SELECTED != 0;
        FillRect(
            item.hDC,
            &item.rcItem,
            GetSysColorBrush(if selected {
                COLOR_HIGHLIGHT
            } else {
                COLOR_WINDOW
            }),
        );
        if let Some(row) = rows.get(item.itemID as usize) {
            let previous = SetTextColor(
                item.hDC,
                GetSysColor(if selected {
                    COLOR_HIGHLIGHTTEXT
                } else {
                    COLOR_WINDOWTEXT
                }),
            );
            let old_background = SetBkMode(item.hDC, TRANSPARENT as i32);
            let marker_left = item.rcItem.left + 8 - GetScrollPos(item.hwndItem, SB_HORZ);
            if row.copied {
                let brush = CreateSolidBrush(0x0000d7ff);
                let old_brush = SelectObject(item.hDC, brush);
                let old_pen = SelectObject(item.hDC, GetStockObject(NULL_PEN));
                let center = (item.rcItem.top + item.rcItem.bottom) / 2;
                Ellipse(
                    item.hDC,
                    marker_left,
                    center - 4,
                    marker_left + 8,
                    center + 4,
                );
                SelectObject(item.hDC, old_pen);
                SelectObject(item.hDC, old_brush);
                DeleteObject(brush);
            }
            let left = marker_left + 14;
            let mut rect = item.rcItem;
            rect.left = left;
            if row.has_image {
                let image_rect = RECT {
                    left,
                    top: rect.top + 6,
                    right: left + THUMBNAIL_WIDTH,
                    bottom: rect.bottom - 6,
                };
                if let Some(image) = &row.thumbnail {
                    image.draw(item.hDC, &image_rect, 0, false);
                }
                rect.left += THUMBNAIL_WIDTH + 12;
            }
            // Allow horizontal scrolling rather than clipping text to the visible width.
            let mut size = SIZE::default();
            let mut label = wide(&row.label);
            GetTextExtentPoint32W(
                item.hDC,
                label.as_ptr(),
                (label.len() - 1) as i32,
                &mut size,
            );
            rect.right = rect.right.max(rect.left + size.cx + 8);
            DrawTextW(
                item.hDC,
                label.as_mut_ptr(),
                (label.len() - 1) as i32,
                &mut rect,
                DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );
            SetBkMode(item.hDC, old_background);
            SetTextColor(item.hDC, previous);
        }
        if item.itemState & ODS_FOCUS != 0 {
            DrawFocusRect(item.hDC, &item.rcItem);
        }
        if !old_font.is_null() {
            SelectObject(item.hDC, old_font);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ptr::null_mut, rc::Rc};
    #[test]
    fn hidden_control_draws_thumbnail_pixels_and_keeps_text_rows_compact() {
        let image = Image::from_dib(crate::image::thumbnail_fixture()).unwrap();
        let thumbnail = Image::from_dib(image.thumbnail().unwrap()).unwrap();
        let state = Rc::new(RefCell::new(vec![
            Visual {
                label: "Image 256 × 128".into(),
                has_image: true,
                copied: true,
                thumbnail: Some(thumbnail),
            },
            Visual {
                label: "Copied text".into(),
                has_image: false,
                copied: false,
                thumbnail: None,
            },
        ]));
        unsafe {
            // Stock controls are never shown; no manager, tray, hotkey or clipboard is started.
            let parent = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("").as_ptr(),
                0,
                0,
                0,
                320,
                86,
                null_mut(),
                null_mut(),
                null_mut(),
                std::ptr::null(),
            );
            assert!(!parent.is_null());
            let list = CreateWindowExW(
                0,
                wide("LISTBOX").as_ptr(),
                wide("").as_ptr(),
                WS_CHILD | LBS_OWNERDRAWVARIABLE as u32 | LBS_HASSTRINGS as u32,
                0,
                0,
                320,
                86,
                parent,
                102usize as HMENU,
                null_mut(),
                std::ptr::null(),
            );
            assert!(!list.is_null());
            SetWindowLongPtrW(list, GWLP_USERDATA, Rc::as_ptr(&state) as isize);
            let mut measure_item = MEASUREITEMSTRUCT {
                itemID: 0,
                ..Default::default()
            };
            measure(list, &mut measure_item);
            assert_eq!(measure_item.itemHeight, 60);
            measure_item.itemID = 1;
            measure(list, &mut measure_item);
            assert_eq!(measure_item.itemHeight, 26);
            let dc = CreateCompatibleDC(null_mut());
            assert!(!dc.is_null());
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: 40,
                    biWidth: 320,
                    biHeight: -86,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = null_mut();
            let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            assert!(!bitmap.is_null());
            let previous = SelectObject(dc, bitmap);
            struct Cleanup {
                parent: HWND,
                dc: HDC,
                bitmap: HBITMAP,
                previous: HGDIOBJ,
            }
            impl Drop for Cleanup {
                fn drop(&mut self) {
                    unsafe {
                        SelectObject(self.dc, self.previous);
                        DeleteObject(self.bitmap);
                        DeleteDC(self.dc);
                        DestroyWindow(self.parent);
                    }
                }
            }
            let _cleanup = Cleanup {
                parent,
                dc,
                bitmap,
                previous,
            };
            let mut item = DRAWITEMSTRUCT {
                CtlType: ODT_LISTBOX,
                CtlID: 102,
                itemID: 0,
                itemAction: ODA_DRAWENTIRE,
                hwndItem: list,
                hDC: dc,
                rcItem: RECT {
                    left: 0,
                    top: 0,
                    right: 320,
                    bottom: 60,
                },
                ..Default::default()
            };
            assert!(draw(&item));
            item.itemID = 1;
            item.rcItem.top = 60;
            item.rcItem.bottom = 86;
            assert!(draw(&item));
            GdiFlush();
            let pixels = std::slice::from_raw_parts(bits.cast::<u8>(), 320 * 86 * 4);
            let pixel = |x: usize, y: usize| &pixels[(y * 320 + x) * 4..(y * 320 + x) * 4 + 3];
            assert_eq!(pixel(12, 30), &[0, 215, 255]);
            assert_eq!(pixel(40, 22), &[0, 0, 255]);
            assert_eq!(pixel(40, 38), &[255, 0, 0]);
            let background = GetSysColor(COLOR_WINDOW);
            let bg = [
                ((background >> 16) & 255) as u8,
                ((background >> 8) & 255) as u8,
                (background & 255) as u8,
            ];
            assert!(
                (61..85).any(|y| (8..100).any(|x| pixel(x, y) != bg)),
                "Text row must actually be rendered"
            );
        }
    }
}
