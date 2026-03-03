use std::ffi::c_void;
use std::mem::size_of;
use std::ptr;

use anyhow::{Result, bail};
use image::{GenericImageView, RgbaImage, imageops::FilterType};
use windows::Win32::Foundation::{
    BOOL, COLORREF, ERROR_SUCCESS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
    CreateBitmap, CreateDIBSection, CreateFontW, CreateRoundRectRgn, CreateSolidBrush,
    DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_LEFT, DT_SINGLELINE, DT_VCENTER,
    DeleteObject, DrawTextW, EndPaint, FF_DONTCARE, FW_NORMAL, FillRect, InvalidateRect,
    OUT_DEFAULT_PRECIS, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_ESCAPE,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION, NIN_SELECT,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, GWLP_USERDATA,
    GetClientRect, GetCursorPos, GetWindowLongPtrW, HMENU, HICON, HWND_TOPMOST, ICONINFO,
    IDC_ARROW, LoadCursorW, PostMessageW, RegisterClassW, SWP_SHOWWINDOW, SetCursor,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WM_CONTEXTMENU, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_RBUTTONUP, WM_SETCURSOR, WM_USER, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

const TRAY_ICON_ID: u32 = 1;

const ACTION_CAPTURE_DEFAULT: usize = 1;
const ACTION_CAPTURE_PRIMARY: usize = 2;
const ACTION_CAPTURE_REGION: usize = 3;
const ACTION_CAPTURE_ALL: usize = 4;
const ACTION_SETTINGS: usize = 5;
const ACTION_QUIT: usize = 6;

pub const TRAY_CALLBACK_MESSAGE: u32 = WM_USER + 1;
pub const TRAY_ACTION_MESSAGE: u32 = WM_APP + 100;

const WM_MOUSELEAVE: u32 = 0x02A3;

const POPUP_WIDTH: i32 = 268;
const POPUP_OUTER_PADDING_Y: i32 = 8;
const POPUP_ROW_HEIGHT: i32 = 36;
const POPUP_SEPARATOR_HEIGHT: i32 = 10;
const POPUP_ROW_MARGIN_X: i32 = 8;
const POPUP_TEXT_PADDING_X: i32 = 14;
const POPUP_CORNER_RADIUS: i32 = 14;
const TRAY_ICON_BASE_SIZE: u32 = 16;
const TRAY_ICON_MAX_SIZE: u32 = 32;

const TRAY_ICON_PNG_16: &[u8] = include_bytes!("assets/tray-icon-16.png");
const TRAY_ICON_PNG_20: &[u8] = include_bytes!("assets/tray-icon-20.png");
const TRAY_ICON_PNG_24: &[u8] = include_bytes!("assets/tray-icon-24.png");
const TRAY_ICON_PNG_32: &[u8] = include_bytes!("assets/tray-icon-32.png");

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TrayAction {
    CaptureDefault,
    CapturePrimary,
    CaptureRegion,
    CaptureAllDisplays,
    Settings,
    Quit,
}

impl TrayAction {
    pub fn from_code(code: usize) -> Option<Self> {
        match code {
            ACTION_CAPTURE_DEFAULT => Some(Self::CaptureDefault),
            ACTION_CAPTURE_PRIMARY => Some(Self::CapturePrimary),
            ACTION_CAPTURE_REGION => Some(Self::CaptureRegion),
            ACTION_CAPTURE_ALL => Some(Self::CaptureAllDisplays),
            ACTION_SETTINGS => Some(Self::Settings),
            ACTION_QUIT => Some(Self::Quit),
            _ => None,
        }
    }

    fn code(self) -> usize {
        match self {
            Self::CaptureDefault => ACTION_CAPTURE_DEFAULT,
            Self::CapturePrimary => ACTION_CAPTURE_PRIMARY,
            Self::CaptureRegion => ACTION_CAPTURE_REGION,
            Self::CaptureAllDisplays => ACTION_CAPTURE_ALL,
            Self::Settings => ACTION_SETTINGS,
            Self::Quit => ACTION_QUIT,
        }
    }
}

#[derive(Clone, Copy)]
enum PopupRow {
    Action(&'static str, TrayAction),
    Separator,
}

const POPUP_ROWS: [PopupRow; 7] = [
    PopupRow::Action("Capture Region", TrayAction::CaptureRegion),
    PopupRow::Action("Capture Primary", TrayAction::CapturePrimary),
    PopupRow::Action("Capture All Displays", TrayAction::CaptureAllDisplays),
    PopupRow::Separator,
    PopupRow::Action("Settings...", TrayAction::Settings),
    PopupRow::Separator,
    PopupRow::Action("Quit", TrayAction::Quit),
];

pub struct TrayHost {
    hwnd: HWND,
    tray_icon: HICON,
}

impl TrayHost {
    pub fn create() -> Result<Self> {
        let hwnd = create_hidden_window()?;
        let tray_icon = load_tray_icon(hwnd)?;
        if let Err(err) = add_tray_icon(hwnd, tray_icon) {
            unsafe {
                let _ = DestroyIcon(tray_icon);
                let _ = DestroyWindow(hwnd);
            }
            return Err(err);
        }

        Ok(Self { hwnd, tray_icon })
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for TrayHost {
    fn drop(&mut self) {
        unsafe {
            let _ = remove_tray_icon(self.hwnd);
            if !self.tray_icon.0.is_null() {
                let _ = DestroyIcon(self.tray_icon);
            }
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn create_hidden_window() -> Result<HWND> {
    let hmodule = unsafe { GetModuleHandleW(PCWSTR::null()).map_err(anyhow::Error::from)? };
    let hinstance = HINSTANCE(hmodule.0);
    register_window_classes(hinstance);

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("PyroTrayWindowClass"),
            w!("PyroTrayWindow"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            HWND::default(),
            HMENU::default(),
            hinstance,
            None,
        )
        .map_err(anyhow::Error::from)?
    };
    Ok(hwnd)
}

fn register_window_classes(hinstance: HINSTANCE) {
    let tray_class = WNDCLASSW {
        lpfnWndProc: Some(tray_window_proc),
        hInstance: hinstance,
        hCursor: unsafe { LoadCursorW(HINSTANCE::default(), IDC_ARROW).unwrap_or_default() },
        lpszClassName: w!("PyroTrayWindowClass"),
        ..Default::default()
    };
    let _ = unsafe { RegisterClassW(&tray_class) };

    let popup_class = WNDCLASSW {
        lpfnWndProc: Some(popup_window_proc),
        hInstance: hinstance,
        hCursor: unsafe { LoadCursorW(HINSTANCE::default(), IDC_ARROW).unwrap_or_default() },
        lpszClassName: w!("PyroTrayPopupMenuClass"),
        ..Default::default()
    };
    let _ = unsafe { RegisterClassW(&popup_class) };
}

fn load_tray_icon(hwnd: HWND) -> Result<HICON> {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let dpi = if dpi == 0 { 96 } else { dpi };
    let preferred_size =
        ((TRAY_ICON_BASE_SIZE * dpi + 48) / 96).clamp(TRAY_ICON_BASE_SIZE, TRAY_ICON_MAX_SIZE);

    let source_png = match preferred_size {
        0..=18 => TRAY_ICON_PNG_16,
        19..=22 => TRAY_ICON_PNG_20,
        23..=28 => TRAY_ICON_PNG_24,
        _ => TRAY_ICON_PNG_32,
    };

    let decoded = image::load_from_memory(source_png).map_err(|err| {
        anyhow::anyhow!("failed to decode embedded tray icon PNG ({}px): {err}", preferred_size)
    })?;
    let (src_w, src_h) = decoded.dimensions();
    if src_w == 0 || src_h == 0 {
        bail!("embedded tray icon PNG has invalid dimensions");
    }

    let mut rgba: RgbaImage = decoded.to_rgba8();
    if src_w != preferred_size || src_h != preferred_size {
        rgba = image::imageops::resize(
            &rgba,
            preferred_size,
            preferred_size,
            FilterType::CatmullRom,
        );
    }

    create_hicon_from_rgba(&rgba)
}

fn create_hicon_from_rgba(rgba: &RgbaImage) -> Result<HICON> {
    let width = i32::try_from(rgba.width()).map_err(|_| anyhow::anyhow!("icon width too large"))?;
    let height =
        i32::try_from(rgba.height()).map_err(|_| anyhow::anyhow!("icon height too large"))?;
    if width <= 0 || height <= 0 {
        bail!("invalid icon dimensions {}x{}", width, height);
    }

    let mut bgra = vec![0u8; rgba.as_raw().len()];
    for (src, dst) in rgba
        .as_raw()
        .chunks_exact(4)
        .zip(bgra.chunks_exact_mut(4))
    {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }

    let mut bitmap = BITMAPINFO::default();
    bitmap.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };

    let mut bits = ptr::null_mut::<c_void>();
    let color_bitmap = unsafe { CreateDIBSection(None, &bitmap, DIB_RGB_COLORS, &mut bits, None, 0) }
        .map_err(anyhow::Error::from)?;
    if color_bitmap.0.is_null() || bits.is_null() {
        if !color_bitmap.0.is_null() {
            unsafe {
                let _ = DeleteObject(color_bitmap);
            }
        }
        bail!("CreateDIBSection for tray icon failed");
    }

    unsafe {
        ptr::copy_nonoverlapping(bgra.as_ptr(), bits.cast::<u8>(), bgra.len());
    }

    let mask_stride = (((width + 15) / 16) * 2).max(1);
    let mask_len = (mask_stride * height) as usize;
    let mask_bits = vec![0u8; mask_len];
    let mask_bitmap = unsafe { CreateBitmap(width, height, 1, 1, Some(mask_bits.as_ptr().cast())) };
    if mask_bitmap.0.is_null() {
        unsafe {
            let _ = DeleteObject(color_bitmap);
        }
        bail!("CreateBitmap mask for tray icon failed");
    }

    let icon_info = ICONINFO {
        fIcon: BOOL(1),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask_bitmap,
        hbmColor: color_bitmap,
    };

    let icon = unsafe { CreateIconIndirect(&icon_info) }.map_err(anyhow::Error::from);

    unsafe {
        let _ = DeleteObject(mask_bitmap);
        let _ = DeleteObject(color_bitmap);
    }

    icon
}

fn add_tray_icon(hwnd: HWND, icon: HICON) -> Result<()> {
    let mut data = NOTIFYICONDATAW::default();
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = TRAY_ICON_ID;
    data.uFlags = NIF_MESSAGE | NIF_TIP | NIF_ICON;
    data.uCallbackMessage = TRAY_CALLBACK_MESSAGE;
    data.hIcon = icon;
    write_wide_cstr("Pyro", &mut data.szTip);

    let added = unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool();
    if !added {
        bail!("Shell_NotifyIconW(NIM_ADD) failed");
    }

    data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    let _ = unsafe { Shell_NotifyIconW(NIM_SETVERSION, &data) };

    Ok(())
}

fn remove_tray_icon(hwnd: HWND) -> Result<()> {
    let mut data = NOTIFYICONDATAW::default();
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = TRAY_ICON_ID;
    let removed = unsafe { Shell_NotifyIconW(NIM_DELETE, &data) }.as_bool();
    if !removed {
        bail!("Shell_NotifyIconW(NIM_DELETE) failed");
    }
    Ok(())
}

unsafe extern "system" fn tray_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == TRAY_CALLBACK_MESSAGE {
        let raw_event = lparam.0 as u32;
        tracing::debug!("tray callback raw event={raw_event}");

        if matches_notify_event(raw_event, WM_LBUTTONDBLCLK)
            || matches_notify_event(raw_event, NIN_SELECT)
        {
            let _ = post_action(hwnd, TrayAction::CaptureDefault);
            return LRESULT(0);
        }

        if matches_notify_event(raw_event, WM_RBUTTONUP)
            || matches_notify_event(raw_event, WM_CONTEXTMENU)
        {
            let _ = show_modern_popup(hwnd);
            return LRESULT(0);
        }

        return LRESULT(0);
    }

    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

struct PopupState {
    owner: HWND,
    dark_mode: bool,
    hovered_row: Option<usize>,
    tracking_mouse: bool,
}

unsafe extern "system" fn popup_window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCCREATE => {
            let create = unsafe {
                &*(lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW)
            };
            let state_ptr = create.lpCreateParams as *mut PopupState;
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
            }
            return LRESULT(1);
        }
        WM_MOUSEMOVE => {
            on_popup_mouse_move(hwnd, lparam);
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            if let Some(state) = unsafe { popup_state_mut(hwnd) } {
                state.tracking_mouse = false;
                if state.hovered_row.take().is_some() {
                    unsafe {
                        let _ = InvalidateRect(hwnd, None, BOOL(0));
                    }
                }
            }
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            on_popup_click(hwnd, lparam);
            return LRESULT(0);
        }
        WM_SETCURSOR => {
            unsafe {
                let cursor = LoadCursorW(HINSTANCE::default(), IDC_ARROW).unwrap_or_default();
                let _ = SetCursor(cursor);
            }
            return LRESULT(1);
        }
        WM_KEYDOWN => {
            if wparam.0 as u32 == VK_ESCAPE.0 as u32 {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
            return LRESULT(0);
        }
        WM_KILLFOCUS => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            return LRESULT(0);
        }
        WM_PAINT => {
            paint_popup(hwnd);
            return LRESULT(0);
        }
        WM_NCDESTROY => {
            let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut PopupState;
            if !ptr.is_null() {
                unsafe {
                    drop(Box::from_raw(ptr));
                    let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
            }
            return LRESULT(0);
        }
        _ => {}
    }

    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn show_modern_popup(owner: HWND) -> Result<()> {
    let mut cursor = POINT::default();
    unsafe {
        GetCursorPos(&mut cursor).map_err(anyhow::Error::from)?;
    }

    let state = Box::new(PopupState {
        owner,
        dark_mode: is_dark_mode_enabled(),
        hovered_row: None,
        tracking_mouse: false,
    });
    let state_ptr = Box::into_raw(state);

    let popup = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("PyroTrayPopupMenuClass"),
            w!("PyroTrayPopup"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            owner,
            HMENU::default(),
            HINSTANCE::default(),
            Some(state_ptr.cast::<c_void>()),
        )
    };

    let popup = match popup {
        Ok(hwnd) => hwnd,
        Err(err) => {
            unsafe {
                drop(Box::from_raw(state_ptr));
            }
            return Err(anyhow::Error::from(err));
        }
    };

    let height = popup_height();
    let x = cursor.x - (POPUP_WIDTH / 5);
    let y = cursor.y - 10;

    unsafe {
        SetWindowPos(
            popup,
            HWND_TOPMOST,
            x,
            y,
            POPUP_WIDTH,
            height,
            SWP_SHOWWINDOW,
        )
        .map_err(anyhow::Error::from)?;
        let _ = SetForegroundWindow(popup);
    }

    apply_popup_chrome(
        popup,
        unsafe { popup_state_mut(popup) }
            .map(|s| s.dark_mode)
            .unwrap_or(false),
    );
    Ok(())
}

fn apply_popup_chrome(hwnd: HWND, dark_mode: bool) {
    let dark_flag = if dark_mode { BOOL(1) } else { BOOL(0) };
    let corner = DWMWCP_ROUND;

    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark_flag as *const _ as *const c_void,
            size_of::<BOOL>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const DWM_WINDOW_CORNER_PREFERENCE as *const c_void,
            size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );

        let region = CreateRoundRectRgn(
            0,
            0,
            POPUP_WIDTH + 1,
            popup_height() + 1,
            POPUP_CORNER_RADIUS * 2,
            POPUP_CORNER_RADIUS * 2,
        );
        let _ = SetWindowRgn(hwnd, region, BOOL(1));
    }
}

fn on_popup_mouse_move(hwnd: HWND, lparam: LPARAM) {
    let point = point_from_lparam(lparam);
    if let Some(state) = unsafe { popup_state_mut(hwnd) } {
        if !state.tracking_mouse {
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                let _ = TrackMouseEvent(&mut tracking);
            }
            state.tracking_mouse = true;
        }

        let new_hover = hover_row_at_point(point);
        if new_hover != state.hovered_row {
            state.hovered_row = new_hover;
            unsafe {
                let _ = InvalidateRect(hwnd, None, BOOL(0));
            }
        }
    }
}

