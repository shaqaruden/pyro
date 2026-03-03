use std::ffi::c_void;
use std::mem::size_of;
use std::sync::Once;

use anyhow::Result;
use image::RgbaImage;
use windows::Win32::Foundation::{
    BOOL, COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, CreateSolidBrush, DIB_RGB_COLORS,
    DeleteObject, EndPaint, FrameRect, InvalidateRect, PAINTSTRUCT, SRCCOPY, StretchDIBits,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetClientRect,
    GetCursorPos, GetWindowLongPtrW, GetWindowRect, HWND_TOPMOST, RegisterClassW, SWP_NOACTIVATE,
    SWP_NOCOPYBITS, SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowLongPtrW, SetWindowPos,
    WM_CAPTURECHANGED, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_RBUTTONUP, WM_SIZE, WNDCLASSW, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::platform_windows::virtual_screen_rect;

const BORDER_COLOR_OUTER: COLORREF = rgb(238, 238, 238);
const BORDER_COLOR_INNER: COLORREF = rgb(74, 74, 74);
const MIN_PIN_SIZE: i32 = 120;

static REGISTER_CLASS_ONCE: Once = Once::new();

#[derive(Debug)]
struct PinWindowState {
    source_width: i32,
    source_height: i32,
    bgra_pixels: Vec<u8>,
    dragging: bool,
    drag_offset: POINT,
}

impl PinWindowState {
    fn new(image: &RgbaImage) -> Self {
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
            dragging: false,
            drag_offset: POINT { x: 0, y: 0 },
        }
    }
}

pub fn show_pinned_capture(image: &RgbaImage) -> Result<()> {
    let hmodule = unsafe { GetModuleHandleW(PCWSTR::null()).map_err(anyhow::Error::from)? };
    let hinstance = HINSTANCE(hmodule.0);
    register_window_class(hinstance);

    let (width, height) = initial_window_size(image.width() as i32, image.height() as i32);
    let (x, y) = initial_window_position(width, height);

    let state = Box::new(PinWindowState::new(image));
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

    Ok(())
}

fn register_window_class(hinstance: HINSTANCE) {
    REGISTER_CLASS_ONCE.call_once(|| {
        let class = WNDCLASSW {
            style: windows::Win32::UI::WindowsAndMessaging::CS_HREDRAW
                | windows::Win32::UI::WindowsAndMessaging::CS_VREDRAW,
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
            if wparam.0 as u32 == VK_ESCAPE.0 as u32 {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_RBUTTONUP => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
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

    frame_border(hdc, client, BORDER_COLOR_OUTER, 1);
    let inner = RECT {
        left: client.left + 1,
        top: client.top + 1,
        right: client.right - 1,
        bottom: client.bottom - 1,
    };
    frame_border(hdc, inner, BORDER_COLOR_INNER, 1);

    unsafe {
        let _ = EndPaint(hwnd, &ps);
    }
}

fn start_drag(hwnd: HWND) {
    let Some(state) = (unsafe { state_mut(hwnd) }) else {
        return;
    };
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
            SWP_NOZORDER | SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE,
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

    let mut rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut rect);
    }
    let old_width = (rect.right - rect.left).max(1);
    let old_height = (rect.bottom - rect.top).max(1);

    let step = (delta as f32) / 120.0;
    let scale = 1.08_f32.powf(step);
    let mut new_width = ((old_width as f32) * scale).round() as i32;
    let mut new_height = ((old_height as f32) * scale).round() as i32;

    let virtual_rect = virtual_screen_rect();
    let max_width = (virtual_rect.width() * 2).max(MIN_PIN_SIZE);
    let max_height = (virtual_rect.height() * 2).max(MIN_PIN_SIZE);
    new_width = new_width.clamp(MIN_PIN_SIZE, max_width);
    new_height = new_height.clamp(MIN_PIN_SIZE, max_height);

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

fn initial_window_size(source_width: i32, source_height: i32) -> (i32, i32) {
    let source_width = source_width.max(1);
    let source_height = source_height.max(1);
    let desktop = virtual_screen_rect();
    let max_width = ((desktop.width() as f32) * 0.45).round() as i32;
    let max_height = ((desktop.height() as f32) * 0.45).round() as i32;
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
