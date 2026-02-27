use std::fs;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::ptr::copy_nonoverlapping;

use anyhow::{Context, Result, bail};
use image::RgbaImage;
use time::OffsetDateTime;
use time::format_description;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::Graphics::Gdi::{BI_RGB, BITMAPINFOHEADER};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::UI::Controls::Dialogs::{
    CommDlgExtendedError, GetSaveFileNameW, OFN_EXPLORER, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST,
    OPENFILENAMEW,
};
use windows::core::{PCWSTR, PWSTR};

const CF_DIB: u32 = 8;

pub fn copy_to_clipboard(image: &RgbaImage) -> Result<()> {
    let dib = build_dib(image)?;

    unsafe {
        OpenClipboard(HWND::default()).context("open clipboard")?;
        let _guard = ClipboardGuard;

        EmptyClipboard().context("empty clipboard")?;

        let allocation = GlobalAlloc(GMEM_MOVEABLE, dib.len()).context("allocate clipboard DIB")?;
        let mut transferred = false;

        let result = copy_dib_to_global_memory(allocation, &dib).and_then(|()| {
            SetClipboardData(CF_DIB, HANDLE(allocation.0)).context("set clipboard DIB")?;
            transferred = true;
            Ok(())
        });

        if !transferred {
            let _ = GlobalFree(allocation);
        }

        result
    }
}

pub fn save_png(
    image: &RgbaImage,
    output: Option<PathBuf>,
    default_dir: &Path,
) -> Result<Option<PathBuf>> {
    let path = if let Some(explicit) = output {
        normalize_png_extension(explicit)
    } else {
        let Some(picked) = prompt_save_path(default_dir)? else {
            return Ok(None);
        };
        normalize_png_extension(picked)
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create parent directory {}", parent.display()))?;
    }

    image
        .save(&path)
        .with_context(|| format!("save image to {}", path.display()))?;
    Ok(Some(path))
}

fn normalize_png_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        return path.with_extension("png");
    }
    path
}

fn default_filename() -> String {
    let stamp = timestamp_for_filename();
    format!("pyro-{stamp}.png")
}

fn prompt_save_path(default_dir: &Path) -> Result<Option<PathBuf>> {
    let filter = "PNG Files (*.png)\0*.png\0\0"
        .encode_utf16()
        .collect::<Vec<u16>>();
    let title = "Save Screenshot\0".encode_utf16().collect::<Vec<u16>>();
    let def_ext = "png\0".encode_utf16().collect::<Vec<u16>>();
    let initial_dir = path_to_wide(default_dir);

    const MAX_FILE_CHARS: usize = 32768;
    let mut file_buf = vec![0u16; MAX_FILE_CHARS];
    let filename = default_filename().encode_utf16().collect::<Vec<u16>>();
    let copy_len = filename.len().min(MAX_FILE_CHARS.saturating_sub(1));
    file_buf[..copy_len].copy_from_slice(&filename[..copy_len]);

    let mut ofn = OPENFILENAMEW::default();
    ofn.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    ofn.lpstrFilter = PCWSTR(filter.as_ptr());
    ofn.nFilterIndex = 1;
    ofn.lpstrFile = PWSTR(file_buf.as_mut_ptr());
    ofn.nMaxFile = MAX_FILE_CHARS as u32;
    ofn.lpstrInitialDir = PCWSTR(initial_dir.as_ptr());
    ofn.lpstrTitle = PCWSTR(title.as_ptr());
    ofn.lpstrDefExt = PCWSTR(def_ext.as_ptr());
    ofn.Flags = OFN_EXPLORER | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST;

    let picked = unsafe { GetSaveFileNameW(&mut ofn).as_bool() };
    if !picked {
        let err = unsafe { CommDlgExtendedError() };
        if err.0 == 0 {
            return Ok(None);
        }
        bail!("save dialog failed (code: {})", err.0);
    }

    let len = file_buf
        .iter()
        .position(|&ch| ch == 0)
        .unwrap_or(file_buf.len());
    if len == 0 {
        return Ok(None);
    }

    let selected = String::from_utf16(&file_buf[..len]).context("decode selected save path")?;
    Ok(Some(PathBuf::from(selected)))
}

fn path_to_wide(path: &Path) -> Vec<u16> {
    let mut wide = path.to_string_lossy().encode_utf16().collect::<Vec<u16>>();
    wide.push(0);
    wide
}

fn timestamp_for_filename() -> String {
    let format = format_description::parse("[year][month][day]-[hour][minute][second]")
        .unwrap_or_else(|_| Vec::new());
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    now.format(&format)
        .unwrap_or_else(|_| "capture".to_string())
}

fn build_dib(image: &RgbaImage) -> Result<Vec<u8>> {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let pixel_bytes = width
        .checked_mul(height)
        .and_then(|v| v.checked_mul(4))
        .context("image dimensions are too large")?;

    let header = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width as i32,
        biHeight: height as i32, // CF_DIB prefers bottom-up rows.
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        biSizeImage: pixel_bytes as u32,
        ..Default::default()
    };

    let mut dib = vec![0_u8; size_of::<BITMAPINFOHEADER>() + pixel_bytes];
    let header_bytes = unsafe {
        std::slice::from_raw_parts(
            &header as *const BITMAPINFOHEADER as *const u8,
            size_of::<BITMAPINFOHEADER>(),
        )
    };
    dib[..size_of::<BITMAPINFOHEADER>()].copy_from_slice(header_bytes);

    let src = image.as_raw();
    let dst_pixels = &mut dib[size_of::<BITMAPINFOHEADER>()..];
    for y in 0..height {
        let src_row = height - 1 - y;
        let src_start = src_row * width * 4;
        let dst_start = y * width * 4;

        for x in 0..width {
            let si = src_start + (x * 4);
            let di = dst_start + (x * 4);
            dst_pixels[di] = src[si + 2];
            dst_pixels[di + 1] = src[si + 1];
            dst_pixels[di + 2] = src[si];
            dst_pixels[di + 3] = src[si + 3];
        }
    }

    Ok(dib)
}

fn copy_dib_to_global_memory(handle: HGLOBAL, dib: &[u8]) -> Result<()> {
    unsafe {
        let ptr = GlobalLock(handle);
        if ptr.is_null() {
            bail!("GlobalLock failed");
        }

        copy_nonoverlapping(dib.as_ptr(), ptr as *mut u8, dib.len());

        // GlobalUnlock can report a false value even on success when lock count reaches zero.
        let _ = GlobalUnlock(handle);
    }
    Ok(())
}

struct ClipboardGuard;

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}