fn on_popup_click(hwnd: HWND, lparam: LPARAM) {
    let point = point_from_lparam(lparam);
    let action = action_at_point(point);

    if let Some(state) = unsafe { popup_state_mut(hwnd) } {
        if let Some(action) = action {
            let _ = post_action(state.owner, action);
        }
    }

    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

fn paint_popup(hwnd: HWND) {
    let state = if let Some(value) = unsafe { popup_state_mut(hwnd) } {
        value
    } else {
        return;
    };

    let palette = PopupPalette::from_dark_mode(state.dark_mode);
    let mut paint = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut paint) };
    if hdc.0.is_null() {
        return;
    }

    let mut client = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client);
    }

    let border_brush = unsafe { CreateSolidBrush(palette.border) };
    let background_brush = unsafe { CreateSolidBrush(palette.background) };
    let hover_brush = unsafe { CreateSolidBrush(palette.hover) };
    let separator_brush = unsafe { CreateSolidBrush(palette.separator) };

    unsafe {
        let _ = FillRect(hdc, &client, border_brush);
    }

    let mut inner = client;
    inner.left += 1;
    inner.top += 1;
    inner.right -= 1;
    inner.bottom -= 1;
    unsafe {
        let _ = FillRect(hdc, &inner, background_brush);
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, palette.text);
    }

    let menu_font = create_menu_font();
    let previous_font = if menu_font.0.is_null() {
        None
    } else {
        Some(unsafe { SelectObject(hdc, menu_font) })
    };

    let mut current_y = POPUP_OUTER_PADDING_Y;
    for (index, row) in POPUP_ROWS.iter().enumerate() {
        match row {
            PopupRow::Separator => {
                let line_y = current_y + (POPUP_SEPARATOR_HEIGHT / 2);
                let line = RECT {
                    left: POPUP_ROW_MARGIN_X + 8,
                    top: line_y,
                    right: POPUP_WIDTH - POPUP_ROW_MARGIN_X - 8,
                    bottom: line_y + 1,
                };
                unsafe {
                    let _ = FillRect(hdc, &line, separator_brush);
                }
                current_y += POPUP_SEPARATOR_HEIGHT;
            }
            PopupRow::Action(label, _) => {
                let row_rect = RECT {
                    left: POPUP_ROW_MARGIN_X,
                    top: current_y,
                    right: POPUP_WIDTH - POPUP_ROW_MARGIN_X,
                    bottom: current_y + POPUP_ROW_HEIGHT,
                };

                if state.hovered_row == Some(index) {
                    unsafe {
                        let _ = FillRect(hdc, &row_rect, hover_brush);
                    }
                }

                let mut text_rect = row_rect;
                text_rect.left += POPUP_TEXT_PADDING_X;
                text_rect.right -= POPUP_TEXT_PADDING_X;
                let mut wide = label.encode_utf16().collect::<Vec<u16>>();
                unsafe {
                    let _ = DrawTextW(
                        hdc,
                        &mut wide,
                        &mut text_rect,
                        DT_LEFT | DT_VCENTER | DT_SINGLELINE,
                    );
                }

                current_y += POPUP_ROW_HEIGHT;
            }
        }
    }

    unsafe {
        if let Some(previous_font) = previous_font {
            let _ = SelectObject(hdc, previous_font);
        }
        if !menu_font.0.is_null() {
            let _ = DeleteObject(menu_font);
        }
        let _ = DeleteObject(border_brush);
        let _ = DeleteObject(background_brush);
        let _ = DeleteObject(hover_brush);
        let _ = DeleteObject(separator_brush);
        let _ = EndPaint(hwnd, &paint);
    }
}

