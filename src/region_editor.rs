use std::mem::size_of;

use std::ffi::c_void;

use anyhow::{Result, bail};
use image::{Rgba, RgbaImage};
use windows::Win32::Foundation::{
    BOOL, COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, BitBlt, CreateCompatibleBitmap,
    CreateCompatibleDC, CreateSolidBrush, DIB_RGB_COLORS, DT_CENTER, DT_LEFT, DT_SINGLELINE,
    DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect, InvalidateRect,
    PAINTSTRUCT, SRCCOPY, SelectObject, SetBkMode, SetTextColor, StretchDIBits, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL, VK_ESCAPE, VK_RETURN, VK_Y, VK_Z,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetClientRect, GetMessageW, GetWindowLongPtrW, HWND_TOPMOST, IDC_ARROW, IDC_CROSS, IDC_HAND,
    IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, LWA_ALPHA, LWA_COLORKEY,
    LoadCursorW, MSG, PostQuitMessage, RegisterClassW, SWP_SHOWWINDOW, SetCursor,
    SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    TranslateMessage, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::capture;
use crate::platform_windows::{RectPx, virtual_screen_rect};

const HANDLE_SIZE: i32 = 8;
const MIN_SELECTION: i32 = 2;
const MIN_RECT: i32 = 3;
const STROKE: i32 = 2;
const STROKE_RGBA: [u8; 4] = [255, 94, 94, 255];

const OVERLAY_DIM: COLORREF = rgb(0, 0, 0);
const OVERLAY_ALPHA: u8 = 118;
const OVERLAY_KEY: COLORREF = rgb(255, 0, 255);
const SELECTION_FILL: COLORREF = rgb(58, 58, 58);
const SELECTION_COLOR: COLORREF = rgb(0, 120, 215);
const HANDLE_COLOR: COLORREF = rgb(245, 245, 245);
const RECT_COLOR: COLORREF = rgb(255, 94, 94);

const BAR_BG: COLORREF = rgb(16, 16, 16);
const BAR_BORDER: COLORREF = rgb(72, 72, 72);
const BAR_TEXT: COLORREF = rgb(238, 238, 238);
const BTN_BG: COLORREF = rgb(34, 34, 34);
const BTN_ACTIVE: COLORREF = rgb(0, 120, 215);

const BAR_MARGIN: i32 = 12;
const BAR_GAP: i32 = 10;
const BAR_H: i32 = 38;
const BAR_MIN_W: i32 = 390;
const BAR_MAX_W: i32 = 720;
const BAR_PAD_X: i32 = 8;
const BTN_W: i32 = 90;
const BTN_H: i32 = 26;
const BTN_GAP: i32 = 8;
const INFO_PAD_X: i32 = 12;

#[derive(Debug)]
pub struct RegionEditResult {
    bounds: RectPx,
    annotations: Vec<Annotation>,
}

impl RegionEditResult {
    pub fn bounds(&self) -> RectPx {
        self.bounds
    }
}

#[derive(Debug)]
pub enum RegionEditOutcome {
    Apply(RegionEditResult),
    Cancel,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Tool {
    Select,
    Rectangle,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ResizeHandle {
    NW,
    N,
    NE,
    W,
    E,
    SW,
    S,
    SE,
}

#[derive(Debug, Clone, Copy)]
enum Drag {
    Move {
        offset_x: i32,
        offset_y: i32,
        width: i32,
        height: i32,
    },
    Resize {
        handle: ResizeHandle,
        start_rect: RectPx,
        start_point: POINT,
    },
    NewSelection {
        start: POINT,
    },
    DrawRect {
        start: POINT,
        current: POINT,
    },
}

#[derive(Debug, Clone)]
enum Annotation {
    Rectangle(RectAnn),
}

#[derive(Debug, Clone)]
struct RectAnn {
    rect_abs: RectPx,
    color: [u8; 4],
    thickness: i32,
}

#[derive(Debug, Clone)]
struct SelectionSnapshot {
    width: i32,
    height: i32,
    bgra_pixels: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
struct ToolbarLayout {
    panel: RECT,
    select_btn: RECT,
    rect_btn: RECT,
    info: RECT,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ToolbarHit {
    Select,
    Rect,
    Panel,
}

#[derive(Debug)]
struct State {
    virtual_rect: RectPx,
    selection: RectPx,
    drag: Option<Drag>,
    tool: Tool,
    chrome_hwnd: HWND,
    selection_snapshot: Option<SelectionSnapshot>,
    annotations: Vec<Annotation>,
    redo: Vec<Annotation>,
    done: bool,
    canceled: bool,
}

impl State {
    fn new(initial: RectPx, virtual_rect: RectPx) -> Self {
        Self {
            virtual_rect,
            selection: clamp_rect(initial, virtual_rect),
            drag: None,
            tool: Tool::Select,
            chrome_hwnd: HWND::default(),
            selection_snapshot: None,
            annotations: Vec::new(),
            redo: Vec::new(),
            done: false,
            canceled: false,
        }
    }

    fn set_selection(&mut self, next: RectPx) -> bool {
        if !rect_changed(self.selection, next) {
            return false;
        }
        self.selection = next;
        self.selection_snapshot = None;
        if !self.annotations.is_empty() || !self.redo.is_empty() {
            self.annotations.clear();
            self.redo.clear();
        }
        true
    }

    fn update_drag(&mut self, abs: POINT) -> bool {
        let Some(drag) = self.drag else {
            return false;
        };
        match drag {
            Drag::Move {
                offset_x,
                offset_y,
                width,
                height,
            } => {
                let width = width.max(MIN_SELECTION);
                let height = height.max(MIN_SELECTION);
                let mut left = abs.x - offset_x;
                let mut top = abs.y - offset_y;
                left = left.clamp(self.virtual_rect.left, self.virtual_rect.right - width);
                top = top.clamp(self.virtual_rect.top, self.virtual_rect.bottom - height);
                self.set_selection(RectPx {
                    left,
                    top,
                    right: left + width,
                    bottom: top + height,
                })
            }
            Drag::Resize {
                handle,
                start_rect,
                start_point,
            } => self.set_selection(resize_selection(
                handle,
                start_rect,
                start_point,
                abs,
                self.virtual_rect,
            )),
            Drag::NewSelection { start } => {
                self.set_selection(normalize_abs(start, abs, self.virtual_rect))
            }
            Drag::DrawRect { start, current } => {
                if current.x == abs.x && current.y == abs.y {
                    return false;
                }
                self.drag = Some(Drag::DrawRect {
                    start,
                    current: abs,
                });
                true
            }
        }
    }

    fn pending_rect(&self) -> Option<RectPx> {
        let Drag::DrawRect { start, current } = self.drag? else {
            return None;
        };
        Some(normalize_abs(start, current, self.selection))
    }

    fn finalize_rect(&mut self) -> bool {
        let Some(Drag::DrawRect { start, current }) = self.drag.take() else {
            return false;
        };
        let rect = normalize_abs(start, current, self.selection);
        if rect.width() < MIN_RECT || rect.height() < MIN_RECT {
            return false;
        }
        self.annotations.push(Annotation::Rectangle(RectAnn {
            rect_abs: rect,
            color: STROKE_RGBA,
            thickness: STROKE,
        }));
        self.redo.clear();
        true
    }

    fn undo(&mut self) -> bool {
        let Some(last) = self.annotations.pop() else {
            return false;
        };
        self.redo.push(last);
        true
    }

    fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.annotations.push(next);
        true
    }
}

pub fn edit_region(initial_selection: RectPx) -> Result<RegionEditOutcome> {
    let virtual_rect = virtual_screen_rect();
    if virtual_rect.width() <= 0 || virtual_rect.height() <= 0 {
        bail!("invalid virtual desktop size");
    }

    let hmodule = unsafe { GetModuleHandleW(PCWSTR::null()).map_err(anyhow::Error::from)? };
    let hinstance = HINSTANCE(hmodule.0);
    register_editor_class(hinstance);

    let state_ptr = Box::into_raw(Box::new(State::new(initial_selection, virtual_rect)));
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            w!("PyroRegionEditorClass"),
            w!("PyroRegionEditor"),
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
        Ok(v) => v,
        Err(err) => {
            unsafe {
                drop(Box::from_raw(state_ptr));
            }
            return Err(anyhow::Error::from(err));
        }
    };

    unsafe {
        set_layer_mode(hwnd, Tool::Select).map_err(anyhow::Error::from)?;
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

    let chrome_hwnd = create_chrome_window(hwnd, hinstance, virtual_rect, state_ptr)?;
    if let Some(state) = unsafe { state_mut(hwnd) } {
        state.chrome_hwnd = chrome_hwnd;
    }

    unsafe {
        let _ = InvalidateRect(hwnd, None, BOOL(0));
        let _ = InvalidateRect(chrome_hwnd, None, BOOL(0));
    }

    let mut msg = MSG::default();
    loop {
        if editor_done(hwnd) {
            break;
        }
        let status = unsafe { GetMessageW(&mut msg, HWND::default(), 0, 0) }.0;
        if status == -1 {
            bail!("GetMessageW failed while editing region");
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

    let chrome_hwnd = unsafe {
        state_ref(hwnd)
            .map(|state| state.chrome_hwnd)
            .unwrap_or_default()
    };
    if !chrome_hwnd.0.is_null() {
        unsafe {
            let _ = DestroyWindow(chrome_hwnd);
        }
    }

    let state_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut State;
    let state = if state_ptr.is_null() {
        bail!("region editor state was not available");
    } else {
        unsafe { Box::from_raw(state_ptr) }
    };
    unsafe {
        let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let _ = DestroyWindow(hwnd);
    }

    if state.canceled {
        return Ok(RegionEditOutcome::Cancel);
    }
    if state.selection.width() < MIN_SELECTION || state.selection.height() < MIN_SELECTION {
        bail!("selected region is too small");
    }

    Ok(RegionEditOutcome::Apply(RegionEditResult {
        bounds: state.selection,
        annotations: state.annotations,
    }))
}

pub fn apply_annotations(image: &mut RgbaImage, result: &RegionEditResult) {
    for annotation in &result.annotations {
        match annotation {
            Annotation::Rectangle(rect) => {
                let local = RectPx {
                    left: rect.rect_abs.left - result.bounds.left,
                    top: rect.rect_abs.top - result.bounds.top,
                    right: rect.rect_abs.right - result.bounds.left,
                    bottom: rect.rect_abs.bottom - result.bounds.top,
                };
                draw_rect_outline(image, local, rect.color, rect.thickness);
            }
        }
    }
}

fn register_editor_class(hinstance: HINSTANCE) {
    let klass = WNDCLASSW {
        lpfnWndProc: Some(region_editor_window_proc),
        hInstance: hinstance,
        hCursor: unsafe { LoadCursorW(HINSTANCE::default(), IDC_CROSS).unwrap_or_default() },
        lpszClassName: w!("PyroRegionEditorClass"),
        ..Default::default()
    };
    let _ = unsafe { RegisterClassW(&klass) };

    let chrome = WNDCLASSW {
        lpfnWndProc: Some(region_chrome_window_proc),
        hInstance: hinstance,
        lpszClassName: w!("PyroRegionEditorChromeClass"),
        ..Default::default()
    };
    let _ = unsafe { RegisterClassW(&chrome) };
}

fn create_chrome_window(
    owner: HWND,
    hinstance: HINSTANCE,
    virtual_rect: RectPx,
    state_ptr: *mut State,
) -> Result<HWND> {
    let chrome = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT,
            w!("PyroRegionEditorChromeClass"),
            w!("PyroRegionEditorChrome"),
            WS_POPUP,
            virtual_rect.left,
            virtual_rect.top,
            virtual_rect.width(),
            virtual_rect.height(),
            owner,
            None,
            hinstance,
            Some(state_ptr.cast::<c_void>()),
        )
    }
    .map_err(anyhow::Error::from)?;

    unsafe {
        SetLayeredWindowAttributes(chrome, OVERLAY_KEY, 255, LWA_COLORKEY)
            .map_err(anyhow::Error::from)?;
        SetWindowPos(
            chrome,
            HWND_TOPMOST,
            virtual_rect.left,
            virtual_rect.top,
            virtual_rect.width(),
            virtual_rect.height(),
            SWP_SHOWWINDOW,
        )
        .map_err(anyhow::Error::from)?;
        let _ = ShowWindow(chrome, windows::Win32::UI::WindowsAndMessaging::SW_SHOW);
    }

    Ok(chrome)
}

unsafe extern "system" fn region_editor_window_proc(
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
        WM_KEYDOWN => on_key(hwnd, wparam),
        WM_RBUTTONUP => {
            cancel(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => on_mouse_down(hwnd, lparam),
        WM_MOUSEMOVE => on_mouse_move(hwnd, lparam),
        WM_LBUTTONUP => on_mouse_up(hwnd, lparam),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(hwnd);
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

unsafe extern "system" fn region_chrome_window_proc(
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
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint_chrome(hwnd);
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

fn on_key(hwnd: HWND, wparam: WPARAM) -> LRESULT {
    let key = wparam.0 as u32;
    let ctrl_down = unsafe { GetKeyState(VK_CONTROL.0 as i32) } < 0;

    if key == VK_ESCAPE.0 as u32 {
        cancel(hwnd);
        return LRESULT(0);
    }

    if ctrl_down && key == VK_Z.0 as u32 {
        if let Some(state) = unsafe { state_mut(hwnd) }
            && state.undo()
        {
            invalidate_all(hwnd);
        }
        return LRESULT(0);
    }

    if ctrl_down && key == VK_Y.0 as u32 {
        if let Some(state) = unsafe { state_mut(hwnd) }
            && state.redo()
        {
            invalidate_all(hwnd);
        }
        return LRESULT(0);
    }

    if key == VK_RETURN.0 as u32 {
        if let Some(state) = unsafe { state_mut(hwnd) }
            && state.selection.width() >= MIN_SELECTION
            && state.selection.height() >= MIN_SELECTION
        {
            state.done = true;
        }
        return LRESULT(0);
    }

    unsafe { DefWindowProcW(hwnd, WM_KEYDOWN, wparam, LPARAM(0)) }
}

fn on_mouse_down(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let client = point_from_lparam(lparam);
    let mut started_drag = false;

    if let Some(state) = unsafe { state_mut(hwnd) } {
        let selection_client = to_client_rect(state.selection, state.virtual_rect);
        let bar = toolbar_layout(selection_client, client_rect(state.virtual_rect));
        if let Some(hit) = toolbar_hit(bar, client) {
            let mut changed = false;
            match hit {
                ToolbarHit::Select => {
                    if state.tool != Tool::Select {
                        state.tool = Tool::Select;
                        state.selection_snapshot = None;
                        changed = true;
                    }
                }
                ToolbarHit::Rect => {
                    if state.tool != Tool::Rectangle {
                        state.selection_snapshot = capture_selection_snapshot(state.selection);
                        state.tool = Tool::Rectangle;
                        changed = true;
                    }
                }
                ToolbarHit::Panel => {}
            }
            state.drag = None;
            if changed {
                unsafe {
                    let _ = set_layer_mode(hwnd, state.tool);
                }
            }
            invalidate_all(hwnd);
            update_cursor(hwnd, client);
            return LRESULT(0);
        }

        let abs = clamp_point(
            client_to_abs(client, state.virtual_rect),
            state.virtual_rect,
        );
        match state.tool {
            Tool::Select => {
                if let Some(handle) = hit_handle(selection_client, client) {
                    state.drag = Some(Drag::Resize {
                        handle,
                        start_rect: state.selection,
                        start_point: abs,
                    });
                    started_drag = true;
                } else if point_in(client, selection_client) {
                    state.drag = Some(Drag::Move {
                        offset_x: abs.x - state.selection.left,
                        offset_y: abs.y - state.selection.top,
                        width: state.selection.width(),
                        height: state.selection.height(),
                    });
                    started_drag = true;
                } else {
                    state.drag = Some(Drag::NewSelection { start: abs });
                    let _ = state.set_selection(normalize_abs(abs, abs, state.virtual_rect));
                    started_drag = true;
                }
            }
            Tool::Rectangle => {
                if point_in(client, selection_client) {
                    state.drag = Some(Drag::DrawRect {
                        start: abs,
                        current: abs,
                    });
                    started_drag = true;
                }
            }
        }
    }

    if started_drag {
        unsafe {
            let _ = SetCapture(hwnd);
        }
    }
    invalidate_all(hwnd);
    update_cursor(hwnd, client);
    LRESULT(0)
}

fn on_mouse_move(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let client = point_from_lparam(lparam);
    if let Some(state) = unsafe { state_mut(hwnd) }
        && state.drag.is_some()
    {
        let abs = clamp_point(
            client_to_abs(client, state.virtual_rect),
            state.virtual_rect,
        );
        if state.update_drag(abs) {
            invalidate_all(hwnd);
        }
    }
    update_cursor(hwnd, client);
    LRESULT(0)
}

fn on_mouse_up(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let client = point_from_lparam(lparam);
    let mut repaint = false;
    if let Some(state) = unsafe { state_mut(hwnd) }
        && state.drag.is_some()
    {
        let abs = clamp_point(
            client_to_abs(client, state.virtual_rect),
            state.virtual_rect,
        );
        if state.update_drag(abs) {
            repaint = true;
        }
        if state.finalize_rect() {
            repaint = true;
        } else {
            state.drag = None;
        }
    }
    unsafe {
        let _ = ReleaseCapture();
    }
    if repaint {
        invalidate_all(hwnd);
    }
    update_cursor(hwnd, client);
    LRESULT(0)
}

fn invalidate_all(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(hwnd, None, BOOL(0));
    }
    let chrome_hwnd = unsafe {
        state_ref(hwnd)
            .map(|state| state.chrome_hwnd)
            .unwrap_or_default()
    };
    if !chrome_hwnd.0.is_null() {
        unsafe {
            let _ = InvalidateRect(chrome_hwnd, None, BOOL(0));
        }
    }
}

fn paint(hwnd: HWND) {
    let state = if let Some(v) = unsafe { state_ref(hwnd) } {
        v
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

    let shade = unsafe { CreateSolidBrush(OVERLAY_DIM) };
    let transparent = unsafe { CreateSolidBrush(OVERLAY_KEY) };
    let selection_fill = unsafe { CreateSolidBrush(SELECTION_FILL) };
    let sel_brush = unsafe { CreateSolidBrush(SELECTION_COLOR) };
    let handle_brush = unsafe { CreateSolidBrush(HANDLE_COLOR) };
    let rect_brush = unsafe { CreateSolidBrush(RECT_COLOR) };
    let bar_bg = unsafe { CreateSolidBrush(BAR_BG) };
    let bar_border = unsafe { CreateSolidBrush(BAR_BORDER) };
    let btn_bg = unsafe { CreateSolidBrush(BTN_BG) };
    let btn_active = unsafe { CreateSolidBrush(BTN_ACTIVE) };

    let mem_dc = unsafe { CreateCompatibleDC(hdc) };
    if mem_dc.0.is_null() {
        cleanup_paint_objects(&[
            shade,
            transparent,
            selection_fill,
            sel_brush,
            handle_brush,
            rect_brush,
            bar_bg,
            bar_border,
            btn_bg,
            btn_active,
        ]);
        unsafe {
            let _ = EndPaint(hwnd, &ps);
        }
        return;
    }
    let mem_bitmap = unsafe { CreateCompatibleBitmap(hdc, width, height) };
    if mem_bitmap.0.is_null() {
        unsafe {
            let _ = DeleteDC(mem_dc);
        }
        cleanup_paint_objects(&[
            shade,
            transparent,
            selection_fill,
            sel_brush,
            handle_brush,
            rect_brush,
            bar_bg,
            bar_border,
            btn_bg,
            btn_active,
        ]);
        unsafe {
            let _ = EndPaint(hwnd, &ps);
        }
        return;
    }
    let old_bitmap = unsafe { SelectObject(mem_dc, mem_bitmap) };

    unsafe {
        let _ = FillRect(mem_dc, &client, shade);
    }
    let selection = to_client_rect(state.selection, state.virtual_rect);
    unsafe {
        if state.tool == Tool::Select {
            let _ = FillRect(mem_dc, &selection, transparent);
        } else if let Some(snapshot) = state.selection_snapshot.as_ref() {
            draw_selection_snapshot(mem_dc, snapshot, selection);
        } else {
            let _ = FillRect(mem_dc, &selection, selection_fill);
        }
    }

    for ann in &state.annotations {
        match ann {
            Annotation::Rectangle(rect) => {
                frame_thick(
                    mem_dc,
                    to_client_rect(rect.rect_abs, state.virtual_rect),
                    rect_brush,
                    rect.thickness,
                );
            }
        }
    }
    if let Some(pending) = state.pending_rect() {
        frame_thick(
            mem_dc,
            to_client_rect(pending, state.virtual_rect),
            rect_brush,
            STROKE,
        );
    }

    frame_thick(mem_dc, selection, sel_brush, 2);
    for (_, h) in handle_rects(selection) {
        unsafe {
            let _ = FillRect(mem_dc, &h, handle_brush);
        }
    }

    let bar = toolbar_layout(selection, client);
    unsafe {
        let _ = FillRect(mem_dc, &bar.panel, bar_border);
    }
    let mut inner = bar.panel;
    inner.left += 1;
    inner.top += 1;
    inner.right -= 1;
    inner.bottom -= 1;
    unsafe {
        let _ = FillRect(mem_dc, &inner, bar_bg);
        let _ = SetBkMode(mem_dc, TRANSPARENT);
        let _ = SetTextColor(mem_dc, BAR_TEXT);
    }
    draw_button(
        mem_dc,
        bar.select_btn,
        "Select",
        state.tool == Tool::Select,
        btn_bg,
        btn_active,
    );
    draw_button(
        mem_dc,
        bar.rect_btn,
        "Rectangle",
        state.tool == Tool::Rectangle,
        btn_bg,
        btn_active,
    );

    let info = format!(
        "{}x{} | Rects: {} | Enter capture Esc cancel Ctrl+Z/Y",
        state.selection.width(),
        state.selection.height(),
        state.annotations.len()
    );
    let mut wide = info.encode_utf16().collect::<Vec<u16>>();
    let mut info_rect = bar.info;
    info_rect.left += INFO_PAD_X;
    info_rect.right -= INFO_PAD_X;
    unsafe {
        let _ = DrawTextW(
            mem_dc,
            &mut wide,
            &mut info_rect,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
    }

    unsafe {
        let _ = BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY);
        let _ = SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(mem_bitmap);
        let _ = DeleteDC(mem_dc);
        let _ = EndPaint(hwnd, &ps);
    }
    cleanup_paint_objects(&[
        shade,
        transparent,
        selection_fill,
        sel_brush,
        handle_brush,
        rect_brush,
        bar_bg,
        bar_border,
        btn_bg,
        btn_active,
    ]);
}

fn paint_chrome(hwnd: HWND) {
    let state = if let Some(v) = unsafe { state_ref(hwnd) } {
        v
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

    let clear = unsafe { CreateSolidBrush(OVERLAY_KEY) };
    let sel_brush = unsafe { CreateSolidBrush(SELECTION_COLOR) };
    let handle_brush = unsafe { CreateSolidBrush(HANDLE_COLOR) };
    let rect_brush = unsafe { CreateSolidBrush(RECT_COLOR) };
    let bar_bg = unsafe { CreateSolidBrush(BAR_BG) };
    let bar_border = unsafe { CreateSolidBrush(BAR_BORDER) };
    let btn_bg = unsafe { CreateSolidBrush(BTN_BG) };
    let btn_active = unsafe { CreateSolidBrush(BTN_ACTIVE) };

    let mem_dc = unsafe { CreateCompatibleDC(hdc) };
    if mem_dc.0.is_null() {
        cleanup_paint_objects(&[
            clear,
            sel_brush,
            handle_brush,
            rect_brush,
            bar_bg,
            bar_border,
            btn_bg,
            btn_active,
        ]);
        unsafe {
            let _ = EndPaint(hwnd, &ps);
        }
        return;
    }
    let mem_bitmap = unsafe { CreateCompatibleBitmap(hdc, width, height) };
    if mem_bitmap.0.is_null() {
        unsafe {
            let _ = DeleteDC(mem_dc);
        }
        cleanup_paint_objects(&[
            clear,
            sel_brush,
            handle_brush,
            rect_brush,
            bar_bg,
            bar_border,
            btn_bg,
            btn_active,
        ]);
        unsafe {
            let _ = EndPaint(hwnd, &ps);
        }
        return;
    }
    let old_bitmap = unsafe { SelectObject(mem_dc, mem_bitmap) };

    unsafe {
        let _ = FillRect(mem_dc, &client, clear);
    }

    let selection = to_client_rect(state.selection, state.virtual_rect);
    for ann in &state.annotations {
        match ann {
            Annotation::Rectangle(rect) => {
                frame_thick(
                    mem_dc,
                    to_client_rect(rect.rect_abs, state.virtual_rect),
                    rect_brush,
                    rect.thickness,
                );
            }
        }
    }
    if let Some(pending) = state.pending_rect() {
        frame_thick(
            mem_dc,
            to_client_rect(pending, state.virtual_rect),
            rect_brush,
            STROKE,
        );
    }

    frame_thick(mem_dc, selection, sel_brush, 2);
    for (_, h) in handle_rects(selection) {
        unsafe {
            let _ = FillRect(mem_dc, &h, handle_brush);
        }
    }

    let bar = toolbar_layout(selection, client);
    unsafe {
        let _ = FillRect(mem_dc, &bar.panel, bar_border);
    }
    let mut inner = bar.panel;
    inner.left += 1;
    inner.top += 1;
    inner.right -= 1;
    inner.bottom -= 1;
    unsafe {
        let _ = FillRect(mem_dc, &inner, bar_bg);
        let _ = SetBkMode(mem_dc, TRANSPARENT);
        let _ = SetTextColor(mem_dc, BAR_TEXT);
    }
    draw_button(
        mem_dc,
        bar.select_btn,
        "Select",
        state.tool == Tool::Select,
        btn_bg,
        btn_active,
    );
    draw_button(
        mem_dc,
        bar.rect_btn,
        "Rectangle",
        state.tool == Tool::Rectangle,
        btn_bg,
        btn_active,
    );

    let info = format!(
        "{}x{} | Rects: {} | Enter capture Esc cancel Ctrl+Z/Y",
        state.selection.width(),
        state.selection.height(),
        state.annotations.len()
    );
    let mut wide = info.encode_utf16().collect::<Vec<u16>>();
    let mut info_rect = bar.info;
    info_rect.left += INFO_PAD_X;
    info_rect.right -= INFO_PAD_X;
    unsafe {
        let _ = DrawTextW(
            mem_dc,
            &mut wide,
            &mut info_rect,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );
    }

    unsafe {
        let _ = BitBlt(hdc, 0, 0, width, height, mem_dc, 0, 0, SRCCOPY);
        let _ = SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(mem_bitmap);
        let _ = DeleteDC(mem_dc);
        let _ = EndPaint(hwnd, &ps);
    }
    cleanup_paint_objects(&[
        clear,
        sel_brush,
        handle_brush,
        rect_brush,
        bar_bg,
        bar_border,
        btn_bg,
        btn_active,
    ]);
}

fn cleanup_paint_objects(brushes: &[windows::Win32::Graphics::Gdi::HBRUSH]) {
    for brush in brushes {
        unsafe {
            let _ = DeleteObject(*brush);
        }
    }
}

unsafe fn set_layer_mode(hwnd: HWND, tool: Tool) -> windows::core::Result<()> {
    let use_color_key = tool == Tool::Select;
    let flags = if use_color_key {
        LWA_ALPHA | LWA_COLORKEY
    } else {
        LWA_ALPHA
    };
    let color_key = if use_color_key {
        OVERLAY_KEY
    } else {
        COLORREF(0)
    };
    unsafe { SetLayeredWindowAttributes(hwnd, color_key, OVERLAY_ALPHA, flags) }
}

fn capture_selection_snapshot(selection: RectPx) -> Option<SelectionSnapshot> {
    let frame = capture::capture_rect(selection).ok()?;
    let width = i32::try_from(frame.image.width()).ok()?;
    let height = i32::try_from(frame.image.height()).ok()?;
    if width <= 0 || height <= 0 {
        return None;
    }

    Some(SelectionSnapshot {
        width,
        height,
        bgra_pixels: rgba_to_bgra(frame.image.as_raw()),
    })
}

fn draw_selection_snapshot(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    snapshot: &SelectionSnapshot,
    dest: RECT,
) {
    let dest_w = dest.right - dest.left;
    let dest_h = dest.bottom - dest.top;
    if dest_w <= 0 || dest_h <= 0 {
        return;
    }

    let mut bitmap = BITMAPINFO::default();
    bitmap.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: snapshot.width,
        biHeight: -snapshot.height,
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
            dest_w,
            dest_h,
            0,
            0,
            snapshot.width,
            snapshot.height,
            Some(snapshot.bgra_pixels.as_ptr().cast::<c_void>()),
            &bitmap,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }
}

fn rgba_to_bgra(rgba: &[u8]) -> Vec<u8> {
    let mut pixels = rgba.to_vec();
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    pixels
}

fn draw_button(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    label: &str,
    active: bool,
    normal: windows::Win32::Graphics::Gdi::HBRUSH,
    active_brush: windows::Win32::Graphics::Gdi::HBRUSH,
) {
    unsafe {
        let _ = FillRect(hdc, &rect, if active { active_brush } else { normal });
    }
    let mut wide = label.encode_utf16().collect::<Vec<u16>>();
    let mut text_rect = rect;
    unsafe {
        let _ = DrawTextW(
            hdc,
            &mut wide,
            &mut text_rect,
            DT_CENTER | DT_SINGLELINE | DT_VCENTER,
        );
    }
}

fn frame_thick(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    brush: windows::Win32::Graphics::Gdi::HBRUSH,
    thickness: i32,
) {
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
}

fn editor_done(hwnd: HWND) -> bool {
    unsafe { state_ref(hwnd).map(|s| s.done).unwrap_or(true) }
}

fn cancel(hwnd: HWND) {
    if let Some(state) = unsafe { state_mut(hwnd) } {
        state.canceled = true;
        state.done = true;
        state.drag = None;
    }
    unsafe {
        let _ = ReleaseCapture();
    }
}

fn resize_selection(
    handle: ResizeHandle,
    start: RectPx,
    start_pt: POINT,
    current: POINT,
    bounds: RectPx,
) -> RectPx {
    let dx = current.x - start_pt.x;
    let dy = current.y - start_pt.y;
    let mut left = start.left;
    let mut right = start.right;
    let mut top = start.top;
    let mut bottom = start.bottom;

    let move_left = matches!(
        handle,
        ResizeHandle::NW | ResizeHandle::W | ResizeHandle::SW
    );
    let move_right = matches!(
        handle,
        ResizeHandle::NE | ResizeHandle::E | ResizeHandle::SE
    );
    let move_top = matches!(
        handle,
        ResizeHandle::NW | ResizeHandle::N | ResizeHandle::NE
    );
    let move_bottom = matches!(
        handle,
        ResizeHandle::SW | ResizeHandle::S | ResizeHandle::SE
    );

    if move_left {
        left = (start.left + dx).clamp(bounds.left, right - MIN_SELECTION);
    }
    if move_right {
        right = (start.right + dx).clamp(left + MIN_SELECTION, bounds.right);
    }
    if move_top {
        top = (start.top + dy).clamp(bounds.top, bottom - MIN_SELECTION);
    }
    if move_bottom {
        bottom = (start.bottom + dy).clamp(top + MIN_SELECTION, bounds.bottom);
    }

    RectPx {
        left,
        top,
        right,
        bottom,
    }
}

fn normalize_abs(start: POINT, end: POINT, bounds: RectPx) -> RectPx {
    let sx = start.x.clamp(bounds.left, bounds.right);
    let sy = start.y.clamp(bounds.top, bounds.bottom);
    let ex = end.x.clamp(bounds.left, bounds.right);
    let ey = end.y.clamp(bounds.top, bounds.bottom);
    RectPx {
        left: sx.min(ex),
        top: sy.min(ey),
        right: sx.max(ex),
        bottom: sy.max(ey),
    }
}

fn clamp_point(p: POINT, bounds: RectPx) -> POINT {
    POINT {
        x: p.x.clamp(bounds.left, bounds.right),
        y: p.y.clamp(bounds.top, bounds.bottom),
    }
}

fn client_to_abs(client: POINT, bounds: RectPx) -> POINT {
    POINT {
        x: bounds.left + client.x,
        y: bounds.top + client.y,
    }
}

fn toolbar_layout(selection: RECT, client: RECT) -> ToolbarLayout {
    let avail_w = (client.right - client.left - (BAR_MARGIN * 2)).max(1);
    let width = (selection.right - selection.left + 220)
        .clamp(BAR_MIN_W, BAR_MAX_W)
        .min(avail_w);
    let center_x = selection.left + ((selection.right - selection.left) / 2);
    let min_left = client.left + BAR_MARGIN;
    let max_left = client.right - BAR_MARGIN - width;
    let left = if max_left < min_left {
        min_left
    } else {
        (center_x - (width / 2)).clamp(min_left, max_left)
    };
    let top = if selection.top - BAR_GAP - BAR_H >= client.top + BAR_MARGIN {
        selection.top - BAR_GAP - BAR_H
    } else if selection.bottom + BAR_GAP + BAR_H <= client.bottom - BAR_MARGIN {
        selection.bottom + BAR_GAP
    } else {
        (client.top + BAR_MARGIN).min(client.bottom - BAR_MARGIN - BAR_H)
    };
    let panel = RECT {
        left,
        top,
        right: left + width,
        bottom: top + BAR_H,
    };

    let btn_top = panel.top + ((BAR_H - BTN_H) / 2);
    let select_btn = RECT {
        left: panel.left + BAR_PAD_X,
        top: btn_top,
        right: panel.left + BAR_PAD_X + BTN_W,
        bottom: btn_top + BTN_H,
    };
    let rect_btn = RECT {
        left: select_btn.right + BTN_GAP,
        top: btn_top,
        right: select_btn.right + BTN_GAP + BTN_W,
        bottom: btn_top + BTN_H,
    };
    let info = RECT {
        left: rect_btn.right + BTN_GAP,
        top: panel.top,
        right: panel.right - BAR_PAD_X,
        bottom: panel.bottom,
    };

    ToolbarLayout {
        panel,
        select_btn,
        rect_btn,
        info,
    }
}

fn toolbar_hit(layout: ToolbarLayout, p: POINT) -> Option<ToolbarHit> {
    if point_in(p, layout.select_btn) {
        return Some(ToolbarHit::Select);
    }
    if point_in(p, layout.rect_btn) {
        return Some(ToolbarHit::Rect);
    }
    if point_in(p, layout.panel) {
        return Some(ToolbarHit::Panel);
    }
    None
}

fn hit_handle(selection: RECT, p: POINT) -> Option<ResizeHandle> {
    for (handle, rect) in handle_rects(selection) {
        if point_in(p, rect) {
            return Some(handle);
        }
    }
    None
}

fn update_cursor(hwnd: HWND, client: POINT) {
    let Some(state) = (unsafe { state_ref(hwnd) }) else {
        return;
    };
    let selection = to_client_rect(state.selection, state.virtual_rect);
    let bar = toolbar_layout(selection, client_rect(state.virtual_rect));

    let cursor_id = if let Some(hit) = toolbar_hit(bar, client) {
        match hit {
            ToolbarHit::Select | ToolbarHit::Rect => IDC_HAND,
            ToolbarHit::Panel => IDC_ARROW,
        }
    } else {
        match state.tool {
            Tool::Select => {
                let handle = match state.drag {
                    Some(Drag::Resize { handle, .. }) => Some(handle),
                    _ => hit_handle(selection, client),
                };
                if let Some(h) = handle {
                    cursor_for_handle(h)
                } else if matches!(state.drag, Some(Drag::Move { .. }))
                    || point_in(client, selection)
                {
                    IDC_SIZEALL
                } else {
                    IDC_CROSS
                }
            }
            Tool::Rectangle => {
                if point_in(client, selection) {
                    IDC_CROSS
                } else {
                    IDC_ARROW
                }
            }
        }
    };

    unsafe {
        if let Ok(cursor) = LoadCursorW(HINSTANCE::default(), cursor_id) {
            let _ = SetCursor(cursor);
        }
    }
}

fn cursor_for_handle(handle: ResizeHandle) -> PCWSTR {
    match handle {
        ResizeHandle::NW | ResizeHandle::SE => IDC_SIZENWSE,
        ResizeHandle::NE | ResizeHandle::SW => IDC_SIZENESW,
        ResizeHandle::N | ResizeHandle::S => IDC_SIZENS,
        ResizeHandle::W | ResizeHandle::E => IDC_SIZEWE,
    }
}

fn handle_rects(selection: RECT) -> [(ResizeHandle, RECT); 8] {
    let right = selection.right - 1;
    let bottom = selection.bottom - 1;
    let mid_x = selection.left + (selection.right - selection.left) / 2;
    let mid_y = selection.top + (selection.bottom - selection.top) / 2;
    [
        (ResizeHandle::NW, handle_rect(selection.left, selection.top)),
        (ResizeHandle::N, handle_rect(mid_x, selection.top)),
        (ResizeHandle::NE, handle_rect(right, selection.top)),
        (ResizeHandle::W, handle_rect(selection.left, mid_y)),
        (ResizeHandle::E, handle_rect(right, mid_y)),
        (ResizeHandle::SW, handle_rect(selection.left, bottom)),
        (ResizeHandle::S, handle_rect(mid_x, bottom)),
        (ResizeHandle::SE, handle_rect(right, bottom)),
    ]
}

fn handle_rect(cx: i32, cy: i32) -> RECT {
    let half = HANDLE_SIZE / 2;
    RECT {
        left: cx - half,
        top: cy - half,
        right: cx - half + HANDLE_SIZE,
        bottom: cy - half + HANDLE_SIZE,
    }
}

fn to_client_rect(abs: RectPx, virtual_rect: RectPx) -> RECT {
    RECT {
        left: abs.left - virtual_rect.left,
        top: abs.top - virtual_rect.top,
        right: abs.right - virtual_rect.left,
        bottom: abs.bottom - virtual_rect.top,
    }
}

fn client_rect(virtual_rect: RectPx) -> RECT {
    RECT {
        left: 0,
        top: 0,
        right: virtual_rect.width(),
        bottom: virtual_rect.height(),
    }
}

fn clamp_rect(rect: RectPx, bounds: RectPx) -> RectPx {
    let mut left = rect.left.clamp(bounds.left, bounds.right);
    let mut top = rect.top.clamp(bounds.top, bounds.bottom);
    let mut right = rect.right.clamp(bounds.left, bounds.right);
    let mut bottom = rect.bottom.clamp(bounds.top, bounds.bottom);
    if right < left {
        std::mem::swap(&mut left, &mut right);
    }
    if bottom < top {
        std::mem::swap(&mut top, &mut bottom);
    }
    if right - left < MIN_SELECTION {
        right = (left + MIN_SELECTION).min(bounds.right);
        left = (right - MIN_SELECTION).max(bounds.left);
    }
    if bottom - top < MIN_SELECTION {
        bottom = (top + MIN_SELECTION).min(bounds.bottom);
        top = (bottom - MIN_SELECTION).max(bounds.top);
    }
    RectPx {
        left,
        top,
        right,
        bottom,
    }
}

fn draw_rect_outline(image: &mut RgbaImage, rect: RectPx, color: [u8; 4], thickness: i32) {
    let width = image.width() as i32;
    let height = image.height() as i32;
    if width <= 0 || height <= 0 {
        return;
    }
    let left = rect.left.clamp(0, width);
    let right = rect.right.clamp(0, width);
    let top = rect.top.clamp(0, height);
    let bottom = rect.bottom.clamp(0, height);
    if right - left < 1 || bottom - top < 1 {
        return;
    }
    let px = Rgba(color);
    let max_strokes = ((right - left).min(bottom - top) / 2).max(1);
    let strokes = thickness.max(1).min(max_strokes);
    for i in 0..strokes {
        let l = left + i;
        let r = right - 1 - i;
        let t = top + i;
        let b = bottom - 1 - i;
        if l > r || t > b {
            break;
        }
        for x in l..=r {
            image.put_pixel(x as u32, t as u32, px);
            image.put_pixel(x as u32, b as u32, px);
        }
        for y in t..=b {
            image.put_pixel(l as u32, y as u32, px);
            image.put_pixel(r as u32, y as u32, px);
        }
    }
}

fn rect_changed(a: RectPx, b: RectPx) -> bool {
    a.left != b.left || a.top != b.top || a.right != b.right || a.bottom != b.bottom
}

fn point_in(p: POINT, r: RECT) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}

fn point_from_lparam(lparam: LPARAM) -> POINT {
    let raw = lparam.0 as u32;
    POINT {
        x: (raw & 0xFFFF) as i16 as i32,
        y: ((raw >> 16) & 0xFFFF) as i16 as i32,
    }
}

unsafe fn state_ref(hwnd: HWND) -> Option<&'static State> {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut State;
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}

unsafe fn state_mut(hwnd: HWND) -> Option<&'static mut State> {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut State;
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &mut *ptr })
    }
}

const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF((red as u32) | ((green as u32) << 8) | ((blue as u32) << 16))
}
