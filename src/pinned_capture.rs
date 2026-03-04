use std::ffi::c_void;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::Once;

use anyhow::{Context, Result};
use image::RgbaImage;
use windows::Win32::Foundation::{
    BOOL, COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, CreateSolidBrush, DIB_RGB_COLORS,
    DT_CALCRECT, DT_LEFT, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FrameRect,
    InvalidateRect, PAINTSTRUCT, SRCCOPY, SetBkMode, SetTextColor, StretchDIBits, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL, VK_ESCAPE, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CREATESTRUCTW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, GWLP_USERDATA, GetClientRect, GetCursorPos, GetWindowLongPtrW, GetWindowRect,
    HWND_TOPMOST, KillTimer, MF_CHECKED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, RegisterClassW,
    SWP_NOACTIVATE, SWP_NOCOPYBITS, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetForegroundWindow,
    SetTimer, SetWindowLongPtrW, SetWindowPos, TPM_RIGHTBUTTON, TrackPopupMenu,
    WM_CAPTURECHANGED, WM_COMMAND, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_RBUTTONUP, WM_SIZE,
    WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::output::{copy_to_clipboard, save_png};
use crate::platform_windows::virtual_screen_rect;

const BORDER_COLOR_OUTER: COLORREF = rgb(238, 238, 238);
const BORDER_COLOR_INNER: COLORREF = rgb(74, 74, 74);
const BORDER_COLOR_LOCKED_OUTER: COLORREF = rgb(102, 174, 255);
const HUD_BG: COLORREF = rgb(24, 24, 24);
const HUD_BORDER: COLORREF = rgb(180, 184, 190);
const HUD_TEXT: COLORREF = rgb(236, 239, 243);
const HUD_PAD_X: i32 = 8;
const HUD_PAD_Y: i32 = 5;
const HUD_TEXT_EXTRA_H: i32 = 2;
const HUD_MARGIN: i32 = 8;
const HUD_TIMER_ID: usize = 1;
const HUD_DURATION_MS: u32 = 1100;
const MIN_PIN_SIZE: i32 = 120;
const INITIAL_PIN_MAX_SCREEN_FRACTION: f32 = 0.70;
const PIN_MENU_COPY: u32 = 1001;
const PIN_MENU_SAVE_AS: u32 = 1002;
const PIN_MENU_RESET_ZOOM: u32 = 1003;
const PIN_MENU_CLOSE: u32 = 1004;
const PIN_MENU_LOCK: u32 = 1005;

static REGISTER_CLASS_ONCE: Once = Once::new();

#[derive(Debug)]
struct PinWindowState {
    source_width: i32,
    source_height: i32,
    bgra_pixels: Vec<u8>,
    save_dir: PathBuf,
    locked: bool,
    hud_visible: bool,
    dragging: bool,
    drag_offset: POINT,
}

impl PinWindowState {
    fn new(image: &RgbaImage, save_dir: &Path) -> Self {
        let source_width = image.width().max(1) as i32;
        let source_height = image.height().max(1) as i32;
        let mut bgra_pixels = image.as_raw().to_vec();
        for pixel in bgra_pixels.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        Self {
            source_width,
            source_height,
            bgra_pixels,
            save_dir: save_dir.to_path_buf(),
            locked: false,
            hud_visible: true,
            dragging: false,
            drag_offset: POINT { x: 0, y: 0 },
        }
    }
}

pub fn show_pinned_capture(image: &RgbaImage, save_dir: &Path) -> Result<()> {
    let hmodule = unsafe { GetModuleHandleW(PCWSTR::null()).map_err(anyhow::Error::from)? };
    let hinstance = HINSTANCE(hmodule.0);
    register_window_class(hinstance);

    let (width, height) = initial_window_size(image.width() as i32, image.height() as i32);
    let (x, y) = initial_window_position(width, height);

    let state = Box::new(PinWindowState::new(image, save_dir));
    let state_ptr = Box::into_raw(state);
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("PyroPinnedCaptureClass"),
            w!("PyroPinnedCapture"),
            WS_POPUP,
            x,
            y,
            width,
            height,
            HWND::default(),
            None,
            hinstance,
            Some(state_ptr.cast::<c_void>()),
        )
    };

    let hwnd = match hwnd {
        Ok(value) => value,
        Err(err) => {
            unsafe {
                drop(Box::from_raw(state_ptr));
            }
            return Err(anyhow::Error::from(err));
        }
    };

    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            x,
            y,
            width,
            height,
            SWP_SHOWWINDOW | SWP_NOACTIVATE,
        );
    }
    show_hud(hwnd);

    Ok(())
}

