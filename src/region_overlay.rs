use std::ffi::c_void;

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{
    BOOL, COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush, DeleteDC,
    DeleteObject, EndPaint, FillRect, FrameRect, InvalidateRect, PAINTSTRUCT, SRCCOPY,
    SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, VK_ESCAPE, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetClientRect, GetMessageW, GetWindowLongPtrW, HWND_TOPMOST, IDC_CROSS, LWA_ALPHA,
    LWA_COLORKEY, LoadCursorW, MSG, PostQuitMessage, RegisterClassW, SWP_SHOWWINDOW,
    SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TranslateMessage, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::platform_windows::{RectPx, virtual_screen_rect};

const HANDLE_SIZE: i32 = 8;
const OVERLAY_DIM_COLOR: COLORREF = rgb(0, 0, 0);
const OVERLAY_ALPHA: u8 = 118;
const OVERLAY_TRANSPARENT_KEY: COLORREF = rgb(255, 0, 255);
const SELECTION_BORDER_COLOR: COLORREF = rgb(0, 120, 215);
const HANDLE_COLOR: COLORREF = rgb(245, 245, 245);

#[derive(Debug)]
struct OverlayState {
    virtual_rect: RectPx,
    drag_start: Option<POINT>,
    drag_current: Option<POINT>,
    selected_rect: Option<RectPx>,
    require_enter_confirm: bool,
    done: bool,
    canceled: bool,
}

impl OverlayState {
    fn new(virtual_rect: RectPx, require_enter_confirm: bool) -> Self {
        Self {
            virtual_rect,
            drag_start: None,
            drag_current: None,
            selected_rect: None,
            require_enter_confirm,
            done: false,
            canceled: false,
        }
    }

    fn update_drag(&mut self, current: POINT) -> bool {
        if let Some(start) = self.drag_start {
            self.drag_current = Some(current);
            let next = normalize_points(start, current, self.virtual_rect);
            if rect_changed(self.selected_rect, next) {
                self.selected_rect = Some(next);
                return true;
            }
        }

        false
    }
}

pub fn select_region() -> Result<RectPx> {
    select_region_inner(true)?.context("region selection canceled")
}

pub fn select_region_immediate() -> Result<Option<RectPx>> {
    select_region_inner(false)
}

fn select_region_inner(require_enter_confirm: bool) -> Result<Option<RectPx>> {
    let virtual_rect = virtual_screen_rect();
    if virtual_rect.width() <= 0 || virtual_rect.height() <= 0 {
        bail!("invalid virtual desktop size");
    }

    let hmodule = unsafe { GetModuleHandleW(PCWSTR::null()).map_err(anyhow::Error::from)? };
    let hinstance = HINSTANCE(hmodule.0);
    register_overlay_class(hinstance);

    let state = Box::new(OverlayState::new(virtual_rect, require_enter_confirm));
    let state_ptr = Box::into_raw(state);

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            w!("PyroRegionOverlayClass"),
            w!("PyroRegionOverlay"),
            WS_POPUP,
            virtual_rect.left,
            virtual_rect.top,
            virtual_rect.width(),
            virtual_rect.height(),
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
        SetLayeredWindowAttributes(
            hwnd,
            OVERLAY_TRANSPARENT_KEY,
            OVERLAY_ALPHA,
            LWA_ALPHA | LWA_COLORKEY,
        )
        .map_err(anyhow::Error::from)?;

        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            virtual_rect.left,
            virtual_rect.top,
            virtual_rect.width(),
            virtual_rect.height(),
            SWP_SHOWWINDOW,
        )
        .map_err(anyhow::Error::from)?;
        let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
    }

    let mut msg = MSG::default();
    loop {
        if overlay_done(hwnd) {
            break;
        }

        let status = unsafe { GetMessageW(&mut msg, HWND::default(), 0, 0) }.0;
        if status == -1 {
            bail!("GetMessageW failed while selecting region");
        }
        if status == 0 {
            unsafe {
                PostQuitMessage(msg.wParam.0 as i32);
            }
            break;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    let state_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut OverlayState;
    let state = if state_ptr.is_null() {
        bail!("overlay state was not available");
    } else {
        unsafe { Box::from_raw(state_ptr) }
    };

    unsafe {
        let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let _ = DestroyWindow(hwnd);
    }

    if state.canceled {
        return Ok(None);
    }

    let selected = state.selected_rect.context("no region was selected")?;
    if selected.width() < 2 || selected.height() < 2 {
        bail!("selected region is too small");
    }

    Ok(Some(selected))
}

fn register_overlay_class(hinstance: HINSTANCE) {
    let klass = WNDCLASSW {
        lpfnWndProc: Some(overlay_window_proc),
        hInstance: hinstance,
        hCursor: unsafe { LoadCursorW(HINSTANCE::default(), IDC_CROSS).unwrap_or_default() },
        lpszClassName: w!("PyroRegionOverlayClass"),
        ..Default::default()
    };
    let _ = unsafe { RegisterClassW(&klass) };
}

unsafe extern "system" fn overlay_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
            let state_ptr = create.lpCreateParams as *mut OverlayState;
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
            }
            LRESULT(1)
        }
        WM_KEYDOWN => {
            if wparam.0 as u32 == VK_ESCAPE.0 as u32 {
                cancel_selection(hwnd);
                return LRESULT(0);
            }

            if wparam.0 as u32 == VK_RETURN.0 as u32 {
                if let Some(state) = unsafe { overlay_state_mut(hwnd) }
                    && state.selected_rect.is_some()
                {
                    state.done = true;
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                }
                return LRESULT(0);
            }

            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_RBUTTONUP => {
            cancel_selection(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let point = point_from_lparam(lparam);
            if let Some(state) = unsafe { overlay_state_mut(hwnd) } {
                state.drag_start = Some(point);
                state.drag_current = Some(point);
                state.selected_rect = Some(normalize_points(point, point, state.virtual_rect));
            }
            unsafe {
                let _ = SetCapture(hwnd);
                let _ = InvalidateRect(hwnd, None, BOOL(0));
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let point = point_from_lparam(lparam);
            if let Some(state) = unsafe { overlay_state_mut(hwnd) } {
                if state.drag_start.is_some() && state.update_drag(point) {
                    unsafe {
                        let _ = InvalidateRect(hwnd, None, BOOL(0));
                    }
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(state) = unsafe { overlay_state_mut(hwnd) } {
                if state.drag_start.is_some() {
                    let point = point_from_lparam(lparam);
                    let _ = state.update_drag(point);
                    state.drag_start = None;
                    state.drag_current = None;
                    if !state.require_enter_confirm {
                        state.done = true;
                    }
                    unsafe {
                        let _ = InvalidateRect(hwnd, None, BOOL(0));
                    }
                }
            }
            unsafe {
                let _ = ReleaseCapture();
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint_overlay(hwnd);
            LRESULT(0)
        }
        WM_NCDESTROY => {
            unsafe {
                let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn paint_overlay(hwnd: HWND) {
    let state = if let Some(value) = unsafe { overlay_state_mut(hwnd) } {
        value
    } else {
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
    if width <= 0 || height <= 0 {
        unsafe {
            let _ = EndPaint(hwnd, &ps);
        }
        return;
    }

    let shade = unsafe { CreateSolidBrush(OVERLAY_DIM_COLOR) };
    let transparent_cutout = unsafe { CreateSolidBrush(OVERLAY_TRANSPARENT_KEY) };
    let selection_border = unsafe { CreateSolidBrush(SELECTION_BORDER_COLOR) };
    let handle_brush = unsafe { CreateSolidBrush(HANDLE_COLOR) };

    let mem_dc = unsafe { CreateCompatibleDC(hdc) };
    if mem_dc.0.is_null() {
        unsafe {
            let _ = DeleteObject(shade);
            let _ = DeleteObject(transparent_cutout);
            let _ = DeleteObject(selection_border);
            let _ = DeleteObject(handle_brush);
            let _ = EndPaint(hwnd, &ps);
        }
        return;
    }

    let mem_bitmap = unsafe { CreateCompatibleBitmap(hdc, width, height) };
    if mem_bitmap.0.is_null() {
        unsafe {
            let _ = DeleteDC(mem_dc);
            let _ = DeleteObject(shade);
            let _ = DeleteObject(transparent_cutout);
            let _ = DeleteObject(selection_border);
            let _ = DeleteObject(handle_brush);
            let _ = EndPaint(hwnd, &ps);
        }
        return;
    }

    let old_bitmap = unsafe { SelectObject(mem_dc, mem_bitmap) };

    unsafe {
        let _ = FillRect(mem_dc, &client, shade);
    }

    if let Some(selection) = state.selected_rect {
        let selection_rect = to_overlay_client_rect(selection, state.virtual_rect);
        if selection_rect.right > selection_rect.left && selection_rect.bottom > selection_rect.top
        {
            unsafe {
                let _ = FillRect(mem_dc, &selection_rect, transparent_cutout);
                let _ = FrameRect(mem_dc, &selection_rect, selection_border);
            }

            let mut inner = selection_rect;
            inner.left += 1;
            inner.top += 1;
            inner.right -= 1;
            inner.bottom -= 1;

            if inner.right > inner.left && inner.bottom > inner.top {
                unsafe {
                    let _ = FrameRect(mem_dc, &inner, selection_border);
                }
            }

            for handle in selection_handle_rects(selection_rect) {
                unsafe {
                    let _ = FillRect(mem_dc, &handle, handle_brush);
                }
            }
        }
    }

    unsafe {
        let _ = BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY);
        let _ = SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(mem_bitmap);
        let _ = DeleteDC(mem_dc);
        let _ = DeleteObject(shade);
        let _ = DeleteObject(transparent_cutout);
        let _ = DeleteObject(selection_border);
        let _ = DeleteObject(handle_brush);
        let _ = EndPaint(hwnd, &ps);
    }
}

fn normalize_points(start: POINT, end: POINT, virtual_rect: RectPx) -> RectPx {
    let max_x = virtual_rect.width().max(0);
    let max_y = virtual_rect.height().max(0);
    let sx = start.x.clamp(0, max_x);
    let sy = start.y.clamp(0, max_y);
    let ex = end.x.clamp(0, max_x);
    let ey = end.y.clamp(0, max_y);

    let left = sx.min(ex) + virtual_rect.left;
    let right = sx.max(ex) + virtual_rect.left;
    let top = sy.min(ey) + virtual_rect.top;
    let bottom = sy.max(ey) + virtual_rect.top;

    RectPx {
        left,
        top,
        right,
        bottom,
    }
}

fn point_from_lparam(lparam: LPARAM) -> POINT {
    let raw = lparam.0 as u32;
    let x = (raw & 0xFFFF) as i16 as i32;
    let y = ((raw >> 16) & 0xFFFF) as i16 as i32;
    POINT { x, y }
}

fn rect_changed(existing: Option<RectPx>, next: RectPx) -> bool {
    let Some(current) = existing else {
        return true;
    };

    current.left != next.left
        || current.top != next.top
        || current.right != next.right
        || current.bottom != next.bottom
}

fn to_overlay_client_rect(selection: RectPx, virtual_rect: RectPx) -> RECT {
    RECT {
        left: selection.left - virtual_rect.left,
        top: selection.top - virtual_rect.top,
        right: selection.right - virtual_rect.left,
        bottom: selection.bottom - virtual_rect.top,
    }
}

fn selection_handle_rects(selection: RECT) -> [RECT; 8] {
    let right = selection.right - 1;
    let bottom = selection.bottom - 1;
    let mid_x = selection.left + (selection.right - selection.left) / 2;
    let mid_y = selection.top + (selection.bottom - selection.top) / 2;

    [
        handle_rect(selection.left, selection.top),
        handle_rect(mid_x, selection.top),
        handle_rect(right, selection.top),
        handle_rect(selection.left, mid_y),
        handle_rect(right, mid_y),
        handle_rect(selection.left, bottom),
        handle_rect(mid_x, bottom),
        handle_rect(right, bottom),
    ]
}

fn handle_rect(center_x: i32, center_y: i32) -> RECT {
    let half = HANDLE_SIZE / 2;
    RECT {
        left: center_x - half,
        top: center_y - half,
        right: center_x - half + HANDLE_SIZE,
        bottom: center_y - half + HANDLE_SIZE,
    }
}

fn overlay_done(hwnd: HWND) -> bool {
    unsafe { overlay_state(hwnd).map(|state| state.done).unwrap_or(true) }
}

fn cancel_selection(hwnd: HWND) {
    if let Some(state) = unsafe { overlay_state_mut(hwnd) } {
        state.canceled = true;
        state.done = true;
    }
    unsafe {
        let _ = ReleaseCapture();
    }
}

unsafe fn overlay_state(hwnd: HWND) -> Option<&'static OverlayState> {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut OverlayState;
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

unsafe fn overlay_state_mut(hwnd: HWND) -> Option<&'static mut OverlayState> {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut OverlayState;
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &mut *ptr })
    }
}

const fn rgb(red: u8, green: u8, blue: u8) -> windows::Win32::Foundation::COLORREF {
    windows::Win32::Foundation::COLORREF(
        (red as u32) | ((green as u32) << 8) | ((blue as u32) << 16),
    )
}
