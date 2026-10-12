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
            let scale = ((rect.right - 16).max(1) as f64 / image.width as f64)
                .min((rect.bottom - 16).max(1) as f64 / image.height.unsigned_abs() as f64)
                .min(1.0);
            let width = (image.width as f64 * scale).max(1.0) as i32;
            let height = (image.height.unsigned_abs() as f64 * scale).max(1.0) as i32;
            let x = (rect.right - width) / 2;
            let y = (rect.bottom - height) / 2;
            SetStretchBltMode(dc, HALFTONE);
            SetBrushOrgEx(dc, 0, 0, std::ptr::null_mut());
            let bytes = image.data.as_ptr().cast::<u8>();
            StretchDIBits(
                dc,
                x,
                y,
                width,
                height,
                0,
                0,
                image.width,
                image.height.unsigned_abs() as i32,
                bytes.add(image.offset).cast(),
                bytes.cast(),
                DIB_RGB_COLORS,
                SRCCOPY,
            );
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
}