fn register_window_class(hinstance: HINSTANCE) {
    REGISTER_CLASS_ONCE.call_once(|| {
        let class = WNDCLASSW {
            style: windows::Win32::UI::WindowsAndMessaging::CS_HREDRAW
                | windows::Win32::UI::WindowsAndMessaging::CS_VREDRAW
                | windows::Win32::UI::WindowsAndMessaging::CS_DBLCLKS,
            lpfnWndProc: Some(pin_window_proc),
            hInstance: hinstance,
            lpszClassName: w!("PyroPinnedCaptureClass"),
            hCursor: unsafe {
                windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
                    HINSTANCE::default(),
                    windows::Win32::UI::WindowsAndMessaging::IDC_SIZEALL,
                )
                .unwrap_or_default()
            },
            ..Default::default()
        };
        unsafe {
            let _ = RegisterClassW(&class);
        }
    });
}

unsafe extern "system" fn pin_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
            }
            LRESULT(1)
        }
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            start_drag(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONDBLCLK => {
            reset_zoom(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            update_drag(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED => {
            stop_drag(hwnd);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            scale_from_wheel(hwnd, wparam, lparam);
            LRESULT(0)
        }
        WM_SIZE => {
            unsafe {
                let _ = InvalidateRect(hwnd, None, BOOL(0));
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let key = wparam.0 as u32;
            let ctrl_down = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
            if key == VK_ESCAPE.0 as u32 {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                return LRESULT(0);
            }
            if ctrl_down && key == u32::from(b'C') {
                if let Err(err) = perform_copy(hwnd) {
                    tracing::error!("pinned capture copy failed: {err:#}");
                }
                return LRESULT(0);
            }
            if ctrl_down && key == u32::from(b'S') {
                if let Err(err) = perform_save(hwnd) {
                    tracing::error!("pinned capture save failed: {err:#}");
                }
                return LRESULT(0);
            }
            if key == u32::from(b'L') {
                toggle_lock(hwnd);
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_RBUTTONUP => {
            show_context_menu(hwnd);
            LRESULT(0)
        }
        WM_COMMAND => {
            let command = (wparam.0 & 0xFFFF) as u32;
            match command {
                PIN_MENU_COPY => {
                    if let Err(err) = perform_copy(hwnd) {
                        tracing::error!("pinned capture copy failed: {err:#}");
                    }
                }
                PIN_MENU_SAVE_AS => {
                    if let Err(err) = perform_save(hwnd) {
                        tracing::error!("pinned capture save failed: {err:#}");
                    }
                }
                PIN_MENU_RESET_ZOOM => {
                    reset_zoom(hwnd);
                }
                PIN_MENU_CLOSE => unsafe {
                    let _ = DestroyWindow(hwnd);
                },
                PIN_MENU_LOCK => {
                    toggle_lock(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == HUD_TIMER_ID {
                hide_hud(hwnd);
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_NCDESTROY => {
            unsafe {
                let _ = KillTimer(hwnd, HUD_TIMER_ID);
            }
            let state_ptr =
                unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut PinWindowState;
            if !state_ptr.is_null() {
                unsafe {
                    drop(Box::from_raw(state_ptr));
                    let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn paint(hwnd: HWND) {
    let state = unsafe { state_ref(hwnd) };
    let Some(state) = state else {
        return;
    };

    let mut ps = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
    if hdc.0.is_null() {
        return;
    }

    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }
    let width = client.right - client.left;
    let height = client.bottom - client.top;
    if width > 0 && height > 0 {
        let mut bitmap = BITMAPINFO::default();
        bitmap.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: state.source_width,
            biHeight: -state.source_height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };

        unsafe {
            let _ = StretchDIBits(
                hdc,
                0,
                0,
                width,
                height,
                0,
                0,
                state.source_width,
                state.source_height,
                Some(state.bgra_pixels.as_ptr().cast::<c_void>()),
                &bitmap,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
        }
    }

    let outer_border = if state.locked {
        BORDER_COLOR_LOCKED_OUTER
    } else {
        BORDER_COLOR_OUTER
    };
    frame_border(hdc, client, outer_border, 1);
    let inner = RECT {
        left: client.left + 1,
        top: client.top + 1,
        right: client.right - 1,
        bottom: client.bottom - 1,
    };
    frame_border(hdc, inner, BORDER_COLOR_INNER, 1);
    if state.hud_visible {
        draw_hud(hdc, client, state.source_width, state.source_height, width, height);
    }

    unsafe {
        let _ = EndPaint(hwnd, &ps);
    }
}

fn start_drag(hwnd: HWND) {
    let Some(state) = (unsafe { state_mut(hwnd) }) else {
        return;
    };
    if state.locked {
        return;
    }
    let mut cursor = POINT::default();
    let mut rect = RECT::default();
    unsafe {
        let _ = GetCursorPos(&mut cursor);
        let _ = GetWindowRect(hwnd, &mut rect);
        let _ = SetCapture(hwnd);
    }
    state.dragging = true;
    state.drag_offset = POINT {
        x: cursor.x - rect.left,
        y: cursor.y - rect.top,
    };
}

fn update_drag(hwnd: HWND) {
    let Some(state) = (unsafe { state_mut(hwnd) }) else {
        return;
    };
    if !state.dragging {
        return;
    }
    let mut cursor = POINT::default();
    let mut rect = RECT::default();
    unsafe {
        let _ = GetCursorPos(&mut cursor);
        let _ = GetWindowRect(hwnd, &mut rect);
    }
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let mut next_x = cursor.x - state.drag_offset.x;
    let mut next_y = cursor.y - state.drag_offset.y;
    clamp_window_position(&mut next_x, &mut next_y, width, height);
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND::default(),
            next_x,
            next_y,
            0,
            0,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSIZE,
        );
    }
}

fn stop_drag(hwnd: HWND) {
    if let Some(state) = unsafe { state_mut(hwnd) } {
        state.dragging = false;
    }
    unsafe {
        let _ = ReleaseCapture();
    }
}

fn scale_from_wheel(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    let delta = ((wparam.0 >> 16) as u16 as i16) as i32;
    if delta == 0 {
        return;
    }
    let locked = unsafe { state_ref(hwnd).map(|state| state.locked).unwrap_or(false) };
    if locked {
        return;
    }

    let mut rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut rect);
    }
    let old_width = (rect.right - rect.left).max(1);
    let old_height = (rect.bottom - rect.top).max(1);

    let step = (delta as f32) / 120.0;
    let ctrl_down = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;
    let shift_down = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
    let base_scale = if shift_down {
        1.16_f32
    } else if ctrl_down {
        1.03_f32
    } else {
        1.08_f32
    };
    let scale = base_scale.powf(step);
    let mut new_width = ((old_width as f32) * scale).round() as i32;
    let mut new_height = ((old_height as f32) * scale).round() as i32;
    (new_width, new_height) = clamp_zoom_size(new_width, new_height);

    let cursor = POINT {
        x: ((lparam.0 as u32 & 0xFFFF) as i16) as i32,
        y: (((lparam.0 as u32 >> 16) & 0xFFFF) as i16) as i32,
    };
    let rel_x = ((cursor.x - rect.left) as f32 / old_width as f32).clamp(0.0, 1.0);
    let rel_y = ((cursor.y - rect.top) as f32 / old_height as f32).clamp(0.0, 1.0);

    let mut next_x = (cursor.x as f32 - rel_x * new_width as f32).round() as i32;
    let mut next_y = (cursor.y as f32 - rel_y * new_height as f32).round() as i32;
    clamp_window_position(&mut next_x, &mut next_y, new_width, new_height);

    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            next_x,
            next_y,
            new_width,
            new_height,
            SWP_NOACTIVATE | SWP_NOCOPYBITS,
        );
        let _ = InvalidateRect(hwnd, None, BOOL(0));
    }
    show_hud(hwnd);
}

fn clamp_window_position(x: &mut i32, y: &mut i32, width: i32, height: i32) {
    let desktop = virtual_screen_rect();
    let keep_visible = 64;
    let min_x = desktop.left - width + keep_visible;
    let max_x = desktop.right - keep_visible;
    let min_y = desktop.top - height + keep_visible;
    let max_y = desktop.bottom - keep_visible;
    *x = (*x).clamp(min_x, max_x);
    *y = (*y).clamp(min_y, max_y);
}

fn clamp_zoom_size(width: i32, height: i32) -> (i32, i32) {
    let virtual_rect = virtual_screen_rect();
    let max_width = (virtual_rect.width() * 2).max(MIN_PIN_SIZE);
    let max_height = (virtual_rect.height() * 2).max(MIN_PIN_SIZE);
    (
        width.clamp(MIN_PIN_SIZE, max_width),
        height.clamp(MIN_PIN_SIZE, max_height),
    )
}

fn reset_zoom(hwnd: HWND) {
    let Some(state) = (unsafe { state_ref(hwnd) }) else {
        return;
    };
    if state.locked {
        return;
    }
    let (target_w, target_h) = clamp_zoom_size(state.source_width, state.source_height);

    let mut rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut rect);
    }
    let center_x = rect.left + ((rect.right - rect.left) / 2);
    let center_y = rect.top + ((rect.bottom - rect.top) / 2);

    let mut next_x = center_x - (target_w / 2);
    let mut next_y = center_y - (target_h / 2);
    clamp_window_position(&mut next_x, &mut next_y, target_w, target_h);

    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            next_x,
            next_y,
            target_w,
            target_h,
            SWP_NOACTIVATE | SWP_NOCOPYBITS,
        );
        let _ = InvalidateRect(hwnd, None, BOOL(0));
    }
    show_hud(hwnd);
}

fn show_context_menu(hwnd: HWND) {
    let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
        return;
    };

    let locked = unsafe { state_ref(hwnd).map(|state| state.locked).unwrap_or(false) };
    let lock_flag = if locked { MF_CHECKED } else { MF_UNCHECKED };

    unsafe {
        let _ = AppendMenuW(menu, MF_STRING, PIN_MENU_COPY as usize, w!("Copy\tCtrl+C"));
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            PIN_MENU_SAVE_AS as usize,
            w!("Save As...\tCtrl+S"),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING | lock_flag, PIN_MENU_LOCK as usize, w!("Lock Move/Zoom\tL"));
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            PIN_MENU_RESET_ZOOM as usize,
            w!("Reset Zoom (100%)\tDouble-click"),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, PIN_MENU_CLOSE as usize, w!("Close"));
    }

    let mut cursor = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut cursor);
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON,
            cursor.x,
            cursor.y,
            0,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
    }
}