fn popup_height() -> i32 {
    POPUP_OUTER_PADDING_Y * 2
        + POPUP_ROWS
            .iter()
            .map(|row| match row {
                PopupRow::Action(_, _) => POPUP_ROW_HEIGHT,
                PopupRow::Separator => POPUP_SEPARATOR_HEIGHT,
            })
            .sum::<i32>()
}

fn hover_row_at_point(point: POINT) -> Option<usize> {
    let mut current_y = POPUP_OUTER_PADDING_Y;
    for (index, row) in POPUP_ROWS.iter().enumerate() {
        match row {
            PopupRow::Separator => {
                current_y += POPUP_SEPARATOR_HEIGHT;
            }
            PopupRow::Action(_, _) => {
                let row_rect = RECT {
                    left: POPUP_ROW_MARGIN_X,
                    top: current_y,
                    right: POPUP_WIDTH - POPUP_ROW_MARGIN_X,
                    bottom: current_y + POPUP_ROW_HEIGHT,
                };
                if point_in_rect(point, row_rect) {
                    return Some(index);
                }
                current_y += POPUP_ROW_HEIGHT;
            }
        }
    }

    None
}

fn action_at_point(point: POINT) -> Option<TrayAction> {
    let hovered = hover_row_at_point(point)?;
    match POPUP_ROWS.get(hovered)? {
        PopupRow::Action(_, action) => Some(*action),
        PopupRow::Separator => None,
    }
}

