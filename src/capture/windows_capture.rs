use std::ffi::c_void;
use std::mem::size_of;

use anyhow::{Context, Result, bail};
use image::RgbaImage;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
    CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HGDIOBJ,
    ReleaseDC, SRCCOPY, SelectObject,
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

    let bitmap = unsafe { CreateCompatibleBitmap(screen_dc, width, height) };
    if bitmap.0.is_null() {
        unsafe {
            let _ = DeleteDC(memory_dc);
            let _ = ReleaseDC(HWND::default(), screen_dc);
        }
        bail!("CreateCompatibleBitmap failed");
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

    let mut bitmap_info = BITMAPINFO::default();
    bitmap_info.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height, // negative = top-down DIB
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };

    let rows = unsafe {
        GetDIBits(
            memory_dc,
            bitmap,
            0,
            height as u32,
            Some(buffer.as_mut_ptr() as *mut c_void),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        )
    };

    unsafe {
        let _ = SelectObject(memory_dc, original_obj);
        let _ = DeleteObject(bitmap);
        let _ = DeleteDC(memory_dc);
        let _ = ReleaseDC(HWND::default(), screen_dc);
    }

    if rows == 0 {
        bail!("GetDIBits failed");
    }

    // Convert BGRA from GDI to RGBA.
    for pixel in buffer.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    RgbaImage::from_raw(width as u32, height as u32, buffer)
        .context("failed to construct RGBA image buffer")
}