fn perform_copy(hwnd: HWND) -> Result<()> {
    let image = pinned_image_rgba(hwnd)?;
    copy_to_clipboard(&image).context("copy pinned capture to clipboard")
}

fn perform_save(hwnd: HWND) -> Result<()> {
    let (image, save_dir) = {
        let state = unsafe { state_ref(hwnd) }.context("pinned capture state missing")?;
        let image =
            rgba_from_bgra(state.source_width, state.source_height, &state.bgra_pixels)
                .context("decode pinned capture image")?;
        (image, state.save_dir.clone())
    };
    let _ = save_png(&image, None, &save_dir).context("save pinned capture")?;
    Ok(())
}

fn pinned_image_rgba(hwnd: HWND) -> Result<RgbaImage> {
    let state = unsafe { state_ref(hwnd) }.context("pinned capture state missing")?;
    rgba_from_bgra(state.source_width, state.source_height, &state.bgra_pixels)
        .context("decode pinned capture image")
}

fn rgba_from_bgra(width: i32, height: i32, bgra: &[u8]) -> Option<RgbaImage> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let expected_len = width as usize * height as usize * 4;
    if bgra.len() != expected_len {
        return None;
    }
    let mut rgba = bgra.to_vec();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    RgbaImage::from_raw(width as u32, height as u32, rgba)
}