fn point_in_rect(point: POINT, rect: RECT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

fn point_from_lparam(lparam: LPARAM) -> POINT {
    let raw = lparam.0 as u32;
    let x = (raw & 0xFFFF) as i16 as i32;
    let y = ((raw >> 16) & 0xFFFF) as i16 as i32;
    POINT { x, y }
}

fn post_action(hwnd: HWND, action: TrayAction) -> Result<()> {
    unsafe {
        PostMessageW(hwnd, TRAY_ACTION_MESSAGE, WPARAM(action.code()), LPARAM(0))
            .map_err(anyhow::Error::from)?;
    }
    Ok(())
}

fn write_wide_cstr(input: &str, output: &mut [u16]) {
    output.fill(0);
    for (index, unit) in input.encode_utf16().enumerate() {
        if index + 1 >= output.len() {
            break;
        }
        output[index] = unit;
    }
}

fn matches_notify_event(raw: u32, expected: u32) -> bool {
    raw == expected || (raw & 0xFFFF) == expected
}

unsafe fn popup_state_mut(hwnd: HWND) -> Option<&'static mut PopupState> {
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut PopupState;
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &mut *ptr })
    }
}

fn is_dark_mode_enabled() -> bool {
    let mut value: u32 = 1;
    let mut size = size_of::<u32>() as u32;

    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast::<c_void>()),
            Some(&mut size),
        )
    };

    status == ERROR_SUCCESS && value == 0
}

