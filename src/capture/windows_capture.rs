use std::ffi::c_void;
use std::mem::size_of;

use anyhow::{Context, Result, bail};
use image::RgbaImage;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
};

use crate::platform_windows::RectPx;

pub fn capture_rect(bounds: RectPx) -> Result<RgbaImage> {
    let width = bounds.width();
    let height = bounds.height();
    if width <= 0 || height <= 0 {
        bail!("invalid capture dimensions {}x{}", width, height);
    }

    let mut buffer = vec![0_u8; width as usize * height as usize * 4];
    let screen_dc = unsafe { GetDC(HWND::default()) };
    if screen_dc.0.is_null() {
        bail!("GetDC failed");
    }

    let memory_dc = unsafe { CreateCompatibleDC(screen_dc) };
    if memory_dc.0.is_null() {
        unsafe {
            let _ = ReleaseDC(HWND::default(), screen_dc);
        }
        bail!("CreateCompatibleDC failed");
    }

    let mut bitmap_info = BITMAPINFO::default();
    bitmap_info.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height, // top-down DIB
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };

    let mut dib_bits = std::ptr::null_mut::<c_void>();
    let bitmap = unsafe {
        CreateDIBSection(
            screen_dc,
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut dib_bits,
            None,
            0,
        )
    };
    let bitmap = match bitmap {
        Ok(value) => value,
        Err(_) => {
            unsafe {
                let _ = DeleteDC(memory_dc);
                let _ = ReleaseDC(HWND::default(), screen_dc);
            }
            bail!("CreateDIBSection failed");
        }
    };
    if bitmap.0.is_null() || dib_bits.is_null() {
        unsafe {
            let _ = DeleteDC(memory_dc);
            let _ = ReleaseDC(HWND::default(), screen_dc);
        }
        bail!("CreateDIBSection returned null bitmap");
    }

    let original_obj = unsafe { SelectObject(memory_dc, HGDIOBJ(bitmap.0)) };
    if original_obj.0.is_null() {
        unsafe {
            let _ = DeleteObject(bitmap);
            let _ = DeleteDC(memory_dc);
            let _ = ReleaseDC(HWND::default(), screen_dc);
        }
        bail!("SelectObject failed");
    }

    let copied = unsafe {
        BitBlt(
            memory_dc,
            0,
            0,
            width,
            height,
            screen_dc,
            bounds.left,
            bounds.top,
            SRCCOPY | CAPTUREBLT,
        )
    };
    if copied.is_err() {
        unsafe {
            let _ = SelectObject(memory_dc, original_obj);
            let _ = DeleteObject(bitmap);
            let _ = DeleteDC(memory_dc);
            let _ = ReleaseDC(HWND::default(), screen_dc);
        }
        bail!("BitBlt failed");
    }

    unsafe {
        let source = std::slice::from_raw_parts(dib_bits.cast::<u8>(), buffer.len());
        buffer.copy_from_slice(source);
    }

    unsafe {
        let _ = SelectObject(memory_dc, original_obj);
        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(memory_dc);
        let _ = ReleaseDC(HWND::default(), screen_dc);
    }

    // Convert BGRA from GDI to RGBA and force opaque alpha.
    // GDI does not guarantee meaningful alpha values in screen captures.
    for pixel in buffer.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }

    RgbaImage::from_raw(width as u32, height as u32, buffer)
        .context("failed to construct RGBA image buffer")
}