fn initial_window_size(source_width: i32, source_height: i32) -> (i32, i32) {
    let source_width = source_width.max(1);
    let source_height = source_height.max(1);
    let desktop = virtual_screen_rect();
    let max_width = ((desktop.width() as f32) * INITIAL_PIN_MAX_SCREEN_FRACTION).round() as i32;
    let max_height = ((desktop.height() as f32) * INITIAL_PIN_MAX_SCREEN_FRACTION).round() as i32;
    let scale = (max_width as f32 / source_width as f32)
        .min(max_height as f32 / source_height as f32)
        .min(1.0);
    let width = ((source_width as f32) * scale).round() as i32;
    let height = ((source_height as f32) * scale).round() as i32;
    (width.max(MIN_PIN_SIZE), height.max(MIN_PIN_SIZE))
}

fn initial_window_position(width: i32, height: i32) -> (i32, i32) {
    let desktop = virtual_screen_rect();
    let x = desktop.left + ((desktop.width() - width) / 2);
    let y = desktop.top + ((desktop.height() - height) / 2);
    (x, y)
}

fn toggle_lock(hwnd: HWND) {
    let mut became_locked = false;
    if let Some(state) = unsafe { state_mut(hwnd) } {
        state.locked = !state.locked;
        if state.locked {
            state.dragging = false;
            became_locked = true;
        }
    }
    if became_locked {
        unsafe {
            let _ = ReleaseCapture();
        }
    }
    show_hud(hwnd);
}

