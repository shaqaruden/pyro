use std::ffi::c_void;

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{
    BOOL, COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_OVER, AlphaBlend, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BeginPaint,
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen, CreateSolidBrush,
    DIB_RGB_COLORS, DT_CALCRECT, DT_CENTER, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject,
    DrawTextW, EndPaint, FillRect, FrameRect, IntersectClipRect, InvalidateRect, PAINTSTRUCT,
    PS_SOLID, RestoreDC, RoundRect, SRCCOPY, SaveDC, SelectObject, SetBkMode, SetTextColor,
    StretchDIBits, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, VK_ESCAPE, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetClientRect, GetMessageW, GetWindowLongPtrW, HCURSOR, HWND_TOPMOST, LWA_ALPHA, LWA_COLORKEY,
    MSG, PostQuitMessage, RegisterClassW, SWP_SHOWWINDOW, SetForegroundWindow,
    SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
    WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCREATE,
    WM_NCDESTROY, WM_PAINT, WM_RBUTTONUP, WM_SETCURSOR, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::capture::CaptureFrame;
use crate::platform_windows::{RectPx, virtual_screen_rect};

const HANDLE_SIZE: i32 = 8;
const OVERLAY_DIM_COLOR: COLORREF = rgb(0, 0, 0);
const OVERLAY_ALPHA: u8 = 118;
const OVERLAY_TRANSPARENT_KEY: COLORREF = rgb(255, 0, 255);
const SELECTION_BORDER_COLOR: COLORREF = rgb(0, 120, 215);
const HANDLE_COLOR: COLORREF = rgb(245, 245, 245);
const CURSOR_OUTLINE_COLOR: COLORREF = rgb(0, 0, 0);
const CURSOR_FILL_COLOR: COLORREF = rgb(255, 255, 255);
const CURSOR_HALF_SPAN: i32 = 10;
const CURSOR_OUTER_HALF_THICKNESS: i32 = 2;
const CURSOR_INNER_HALF_THICKNESS: i32 = 1;
const SIZE_BADGE_BG: COLORREF = rgb(24, 24, 24);
const SIZE_BADGE_BORDER: COLORREF = rgb(228, 228, 228);
const SIZE_BADGE_TEXT: COLORREF = rgb(240, 240, 240);
const SIZE_BADGE_PAD_X: i32 = 10;
const SIZE_BADGE_PAD_Y: i32 = 6;
const SIZE_BADGE_TEXT_EXTRA_H: i32 = 2;
const SIZE_BADGE_RADIUS: i32 = 8;

#[derive(Debug)]
struct OverlayState {
    virtual_rect: RectPx,
    frozen_snapshot: Option<FrozenOverlaySnapshot>,
    cursor_point: Option<POINT>,
    drag_start: Option<POINT>,
    drag_current: Option<POINT>,
    selected_rect: Option<RectPx>,
    require_enter_confirm: bool,
    done: bool,
    canceled: bool,
}

impl OverlayState {
    fn new(
        virtual_rect: RectPx,
        frozen_snapshot: Option<FrozenOverlaySnapshot>,
        require_enter_confirm: bool,
    ) -> Self {
        Self {
            virtual_rect,
            frozen_snapshot,
            cursor_point: None,
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

#[derive(Debug)]
struct FrozenOverlaySnapshot {
    width: i32,
    height: i32,
    bgra_pixels: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct PrecomputedSelectionSnapshot {
    pub width: i32,
    pub height: i32,
    pub bgra_pixels: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct RegionSelection {
    pub rect: RectPx,
    pub precomputed_snapshot: Option<PrecomputedSelectionSnapshot>,
}

pub fn select_region() -> Result<RectPx> {
    select_region_inner(true, None)?
        .map(|selection| selection.rect)
        .context("region selection canceled")
}

#[allow(dead_code)]
pub fn select_region_immediate() -> Result<Option<RectPx>> {
    Ok(select_region_inner(false, None)?.map(|selection| selection.rect))
}

pub fn select_region_from_frame(frame: &CaptureFrame) -> Result<RegionSelection> {
    select_region_inner(true, Some(frame_to_overlay_snapshot(frame)?))?
        .context("region selection canceled")
}

pub fn select_region_immediate_from_frame(frame: &CaptureFrame) -> Result<Option<RegionSelection>> {
    select_region_inner(false, Some(frame_to_overlay_snapshot(frame)?))
}

fn select_region_inner(
    require_enter_confirm: bool,
    frozen_snapshot: Option<FrozenOverlaySnapshot>,
) -> Result<Option<RegionSelection>> {
    let virtual_rect = virtual_screen_rect();
    if virtual_rect.width() <= 0 || virtual_rect.height() <= 0 {
        bail!("invalid virtual desktop size");
    }
    if let Some(snapshot) = frozen_snapshot.as_ref()
        && (snapshot.width != virtual_rect.width() || snapshot.height != virtual_rect.height())
    {
        bail!(
            "frozen frame dimensions {}x{} do not match virtual desktop {}x{}",
            snapshot.width,
            snapshot.height,
            virtual_rect.width(),
            virtual_rect.height()
        );
    }

    let hmodule = unsafe { GetModuleHandleW(PCWSTR::null()).map_err(anyhow::Error::from)? };
    let hinstance = HINSTANCE(hmodule.0);
    register_overlay_class(hinstance);

    let state = Box::new(OverlayState::new(
        virtual_rect,
        frozen_snapshot,
        require_enter_confirm,
    ));
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
        let use_frozen = overlay_state(hwnd)
            .map(|state| state.frozen_snapshot.is_some())
            .unwrap_or(false);
        if use_frozen {
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA)
                .map_err(anyhow::Error::from)?;
        } else {
            SetLayeredWindowAttributes(
                hwnd,
                OVERLAY_TRANSPARENT_KEY,
                OVERLAY_ALPHA,
                LWA_ALPHA | LWA_COLORKEY,
            )
            .map_err(anyhow::Error::from)?;
        }

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

    let precomputed_snapshot = state
        .frozen_snapshot
        .as_ref()
        .and_then(|snapshot| crop_selection_snapshot(snapshot, state.virtual_rect, selected));

    Ok(Some(RegionSelection {
        rect: selected,
        precomputed_snapshot,
    }))
}

fn register_overlay_class(hinstance: HINSTANCE) {
    let klass = WNDCLASSW {
        lpfnWndProc: Some(overlay_window_proc),
        hInstance: hinstance,
        hCursor: Default::default(),
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
                update_cursor_indicator(hwnd, state, point);
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
            let mut selection_changed = false;
            if let Some(state) = unsafe { overlay_state_mut(hwnd) } {
                update_cursor_indicator(hwnd, state, point);
                if state.drag_start.is_some() {
                    selection_changed = state.update_drag(point);
                }
            }
            if selection_changed {
                unsafe {
                    let _ = InvalidateRect(hwnd, None, BOOL(0));
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some(state) = unsafe { overlay_state_mut(hwnd) } {
                if state.drag_start.is_some() {
                    let point = point_from_lparam(lparam);
                    update_cursor_indicator(hwnd, state, point);
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
        WM_SETCURSOR => {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetCursor(HCURSOR::default());
            }
            LRESULT(1)
        }
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
    let cursor_outline_brush = unsafe { CreateSolidBrush(CURSOR_OUTLINE_COLOR) };
    let cursor_fill_brush = unsafe { CreateSolidBrush(CURSOR_FILL_COLOR) };

    let mem_dc = unsafe { CreateCompatibleDC(hdc) };
    if mem_dc.0.is_null() {
        unsafe {
            let _ = DeleteObject(shade);
            let _ = DeleteObject(transparent_cutout);
            let _ = DeleteObject(selection_border);
            let _ = DeleteObject(handle_brush);
            let _ = DeleteObject(cursor_outline_brush);
            let _ = DeleteObject(cursor_fill_brush);
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
            let _ = DeleteObject(cursor_outline_brush);
            let _ = DeleteObject(cursor_fill_brush);
            let _ = EndPaint(hwnd, &ps);
        }
        return;
    }

    let old_bitmap = unsafe { SelectObject(mem_dc, mem_bitmap) };

    let selection_rect = state
        .selected_rect
        .map(|selection| to_overlay_client_rect(selection, state.virtual_rect));

    if let Some(snapshot) = state.frozen_snapshot.as_ref() {
        draw_snapshot(
            mem_dc,
            &client,
            &snapshot.bgra_pixels,
            snapshot.width,
            snapshot.height,
        );
        draw_dim_overlay(mem_dc, client, OVERLAY_ALPHA);
        if let Some(sel) = selection_rect
            && sel.right > sel.left
            && sel.bottom > sel.top
        {
            unsafe {
                let saved = SaveDC(mem_dc);
                if saved != 0 {
                    let _ = IntersectClipRect(mem_dc, sel.left, sel.top, sel.right, sel.bottom);
                    draw_snapshot(
                        mem_dc,
                        &client,
                        &snapshot.bgra_pixels,
                        snapshot.width,
                        snapshot.height,
                    );
                    let _ = RestoreDC(mem_dc, saved);
                }
            }
        }
    } else {
        unsafe {
            let _ = FillRect(mem_dc, &client, shade);
        }
    }

    if let Some(selection_rect) = selection_rect {
        if selection_rect.right > selection_rect.left && selection_rect.bottom > selection_rect.top
        {
            if state.frozen_snapshot.is_none() {
                unsafe {
                    let _ = FillRect(mem_dc, &selection_rect, transparent_cutout);
                }
            }
            unsafe {
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

    if let Some(cursor) = state.cursor_point {
        draw_cursor_indicator(mem_dc, cursor, cursor_outline_brush, cursor_fill_brush);
    }
    if state.drag_start.is_some()
        && let Some(selection_rect) = selection_rect
        && selection_rect.right > selection_rect.left
        && selection_rect.bottom > selection_rect.top
    {
        draw_size_badge(
            mem_dc,
            selection_rect,
            selection_rect.right - selection_rect.left,
            selection_rect.bottom - selection_rect.top,
        );
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
        let _ = DeleteObject(cursor_outline_brush);
        let _ = DeleteObject(cursor_fill_brush);
        let _ = EndPaint(hwnd, &ps);
    }
}

fn draw_snapshot(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    dest: &RECT,
    bgra_pixels: &[u8],
    width: i32,
    height: i32,
) {
    if width <= 0 || height <= 0 {
        return;
    }
    let mut bitmap = BITMAPINFO::default();
    bitmap.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    unsafe {
        let _ = StretchDIBits(
            hdc,
            dest.left,
            dest.top,
            dest.right - dest.left,
            dest.bottom - dest.top,
            0,
            0,
            width,
            height,
            Some(bgra_pixels.as_ptr().cast::<c_void>()),
            &bitmap,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }
}

fn draw_dim_overlay(hdc: windows::Win32::Graphics::Gdi::HDC, dest: RECT, alpha: u8) {
    let width = dest.right - dest.left;
    let height = dest.bottom - dest.top;
    if width <= 0 || height <= 0 || alpha == 0 {
        return;
    }

    let src_dc = unsafe { CreateCompatibleDC(hdc) };
    if src_dc.0.is_null() {
        return;
    }

    let src_bitmap = unsafe { CreateCompatibleBitmap(hdc, 1, 1) };
    if src_bitmap.0.is_null() {
        unsafe {
            let _ = DeleteDC(src_dc);
        }
        return;
    }
    let old_bitmap = unsafe { SelectObject(src_dc, src_bitmap) };
    let black = unsafe { CreateSolidBrush(rgb(0, 0, 0)) };
    let one = RECT {
        left: 0,
        top: 0,
        right: 1,
        bottom: 1,
    };
    unsafe {
        let _ = FillRect(src_dc, &one, black);
    }

    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: alpha,
        AlphaFormat: 0,
    };
    unsafe {
        let _ = AlphaBlend(
            hdc, dest.left, dest.top, width, height, src_dc, 0, 0, 1, 1, blend,
        );
        let _ = DeleteObject(black);
        let _ = SelectObject(src_dc, old_bitmap);
        let _ = DeleteObject(src_bitmap);
        let _ = DeleteDC(src_dc);
    }
}

fn frame_to_overlay_snapshot(frame: &CaptureFrame) -> Result<FrozenOverlaySnapshot> {
    let width = i32::try_from(frame.image.width()).context("frozen frame width overflow")?;
    let height = i32::try_from(frame.image.height()).context("frozen frame height overflow")?;
    if width <= 0 || height <= 0 {
        bail!("invalid frozen frame dimensions {}x{}", width, height);
    }

    let pixels = frame.image.as_raw();
    if pixels.len() != width as usize * height as usize * 4 {
        bail!("invalid frozen frame pixel buffer length");
    }
    let bgra_pixels = rgba_to_bgra(pixels);

    Ok(FrozenOverlaySnapshot {
        width,
        height,
        bgra_pixels,
    })
}

fn crop_selection_snapshot(
    snapshot: &FrozenOverlaySnapshot,
    virtual_rect: RectPx,
    selected: RectPx,
) -> Option<PrecomputedSelectionSnapshot> {
    let width = selected.width();
    let height = selected.height();
    if width <= 0 || height <= 0 {
        return None;
    }

    let src_left = selected.left - virtual_rect.left;
    let src_top = selected.top - virtual_rect.top;
    if src_left < 0
        || src_top < 0
        || src_left + width > snapshot.width
        || src_top + height > snapshot.height
    {
        return None;
    }

    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let src_stride = snapshot.width as usize * 4;
    let row_bytes = width as usize * 4;
    for row in 0..height as usize {
        let src_row = (src_top as usize + row) * src_stride;
        let src_off = src_row + (src_left as usize * 4);
        let dst_off = row * row_bytes;
        pixels[dst_off..dst_off + row_bytes]
            .copy_from_slice(&snapshot.bgra_pixels[src_off..src_off + row_bytes]);
    }

    Some(PrecomputedSelectionSnapshot {
        width,
        height,
        bgra_pixels: pixels,
    })
}

fn rgba_to_bgra(rgba: &[u8]) -> Vec<u8> {
    let mut bgra = Vec::with_capacity(rgba.len());
    for px in rgba.chunks_exact(4) {
        bgra.push(px[2]);
        bgra.push(px[1]);
        bgra.push(px[0]);
        bgra.push(255);
    }
    bgra
}

fn update_cursor_indicator(hwnd: HWND, state: &mut OverlayState, next: POINT) {
    if state
        .cursor_point
        .map(|current| current.x == next.x && current.y == next.y)
        .unwrap_or(false)
    {
        return;
    }

    if let Some(previous) = state.cursor_point {
        let rect = cursor_indicator_rect(previous);
        unsafe {
            let _ = InvalidateRect(hwnd, Some(&rect), BOOL(0));
        }
    }
    state.cursor_point = Some(next);
    let rect = cursor_indicator_rect(next);
    unsafe {
        let _ = InvalidateRect(hwnd, Some(&rect), BOOL(0));
    }
}

fn cursor_indicator_rect(point: POINT) -> RECT {
    let half = CURSOR_HALF_SPAN + CURSOR_OUTER_HALF_THICKNESS + 2;
    RECT {
        left: point.x - half,
        top: point.y - half,
        right: point.x + half + 1,
        bottom: point.y + half + 1,
    }
}

fn draw_cursor_indicator(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    point: POINT,
    outline_brush: windows::Win32::Graphics::Gdi::HBRUSH,
    fill_brush: windows::Win32::Graphics::Gdi::HBRUSH,
) {
    let outer_v = RECT {
        left: point.x - CURSOR_OUTER_HALF_THICKNESS,
        top: point.y - CURSOR_HALF_SPAN,
        right: point.x + CURSOR_OUTER_HALF_THICKNESS + 1,
        bottom: point.y + CURSOR_HALF_SPAN + 1,
    };
    let outer_h = RECT {
        left: point.x - CURSOR_HALF_SPAN,
        top: point.y - CURSOR_OUTER_HALF_THICKNESS,
        right: point.x + CURSOR_HALF_SPAN + 1,
        bottom: point.y + CURSOR_OUTER_HALF_THICKNESS + 1,
    };
    let inner_v = RECT {
        left: point.x - CURSOR_INNER_HALF_THICKNESS,
        top: point.y - CURSOR_HALF_SPAN + 1,
        right: point.x + CURSOR_INNER_HALF_THICKNESS + 1,
        bottom: point.y + CURSOR_HALF_SPAN,
    };
    let inner_h = RECT {
        left: point.x - CURSOR_HALF_SPAN + 1,
        top: point.y - CURSOR_INNER_HALF_THICKNESS,
        right: point.x + CURSOR_HALF_SPAN,
        bottom: point.y + CURSOR_INNER_HALF_THICKNESS + 1,
    };
    unsafe {
        let _ = FillRect(hdc, &outer_v, outline_brush);
        let _ = FillRect(hdc, &outer_h, outline_brush);
        let _ = FillRect(hdc, &inner_v, fill_brush);
        let _ = FillRect(hdc, &inner_h, fill_brush);
    }
}

fn draw_size_badge(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    selection: RECT,
    width: i32,
    height: i32,
) {
    let selection_w = selection.right - selection.left;
    let selection_h = selection.bottom - selection.top;
    if selection_w <= 0 || selection_h <= 0 {
        return;
    }

    let label = format!("{width} x {height}");
    let mut wide = label.encode_utf16().collect::<Vec<u16>>();
    let mut text_rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe {
        let _ = DrawTextW(
            hdc,
            &mut wide,
            &mut text_rect,
            DT_CALCRECT | DT_SINGLELINE | DT_CENTER,
        );
    }
    let text_w = (text_rect.right - text_rect.left).max(1);
    let text_h = (text_rect.bottom - text_rect.top + SIZE_BADGE_TEXT_EXTRA_H).max(1);
    let badge_w = text_w + (SIZE_BADGE_PAD_X * 2);
    let badge_h = text_h + (SIZE_BADGE_PAD_Y * 2);
    let center_x = selection.left + (selection_w / 2);
    let center_y = selection.top + (selection_h / 2);
    let badge = RECT {
        left: center_x - (badge_w / 2),
        top: center_y - (badge_h / 2),
        right: center_x + ((badge_w + 1) / 2),
        bottom: center_y + ((badge_h + 1) / 2),
    };
    draw_rounded_box(hdc, badge, SIZE_BADGE_BG, SIZE_BADGE_BORDER, SIZE_BADGE_RADIUS);
    unsafe {
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, SIZE_BADGE_TEXT);
    }
    let mut draw_rect = RECT {
        left: badge.left + SIZE_BADGE_PAD_X,
        top: badge.top + SIZE_BADGE_PAD_Y,
        right: badge.right - SIZE_BADGE_PAD_X,
        bottom: badge.bottom - SIZE_BADGE_PAD_Y,
    };
    let mut draw_wide = label.encode_utf16().collect::<Vec<u16>>();
    unsafe {
        let _ = DrawTextW(
            hdc,
            &mut draw_wide,
            &mut draw_rect,
            DT_CENTER | DT_SINGLELINE | DT_VCENTER,
        );
    }
}

fn draw_rounded_box(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    fill: COLORREF,
    border: COLORREF,
    radius: i32,
) {
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return;
    }
    let pen = unsafe { CreatePen(PS_SOLID, 1, border) };
    let brush = unsafe { CreateSolidBrush(fill) };
    if pen.0.is_null() || brush.0.is_null() {
        unsafe {
            if !pen.0.is_null() {
                let _ = DeleteObject(pen);
            }
            if !brush.0.is_null() {
                let _ = DeleteObject(brush);
            }
        }
        return;
    }
    unsafe {
        let old_pen = SelectObject(hdc, pen);
        let old_brush = SelectObject(hdc, brush);
        let round = radius.max(2);
        let _ = RoundRect(
            hdc,
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            round,
            round,
        );
        let _ = SelectObject(hdc, old_pen);
        let _ = SelectObject(hdc, old_brush);
        let _ = DeleteObject(pen);
        let _ = DeleteObject(brush);
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
