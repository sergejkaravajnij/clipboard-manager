use std::{cell::RefCell, ptr::null};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    UI::{Shell::*, WindowsAndMessaging::*},
};

pub struct Image {
    data: Vec<u32>,
    offset: usize,
    width: i32,
    height: i32,
}

pub type ImageState = RefCell<Option<Image>>;

impl Image {
    pub fn draw(&self, dc: HDC, rect: &RECT, padding: i32, upscale: bool) -> bool {
        unsafe {
            let mut scale =
                ((rect.right - rect.left - 2 * padding).max(1) as f64 / self.width as f64).min(
                    (rect.bottom - rect.top - 2 * padding).max(1) as f64
                        / self.height.unsigned_abs() as f64,
                );
            if !upscale {
                scale = scale.min(1.0);
            }
            let width = (self.width as f64 * scale).max(1.0) as i32;
            let height = (self.height.unsigned_abs() as f64 * scale).max(1.0) as i32;
            let x = rect.left + (rect.right - rect.left - width) / 2;
            let y = rect.top + (rect.bottom - rect.top - height) / 2;
            SetStretchBltMode(dc, if scale > 1.0 { COLORONCOLOR } else { HALFTONE });
            SetBrushOrgEx(dc, 0, 0, std::ptr::null_mut());
            let bytes = self.data.as_ptr().cast::<u8>();
            let result = StretchDIBits(
                dc,
                x,
                y,
                width,
                height,
                0,
                0,
                self.width,
                self.height.unsigned_abs() as i32,
                bytes.add(self.offset).cast(),
                bytes.cast(),
                DIB_RGB_COLORS,
                SRCCOPY,
            );
            result != 0 && result != -1
        }
    }

    pub fn thumbnail(&self) -> Option<Vec<u8>> {
        let scale = (64.0 / self.width as f64).min(48.0 / self.height.unsigned_abs() as f64);
        let width = (self.width as f64 * scale).max(1.0) as i32;
        let height = (self.height.unsigned_abs() as f64 * scale).max(1.0) as i32;
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            if screen.is_null() {
                return None;
            }
            let dc = CreateCompatibleDC(screen);
            if dc.is_null() {
                ReleaseDC(std::ptr::null_mut(), screen);
                return None;
            }
            let header = BITMAPINFOHEADER {
                biSize: 40,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..Default::default()
            };
            let info = BITMAPINFO {
                bmiHeader: header,
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(
                dc,
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                std::ptr::null_mut(),
                0,
            );
            if bitmap.is_null() {
                DeleteDC(dc);
                ReleaseDC(std::ptr::null_mut(), screen);
                return None;
            }
            let previous = SelectObject(dc, bitmap);
            let rect = RECT {
                left: 0,
                top: 0,
                right: width,
                bottom: height,
            };
            FillRect(dc, &rect, GetSysColorBrush(COLOR_WINDOW));
            let drawn = self.draw(dc, &rect, 0, true);
            GdiFlush();
            let result = if drawn && !bits.is_null() {
                let mut data = Vec::with_capacity(40 + (width * height * 4) as usize);
                data.extend_from_slice(std::slice::from_raw_parts(
                    (&header as *const BITMAPINFOHEADER).cast::<u8>(),
                    40,
                ));
                data.extend_from_slice(std::slice::from_raw_parts(
                    bits.cast::<u8>(),
                    (width * height * 4) as usize,
                ));
                Some(data)
            } else {
                None
            };
            SelectObject(dc, previous);
            DeleteObject(bitmap);
            DeleteDC(dc);
            ReleaseDC(std::ptr::null_mut(), screen);
            result
        }
    }
    pub fn from_dib(bytes: Vec<u8>) -> Option<Self> {
        if bytes.len() < 40 {
            return None;
        }
        let header = u32::from_le_bytes(bytes[0..4].try_into().ok()?) as usize;
        if header < 40 || header > bytes.len() {
            return None;
        }
        let width = i32::from_le_bytes(bytes[4..8].try_into().ok()?);
        let height = i32::from_le_bytes(bytes[8..12].try_into().ok()?);
        let bpp = u16::from_le_bytes(bytes[14..16].try_into().ok()?);
        let compression = u32::from_le_bytes(bytes[16..20].try_into().ok()?);
        let colors = u32::from_le_bytes(bytes[32..36].try_into().ok()?) as usize;
        if width <= 0 || height == 0 || height == i32::MIN || ![1, 4, 8, 16, 24, 32].contains(&bpp)
        {
            return None;
        }
        let palette = if colors > 0 {
            colors
        } else if bpp <= 8 {
            1usize << bpp
        } else {
            0
        };
        let masks = if header == 40 && compression == 3 {
            12
        } else if header == 40 && compression == 6 {
            16
        } else {
            0
        };
        let offset = header
            .checked_add(palette.checked_mul(4)?)?
            .checked_add(masks)?;
        if offset >= bytes.len() {
            return None;
        }
        if compression == 0 || compression == 3 || compression == 6 {
            let stride = (width as usize)
                .checked_mul(bpp as usize)?
                .div_ceil(32)
                .checked_mul(4)?;
            let needed = stride.checked_mul(height.unsigned_abs() as usize)?;
            if needed > bytes.len() - offset {
                return None;
            }
        } else if compression != 1 && compression != 2 {
            return None;
        }
        let mut data = vec![0u32; bytes.len().div_ceil(4)];
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), data.as_mut_ptr().cast(), bytes.len());
        }
        Some(Self {
            data,
            offset,
            width,
            height,
        })
    }
}