fn show_hud(hwnd: HWND) {
    if let Some(state) = unsafe { state_mut(hwnd) } {
        state.hud_visible = true;
    }
    unsafe {
        let _ = SetTimer(hwnd, HUD_TIMER_ID, HUD_DURATION_MS, None);
        let _ = InvalidateRect(hwnd, None, BOOL(0));
    }
}

fn hide_hud(hwnd: HWND) {
    if let Some(state) = unsafe { state_mut(hwnd) } {
        state.hud_visible = false;
    }
    unsafe {
        let _ = KillTimer(hwnd, HUD_TIMER_ID);
        let _ = InvalidateRect(hwnd, None, BOOL(0));
    }
}

fn draw_hud(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    client: RECT,
    source_width: i32,
    source_height: i32,
    current_width: i32,
    current_height: i32,
) {
    if source_width <= 0 || source_height <= 0 || current_width <= 0 || current_height <= 0 {
        return;
    }

    let scale = ((current_width as f32 / source_width as f32)
        .min(current_height as f32 / source_height as f32)
        * 100.0)
        .round();
    let client_w = (client.right - client.left).max(0);
    let client_h = (client.bottom - client.top).max(0);
    let max_box_w = (client_w - (HUD_MARGIN * 2)).max(0);
    let max_box_h = (client_h - (HUD_MARGIN * 2)).max(0);
    if max_box_w < 40 || max_box_h < 20 {
        return;
    }

    let full_label = format!("{scale:.0}%  |  {current_width}x{current_height}");
    let compact_label = format!("{scale:.0}%");
    let (label, text_w, text_h) = {
        let (full_w, full_h) = measure_text(hdc, &full_label);
        let full_box_w = full_w + (HUD_PAD_X * 2);
        let full_box_h = full_h + (HUD_PAD_Y * 2);
        if full_box_w <= max_box_w && full_box_h <= max_box_h {
            (full_label, full_w, full_h)
        } else {
            let (compact_w, compact_h) = measure_text(hdc, &compact_label);
            let compact_box_w = compact_w + (HUD_PAD_X * 2);
            let compact_box_h = compact_h + (HUD_PAD_Y * 2);
            if compact_box_w > max_box_w || compact_box_h > max_box_h {
                return;
            }
            (compact_label, compact_w, compact_h)
        }
    };

    let box_w = text_w + (HUD_PAD_X * 2);
    let box_h = text_h + (HUD_PAD_Y * 2);
    let box_rect = RECT {
        left: (client.right - HUD_MARGIN - box_w).max(client.left + 2),
        top: (client.bottom - HUD_MARGIN - box_h).max(client.top + 2),
        right: (client.right - HUD_MARGIN).max(client.left + 2),
        bottom: (client.bottom - HUD_MARGIN).max(client.top + 2),
    };

    let fill = unsafe { CreateSolidBrush(HUD_BG) };
    if !fill.0.is_null() {
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::FillRect(hdc, &box_rect, fill);
            let _ = DeleteObject(fill);
        }
    }
    frame_border(hdc, box_rect, HUD_BORDER, 1);
    let mut draw_rect = RECT {
        left: box_rect.left + HUD_PAD_X,
        top: box_rect.top + HUD_PAD_Y,
        right: box_rect.right - HUD_PAD_X,
        bottom: box_rect.bottom - HUD_PAD_Y,
    };
    let mut draw_wide = label.encode_utf16().collect::<Vec<u16>>();
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, HUD_TEXT);
        let _ = DrawTextW(
            hdc,
            &mut draw_wide,
            &mut draw_rect,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
    }
}