fn create_menu_font() -> windows::Win32::Graphics::Gdi::HFONT {
    for face in [
        w!("Segoe UI Variable Text"),
        w!("Segoe UI Variable"),
        w!("Segoe UI"),
    ] {
        let font = create_font_for_face(face);
        if !font.0.is_null() {
            return font;
        }
    }

    windows::Win32::Graphics::Gdi::HFONT::default()
}

fn create_font_for_face(face: PCWSTR) -> windows::Win32::Graphics::Gdi::HFONT {
    unsafe {
        CreateFontW(
            -16,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET.0 as u32,
            OUT_DEFAULT_PRECIS.0 as u32,
            CLIP_DEFAULT_PRECIS.0 as u32,
            CLEARTYPE_QUALITY.0 as u32,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            face,
        )
    }
}

struct PopupPalette {
    background: COLORREF,
    border: COLORREF,
    hover: COLORREF,
    separator: COLORREF,
    text: COLORREF,
}

impl PopupPalette {
    fn from_dark_mode(dark_mode: bool) -> Self {
        if dark_mode {
            Self {
                background: rgb(32, 32, 32),
                border: rgb(58, 58, 58),
                hover: rgb(62, 62, 62),
                separator: rgb(77, 77, 77),
                text: rgb(240, 240, 240),
            }
        } else {
            Self {
                background: rgb(248, 248, 248),
                border: rgb(216, 216, 216),
                hover: rgb(232, 232, 232),
                separator: rgb(224, 224, 224),
                text: rgb(26, 26, 26),
            }
        }
    }
}

const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF((red as u32) | ((green as u32) << 8) | ((blue as u32) << 16))
}