pub unsafe extern "system" fn paint(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    state: usize,
) -> LRESULT {
    unsafe {
        if msg != WM_PAINT {
            return DefSubclassProc(hwnd, msg, w, l);
        }
        let state = &*(state as *const ImageState);
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect);
        FillRect(dc, &rect, (COLOR_WINDOW + 1) as HBRUSH);
        if let Some(image) = state.borrow().as_ref() {
            image.draw(dc, &rect, 8, false);
        }
        EndPaint(hwnd, &ps);
        0
    }
}

pub fn invalidate(hwnd: HWND) {
    unsafe {
        InvalidateRect(hwnd, null(), 1);
    }
}

#[cfg(test)]
pub fn thumbnail_fixture() -> Vec<u8> {
    let mut dib = vec![0u8; 40 + 256 * 128 * 4];
    dib[..4].copy_from_slice(&40u32.to_le_bytes());
    dib[4..8].copy_from_slice(&256i32.to_le_bytes());
    dib[8..12].copy_from_slice(&(-128i32).to_le_bytes());
    dib[12..14].copy_from_slice(&1u16.to_le_bytes());
    dib[14..16].copy_from_slice(&32u16.to_le_bytes());
    for y in 0..128 {
        for x in 0..256 {
            let start = 40 + (y * 256 + x) * 4;
            dib[start..start + 4].copy_from_slice(if y < 64 {
                &[0, 0, 255, 255]
            } else {
                &[255, 0, 0, 255]
            });
        }
    }
    dib
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_truncated_or_invalid_dib() {
        assert!(Image::from_dib(vec![0; 12]).is_none());
        let mut data = vec![0; 40];
        data[..4].copy_from_slice(&40u32.to_le_bytes());
        data[4..8].copy_from_slice(&10i32.to_le_bytes());
        data[8..12].copy_from_slice(&10i32.to_le_bytes());
        data[14..16].copy_from_slice(&32u16.to_le_bytes());
        assert!(Image::from_dib(data).is_none());
    }

    #[test]
    fn thumbnail_is_bounded_and_preserves_image_orientation() {
        let original = Image::from_dib(thumbnail_fixture()).unwrap();
        let dib = original
            .thumbnail()
            .expect("Native thumbnail rendering failed");
        assert_eq!(i32::from_le_bytes(dib[4..8].try_into().unwrap()), 64);
        assert_eq!(i32::from_le_bytes(dib[8..12].try_into().unwrap()), -32);
        assert_eq!(dib.len(), 40 + 64 * 32 * 4);
        assert_eq!(
            &dib[40 + (8 * 64 + 32) * 4..40 + (8 * 64 + 32) * 4 + 3],
            &[0, 0, 255]
        );
        assert_eq!(
            &dib[40 + (24 * 64 + 32) * 4..40 + (24 * 64 + 32) * 4 + 3],
            &[255, 0, 0]
        );
        assert!(Image::from_dib(dib).is_some());
    }
}
