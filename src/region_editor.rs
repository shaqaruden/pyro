use std::mem::size_of;

use std::ffi::c_void;

use anyhow::{Result, bail};
use image::{Rgba, RgbaImage};
use windows::Win32::Foundation::{
    BOOL, COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BS_SOLID, BeginPaint, BitBlt, CreateCompatibleBitmap,
    CreateCompatibleDC, CreatePen, CreateSolidBrush, DIB_RGB_COLORS, DT_CENTER, DT_LEFT,
    DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, EndPaint, ExtCreatePen, FillRect,
    FrameRect, InvalidateRect, LOGBRUSH, LineTo, MoveToEx, PAINTSTRUCT, PS_ENDCAP_ROUND,
    PS_GEOMETRIC, PS_JOIN_ROUND, PS_SOLID, SRCCOPY, SelectObject, SetBkMode, SetTextColor,
    StretchDIBits, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL, VK_DELETE, VK_ESCAPE, VK_RETURN, VK_SHIFT,
    VK_Y, VK_Z,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetClientRect, GetCursorPos, GetMessageW, GetWindowLongPtrW, HTTRANSPARENT, HWND_TOPMOST,
    IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE,
    IDC_SIZEWE, LWA_ALPHA, LWA_COLORKEY, LoadCursorW, MSG, PostQuitMessage, RegisterClassW,
    SWP_SHOWWINDOW, SetCursor, SetForegroundWindow, SetLayeredWindowAttributes, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, TranslateMessage, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST, WM_PAINT,
    WM_RBUTTONUP, WM_SETCURSOR, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::capture;
use crate::platform_windows::{RectPx, virtual_screen_rect};

const HANDLE_SIZE: i32 = 8;
const MIN_SELECTION: i32 = 2;
const MIN_RECT: i32 = 3;
const DEFAULT_COLOR_INDEX: usize = 0;
const DEFAULT_THICKNESS_INDEX: usize = 0;
const ANNOTATION_COLORS: [[u8; 4]; 8] = [
    [255, 94, 94, 255],
    [255, 170, 67, 255],
    [255, 226, 86, 255],
    [95, 211, 130, 255],
    [72, 180, 255, 255],
    [90, 128, 255, 255],
    [182, 122, 255, 255],
    [255, 255, 255, 255],
];
const THICKNESS_STEPS: [i32; 5] = [2, 4, 6, 8, 12];
const MARKER_ALPHA: u8 = 112;
const MARKER_THICKNESS_SCALE: i32 = 3;
const MARKER_MIN_THICKNESS: i32 = 8;

const OVERLAY_DIM: COLORREF = rgb(0, 0, 0);
const OVERLAY_ALPHA: u8 = 118;
const OVERLAY_KEY: COLORREF = rgb(255, 0, 255);
const SELECTION_FILL: COLORREF = rgb(58, 58, 58);
const SELECTION_COLOR: COLORREF = rgb(0, 120, 215);
const HANDLE_COLOR: COLORREF = rgb(245, 245, 245);

const BAR_BG: COLORREF = rgb(16, 16, 16);
const BAR_BORDER: COLORREF = rgb(72, 72, 72);
const BAR_TEXT: COLORREF = rgb(238, 238, 238);
const BTN_BG: COLORREF = rgb(34, 34, 34);
const BTN_ACTIVE: COLORREF = rgb(0, 120, 215);

const BAR_MARGIN: i32 = 12;
const BAR_GAP: i32 = 10;
const BAR_H: i32 = 38;
const BAR_MIN_W: i32 = 980;
const BAR_MAX_W: i32 = 1360;
const BAR_PAD_X: i32 = 8;
const BTN_W: i32 = 90;
const BTN_H: i32 = 26;
const BTN_GAP: i32 = 8;
const TOOL_GROUP_GAP: i32 = 12;
const SWATCH_SIZE: i32 = 16;
const SWATCH_GAP: i32 = 6;
const THICK_BTN_W: i32 = 24;
const THICK_VALUE_W: i32 = 44;
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
    Ellipse,
    Line,
    Arrow,
    Marker,
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
    DrawEllipse {
        start: POINT,
        current: POINT,
    },
    DrawLine {
        start: POINT,
        current: POINT,
        arrow: bool,
    },
    DrawMarker {
        start: POINT,
        current: POINT,
    },
}

#[derive(Debug, Clone)]
enum Annotation {
    Rectangle(RectAnn),
    Ellipse(EllipseAnn),
    Line(LineAnn),
    Marker(MarkerAnn),
}

#[derive(Debug, Clone)]
struct RectAnn {
    rect_abs: RectPx,
    color: [u8; 4],
    thickness: i32,
}

#[derive(Debug, Clone)]
struct LineAnn {
    start_abs: POINT,
    end_abs: POINT,
    color: [u8; 4],
    thickness: i32,
    arrow: bool,
}

#[derive(Debug, Clone)]
struct EllipseAnn {
    rect_abs: RectPx,
    color: [u8; 4],
    thickness: i32,
}