fn measure_text(hdc: windows::Win32::Graphics::Gdi::HDC, text: &str) -> (i32, i32) {
    let mut wide = text.encode_utf16().collect::<Vec<u16>>();
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe {
        let _ = DrawTextW(
            hdc,
            &mut wide,
            &mut rect,
            DT_CALCRECT | DT_SINGLELINE | DT_LEFT,
        );
    }
    (
        (rect.right - rect.left).max(1),
        (rect.bottom - rect.top + HUD_TEXT_EXTRA_H).max(1),
    )
}

fn frame_border(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    color: COLORREF,
    thickness: i32,
) {
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    let brush = unsafe { CreateSolidBrush(color) };
    if brush.0.is_null() {
        return;
    }
    for inset in 0..thickness.max(1) {
        let frame = RECT {
            left: rect.left + inset,
            top: rect.top + inset,
            right: rect.right - inset,
            bottom: rect.bottom - inset,
        };
        if frame.right <= frame.left || frame.bottom <= frame.top {
            break;
        }
        unsafe {
            let _ = FrameRect(hdc, &frame, brush);
        }
    }
    unsafe {
        let _ = DeleteObject(brush);
    }
}

unsafe fn state_ref(hwnd: HWND) -> Option<&'static PinWindowState> {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const PinWindowState;
    unsafe { ptr.as_ref() }
}

unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut PinWindowState> {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut PinWindowState;
    unsafe { ptr.as_mut() }
}

const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF((red as u32) | ((green as u32) << 8) | ((blue as u32) << 16))
}