#[derive(Debug, Clone)]
struct MarkerAnn {
    points_abs: Vec<POINT>,
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
    ellipse_btn: RECT,
    line_btn: RECT,
    arrow_btn: RECT,
    marker_btn: RECT,
    swatches: [RECT; ANNOTATION_COLORS.len()],
    thinner_btn: RECT,
    thicker_btn: RECT,
    thickness_value: RECT,
    info: RECT,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ToolbarHit {
    Select,
    Rect,
    Ellipse,
    Line,
    Arrow,
    Marker,
    Color(usize),
    ThicknessDown,
    ThicknessUp,
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
    selected_annotation: Option<usize>,
    stroke_color_idx: usize,
    stroke_thickness_idx: usize,
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
            selected_annotation: None,
            stroke_color_idx: DEFAULT_COLOR_INDEX,
            stroke_thickness_idx: DEFAULT_THICKNESS_INDEX,
            done: false,
            canceled: false,
        }
    }

    fn stroke_color(&self) -> [u8; 4] {
        ANNOTATION_COLORS[self.stroke_color_idx]
    }

    fn stroke_thickness(&self) -> i32 {
        THICKNESS_STEPS[self.stroke_thickness_idx]
    }

    fn marker_color(&self) -> [u8; 4] {
        let base = self.stroke_color();
        [base[0], base[1], base[2], MARKER_ALPHA]
    }

    fn marker_thickness(&self) -> i32 {
        (self.stroke_thickness() * MARKER_THICKNESS_SCALE).max(MARKER_MIN_THICKNESS)
    }

    fn set_stroke_color(&mut self, idx: usize) -> bool {
        if idx >= ANNOTATION_COLORS.len() || self.stroke_color_idx == idx {
            return false;
        }
        self.stroke_color_idx = idx;
        true
    }

    fn adjust_stroke_thickness(&mut self, delta: i32) -> bool {
        let current = self.stroke_thickness_idx as i32;
        let next = (current + delta).clamp(0, THICKNESS_STEPS.len() as i32 - 1);
        if next == current {
            return false;
        }
        self.stroke_thickness_idx = next as usize;
        true
    }

    fn set_selection(&mut self, next: RectPx) -> bool {
        if !rect_changed(self.selection, next) {
            return false;
        }
        self.selection = next;
        self.selection_snapshot = None;
        self.selected_annotation = None;
        if !self.annotations.is_empty() || !self.redo.is_empty() {
            self.annotations.clear();
            self.redo.clear();
        }
        true
    }

    fn update_drag(&mut self, abs: POINT, shift_down: bool) -> bool {
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
            Drag::DrawEllipse { start, current } => {
                let next = if shift_down {
                    constrain_equal_axes(start, abs, current)
                } else {
                    abs
                };
                if current.x == next.x && current.y == next.y {
                    return false;
                }
                self.drag = Some(Drag::DrawEllipse {
                    start,
                    current: next,
                });
                true
            }
            Drag::DrawLine {
                start,
                current,
                arrow,
            } => {
                if current.x == abs.x && current.y == abs.y {
                    return false;
                }
                self.drag = Some(Drag::DrawLine {
                    start,
                    current: abs,
                    arrow,
                });
                true
            }
            Drag::DrawMarker { start, current } => {
                let next = if shift_down {
                    snap_point_to_45(start, abs)
                } else {
                    abs
                };
                if next.x == current.x && next.y == current.y {
                    return false;
                }
                self.drag = Some(Drag::DrawMarker {
                    start,
                    current: next,
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

    fn pending_ellipse(&self) -> Option<RectPx> {
        let Drag::DrawEllipse { start, current } = self.drag? else {
            return None;
        };
        Some(normalize_abs(start, current, self.selection))
    }

    fn pending_line(&self) -> Option<(POINT, POINT, bool)> {
        let Drag::DrawLine {
            start,
            current,
            arrow,
        } = self.drag?
        else {
            return None;
        };
        Some((start, current, arrow))
    }

    fn pending_marker(&self) -> Option<(POINT, POINT)> {
        let Drag::DrawMarker { start, current } = self.drag? else {
            return None;
        };
        if (start.x - current.x).abs() < 1 && (start.y - current.y).abs() < 1 {
            return None;
        }
        Some((start, current))
    }

    fn finalize_draw(&mut self) -> bool {
        let stroke_color = self.stroke_color();
        let stroke_thickness = self.stroke_thickness();
        match self.drag.take() {
            Some(Drag::DrawRect { start, current }) => {
                let rect = normalize_abs(start, current, self.selection);
                if rect.width() < MIN_RECT || rect.height() < MIN_RECT {
                    return false;
                }
                self.annotations.push(Annotation::Rectangle(RectAnn {
                    rect_abs: rect,
                    color: stroke_color,
                    thickness: stroke_thickness,
                }));
                self.redo.clear();
                self.selected_annotation = Some(self.annotations.len() - 1);
                true
            }
            Some(Drag::DrawEllipse { start, current }) => {
                let rect = normalize_abs(start, current, self.selection);
                if rect.width() < MIN_RECT || rect.height() < MIN_RECT {
                    return false;
                }
                self.annotations.push(Annotation::Ellipse(EllipseAnn {
                    rect_abs: rect,
                    color: stroke_color,
                    thickness: stroke_thickness,
                }));
                self.redo.clear();
                self.selected_annotation = Some(self.annotations.len() - 1);
                true
            }
            Some(Drag::DrawLine {
                start,
                current,
                arrow,
            }) => {
                if (start.x - current.x).abs() < 1 && (start.y - current.y).abs() < 1 {
                    return false;
                }
                self.annotations.push(Annotation::Line(LineAnn {
                    start_abs: start,
                    end_abs: current,
                    color: stroke_color,
                    thickness: stroke_thickness,
                    arrow,
                }));
                self.redo.clear();
                self.selected_annotation = Some(self.annotations.len() - 1);
                true
            }
            Some(Drag::DrawMarker { start, current }) => {
                if (start.x - current.x).abs() < 1 && (start.y - current.y).abs() < 1 {
                    return false;
                }
                let marker_color = self.marker_color();
                let marker_thickness = self.marker_thickness();
                self.annotations.push(Annotation::Marker(MarkerAnn {
                    points_abs: vec![start, current],
                    color: marker_color,
                    thickness: marker_thickness,
                }));
                self.redo.clear();
                self.selected_annotation = Some(self.annotations.len() - 1);
                true
            }
            other => {
                self.drag = other;
                false
            }
        }
    }

    fn undo(&mut self) -> bool {
        let Some(last) = self.annotations.pop() else {
            return false;
        };
        self.redo.push(last);
        self.selected_annotation = None;
        true
    }

    fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.annotations.push(next);
        self.selected_annotation = None;
        true
    }

    fn clear_drag_state(&mut self) {
        self.drag = None;
    }

    fn select_annotation_at(&mut self, point_abs: POINT) -> bool {
        let previous = self.selected_annotation;
        self.selected_annotation = hit_annotation(&self.annotations, point_abs);
        self.selected_annotation != previous
    }

    fn delete_selected_annotation(&mut self) -> bool {
        let Some(idx) = self.selected_annotation else {
            return false;
        };
        if idx >= self.annotations.len() {
            self.selected_annotation = None;
            return false;
        }
        self.annotations.remove(idx);
        self.redo.clear();
        self.selected_annotation = None;
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
            Annotation::Ellipse(ellipse) => {
                let local = RectPx {
                    left: ellipse.rect_abs.left - result.bounds.left,
                    top: ellipse.rect_abs.top - result.bounds.top,
                    right: ellipse.rect_abs.right - result.bounds.left,
                    bottom: ellipse.rect_abs.bottom - result.bounds.top,
                };
                draw_ellipse_outline(image, local, ellipse.color, ellipse.thickness);
            }
            Annotation::Line(line) => {
                let start = (
                    line.start_abs.x - result.bounds.left,
                    line.start_abs.y - result.bounds.top,
                );
                let end = (
                    line.end_abs.x - result.bounds.left,
                    line.end_abs.y - result.bounds.top,
                );
                draw_line(image, start, end, line.color, line.thickness);
                if line.arrow {
                    draw_arrow_head(image, start, end, line.color, line.thickness);
                }
            }
            Annotation::Marker(marker) => {
                if marker.points_abs.len() < 2 {
                    continue;
                }
                let mut last = marker.points_abs[0];
                for point in marker.points_abs.iter().copied().skip(1) {
                    draw_line(
                        image,
                        (last.x - result.bounds.left, last.y - result.bounds.top),
                        (point.x - result.bounds.left, point.y - result.bounds.top),
                        marker.color,
                        marker.thickness,
                    );
                    last = point;
                }
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
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
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
        WM_MOUSEWHEEL => on_mouse_wheel(hwnd, wparam),
        WM_SETCURSOR => on_set_cursor(hwnd),
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
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
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

    if key == VK_DELETE.0 as u32 {
        if let Some(state) = unsafe { state_mut(hwnd) }
            && state.delete_selected_annotation()
        {
            invalidate_all(hwnd);
        }
        return LRESULT(0);
    }

    if !ctrl_down {
        if key == 0xDB {
            if let Some(state) = unsafe { state_mut(hwnd) }
                && state.adjust_stroke_thickness(-1)
            {
                invalidate_all(hwnd);
            }
            return LRESULT(0);
        }

        if key == 0xDD {
            if let Some(state) = unsafe { state_mut(hwnd) }
                && state.adjust_stroke_thickness(1)
            {
                invalidate_all(hwnd);
            }
            return LRESULT(0);
        }

        let color_idx = if (0x31..=0x38).contains(&key) {
            Some((key - 0x31) as usize)
        } else if (0x61..=0x68).contains(&key) {
            Some((key - 0x61) as usize)
        } else {
            None
        };
        if let Some(idx) = color_idx {
            if let Some(state) = unsafe { state_mut(hwnd) }
                && state.set_stroke_color(idx)
            {
                invalidate_all(hwnd);
            }
            return LRESULT(0);
        }

        if let Some(state) = unsafe { state_mut(hwnd) } {
            let next_tool = match key {
                0x52 => Some(Tool::Rectangle), // R
                0x45 => Some(Tool::Ellipse),   // E
                0x4C => Some(Tool::Line),      // L
                0x41 => Some(Tool::Arrow),     // A
                0x4D => Some(Tool::Marker),    // M
                0x53 => Some(Tool::Select),    // S
                _ => None,
            };
            if let Some(tool) = next_tool
                && state.tool != tool
            {
                state.tool = tool;
                if tool == Tool::Select {
                    state.selection_snapshot = None;
                } else {
                    state.selection_snapshot = capture_selection_snapshot(state.selection);
                }
                state.clear_drag_state();
                unsafe {
                    let _ = set_layer_mode(hwnd, state.tool);
                }
                invalidate_all(hwnd);
                return LRESULT(0);
            }
        }
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
            let mut layer_changed = false;
            match hit {
                ToolbarHit::Select => {
                    if state.tool != Tool::Select {
                        state.tool = Tool::Select;
                        state.selection_snapshot = None;
                        changed = true;
                        layer_changed = true;
                    }
                }
                ToolbarHit::Rect => {
                    if state.tool != Tool::Rectangle {
                        state.selection_snapshot = capture_selection_snapshot(state.selection);
                        state.tool = Tool::Rectangle;
                        changed = true;
                        layer_changed = true;
                    }
                }
                ToolbarHit::Ellipse => {
                    if state.tool != Tool::Ellipse {
                        state.selection_snapshot = capture_selection_snapshot(state.selection);
                        state.tool = Tool::Ellipse;
                        changed = true;
                        layer_changed = true;
                    }
                }
                ToolbarHit::Line => {
                    if state.tool != Tool::Line {
                        state.selection_snapshot = capture_selection_snapshot(state.selection);
                        state.tool = Tool::Line;
                        changed = true;
                        layer_changed = true;
                    }
                }
                ToolbarHit::Arrow => {
                    if state.tool != Tool::Arrow {
                        state.selection_snapshot = capture_selection_snapshot(state.selection);
                        state.tool = Tool::Arrow;
                        changed = true;
                        layer_changed = true;
                    }
                }
                ToolbarHit::Marker => {
                    if state.tool != Tool::Marker {
                        state.selection_snapshot = capture_selection_snapshot(state.selection);
                        state.tool = Tool::Marker;
                        changed = true;
                        layer_changed = true;
                    }
                }
                ToolbarHit::Color(idx) => {
                    changed = state.set_stroke_color(idx) || changed;
                }
                ToolbarHit::ThicknessDown => {
                    changed = state.adjust_stroke_thickness(-1) || changed;
                }
                ToolbarHit::ThicknessUp => {
                    changed = state.adjust_stroke_thickness(1) || changed;
                }
                ToolbarHit::Panel => {}
            }
            state.clear_drag_state();
            if layer_changed {
                unsafe {
                    let _ = set_layer_mode(hwnd, state.tool);
                }
            }
            if changed {
                invalidate_all(hwnd);
            }
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
                    if state.select_annotation_at(abs) {
                        invalidate_all(hwnd);
                        update_cursor(hwnd, client);
                        return LRESULT(0);
                    }
                    state.selected_annotation = None;
                    state.drag = Some(Drag::Move {
                        offset_x: abs.x - state.selection.left,
                        offset_y: abs.y - state.selection.top,
                        width: state.selection.width(),
                        height: state.selection.height(),
                    });
                    started_drag = true;
                } else {
                    state.selected_annotation = None;
                    state.drag = Some(Drag::NewSelection { start: abs });
                    let _ = state.set_selection(normalize_abs(abs, abs, state.virtual_rect));
                    started_drag = true;
                }
            }
            Tool::Rectangle => {
                if point_in(client, selection_client) {
                    state.selected_annotation = None;
                    state.drag = Some(Drag::DrawRect {
                        start: abs,
                        current: abs,
                    });
                    started_drag = true;
                }
            }
            Tool::Ellipse => {
                if point_in(client, selection_client) {
                    state.selected_annotation = None;
                    state.drag = Some(Drag::DrawEllipse {
                        start: abs,
                        current: abs,
                    });
                    started_drag = true;
                }
            }
            Tool::Line => {
                if point_in(client, selection_client) {
                    state.selected_annotation = None;
                    state.drag = Some(Drag::DrawLine {
                        start: abs,
                        current: abs,
                        arrow: false,
                    });
                    started_drag = true;
                }
            }
            Tool::Arrow => {
                if point_in(client, selection_client) {
                    state.selected_annotation = None;
                    state.drag = Some(Drag::DrawLine {
                        start: abs,
                        current: abs,
                        arrow: true,
                    });
                    started_drag = true;
                }
            }
            Tool::Marker => {
                if point_in(client, selection_client) {
                    state.selected_annotation = None;
                    state.drag = Some(Drag::DrawMarker {
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
    let mut changed_tool: Option<Tool> = None;
    if let Some(state) = unsafe { state_mut(hwnd) }
        && state.drag.is_some()
    {
        let shift_down = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
        let abs = clamp_point(
            client_to_abs(client, state.virtual_rect),
            state.virtual_rect,
        );
        if state.update_drag(abs, shift_down) {
            changed_tool = Some(state.tool);
        }
    }
    if let Some(tool) = changed_tool {
        invalidate_for_tool_drag(hwnd, tool);
    }
    update_cursor(hwnd, client);
    LRESULT(0)
}

fn on_mouse_up(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let client = point_from_lparam(lparam);
    let mut repaint = false;
    let mut tool_for_repaint = Tool::Select;
    let mut finalized_annotation = false;
    if let Some(state) = unsafe { state_mut(hwnd) }
        && state.drag.is_some()
    {
        tool_for_repaint = state.tool;
        let shift_down = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
        let abs = clamp_point(
            client_to_abs(client, state.virtual_rect),
            state.virtual_rect,
        );
        if state.update_drag(abs, shift_down) {
            repaint = true;
        }
        if state.finalize_draw() {
            repaint = true;
            finalized_annotation = true;
        } else {
            state.clear_drag_state();
        }
    }
    unsafe {
        let _ = ReleaseCapture();
    }
    if repaint {
        if finalized_annotation {
            invalidate_all(hwnd);
        } else {
            invalidate_for_tool_drag(hwnd, tool_for_repaint);
        }
    }
    update_cursor(hwnd, client);
    LRESULT(0)
}

fn on_mouse_wheel(hwnd: HWND, wparam: WPARAM) -> LRESULT {
    let delta = ((wparam.0 >> 16) as u16 as i16) as i32;
    if delta == 0 {
        return LRESULT(0);
    }

    let direction = if delta > 0 { 1 } else { -1 };
    let steps = (delta.abs() / 120).max(1);
    let mut changed = false;

    if let Some(state) = unsafe { state_mut(hwnd) }
        && state.tool != Tool::Select
    {
        for _ in 0..steps {
            changed = state.adjust_stroke_thickness(direction) || changed;
        }
    }

    if changed {
        invalidate_all(hwnd);
    }
    LRESULT(0)
}

fn on_set_cursor(hwnd: HWND) -> LRESULT {
    let Some(state) = (unsafe { state_ref(hwnd) }) else {
        return LRESULT(0);
    };
    let mut screen = POINT::default();
    if unsafe { GetCursorPos(&mut screen) }.is_err() {
        return LRESULT(0);
    }
    let client = POINT {
        x: screen.x - state.virtual_rect.left,
        y: screen.y - state.virtual_rect.top,
    };
    update_cursor(hwnd, client);
    LRESULT(1)
}

fn invalidate_all(hwnd: HWND) {
    invalidate_base(hwnd);
    invalidate_chrome(hwnd);
}

fn invalidate_base(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(hwnd, None, BOOL(0));
    }
}

fn invalidate_base_selection(hwnd: HWND) {
    if let Some(state) = unsafe { state_ref(hwnd) } {
        let selection = to_client_rect(state.selection, state.virtual_rect);
        unsafe {
            let _ = InvalidateRect(hwnd, Some(&selection), BOOL(0));
        }
    } else {
        invalidate_base(hwnd);
    }
}

fn invalidate_chrome(hwnd: HWND) {
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

fn invalidate_for_tool_drag(hwnd: HWND, tool: Tool) {
    match tool {
        Tool::Select => invalidate_all(hwnd),
        Tool::Marker => invalidate_base_selection(hwnd),
        Tool::Rectangle | Tool::Ellipse | Tool::Line | Tool::Arrow => invalidate_chrome(hwnd),
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
    let dirty = ps.rcPaint;
    let dirty_width = dirty.right - dirty.left;
    let dirty_height = dirty.bottom - dirty.top;
    if dirty_width <= 0 || dirty_height <= 0 {
        unsafe {
            let _ = EndPaint(hwnd, &ps);
        }
        return;
    }

    let shade = unsafe { CreateSolidBrush(OVERLAY_DIM) };
    let transparent = unsafe { CreateSolidBrush(OVERLAY_KEY) };
    let selection_fill = unsafe { CreateSolidBrush(SELECTION_FILL) };
    let mem_dc = unsafe { CreateCompatibleDC(hdc) };
    if mem_dc.0.is_null() {
        cleanup_paint_objects(&[shade, transparent, selection_fill]);
        unsafe {
            let _ = EndPaint(hwnd, &ps);
        }
        return;
    }
    let mem_bitmap = unsafe { CreateCompatibleBitmap(hdc, dirty_width, dirty_height) };
    if mem_bitmap.0.is_null() {
        unsafe {
            let _ = DeleteDC(mem_dc);
        }
        cleanup_paint_objects(&[shade, transparent, selection_fill]);
        unsafe {
            let _ = EndPaint(hwnd, &ps);
        }
        return;
    }
    let old_bitmap = unsafe { SelectObject(mem_dc, mem_bitmap) };
    let dirty_local = RECT {
        left: 0,
        top: 0,
        right: dirty_width,
        bottom: dirty_height,
    };

    unsafe {
        let _ = FillRect(mem_dc, &dirty_local, shade);
    }
    let selection_client = to_client_rect(state.selection, state.virtual_rect);
    let selection = offset_rect(selection_client, dirty.left, dirty.top);
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
        if let Annotation::Marker(marker) = ann {
            draw_marker_overlay(
                mem_dc,
                marker
                    .points_abs
                    .iter()
                    .copied()
                    .map(|p| to_client_point(p, state.virtual_rect))
                    .map(|p| offset_point(p, dirty.left, dirty.top)),
                rgba_to_colorref(marker.color),
                marker.thickness,
            );
        }
    }
    if let Some((start, end)) = state.pending_marker() {
        draw_marker_overlay(
            mem_dc,
            [start, end]
                .into_iter()
                .map(|p| to_client_point(p, state.virtual_rect))
                .map(|p| offset_point(p, dirty.left, dirty.top)),
            rgba_to_colorref(state.marker_color()),
            state.marker_thickness(),
        );
    }

    unsafe {
        let _ = BitBlt(
            hdc,
            dirty.left,
            dirty.top,
            dirty_width,
            dirty_height,
            mem_dc,
            0,
            0,
            SRCCOPY,
        );
        let _ = SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(mem_bitmap);
        let _ = DeleteDC(mem_dc);
        let _ = EndPaint(hwnd, &ps);
    }
    cleanup_paint_objects(&[shade, transparent, selection_fill]);
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
    let stroke_color = rgba_to_colorref(state.stroke_color());
    let stroke_thickness = state.stroke_thickness();
    for ann in &state.annotations {
        match ann {
            Annotation::Rectangle(rect) => {
                frame_thick_color(
                    mem_dc,
                    to_client_rect(rect.rect_abs, state.virtual_rect),
                    rgba_to_colorref(rect.color),
                    rect.thickness,
                );
            }
            Annotation::Ellipse(ellipse) => {
                draw_ellipse_outline_overlay(
                    mem_dc,
                    to_client_rect(ellipse.rect_abs, state.virtual_rect),
                    rgba_to_colorref(ellipse.color),
                    ellipse.thickness,
                );
            }
            Annotation::Line(line) => {
                draw_line_overlay(
                    mem_dc,
                    to_client_point(line.start_abs, state.virtual_rect),
                    to_client_point(line.end_abs, state.virtual_rect),
                    rgba_to_colorref(line.color),
                    line.thickness,
                );
                if line.arrow {
                    draw_arrow_head_overlay(
                        mem_dc,
                        to_client_point(line.start_abs, state.virtual_rect),
                        to_client_point(line.end_abs, state.virtual_rect),
                        rgba_to_colorref(line.color),
                        line.thickness,
                    );
                }
            }
            Annotation::Marker(_) => {}
        }
    }
    if let Some(pending) = state.pending_rect() {
        frame_thick_color(
            mem_dc,
            to_client_rect(pending, state.virtual_rect),
            stroke_color,
            stroke_thickness,
        );
    }
    if let Some((start, end, arrow)) = state.pending_line() {
        let start_client = to_client_point(start, state.virtual_rect);
        let end_client = to_client_point(end, state.virtual_rect);
        draw_line_overlay(
            mem_dc,
            start_client,
            end_client,
            stroke_color,
            stroke_thickness,
        );
        if arrow {
            draw_arrow_head_overlay(
                mem_dc,
                start_client,
                end_client,
                stroke_color,
                stroke_thickness,
            );
        }
    }
    if let Some(pending) = state.pending_ellipse() {
        draw_ellipse_outline_overlay(
            mem_dc,
            to_client_rect(pending, state.virtual_rect),
            stroke_color,
            stroke_thickness,
        );
    }
    if let Some(selected_idx) = state.selected_annotation
        && let Some(bounds) = state
            .annotations
            .get(selected_idx)
            .and_then(annotation_bounds_abs)
    {
        frame_thick_color(
            mem_dc,
            to_client_rect(bounds, state.virtual_rect),
            rgb(255, 255, 255),
            1,
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
    draw_button(
        mem_dc,
        bar.ellipse_btn,
        "Ellipse",
        state.tool == Tool::Ellipse,
        btn_bg,
        btn_active,
    );
    draw_button(
        mem_dc,
        bar.line_btn,
        "Line",
        state.tool == Tool::Line,
        btn_bg,
        btn_active,
    );
    draw_button(
        mem_dc,
        bar.arrow_btn,
        "Arrow",
        state.tool == Tool::Arrow,
        btn_bg,
        btn_active,
    );
    draw_button(
        mem_dc,
        bar.marker_btn,
        "Marker",
        state.tool == Tool::Marker,
        btn_bg,
        btn_active,
    );
    for (idx, swatch) in bar.swatches.iter().copied().enumerate() {
        let swatch_brush = unsafe { CreateSolidBrush(rgba_to_colorref(ANNOTATION_COLORS[idx])) };
        if !swatch_brush.0.is_null() {
            unsafe {
                let _ = FillRect(mem_dc, &swatch, swatch_brush);
            }
            unsafe {
                let _ = DeleteObject(swatch_brush);
            }
        }
        frame_thick(
            mem_dc,
            swatch,
            if idx == state.stroke_color_idx {
                btn_active
            } else {
                bar_border
            },
            if idx == state.stroke_color_idx { 2 } else { 1 },
        );
    }
    draw_button(mem_dc, bar.thinner_btn, "-", false, btn_bg, btn_active);
    draw_button(mem_dc, bar.thicker_btn, "+", false, btn_bg, btn_active);
    frame_thick(mem_dc, bar.thickness_value, bar_border, 1);
    unsafe {
        let _ = FillRect(mem_dc, &bar.thickness_value, btn_bg);
    }
    let mut px_label = format!("{}px", stroke_thickness)
        .encode_utf16()
        .collect::<Vec<u16>>();
    let mut px_rect = bar.thickness_value;
    unsafe {
        let _ = DrawTextW(
            mem_dc,
            &mut px_label,
            &mut px_rect,
            DT_CENTER | DT_SINGLELINE | DT_VCENTER,
        );
    }

    let info = format!(
        "{}x{} | Ann: {} | Color 1-8 | Thickness [ ] / wheel | Marker = translucent highlighter (hold Shift to snap 45deg) | Del removes selected | Enter capture | Esc cancel | Ctrl+Z/Y",
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

fn frame_thick_color(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    color: COLORREF,
    thickness: i32,
) {
    let brush = unsafe { CreateSolidBrush(color) };
    if brush.0.is_null() {
        return;
    }
    frame_thick(hdc, rect, brush, thickness);
    unsafe {
        let _ = DeleteObject(brush);
    }
}

fn draw_line_overlay(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    start: POINT,
    end: POINT,
    color: COLORREF,
    thickness: i32,
) {
    let width = thickness.max(1) as u32;
    let style = PS_GEOMETRIC | PS_SOLID | PS_ENDCAP_ROUND | PS_JOIN_ROUND;
    let brush = LOGBRUSH {
        lbStyle: BS_SOLID,
        lbColor: color,
        lbHatch: 0,
    };
    let mut pen = unsafe { ExtCreatePen(style, width, &brush, None) };
    if pen.0.is_null() {
        pen = unsafe { CreatePen(PS_SOLID, thickness.max(1), color) };
    }
    if pen.0.is_null() {
        return;
    }

    unsafe {
        let previous = SelectObject(hdc, pen);
        let _ = MoveToEx(hdc, start.x, start.y, None);
        let _ = LineTo(hdc, end.x, end.y);
        let _ = SelectObject(hdc, previous);
        let _ = DeleteObject(pen);
    }
}

fn draw_arrow_head_overlay(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    start: POINT,
    end: POINT,
    color: COLORREF,
    thickness: i32,
) {
    let dx = (end.x - start.x) as f32;
    let dy = (end.y - start.y) as f32;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }

    let ux = dx / len;
    let uy = dy / len;
    let px = -uy;
    let py = ux;
    let head_len = (10 + (thickness * 2)) as f32;
    let head_width = (5 + thickness) as f32;
    let left = POINT {
        x: (end.x as f32 - (ux * head_len) + (px * head_width)).round() as i32,
        y: (end.y as f32 - (uy * head_len) + (py * head_width)).round() as i32,
    };
    let right = POINT {
        x: (end.x as f32 - (ux * head_len) - (px * head_width)).round() as i32,
        y: (end.y as f32 - (uy * head_len) - (py * head_width)).round() as i32,
    };
    draw_line_overlay(hdc, end, left, color, thickness);
    draw_line_overlay(hdc, end, right, color, thickness);
}

fn draw_ellipse_outline_overlay(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    color: COLORREF,
    thickness: i32,
) {
    if rect.right - rect.left < 2 || rect.bottom - rect.top < 2 {
        return;
    }
    let cx = (rect.left + rect.right) as f32 * 0.5;
    let cy = (rect.top + rect.bottom) as f32 * 0.5;
    let rx = (rect.right - rect.left) as f32 * 0.5;
    let ry = (rect.bottom - rect.top) as f32 * 0.5;
    if rx < 1.0 || ry < 1.0 {
        return;
    }

    let steps = ellipse_steps(rx, ry);
    let mut prev = ellipse_point(cx, cy, rx, ry, 0.0);
    for i in 1..=steps {
        let t = (i as f32 / steps as f32) * std::f32::consts::TAU;
        let next = ellipse_point(cx, cy, rx, ry, t);
        draw_line_overlay(hdc, prev, next, color, thickness);
        prev = next;
    }
}

fn draw_marker_overlay(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    points: impl IntoIterator<Item = POINT>,
    color: COLORREF,
    thickness: i32,
) {
    let mut iter = points.into_iter();
    let Some(mut last) = iter.next() else {
        return;
    };
    for point in iter {
        draw_line_overlay(hdc, last, point, color, thickness);
        last = point;
    }
}

fn editor_done(hwnd: HWND) -> bool {
    unsafe { state_ref(hwnd).map(|s| s.done).unwrap_or(true) }
}

fn cancel(hwnd: HWND) {
    if let Some(state) = unsafe { state_mut(hwnd) } {
        state.canceled = true;
        state.done = true;
        state.clear_drag_state();
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
    let swatch_top = panel.top + ((BAR_H - SWATCH_SIZE) / 2);
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
    let ellipse_btn = RECT {
        left: rect_btn.right + BTN_GAP,
        top: btn_top,
        right: rect_btn.right + BTN_GAP + BTN_W,
        bottom: btn_top + BTN_H,
    };
    let line_btn = RECT {
        left: ellipse_btn.right + BTN_GAP,
        top: btn_top,
        right: ellipse_btn.right + BTN_GAP + BTN_W,
        bottom: btn_top + BTN_H,
    };
    let arrow_btn = RECT {
        left: line_btn.right + BTN_GAP,
        top: btn_top,
        right: line_btn.right + BTN_GAP + BTN_W,
        bottom: btn_top + BTN_H,
    };
    let marker_btn = RECT {
        left: arrow_btn.right + BTN_GAP,
        top: btn_top,
        right: arrow_btn.right + BTN_GAP + BTN_W,
        bottom: btn_top + BTN_H,
    };
    let mut swatches = [RECT::default(); ANNOTATION_COLORS.len()];
    let mut x = marker_btn.right + TOOL_GROUP_GAP;
    for swatch in &mut swatches {
        *swatch = RECT {
            left: x,
            top: swatch_top,
            right: x + SWATCH_SIZE,
            bottom: swatch_top + SWATCH_SIZE,
        };
        x += SWATCH_SIZE + SWATCH_GAP;
    }
    x += TOOL_GROUP_GAP - SWATCH_GAP;
    let thinner_btn = RECT {
        left: x,
        top: btn_top,
        right: x + THICK_BTN_W,
        bottom: btn_top + BTN_H,
    };
    let thickness_value = RECT {
        left: thinner_btn.right + SWATCH_GAP,
        top: btn_top,
        right: thinner_btn.right + SWATCH_GAP + THICK_VALUE_W,
        bottom: btn_top + BTN_H,
    };
    let thicker_btn = RECT {
        left: thickness_value.right + SWATCH_GAP,
        top: btn_top,
        right: thickness_value.right + SWATCH_GAP + THICK_BTN_W,
        bottom: btn_top + BTN_H,
    };
    let info_left = thicker_btn.right + BTN_GAP;
    let info_right = (panel.right - BAR_PAD_X).max(info_left);
    let info = RECT {
        left: info_left,
        top: panel.top,
        right: info_right,
        bottom: panel.bottom,
    };

    ToolbarLayout {
        panel,
        select_btn,
        rect_btn,
        ellipse_btn,
        line_btn,
        arrow_btn,
        marker_btn,
        swatches,
        thinner_btn,
        thicker_btn,
        thickness_value,
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
    if point_in(p, layout.ellipse_btn) {
        return Some(ToolbarHit::Ellipse);
    }
    if point_in(p, layout.line_btn) {
        return Some(ToolbarHit::Line);
    }
    if point_in(p, layout.arrow_btn) {
        return Some(ToolbarHit::Arrow);
    }
    if point_in(p, layout.marker_btn) {
        return Some(ToolbarHit::Marker);
    }
    for (idx, swatch) in layout.swatches.iter().copied().enumerate() {
        if point_in(p, swatch) {
            return Some(ToolbarHit::Color(idx));
        }
    }
    if point_in(p, layout.thinner_btn) {
        return Some(ToolbarHit::ThicknessDown);
    }
    if point_in(p, layout.thicker_btn) {
        return Some(ToolbarHit::ThicknessUp);
    }
    if point_in(p, layout.thickness_value) {
        return Some(ToolbarHit::Panel);
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
            ToolbarHit::Select
            | ToolbarHit::Rect
            | ToolbarHit::Ellipse
            | ToolbarHit::Line
            | ToolbarHit::Arrow
            | ToolbarHit::Marker
            | ToolbarHit::Color(_)
            | ToolbarHit::ThicknessDown
            | ToolbarHit::ThicknessUp => IDC_HAND,
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
            Tool::Ellipse => {
                if point_in(client, selection) {
                    IDC_CROSS
                } else {
                    IDC_ARROW
                }
            }
            Tool::Line | Tool::Arrow => {
                if point_in(client, selection) {
                    IDC_CROSS
                } else {
                    IDC_ARROW
                }
            }
            Tool::Marker => {
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

fn to_client_point(abs: POINT, virtual_rect: RectPx) -> POINT {
    POINT {
        x: abs.x - virtual_rect.left,
        y: abs.y - virtual_rect.top,
    }
}

fn offset_rect(rect: RECT, offset_x: i32, offset_y: i32) -> RECT {
    RECT {
        left: rect.left - offset_x,
        top: rect.top - offset_y,
        right: rect.right - offset_x,
        bottom: rect.bottom - offset_y,
    }
}

fn offset_point(point: POINT, offset_x: i32, offset_y: i32) -> POINT {
    POINT {
        x: point.x - offset_x,
        y: point.y - offset_y,
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

fn draw_ellipse_outline(image: &mut RgbaImage, rect: RectPx, color: [u8; 4], thickness: i32) {
    if rect.width() < 2 || rect.height() < 2 {
        return;
    }
    let cx = (rect.left + rect.right) as f32 * 0.5;
    let cy = (rect.top + rect.bottom) as f32 * 0.5;
    let rx = (rect.right - rect.left) as f32 * 0.5;
    let ry = (rect.bottom - rect.top) as f32 * 0.5;
    if rx < 1.0 || ry < 1.0 {
        return;
    }

    let steps = ellipse_steps(rx, ry);
    let mut prev = ellipse_point(cx, cy, rx, ry, 0.0);
    for i in 1..=steps {
        let t = (i as f32 / steps as f32) * std::f32::consts::TAU;
        let next = ellipse_point(cx, cy, rx, ry, t);
        draw_line(
            image,
            (prev.x, prev.y),
            (next.x, next.y),
            color,
            thickness.max(1),
        );
        prev = next;
    }
}

fn draw_line(
    image: &mut RgbaImage,
    start: (i32, i32),
    end: (i32, i32),
    color: [u8; 4],
    thickness: i32,
) {
    let dx = (end.0 - start.0) as f32;
    let dy = (end.1 - start.1) as f32;
    let len = (dx * dx + dy * dy).sqrt();
    let radius = (thickness.max(1) as f32) * 0.5;

    if len < 0.5 {
        draw_brush_dot_aa(image, start.0 as f32, start.1 as f32, color, radius);
        return;
    }

    // Sample at half-pixel intervals for smoother diagonal strokes.
    let steps = (len * 2.0).ceil().max(1.0) as i32;
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = start.0 as f32 + (dx * t);
        let y = start.1 as f32 + (dy * t);
        draw_brush_dot_aa(image, x, y, color, radius);
    }
}

fn draw_arrow_head(
    image: &mut RgbaImage,
    start: (i32, i32),
    end: (i32, i32),
    color: [u8; 4],
    thickness: i32,
) {
    let dx = (end.0 - start.0) as f32;
    let dy = (end.1 - start.1) as f32;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }

    let ux = dx / len;
    let uy = dy / len;
    let px = -uy;
    let py = ux;
    let head_len = (10 + (thickness * 2)) as f32;
    let head_width = (5 + thickness) as f32;
    let left = (
        (end.0 as f32 - (ux * head_len) + (px * head_width)).round() as i32,
        (end.1 as f32 - (uy * head_len) + (py * head_width)).round() as i32,
    );
    let right = (
        (end.0 as f32 - (ux * head_len) - (px * head_width)).round() as i32,
        (end.1 as f32 - (uy * head_len) - (py * head_width)).round() as i32,
    );
    draw_line(image, end, left, color, thickness);
    draw_line(image, end, right, color, thickness);
}

fn draw_brush_dot_aa(image: &mut RgbaImage, x: f32, y: f32, color: [u8; 4], radius: f32) {
    let width = image.width() as i32;
    let height = image.height() as i32;
    if width <= 0 || height <= 0 {
        return;
    }

    let feather = 1.0_f32;
    let x_min = (x - radius - feather).floor().max(0.0) as i32;
    let x_max = (x + radius + feather).ceil().min((width - 1) as f32) as i32;
    let y_min = (y - radius - feather).floor().max(0.0) as i32;
    let y_max = (y + radius + feather).ceil().min((height - 1) as f32) as i32;

    for py in y_min..=y_max {
        for px_x in x_min..=x_max {
            let dx = (px_x as f32 + 0.5) - x;
            let dy = (py as f32 + 0.5) - y;
            let dist = (dx * dx + dy * dy).sqrt();
            let coverage = (radius + feather - dist).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            blend_pixel(image, px_x, py, color, coverage);
        }
    }
}

fn blend_pixel(image: &mut RgbaImage, x: i32, y: i32, color: [u8; 4], coverage: f32) {
    let dst = image.get_pixel_mut(x as u32, y as u32);
    let src_a = (color[3] as f32 / 255.0) * coverage;
    if src_a <= 0.0 {
        return;
    }

    let inv = 1.0 - src_a;
    let mut out = [0u8; 4];
    for i in 0..3 {
        out[i] = ((color[i] as f32 * src_a) + (dst.0[i] as f32 * inv))
            .round()
            .clamp(0.0, 255.0) as u8;
    }
    let dst_a = dst.0[3] as f32 / 255.0;
    out[3] = ((src_a + (dst_a * inv)).clamp(0.0, 1.0) * 255.0).round() as u8;
    *dst = Rgba(out);
}

fn ellipse_steps(rx: f32, ry: f32) -> i32 {
    ((rx + ry) * 2.2).round().clamp(24.0, 360.0) as i32
}

fn ellipse_point(cx: f32, cy: f32, rx: f32, ry: f32, angle: f32) -> POINT {
    POINT {
        x: (cx + (rx * angle.cos())).round() as i32,
        y: (cy + (ry * angle.sin())).round() as i32,
    }
}

fn snap_point_to_45(origin: POINT, raw: POINT) -> POINT {
    let dx = (raw.x - origin.x) as f32;
    let dy = (raw.y - origin.y) as f32;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.5 {
        return origin;
    }
    let angle = dy.atan2(dx);
    let snapped = (angle / (std::f32::consts::FRAC_PI_4)).round() * std::f32::consts::FRAC_PI_4;
    POINT {
        x: (origin.x as f32 + (len * snapped.cos())).round() as i32,
        y: (origin.y as f32 + (len * snapped.sin())).round() as i32,
    }
}

fn constrain_equal_axes(origin: POINT, raw: POINT, fallback: POINT) -> POINT {
    let dx = raw.x - origin.x;
    let dy = raw.y - origin.y;
    if dx == 0 && dy == 0 {
        return raw;
    }

    let side = dx.abs().max(dy.abs());
    let fdx = fallback.x - origin.x;
    let fdy = fallback.y - origin.y;
    let sx = if dx != 0 {
        dx.signum()
    } else if fdx < 0 {
        -1
    } else {
        1
    };
    let sy = if dy != 0 {
        dy.signum()
    } else if fdy < 0 {
        -1
    } else {
        1
    };

    POINT {
        x: origin.x + (sx * side),
        y: origin.y + (sy * side),
    }
}

fn annotation_bounds_abs(annotation: &Annotation) -> Option<RectPx> {
    match annotation {
        Annotation::Rectangle(rect) => Some(rect.rect_abs),
        Annotation::Ellipse(ellipse) => Some(ellipse.rect_abs),
        Annotation::Line(line) => {
            let pad = line.thickness.max(1);
            Some(RectPx {
                left: line.start_abs.x.min(line.end_abs.x) - pad,
                top: line.start_abs.y.min(line.end_abs.y) - pad,
                right: line.start_abs.x.max(line.end_abs.x) + pad + 1,
                bottom: line.start_abs.y.max(line.end_abs.y) + pad + 1,
            })
        }
        Annotation::Marker(marker) => marker_bounds(marker),
    }
}

fn marker_bounds(marker: &MarkerAnn) -> Option<RectPx> {
    let mut iter = marker.points_abs.iter().copied();
    let first = iter.next()?;
    let mut left = first.x;
    let mut top = first.y;
    let mut right = first.x;
    let mut bottom = first.y;
    for p in iter {
        left = left.min(p.x);
        top = top.min(p.y);
        right = right.max(p.x);
        bottom = bottom.max(p.y);
    }
    let pad = marker.thickness.max(1);
    Some(RectPx {
        left: left - pad,
        top: top - pad,
        right: right + pad + 1,
        bottom: bottom + pad + 1,
    })
}

fn hit_annotation(annotations: &[Annotation], point_abs: POINT) -> Option<usize> {
    const HIT_TOLERANCE: f32 = 7.0;
    for idx in (0..annotations.len()).rev() {
        if annotation_hit(&annotations[idx], point_abs, HIT_TOLERANCE) {
            return Some(idx);
        }
    }
    None
}

fn annotation_hit(annotation: &Annotation, point_abs: POINT, tolerance: f32) -> bool {
    match annotation {
        Annotation::Rectangle(rect) => point_near_rect_outline(
            point_abs,
            rect.rect_abs,
            tolerance + rect.thickness as f32 * 0.5,
        ),
        Annotation::Ellipse(ellipse) => point_near_ellipse_outline(
            point_abs,
            ellipse.rect_abs,
            tolerance + ellipse.thickness as f32 * 0.5,
        ),
        Annotation::Line(line) => {
            point_to_segment_distance(point_abs, line.start_abs, line.end_abs)
                <= (tolerance + line.thickness as f32 * 0.5)
        }
        Annotation::Marker(marker) => {
            let mut iter = marker.points_abs.iter().copied();
            let Some(mut last) = iter.next() else {
                return false;
            };
            let threshold = tolerance + marker.thickness as f32 * 0.5;
            for p in iter {
                if point_to_segment_distance(point_abs, last, p) <= threshold {
                    return true;
                }
                last = p;
            }
            false
        }
    }
}

fn point_near_rect_outline(point: POINT, rect: RectPx, tolerance: f32) -> bool {
    let x = point.x as f32;
    let y = point.y as f32;
    let left = rect.left as f32;
    let top = rect.top as f32;
    let right = rect.right as f32;
    let bottom = rect.bottom as f32;

    if x < left - tolerance
        || x > right + tolerance
        || y < top - tolerance
        || y > bottom + tolerance
    {
        return false;
    }
    !(x > left + tolerance
        && x < right - tolerance
        && y > top + tolerance
        && y < bottom - tolerance)
}

fn point_near_ellipse_outline(point: POINT, rect: RectPx, tolerance: f32) -> bool {
    let width = rect.width() as f32;
    let height = rect.height() as f32;
    if width < 2.0 || height < 2.0 {
        return false;
    }
    let cx = (rect.left + rect.right) as f32 * 0.5;
    let cy = (rect.top + rect.bottom) as f32 * 0.5;
    let rx = width * 0.5;
    let ry = height * 0.5;
    let nx = (point.x as f32 - cx) / rx;
    let ny = (point.y as f32 - cy) / ry;
    let radius = (nx * nx + ny * ny).sqrt();
    let eps = tolerance / rx.min(ry).max(1.0);
    radius >= (1.0 - eps) && radius <= (1.0 + eps)
}

fn point_to_segment_distance(point: POINT, a: POINT, b: POINT) -> f32 {
    let px = point.x as f32;
    let py = point.y as f32;
    let ax = a.x as f32;
    let ay = a.y as f32;
    let bx = b.x as f32;
    let by = b.y as f32;
    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = (dx * dx) + (dy * dy);
    if len_sq <= f32::EPSILON {
        let ddx = px - ax;
        let ddy = py - ay;
        return (ddx * ddx + ddy * ddy).sqrt();
    }
    let t = (((px - ax) * dx) + ((py - ay) * dy)) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let cx = ax + (dx * t);
    let cy = ay + (dy * t);
    let ddx = px - cx;
    let ddy = py - cy;
    (ddx * ddx + ddy * ddy).sqrt()
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

const fn rgba_to_colorref(color: [u8; 4]) -> COLORREF {
    rgb(color[0], color[1], color[2])
}

const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF((red as u32) | ((green as u32) << 8) | ((blue as u32) << 16))
}
