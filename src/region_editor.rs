use std::cell::RefCell;
use std::collections::HashSet;
use std::mem::size_of;
use std::ptr;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use std::ffi::c_void;

use anyhow::{Result, bail};
use image::{Rgba, RgbaImage};
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType,
};
use slint::platform::{Platform, PlatformError, WindowAdapter};
use slint::{Color, ComponentHandle};
use windows::Win32::Foundation::{
    BOOL, COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, AlphaBlend, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    BS_SOLID, BeginPaint, BitBlt, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleBitmap,
    CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen, CreateRoundRectRgn,
    CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CALCRECT, DT_CENTER,
    DT_LEFT, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, Ellipse,
    EndPaint, ExtCreatePen, FF_DONTCARE, FillRect, FrameRect, FrameRgn, HGDIOBJ, InvalidateRect,
    LOGBRUSH, LineTo, MoveToEx, OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_ENDCAP_ROUND, PS_GEOMETRIC,
    PS_JOIN_ROUND, PS_SOLID, RestoreDC, RoundRect, SRCCOPY, SaveDC, SelectClipRgn, SelectObject,
    SetBkMode, SetTextColor, StretchDIBits, TRANSPARENT, UpdateWindow,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_CONTROL, VK_DELETE, VK_ESCAPE, VK_MENU, VK_RETURN,
    VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA,
    GetClientRect, GetCursorPos, GetMessageW, GetWindowLongPtrW, HTTRANSPARENT, HWND_TOPMOST,
    IDC_ARROW, IDC_CROSS, IDC_HAND, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE,
    IDC_SIZEWE, KillTimer, LWA_ALPHA, LWA_COLORKEY, LoadCursorW, MSG, PostQuitMessage,
    RegisterClassW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetCursor,
    SetForegroundWindow, SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TranslateMessage, WM_CHAR, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_NCHITTEST, WM_PAINT, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_SETCURSOR, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::capture;
use crate::config::{ANNOTATION_PALETTE_SIZE, EditorShortcutConfig, RadialMenuAnimationSpeed};
use crate::platform_windows::{RectPx, virtual_screen_rect};
use crate::region_overlay::PrecomputedSelectionSnapshot;

const HANDLE_SIZE: i32 = 8;
const MIN_SELECTION: i32 = 2;
const MIN_RECT: i32 = 3;
const DEFAULT_COLOR_INDEX: usize = 0;
const DEFAULT_THICKNESS_INDEX: usize = 1;
const THICKNESS_STEPS: [i32; 5] = [2, 4, 6, 8, 12];
const PIXELATE_BLOCK_STEPS: [i32; 5] = [4, 8, 12, 16, 24];
const BLUR_RADIUS_STEPS: [i32; 5] = [1, 2, 3, 4, 6];
const MARKER_ALPHA: u8 = 112;
const MARKER_THICKNESS_SCALE: i32 = 3;
const MARKER_MIN_THICKNESS: i32 = 8;
const SNAP_ANGLE_INCREMENT_DEGREES: f32 = 5.0;
const TEXT_DEFAULT_W: i32 = 48;
const TEXT_DEFAULT_H: i32 = 24;
const TEXT_PAD: i32 = 4;
const TEXT_SCALE: i32 = 2;
const TEXT_GLYPH_W: i32 = 5 * TEXT_SCALE;
const TEXT_GLYPH_H: i32 = 7 * TEXT_SCALE;
const TEXT_GLYPH_ADVANCE: i32 = TEXT_GLYPH_W + TEXT_SCALE + 1;
const TEXT_SPACE_ADVANCE: i32 = 3 * TEXT_SCALE;
const TEXT_LINE_GAP: i32 = TEXT_SCALE + 2;

const LCARS_PEACH: COLORREF = rgb(246, 156, 116);
const LCARS_GOLD: COLORREF = rgb(243, 192, 101);
const LCARS_CORAL: COLORREF = rgb(229, 123, 112);
const LCARS_ROSE: COLORREF = rgb(214, 116, 155);
const LCARS_PURPLE: COLORREF = rgb(183, 128, 196);
const LCARS_LILAC: COLORREF = rgb(151, 129, 213);
const LCARS_SKY: COLORREF = rgb(119, 167, 223);
const LCARS_PLUM: COLORREF = rgb(87, 66, 129);
const LCARS_PANEL_DARK: COLORREF = rgb(44, 29, 47);
const LCARS_PANEL_MID: COLORREF = rgb(65, 43, 70);
const LCARS_TEXT_LIGHT: COLORREF = rgb(255, 237, 221);
const LCARS_TEXT_DARK: COLORREF = rgb(35, 18, 18);
const LCARS_BORDER: COLORREF = rgb(28, 17, 26);

const OVERLAY_DIM: COLORREF = rgb(0, 0, 0);
const OVERLAY_ALPHA: u8 = 118;
const OVERLAY_KEY: COLORREF = rgb(255, 0, 255);
const SELECTION_FILL: COLORREF = rgb(68, 43, 68);
const SELECTION_COLOR: COLORREF = LCARS_GOLD;
const HANDLE_BORDER_COLOR: [u8; 4] = [72, 45, 69, 230];
const HANDLE_FILL_COLOR: [u8; 4] = [255, 237, 221, 255];
const LINE_AA_EDGE_WIDTH: f32 = 1.0;
const LINE_AA_SUBSAMPLES: [(f32, f32); 4] =
    [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)];

const BAR_BORDER: COLORREF = LCARS_BORDER;
const BAR_TEXT: COLORREF = LCARS_TEXT_LIGHT;
const GROUP_BG_TOOLS: COLORREF = LCARS_PANEL_DARK;
const GROUP_BG_ACTIONS: COLORREF = LCARS_PANEL_MID;
const BTN_BORDER: COLORREF = LCARS_BORDER;

const BAR_MARGIN: i32 = 16;
const BAR_GAP: i32 = 12;
const TOOL_ICON_SIZE: u32 = 16;
const ACTION_ICON_SIZE: u32 = 16;
const LCARS_CAP_W: i32 = 152;
const LCARS_TOP_BAND_H: i32 = 34;
const LCARS_ACTIONS_W: i32 = 276;
const LCARS_READOUT_W: i32 = 320;
const LCARS_ROW_GAP: i32 = 12;
const LCARS_TAG_H: i32 = 18;
const SIZE_BADGE_PAD_X: i32 = 14;
const SIZE_BADGE_PAD_Y: i32 = 8;
const SIZE_BADGE_TEXT_EXTRA_H: i32 = 2;
const SIZE_BADGE_BG: COLORREF = LCARS_GOLD;
const SIZE_BADGE_BORDER: COLORREF = LCARS_BORDER;
const TEXT_COMMIT_FEEDBACK_TIMER_ID: usize = 1;
const RADIAL_COLOR_TIMER_ID: usize = 2;
const RADIAL_ANIM_FRAME_MS: u32 = 16;
const TEXT_COMMIT_FEEDBACK_MS: u32 = 550;
const RADIAL_MENU_RADIUS: i32 = 54;
const RADIAL_SWATCH_RADIUS: i32 = 13;
const RADIAL_MARGIN: i32 = RADIAL_MENU_RADIUS + RADIAL_SWATCH_RADIUS + 4;

const SLINT_TOOLBAR_BASE_W: i32 = 766;
const SLINT_TOOLBAR_BASE_H: i32 = 145;
const SLINT_TOOLBAR_MIN_SCALE: f32 = 0.74;
const UI_ACCENT_TEXT_RGB: [u8; 3] = [254, 163, 5];
const UI_BUTTON_ORANGE_RGB: [u8; 3] = [255, 162, 109];
const UI_BUTTON_BLUE_RGB: [u8; 3] = [173, 170, 243];
const UI_BUTTON_PINK_RGB: [u8; 3] = [214, 166, 216];

slint::slint! {
    import "assets/fonts/Antonio-Bold.ttf";

    export component ToolbarButton inherits Rectangle {
        in property <string> label;
        in property <string> icon_path;
        in property <float> icon_viewbox: 256;
        in property <color> fill;
        in property <bool> large: false;
        in property <float> scale: 1.0;

        background: fill;
        border-radius: large ? self.height * 0.25 : self.height / 2;
        clip: true;

        Path {
            x: root.large ? 8px * root.scale : 7px * root.scale;
            y: root.large ? 7px * root.scale : (parent.height - self.height) / 2;
            width: root.large ? 32.8px * root.scale : 14px * root.scale;
            height: root.large ? 32.8px * root.scale : 14px * root.scale;
            viewbox-width: root.icon_viewbox;
            viewbox-height: root.icon_viewbox;
            commands: root.icon_path;
            fill: #040404;
        }

        Text {
            x: 0px;
            y: root.large ? 38px * root.scale : 8px * root.scale;
            width: (parent.width - 10px) * root.scale;
            height: 15px;
            text: root.label;
            color: #040404;
            font-size: 7.5pt * root.scale;
            font-family: "Antonio";
            font-weight: 700;
            horizontal-alignment: right;
            vertical-alignment: bottom;
        }
    }

    export component ChromeToolbarUi inherits Window {
        in property <float> ui_scale: 1.0;
        in property <string> footer_text;

        in property <string> select_icon_path: "M 248 121.58 a 15.76 15.76 0 0 1 -11.29 15 l -0.2 0.06 l -78 21.84 l -21.84 78 l -0.06 0.2 a 15.77 15.77 0 0 1 -15 11.29 h -0.3 a 15.77 15.77 0 0 1 -15.07 -10.67 L 41 61.41 a 1 1 0 0 1 -0.05 -0.16 A 16 16 0 0 1 61.25 40.9 l 0.16 0.05 l 175.92 65.26 A 15.78 15.78 0 0 1 248 121.58 Z";
        in property <string> rect_icon_path: "M 216 36 H 40 A 20 20 0 0 0 20 56 V 200 a 20 20 0 0 0 20 20 H 216 a 20 20 0 0 0 20 -20 V 56 A 20 20 0 0 0 216 36 Z m -4 160 H 44 V 60 H 212 Z";
        in property <string> ellipse_icon_path: "M 128 20 A 108 108 0 1 0 236 128 A 108.12 108.12 0 0 0 128 20 Z m 0 192 a 84 84 0 1 1 84 -84 A 84.09 84.09 0 0 1 128 212 Z";
        in property <string> line_icon_path: "M 217.47 38.53 a 36 36 0 0 0 -57.95 41 l -80 80 a 36.07 36.07 0 0 0 -41 7 h 0 a 36 36 0 1 0 58 9.95 l 80 -80 a 36 36 0 0 0 41 -57.95 Z m -145 162 a 12 12 0 1 1 0 -17 A 12 12 0 0 1 72.48 200.5 Z m 128 -128 a 12 12 0 0 1 -17 0 h 0 a 12 12 0 1 1 17 0 Z";
        in property <string> arrow_icon_path: "M 204 64 V 168 a 12 12 0 0 1 -24 0 V 93 L 72.49 200.49 a 12 12 0 0 1 -17 -17 L 163 76 H 88 a 12 12 0 0 1 0 -24 H 192 A 12 12 0 0 1 204 64 Z";
        in property <string> marker_icon_path: "M 252.49 107.51 a 12 12 0 0 0 -17 0 L 192 151 L 113 72 l 43.52 -43.51 a 12 12 0 0 0 -17 -17 L 93.17 57.86 a 20 20 0 0 0 -4.72 20.72 L 69.17 97.86 a 20 20 0 0 0 0 28.28 L 71 128 L 15.51 183.51 a 12 12 0 0 0 4.7 19.87 l 72 24 A 11.8 11.8 0 0 0 96 228 a 12 12 0 0 0 8.49 -3.52 L 136 193 l 1.86 1.86 a 20 20 0 0 0 28.28 0 l 19.27 -19.27 a 20.27 20.27 0 0 0 6.59 1.13 a 19.86 19.86 0 0 0 14.14 -5.86 l 46.35 -46.34 A 12 12 0 0 0 252.49 107.51 Z M 92.76 202.27 L 46.21 186.76 L 88 145 l 31 31 Z M 152 175 L 96.49 119.52 h 0 L 89 112 l 15 -15 l 63 63 Z";
        in property <string> text_icon_path: "M 90.86 50.89 a 12 12 0 0 0 -21.72 0 l -64 136 a 12 12 0 0 0 21.71 10.22 L 42.44 164 h 75.12 l 15.58 33.11 a 12 12 0 0 0 21.72 -10.22 Z M 53.74 140 L 80 84.18 L 106.27 140 Z M 200 84 c -13.85 0 -24.77 3.86 -32.45 11.48 a 12 12 0 1 0 16.9 17 c 3 -3 8.26 -4.52 15.55 -4.52 c 11 0 20 7.18 20 16 v 4.39 A 47.28 47.28 0 0 0 200 124 c -24.26 0 -44 17.94 -44 40 s 19.74 40 44 40 a 47.18 47.18 0 0 0 22 -5.38 A 12 12 0 0 0 244 192 V 124 C 244 101.94 224.26 84 200 84 Z m 0 96 c -11 0 -20 -7.18 -20 -16 s 9 -16 20 -16 s 20 7.18 20 16 S 211 180 200 180 Z";
        in property <string> pixelate_icon_path: "M 48 56 V 200 a 12 12 0 0 1 -24 0 V 56 a 12 12 0 0 1 24 0 Z m 86.73 50.7 L 120 111.48 V 96 a 12 12 0 0 0 -24 0 v 15.48 L 81.27 106.7 a 12 12 0 1 0 -7.41 22.82 l 14.72 4.79 l -9.1 12.52 A 12 12 0 1 0 98.9 160.94 l 9.1 -12.52 l 9.1 12.52 a 12 12 0 1 0 19.42 -14.11 l -9.1 -12.52 l 14.72 -4.79 a 12 12 0 1 0 -7.41 -22.82 Z m 115.12 7.7 a 12 12 0 0 0 -15.12 -7.7 L 220 111.48 V 96 a 12 12 0 0 0 -24 0 v 15.48 l -14.73 -4.78 a 12 12 0 1 0 -7.41 22.82 l 14.72 4.79 l -9.1 12.52 a 12 12 0 1 0 19.42 14.11 l 9.1 -12.52 l 9.1 12.52 a 12 12 0 1 0 19.42 -14.11 l -9.1 -12.52 l 14.72 -4.79 A 12 12 0 0 0 249.85 114.4 Z";
        in property <string> blur_icon_path: "M 106 -386 q -6 -6 -6 -14 t 6 -14 q 6 -6 14 -6 t 14 6 q 6 6 6 14 t -6 14 q -6 6 -14 6 t -14 -6 Z m 0 -160 q -6 -6 -6 -14 t 6 -14 q 6 -6 14 -6 t 14 6 q 6 6 6 14 t -6 14 q -6 6 -14 6 t -14 -6 Z m 105.5 334.5 Q 200 -223 200 -240 t 11.5 -28.5 Q 223 -280 240 -280 t 28.5 11.5 Q 280 -257 280 -240 t -11.5 28.5 Q 257 -200 240 -200 t -28.5 -11.5 Z m 0 -160 Q 200 -383 200 -400 t 11.5 -28.5 Q 223 -440 240 -440 t 28.5 11.5 Q 280 -417 280 -400 t -11.5 28.5 Q 257 -360 240 -360 t -28.5 -11.5 Z m 0 -160 Q 200 -543 200 -560 t 11.5 -28.5 Q 223 -600 240 -600 t 28.5 11.5 Q 280 -577 280 -560 t -11.5 28.5 Q 257 -520 240 -520 t -28.5 -11.5 Z m 0 -160 Q 200 -703 200 -720 t 11.5 -28.5 Q 223 -760 240 -760 t 28.5 11.5 Q 280 -737 280 -720 t -11.5 28.5 Q 257 -680 240 -680 t -28.5 -11.5 Z m 146 334 Q 340 -375 340 -400 t 17.5 -42.5 Q 375 -460 400 -460 t 42.5 17.5 Q 460 -425 460 -400 t -17.5 42.5 Q 425 -340 400 -340 t -42.5 -17.5 Z m 0 -160 Q 340 -535 340 -560 t 17.5 -42.5 Q 375 -620 400 -620 t 42.5 17.5 Q 460 -585 460 -560 t -17.5 42.5 Q 425 -500 400 -500 t -42.5 -17.5 Z m 14 306 Q 360 -223 360 -240 t 11.5 -28.5 Q 383 -280 400 -280 t 28.5 11.5 Q 440 -257 440 -240 t -11.5 28.5 Q 417 -200 400 -200 t -28.5 -11.5 Z m 0 -480 Q 360 -703 360 -720 t 11.5 -28.5 Q 383 -760 400 -760 t 28.5 11.5 Q 440 -737 440 -720 t -11.5 28.5 Q 417 -680 400 -680 t -28.5 -11.5 Z M 386 -106 q -6 -6 -6 -14 t 6 -14 q 6 -6 14 -6 t 14 6 q 6 6 6 14 t -6 14 q -6 6 -14 6 t -14 -6 Z m 0 -720 q -6 -6 -6 -14 t 6 -14 q 6 -6 14 -6 t 14 6 q 6 6 6 14 t -6 14 q -6 6 -14 6 t -14 -6 Z m 131.5 468.5 Q 500 -375 500 -400 t 17.5 -42.5 Q 535 -460 560 -460 t 42.5 17.5 Q 620 -425 620 -400 t -17.5 42.5 Q 585 -340 560 -340 t -42.5 -17.5 Z m 0 -160 Q 500 -535 500 -560 t 17.5 -42.5 Q 535 -620 560 -620 t 42.5 17.5 Q 620 -585 620 -560 t -17.5 42.5 Q 585 -500 560 -500 t -42.5 -17.5 Z m 14 306 Q 520 -223 520 -240 t 11.5 -28.5 Q 543 -280 560 -280 t 28.5 11.5 Q 600 -257 600 -240 t -11.5 28.5 Q 577 -200 560 -200 t -28.5 -11.5 Z m 0 -480 Q 520 -703 520 -720 t 11.5 -28.5 Q 543 -760 560 -760 t 28.5 11.5 Q 600 -737 600 -720 t -11.5 28.5 Q 577 -680 560 -680 t -28.5 -11.5 Z M 546 -106 q -6 -6 -6 -14 t 6 -14 q 6 -6 14 -6 t 14 6 q 6 6 6 14 t -6 14 q -6 6 -14 6 t -14 -6 Z m 0 -720 q -6 -6 -6 -14 t 6 -14 q 6 -6 14 -6 t 14 6 q 6 6 6 14 t -6 14 q -6 6 -14 6 t -14 -6 Z m 145.5 614.5 Q 680 -223 680 -240 t 11.5 -28.5 Q 703 -280 720 -280 t 28.5 11.5 Q 760 -257 760 -240 t -11.5 28.5 Q 737 -200 720 -200 t -28.5 -11.5 Z m 0 -160 Q 680 -383 680 -400 t 11.5 -28.5 Q 703 -440 720 -440 t 28.5 11.5 Q 760 -417 760 -400 t -11.5 28.5 Q 737 -360 720 -360 t -28.5 -11.5 Z m 0 -160 Q 680 -543 680 -560 t 11.5 -28.5 Q 703 -600 720 -600 t 28.5 11.5 Q 760 -577 760 -560 t -11.5 28.5 Q 737 -520 720 -520 t -28.5 -11.5 Z m 0 -160 Q 680 -703 680 -720 t 11.5 -28.5 Q 703 -760 720 -760 t 28.5 11.5 Q 760 -737 760 -720 t -11.5 28.5 Q 737 -680 720 -680 t -28.5 -11.5 Z M 826 -386 q -6 -6 -6 -14 t 6 -14 q 6 -6 14 -6 t 14 6 q 6 6 6 14 t -6 14 q -6 6 -14 6 t -14 -6 Z m 0 -160 q -6 -6 -6 -14 t 6 -14 q 6 -6 14 -6 t 14 6 q 6 6 6 14 t -6 14 q -6 6 -14 6 t -14 -6 Z";
        in property <string> copy_icon_path: "M 216 28 H 88 A 12 12 0 0 0 76 40 V 76 H 40 A 12 12 0 0 0 28 88 V 216 a 12 12 0 0 0 12 12 H 168 a 12 12 0 0 0 12 -12 V 180 h 36 a 12 12 0 0 0 12 -12 V 40 A 12 12 0 0 0 216 28 Z M 156 204 H 52 V 100 H 156 Z m 48 -48 H 180 V 88 a 12 12 0 0 0 -12 -12 H 100 V 52 H 204 Z";
        in property <string> save_icon_path: "M 222.14 69.17 L 186.83 33.86 A 19.86 19.86 0 0 0 172.69 28 H 48 A 20 20 0 0 0 28 48 V 208 a 20 20 0 0 0 20 20 H 208 a 20 20 0 0 0 20 -20 V 83.31 A 19.86 19.86 0 0 0 222.14 69.17 Z M 164 204 H 92 V 160 h 72 Z m 40 0 H 188 V 156 a 20 20 0 0 0 -20 -20 H 88 a 20 20 0 0 0 -20 20 v 48 H 52 V 52 H 171 l 33 33 Z M 164 84 a 12 12 0 0 1 -12 12 H 96 a 12 12 0 0 1 0 -24 h 56 A 12 12 0 0 1 164 84 Z";
        in property <string> copy_save_icon_path: "M 180.25 180 L 180.25 212.775 C 180.25 221.127 173.377 228 165.025 228 L 43.225 228 C 34.873 228 28 221.127 28 212.775 L 28 90.975 C 28 82.623 34.873 75.75 43.225 75.75 L 76 75.75 L 76 40 C 76 33.417 81.417 28 88 28 L 216 28 C 222.583 28 228 33.417 228 40 L 228 168 C 228 174.583 222.583 180 216 180 L 180.25 180 Z M 136.858 94.02 L 46.27 94.02 L 46.27 209.73 L 58.45 209.73 L 58.45 173.19 C 58.45 164.838 65.322 157.965 73.675 157.965 L 134.575 157.965 C 142.927 157.965 149.8 164.838 149.8 173.19 L 149.8 209.73 L 161.98 209.73 L 161.98 119.141 L 136.858 94.02 Z M 180.25 156 L 204 156 L 204 52 L 100 52 L 100 75.75 L 138.145 75.75 C 142.183 75.74 146.062 77.347 148.909 80.211 L 175.789 107.091 C 178.653 109.938 180.26 113.817 180.25 117.855 L 180.25 156 Z M 126.519 126.519 C 125.278 127.155 123.875 127.515 122.395 127.515 L 79.765 127.515 C 74.754 127.515 70.63 123.392 70.63 118.38 C 70.63 113.369 74.754 109.245 79.765 109.245 L 122.395 109.245 C 127.406 109.245 131.53 113.369 131.53 118.38 C 131.53 121.911 129.482 125.002 126.519 126.519 Z M 131.53 209.73 L 131.53 176.235 L 76.72 176.235 L 76.72 209.73 L 131.53 209.73 Z";
        in property <string> pin_icon_path: "M 238.15 78.54 L 177.46 17.86 a 20 20 0 0 0 -28.3 0 L 97.2 70 c -12.43 -3.33 -36.68 -5.72 -61.74 14.5 a 20 20 0 0 0 -1.6 29.73 l 45.46 45.47 l -39.8 39.8 a 12 12 0 0 0 17 17 l 39.8 -39.81 l 45.47 45.46 A 20 20 0 0 0 155.91 228 c 0.46 0 0.93 0 1.4 -0.05 A 20 20 0 0 0 171.87 220 c 4.69 -6.23 11 -16.13 14.44 -28 s 3.45 -22.88 0.16 -33.4 l 51.7 -51.87 A 20 20 0 0 0 238.15 78.54 Z m -74.26 68.79 a 12 12 0 0 0 -2.23 13.84 c 3.43 6.86 6.9 21 -6.28 40.65 L 54.08 100.53 c 21.09 -14.59 39.53 -6.64 41 -6 a 11.67 11.67 0 0 0 13.81 -2.29 l 54.43 -54.61 l 55 55 Z";

        in property <color> select_fill;
        in property <color> rect_fill;
        in property <color> ellipse_fill;
        in property <color> line_fill;
        in property <color> arrow_fill;
        in property <color> marker_fill;
        in property <color> text_fill;
        in property <color> pixelate_fill;
        in property <color> blur_fill;
        in property <color> copy_fill;
        in property <color> save_fill;
        in property <color> copy_save_fill;
        in property <color> pin_fill;

        width: 766px * ui_scale;
        height: 145px * ui_scale;
        background: black;

        Path {
            x: 0px;
            y: 0px;
            width: 176px * root.ui_scale;
            height: 35px * root.ui_scale;
            viewbox-width: 177;
            viewbox-height: 36;
            fill: #CC99CC;
            commands: "M 0 35.007083 L 115.978 35.007083 C 115.978 35.007083 115.829 24.954083 125.793 24.954083 C 135.756 24.954083 176.016 25.048083 176.016 25.048083 L 176.016 0 L 24.971 0 C 24.971 0 0 1.391083 0 25.048083 C 0 35.062083 0 35.007083 0 35.007083 Z";
        }

        Rectangle {
            x: 181px * root.ui_scale;
            y: 0px;
            width: 506px * root.ui_scale;
            height: 25px * root.ui_scale;
            background: #9999FF;
        }

        Text {
            x: 692px * root.ui_scale;
            y: 0px;
            width: 50px * root.ui_scale;
            height: 25px * root.ui_scale;
            text: "PYRO";
            color: #fea305;
            font-size: 21.3pt;
            font-family: "Antonio";
            font-weight: 700;
            horizontal-alignment: right;
            vertical-alignment: center;
        }

        Rectangle {
            x: 747px * root.ui_scale;
            y: 0px;
            width: 20px * root.ui_scale;
            height: 145px * root.ui_scale;
            background: #9999FF;
            border-top-right-radius: 10px;
            border-bottom-right-radius: 10px;
        }

        Rectangle {
            x: 0px;
            y: 40px * root.ui_scale;
            width: 116px * root.ui_scale;
            height: 65px * root.ui_scale;
            background: #9999FF;
        }

        Path {
            x: 0px;
            y: 110px * root.ui_scale;
            width: 160px * root.ui_scale;
            height: 35px * root.ui_scale;
            viewbox-width: 160;
            viewbox-height: 35;
            fill: #CC99CC;
            commands: "M 0 0 L 115.978 0 C 115.978 0 115.829 10.053 125.793 10.053 C 135.756 10.053 160 9.959 160 9.959 L 160 35.007 L 24.971 35.007 C 24.971 35.007 0 33.616 0 9.959 C 0 -0.055 0 0 0 0 Z";
        }

        Rectangle {
            x: 165px;
            y: 120px * root.ui_scale;
            width: 425px * root.ui_scale;
            height: 10px * root.ui_scale;
            background: #9999CC;
        }

        Rectangle {
            x: 595px;
            y: 120px * root.ui_scale;
            width: 147px * root.ui_scale;
            height: 10px * root.ui_scale;
            background: #CC6666;
        }

        Rectangle {
            x: 165px * root.ui_scale;
            y: 135px * root.ui_scale;
            width: 207px * root.ui_scale;
            height: 10px * root.ui_scale;
            background: #9999FF;
        }

        Rectangle {
            x: 377px * root.ui_scale;
            y: 135px * root.ui_scale;
            width: 260px * root.ui_scale;
            height: 10px * root.ui_scale;
            background: #9999FF;
        }

        Text {
            x: 644px * root.ui_scale;
            y: 135px * root.ui_scale;
            width: 98px * root.ui_scale;
            height: 10px * root.ui_scale;
            text: root.footer_text;
            color: #fea305;
            font-size: 8.5pt;
            font-family: "Antonio";
            font-weight: 700;
            horizontal-alignment: center;
            vertical-alignment: center;
        }

        Text {
            x: 121px * root.ui_scale;
            y: 30px * root.ui_scale;
            width: 33.5px * root.ui_scale;
            height: 30px * root.ui_scale;
            text: "TOOLS";
            color: #fea305;
            font-family: "Antonio";
            font-weight: 700;
            font-size: 12pt * root.ui_scale;
        }

        ToolbarButton {
            x: 121px * root.ui_scale;
            y: 53px * root.ui_scale;
            width: 64px * root.ui_scale;
            height: 25px * root.ui_scale;
            scale: root.ui_scale;
            label: "SELECT";
            icon_path: root.select_icon_path;
            fill: root.select_fill;
        }

        ToolbarButton {
            x: 121px * root.ui_scale;
            y: 83px * root.ui_scale;
            width: 64px * root.ui_scale;
            height: 25px * root.ui_scale;
            scale: root.ui_scale;
            label: "RECT";
            icon_path: root.rect_icon_path;
            fill: root.rect_fill;
        }

        ToolbarButton {
            x: 190px * root.ui_scale;
            y: 53px * root.ui_scale;
            width: 64px * root.ui_scale;
            height: 25px * root.ui_scale;
            scale: root.ui_scale;
            label: "CIRCLE";
            icon_path: root.ellipse_icon_path;
            fill: root.ellipse_fill;
        }

        ToolbarButton {
            x: 190px * root.ui_scale;
            y: 83px * root.ui_scale;
            width: 64px * root.ui_scale;
            height: 25px * root.ui_scale;
            scale: root.ui_scale;
            label: "LINE";
            icon_path: root.line_icon_path;
            fill: root.line_fill;
        }

        ToolbarButton {
            x: 259px * root.ui_scale;
            y: 53px * root.ui_scale;
            width: 64px * root.ui_scale;
            height: 25px * root.ui_scale;
            scale: root.ui_scale;
            label: "ARROW";
            icon_path: root.arrow_icon_path;
            fill: root.arrow_fill;
        }

        ToolbarButton {
            x: 259px * root.ui_scale;
            y: 83px * root.ui_scale;
            width: 64px * root.ui_scale;
            height: 25px * root.ui_scale;
            scale: root.ui_scale;
            label: "MARKER";
            icon_path: root.marker_icon_path;
            fill: root.marker_fill;
        }

        ToolbarButton {
            x: 328px * root.ui_scale;
            y: 53px * root.ui_scale;
            width: 64px * root.ui_scale;
            height: 25px * root.ui_scale;
            scale: root.ui_scale;
            label: "TEXT";
            icon_path: root.text_icon_path;
            fill: root.text_fill;
        }

        ToolbarButton {
            x: 328px * root.ui_scale;
            y: 83px * root.ui_scale;
            width: 64px * root.ui_scale;
            height: 25px * root.ui_scale;
            scale: root.ui_scale;
            label: "CENSOR";
            icon_path: root.pixelate_icon_path;
            fill: root.pixelate_fill;
        }

        ToolbarButton {
            x: 398px * root.ui_scale;
            y: 54px * root.ui_scale;
            width: 64px * root.ui_scale;
            height: 25px * root.ui_scale;
            scale: root.ui_scale;
            label: "BLUR";
            icon_path: root.blur_icon_path;
            fill: root.blur_fill;
        }

        Text {
            x: 471px * root.ui_scale;
            y: 30px * root.ui_scale;
            width: 60px * root.ui_scale;
            height: 30px * root.ui_scale;
            text: "OUTPUT";
            color: #fea305;
            font-family: "Antonio";
            font-weight: 700;
            font-size: 12pt * root.ui_scale;
        }

        ToolbarButton {
            x: 474px * root.ui_scale;
            y: 54px * root.ui_scale;
            width: 64px * root.ui_scale;
            height: 55px * root.ui_scale;
            scale: root.ui_scale;
            large: true;
            label: "COPY";
            icon_path: root.copy_icon_path;
            fill: root.copy_fill;
        }

        ToolbarButton {
            x: 543px * root.ui_scale;
            y: 54px * root.ui_scale;
            width: 64px * root.ui_scale;
            height: 55px * root.ui_scale;
            scale: root.ui_scale;
            large: true;
            label: "SAVE";
            icon_path: root.save_icon_path;
            fill: root.save_fill;
        }

        ToolbarButton {
            x: 612px * root.ui_scale;
            y: 54px * root.ui_scale;
            width: 64px * root.ui_scale;
            height: 55px * root.ui_scale;
            scale: root.ui_scale;
            large: true;
            label: "COPY/SAVE";
            icon_path: root.copy_save_icon_path;
            fill: root.copy_save_fill;
        }

        ToolbarButton {
            x: 681px * root.ui_scale;
            y: 54px * root.ui_scale;
            width: 54px * root.ui_scale;
            height: 55px * root.ui_scale;
            scale: root.ui_scale;
            large: true;
            label: "PIN";
            icon_path: root.pin_icon_path;
            fill: root.pin_fill;
        }
    }
}

thread_local! {
    static PENDING_SLINT_WINDOW_ADAPTER: RefCell<Option<Rc<dyn WindowAdapter>>> = RefCell::new(None);
}

struct RegionEditorSlintPlatform;

impl Platform for RegionEditorSlintPlatform {
    fn create_window_adapter(&self) -> std::result::Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(PENDING_SLINT_WINDOW_ADAPTER.with(|slot| {
            slot.borrow_mut()
                .take()
                .unwrap_or_else(|| MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer))
        }))
    }
}
pub fn parse_hex_rgb_color(value: &str) -> Result<[u8; 3]> {
    let raw = value.trim();
    let hex = raw.strip_prefix('#').unwrap_or(raw);
    if hex.len() != 6 {
        bail!("expected hex color in #RRGGBB format");
    }
    let red =
        u8::from_str_radix(&hex[0..2], 16).map_err(|_| anyhow::anyhow!("invalid red channel"))?;
    let green =
        u8::from_str_radix(&hex[2..4], 16).map_err(|_| anyhow::anyhow!("invalid green channel"))?;
    let blue =
        u8::from_str_radix(&hex[4..6], 16).map_err(|_| anyhow::anyhow!("invalid blue channel"))?;
    Ok([red, green, blue])
}

fn parse_editor_shortcut(value: &str) -> Result<KeyChord> {
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut key = None::<u32>;

    for raw_token in value.split('+') {
        let token = raw_token.trim();
        if token.is_empty() {
            bail!("shortcut token cannot be empty");
        }
        let token_upper = token.to_ascii_uppercase();
        match token_upper.as_str() {
            "CTRL" | "CONTROL" => ctrl = true,
            "SHIFT" => shift = true,
            "ALT" => alt = true,
            _ => {
                if key.is_some() {
                    bail!("shortcut must include exactly one non-modifier key");
                }
                key = Some(parse_editor_shortcut_key(&token_upper)?);
            }
        }
    }

    let Some(key) = key else {
        bail!("shortcut must include a key");
    };
    Ok(KeyChord {
        key,
        ctrl,
        shift,
        alt,
    })
}

fn parse_editor_shortcut_key(token: &str) -> Result<u32> {
    if token.len() == 1 {
        let ch = token.chars().next().expect("len checked");
        if ch.is_ascii_alphabetic() {
            return Ok(ch.to_ascii_uppercase() as u32);
        }
        if ch.is_ascii_digit() {
            return Ok(ch as u32);
        }
        return match ch {
            '[' => Ok(0xDB),
            ']' => Ok(0xDD),
            ';' => Ok(0xBA),
            '\'' => Ok(0xDE),
            ',' => Ok(0xBC),
            '.' => Ok(0xBE),
            '/' => Ok(0xBF),
            '-' => Ok(0xBD),
            '=' => Ok(0xBB),
            '`' => Ok(0xC0),
            '\\' => Ok(0xDC),
            _ => bail!("unsupported key `{token}`"),
        };
    }

    if let Some(number) = token.strip_prefix('F')
        && let Ok(value) = number.parse::<u32>()
        && (1..=24).contains(&value)
    {
        return Ok(111 + value);
    }

    match token {
        "DELETE" | "DEL" => Ok(VK_DELETE.0 as u32),
        "ENTER" | "RETURN" => Ok(VK_RETURN.0 as u32),
        "ESC" | "ESCAPE" => Ok(VK_ESCAPE.0 as u32),
        "SPACE" => Ok(0x20),
        "TAB" => Ok(0x09),
        "BACKSPACE" | "BKSP" => Ok(0x08),
        "LEFTBRACKET" | "LBRACKET" => Ok(0xDB),
        "RIGHTBRACKET" | "RBRACKET" => Ok(0xDD),
        _ => bail!("unsupported key `{token}`"),
    }
}

fn format_key_chord(chord: KeyChord) -> String {
    let mut parts = Vec::new();
    if chord.ctrl {
        parts.push("Ctrl".to_string());
    }
    if chord.shift {
        parts.push("Shift".to_string());
    }
    if chord.alt {
        parts.push("Alt".to_string());
    }
    parts.push(key_code_label(chord.key));
    parts.join("+")
}

fn key_code_label(key: u32) -> String {
    if (u32::from(b'A')..=u32::from(b'Z')).contains(&key)
        || (u32::from(b'0')..=u32::from(b'9')).contains(&key)
    {
        return (char::from_u32(key).unwrap_or('?')).to_string();
    }

    if (112..=135).contains(&key) {
        return format!("F{}", key - 111);
    }

    match key {
        0xDB => "[".to_string(),
        0xDD => "]".to_string(),
        0xBA => ";".to_string(),
        0xDE => "'".to_string(),
        0xBC => ",".to_string(),
        0xBE => ".".to_string(),
        0xBF => "/".to_string(),
        0xBD => "-".to_string(),
        0xBB => "=".to_string(),
        0xC0 => "`".to_string(),
        0xDC => "\\".to_string(),
        x if x == VK_DELETE.0 as u32 => "Delete".to_string(),
        x if x == VK_RETURN.0 as u32 => "Enter".to_string(),
        x if x == VK_ESCAPE.0 as u32 => "Esc".to_string(),
        0x20 => "Space".to_string(),
        0x09 => "Tab".to_string(),
        0x08 => "Backspace".to_string(),
        _ => format!("VK_{key:#X}"),
    }
}

#[derive(Debug)]
pub struct RegionEditResult {
    bounds: RectPx,
    annotations: Vec<Annotation>,
    output_action: Option<EditorOutputAction>,
}

impl RegionEditResult {
    pub fn bounds(&self) -> RectPx {
        self.bounds
    }

    pub fn output_action(&self) -> Option<EditorOutputAction> {
        self.output_action
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum EditorOutputAction {
    Copy,
    Save,
    CopyAndSave,
    Pin,
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
    Text,
    Pixelate,
    Blur,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
struct KeyChord {
    key: u32,
    ctrl: bool,
    shift: bool,
    alt: bool,
}

impl KeyChord {
    fn matches(self, key: u32, ctrl: bool, shift: bool, alt: bool) -> bool {
        self.key == key && self.ctrl == ctrl && self.shift == shift && self.alt == alt
    }
}

#[derive(Debug, Clone)]
struct BoundShortcut {
    chord: KeyChord,
    label: String,
}

impl BoundShortcut {
    fn parse(value: &str, name: &str) -> Result<Self> {
        let chord = parse_editor_shortcut(value)
            .map_err(|err| anyhow::anyhow!("invalid `{name}` shortcut `{value}`: {err}"))?;
        Ok(Self {
            chord,
            label: format_key_chord(chord),
        })
    }

    fn matches(&self, key: u32, ctrl: bool, shift: bool, alt: bool) -> bool {
        self.chord.matches(key, ctrl, shift, alt)
    }
}

#[derive(Debug, Clone)]
pub struct EditorKeybindings {
    select: BoundShortcut,
    rectangle: BoundShortcut,
    ellipse: BoundShortcut,
    line: BoundShortcut,
    arrow: BoundShortcut,
    marker: BoundShortcut,
    text: BoundShortcut,
    pixelate: BoundShortcut,
    blur: BoundShortcut,
    copy: BoundShortcut,
    save: BoundShortcut,
    copy_and_save: BoundShortcut,
    undo: BoundShortcut,
    redo: BoundShortcut,
    delete_selected: BoundShortcut,
}

impl EditorKeybindings {
    pub fn from_config(config: &EditorShortcutConfig) -> Result<Self> {
        let keybindings = Self {
            select: BoundShortcut::parse(&config.select, "editor.shortcuts.select")?,
            rectangle: BoundShortcut::parse(&config.rectangle, "editor.shortcuts.rectangle")?,
            ellipse: BoundShortcut::parse(&config.ellipse, "editor.shortcuts.ellipse")?,
            line: BoundShortcut::parse(&config.line, "editor.shortcuts.line")?,
            arrow: BoundShortcut::parse(&config.arrow, "editor.shortcuts.arrow")?,
            marker: BoundShortcut::parse(&config.marker, "editor.shortcuts.marker")?,
            text: BoundShortcut::parse(&config.text, "editor.shortcuts.text")?,
            pixelate: BoundShortcut::parse(&config.pixelate, "editor.shortcuts.pixelate")?,
            blur: BoundShortcut::parse(&config.blur, "editor.shortcuts.blur")?,
            copy: BoundShortcut::parse(&config.copy, "editor.shortcuts.copy")?,
            save: BoundShortcut::parse(&config.save, "editor.shortcuts.save")?,
            copy_and_save: BoundShortcut::parse(
                &config.copy_and_save,
                "editor.shortcuts.copy_and_save",
            )?,
            undo: BoundShortcut::parse(&config.undo, "editor.shortcuts.undo")?,
            redo: BoundShortcut::parse(&config.redo, "editor.shortcuts.redo")?,
            delete_selected: BoundShortcut::parse(
                &config.delete_selected,
                "editor.shortcuts.delete_selected",
            )?,
        };
        keybindings.ensure_unique()?;
        Ok(keybindings)
    }

    fn ensure_unique(&self) -> Result<()> {
        let bindings = [
            ("select", &self.select),
            ("rectangle", &self.rectangle),
            ("ellipse", &self.ellipse),
            ("line", &self.line),
            ("arrow", &self.arrow),
            ("marker", &self.marker),
            ("text", &self.text),
            ("pixelate", &self.pixelate),
            ("blur", &self.blur),
            ("copy", &self.copy),
            ("save", &self.save),
            ("copy_and_save", &self.copy_and_save),
            ("undo", &self.undo),
            ("redo", &self.redo),
            ("delete_selected", &self.delete_selected),
        ];
        let mut seen = HashSet::<KeyChord>::new();
        for (name, binding) in bindings {
            if !seen.insert(binding.chord) {
                bail!(
                    "shortcut `{}` for `{name}` conflicts with another editor shortcut",
                    binding.label
                );
            }
        }
        Ok(())
    }

    fn tool_for_key(&self, key: u32, ctrl: bool, shift: bool, alt: bool) -> Option<Tool> {
        if self.select.matches(key, ctrl, shift, alt) {
            return Some(Tool::Select);
        }
        if self.rectangle.matches(key, ctrl, shift, alt) {
            return Some(Tool::Rectangle);
        }
        if self.ellipse.matches(key, ctrl, shift, alt) {
            return Some(Tool::Ellipse);
        }
        if self.line.matches(key, ctrl, shift, alt) {
            return Some(Tool::Line);
        }
        if self.arrow.matches(key, ctrl, shift, alt) {
            return Some(Tool::Arrow);
        }
        if self.marker.matches(key, ctrl, shift, alt) {
            return Some(Tool::Marker);
        }
        if self.text.matches(key, ctrl, shift, alt) {
            return Some(Tool::Text);
        }
        if self.pixelate.matches(key, ctrl, shift, alt) {
            return Some(Tool::Pixelate);
        }
        if self.blur.matches(key, ctrl, shift, alt) {
            return Some(Tool::Blur);
        }
        None
    }

    fn output_action_for_key(
        &self,
        key: u32,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> Option<EditorOutputAction> {
        if self.copy.matches(key, ctrl, shift, alt) {
            return Some(EditorOutputAction::Copy);
        }
        if self.save.matches(key, ctrl, shift, alt) {
            return Some(EditorOutputAction::Save);
        }
        if self.copy_and_save.matches(key, ctrl, shift, alt) {
            return Some(EditorOutputAction::CopyAndSave);
        }
        None
    }
}

impl Default for EditorKeybindings {
    fn default() -> Self {
        Self::from_config(&EditorShortcutConfig::default()).expect("valid default editor shortcuts")
    }
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

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum LineEndpoint {
    Start,
    End,
}

#[derive(Debug, Clone, Copy)]
enum AnnotationHandleHit {
    Resize {
        index: usize,
        bounds: RectPx,
        handle: ResizeHandle,
    },
    LineEndpoint {
        index: usize,
        endpoint: LineEndpoint,
    },
    MarkerEndpoint {
        index: usize,
        endpoint: LineEndpoint,
    },
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
    ResizeAnnotation {
        index: usize,
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
    DrawText {
        start: POINT,
        current: POINT,
    },
    DrawPixelate {
        start: POINT,
        current: POINT,
    },
    DrawBlur {
        start: POINT,
        current: POINT,
    },
    MoveLineEndpoint {
        index: usize,
        endpoint: LineEndpoint,
    },
    MoveMarkerEndpoint {
        index: usize,
        endpoint: LineEndpoint,
    },
    MoveAnnotation {
        index: usize,
        last_point: POINT,
    },
}

#[derive(Debug, Clone)]
enum Annotation {
    Rectangle(RectAnn),
    Ellipse(EllipseAnn),
    Line(LineAnn),
    Marker(MarkerAnn),
    Text(TextAnn),
    Pixelate(PixelateAnn),
    Blur(BlurAnn),
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
struct TextAnn {
    rect_abs: RectPx,
    text: String,
    color: [u8; 4],
}

#[derive(Debug, Clone)]
struct PixelateAnn {
    rect_abs: RectPx,
    block: i32,
}

#[derive(Debug, Clone)]
struct BlurAnn {
    rect_abs: RectPx,
    radius: i32,
}

#[derive(Debug, Clone)]
struct SelectionSnapshot {
    width: i32,
    height: i32,
    bgra_pixels: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
struct FrozenDesktopRef {
    bounds: RectPx,
    width: i32,
    height: i32,
    rgba_ptr: *const u8,
    rgba_len: usize,
}

#[derive(Debug, Clone)]
struct IconMask {
    width: i32,
    height: i32,
    alpha: Vec<u8>,
}

#[derive(Debug, Default, Clone)]
struct ToolbarIcons {
    select: Option<IconMask>,
    rectangle: Option<IconMask>,
    ellipse: Option<IconMask>,
    line: Option<IconMask>,
    arrow: Option<IconMask>,
    marker: Option<IconMask>,
    text: Option<IconMask>,
    pixelate: Option<IconMask>,
    blur: Option<IconMask>,
    copy: Option<IconMask>,
    save: Option<IconMask>,
    copy_save: Option<IconMask>,
    pin: Option<IconMask>,
}


struct ToolbarSlintRenderer {
    window: Rc<MinimalSoftwareWindow>,
    ui: ChromeToolbarUi,
    pixels: Vec<PremultipliedRgbaColor>,
    bgra: Vec<u8>,
    width: u32,
    height: u32,
}

#[derive(Debug)]
struct AaScratch {
    image: RgbaImage,
    used_width: i32,
    used_height: i32,
    bgra: Vec<u8>,
    surface: Option<AaSurface>,
}

#[derive(Debug)]
struct AaSurface {
    dc: windows::Win32::Graphics::Gdi::HDC,
    bitmap: windows::Win32::Graphics::Gdi::HBITMAP,
    old_bitmap: HGDIOBJ,
    bits: *mut c_void,
    width: i32,
    height: i32,
}

impl Default for AaScratch {
    fn default() -> Self {
        Self {
            image: RgbaImage::new(1, 1),
            used_width: 0,
            used_height: 0,
            bgra: Vec::new(),
            surface: None,
        }
    }
}

impl Drop for AaScratch {
    fn drop(&mut self) {
        self.release_surface();
    }
}

impl AaScratch {
    fn prepare(&mut self, width: i32, height: i32) -> bool {
        if width <= 0 || height <= 0 {
            self.used_width = 0;
            self.used_height = 0;
            return false;
        }

        let width_u32 = width as u32;
        let height_u32 = height as u32;
        if self.image.width() < width_u32 || self.image.height() < height_u32 {
            self.image = RgbaImage::from_pixel(width_u32, height_u32, Rgba([0, 0, 0, 0]));
        } else {
            let stride = self.image.width() as usize * 4;
            let row_bytes = width_u32 as usize * 4;
            let raw = self.image.as_mut();
            for row in 0..height_u32 as usize {
                let offset = row * stride;
                raw[offset..offset + row_bytes].fill(0);
            }
        }

        self.used_width = width;
        self.used_height = height;
        true
    }

    fn blit(&mut self, target_hdc: windows::Win32::Graphics::Gdi::HDC, left: i32, top: i32) {
        let width = self.used_width;
        let height = self.used_height;
        if width <= 0 || height <= 0 {
            return;
        }

        let used_len = width as usize * height as usize * 4;
        if self.bgra.len() < used_len {
            self.bgra.resize(used_len, 0);
        }

        let src_stride = self.image.width() as usize * 4;
        let dst_stride = width as usize * 4;
        let src = self.image.as_raw();
        for row in 0..height as usize {
            let src_row = row * src_stride;
            let dst_row = row * dst_stride;
            for col in 0..width as usize {
                let src_idx = src_row + (col * 4);
                let dst_idx = dst_row + (col * 4);
                self.bgra[dst_idx] = src[src_idx + 2];
                self.bgra[dst_idx + 1] = src[src_idx + 1];
                self.bgra[dst_idx + 2] = src[src_idx];
                self.bgra[dst_idx + 3] = src[src_idx + 3];
            }
        }

        if !self.ensure_surface(target_hdc, width, height) {
            return;
        }

        let surface = match self.surface.as_mut() {
            Some(value) => value,
            None => return,
        };
        let surface_stride = surface.width as usize * 4;
        if surface.bits.is_null() {
            return;
        }

        unsafe {
            let dst = std::slice::from_raw_parts_mut(
                surface.bits.cast::<u8>(),
                (surface_stride * surface.height as usize).max(0),
            );
            for row in 0..height as usize {
                let src_row = row * dst_stride;
                let dst_row = row * surface_stride;
                let src_slice = &self.bgra[src_row..src_row + dst_stride];
                let dst_slice = &mut dst[dst_row..dst_row + dst_stride];
                dst_slice.copy_from_slice(src_slice);
            }

            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = AlphaBlend(
                target_hdc, left, top, width, height, surface.dc, 0, 0, width, height, blend,
            );
        }
    }

    fn ensure_surface(
        &mut self,
        target_hdc: windows::Win32::Graphics::Gdi::HDC,
        width: i32,
        height: i32,
    ) -> bool {
        let needs_new = match self.surface.as_ref() {
            Some(existing) => existing.width < width || existing.height < height,
            None => true,
        };
        if !needs_new {
            return true;
        }

        self.release_surface();

        let source_dc = unsafe { CreateCompatibleDC(target_hdc) };
        if source_dc.0.is_null() {
            return false;
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

        let mut bits = std::ptr::null_mut::<c_void>();
        let Ok(dib) =
            (unsafe { CreateDIBSection(source_dc, &bitmap, DIB_RGB_COLORS, &mut bits, None, 0) })
        else {
            unsafe {
                let _ = DeleteDC(source_dc);
            }
            return false;
        };
        if dib.0.is_null() || bits.is_null() {
            unsafe {
                let _ = DeleteObject(dib);
                let _ = DeleteDC(source_dc);
            }
            return false;
        }

        let old_bitmap = unsafe { SelectObject(source_dc, dib) };
        self.surface = Some(AaSurface {
            dc: source_dc,
            bitmap: dib,
            old_bitmap,
            bits,
            width,
            height,
        });
        true
    }

    fn release_surface(&mut self) {
        if let Some(surface) = self.surface.take() {
            unsafe {
                let _ = SelectObject(surface.dc, surface.old_bitmap);
                let _ = DeleteObject(surface.bitmap);
                let _ = DeleteDC(surface.dc);
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ToolbarLayout {
    panel: RECT,
    lcars_cap: RECT,
    cap_footer: RECT,
    top_band: RECT,
    readout: RECT,
    tools_group: RECT,
    tools_tag: RECT,
    actions_group: RECT,
    actions_tag: RECT,
    select_btn: RECT,
    rect_btn: RECT,
    ellipse_btn: RECT,
    line_btn: RECT,
    arrow_btn: RECT,
    marker_btn: RECT,
    text_btn: RECT,
    pixelate_btn: RECT,
    blur_btn: RECT,
    copy_btn: RECT,
    save_btn: RECT,
    copy_save_btn: RECT,
    pin_btn: RECT,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ToolbarHit {
    Select,
    Rect,
    Ellipse,
    Line,
    Arrow,
    Marker,
    Text,
    Pixelate,
    Blur,
    Copy,
    Save,
    CopyAndSave,
    Pin,
    Panel,
}

#[derive(Debug, Clone, Copy)]
struct RadialColorPicker {
    center: POINT,
    hover_color: Option<usize>,
    phase: RadialColorPhase,
    phase_started: Instant,
    animation_duration_ms: u32,
    pending_color: Option<usize>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum RadialColorPhase {
    Opening,
    Open,
    Closing,
}

impl RadialColorPicker {
    fn opening(center: POINT, hover_color: Option<usize>, animation_duration_ms: u32) -> Self {
        Self {
            center,
            hover_color,
            phase: if animation_duration_ms == 0 {
                RadialColorPhase::Open
            } else {
                RadialColorPhase::Opening
            },
            phase_started: Instant::now(),
            animation_duration_ms,
            pending_color: None,
        }
    }

    fn begin_close(&mut self, pending_color: Option<usize>) {
        let current_scale = self.visual_scale();
        self.phase = RadialColorPhase::Closing;
        self.pending_color = pending_color;
        self.hover_color = pending_color;
        if self.animation_duration_ms == 0 {
            self.phase_started = Instant::now();
            return;
        }

        let progress = inverse_close_progress_from_scale(current_scale);
        let elapsed =
            Duration::from_secs_f32((self.animation_duration_ms as f32 / 1000.0) * progress);
        let now = Instant::now();
        self.phase_started = now.checked_sub(elapsed).unwrap_or(now);
    }

    fn progress(&self) -> f32 {
        if self.animation_duration_ms == 0 {
            return 1.0;
        }
        (self.phase_started.elapsed().as_secs_f32() / (self.animation_duration_ms as f32 / 1000.0))
            .clamp(0.0, 1.0)
    }

    fn visual_scale(&self) -> f32 {
        match self.phase {
            RadialColorPhase::Opening => ease_out_cubic(self.progress()),
            RadialColorPhase::Open => 1.0,
            RadialColorPhase::Closing => 1.0 - ease_in_cubic(self.progress()),
        }
    }
}

struct State {
    virtual_rect: RectPx,
    selection: RectPx,
    keybindings: EditorKeybindings,
    text_commit_feedback_color: COLORREF,
    radial_animation_duration_ms: u32,
    annotation_colors: [[u8; 4]; ANNOTATION_PALETTE_SIZE],
    aa_scratch: RefCell<AaScratch>,
    radial_color_picker: Option<RadialColorPicker>,
    drag: Option<Drag>,
    tool: Tool,
    chrome_hwnd: HWND,
    frozen_desktop: Option<FrozenDesktopRef>,
    selection_snapshot: Option<SelectionSnapshot>,
    toolbar_icons: ToolbarIcons,
    slint_toolbar: RefCell<Option<ToolbarSlintRenderer>>,
    annotations: Vec<Annotation>,
    redo: Vec<Annotation>,
    selected_annotation: Option<usize>,
    text_commit_feedback: Option<usize>,
    editing_text: Option<usize>,
    toolbar_hover: Option<ToolbarHit>,
    toolbar_pressed: Option<ToolbarHit>,
    stroke_color_idx: usize,
    stroke_thickness_idx: usize,
    output_action: Option<EditorOutputAction>,
    done: bool,
    canceled: bool,
}

impl State {
    fn new(
        initial: RectPx,
        virtual_rect: RectPx,
        keybindings: EditorKeybindings,
        text_commit_feedback_color: [u8; 3],
        radial_menu_animation_speed: RadialMenuAnimationSpeed,
        annotation_palette: [[u8; 4]; ANNOTATION_PALETTE_SIZE],
        frozen_frame: Option<&capture::CaptureFrame>,
        precomputed_selection_snapshot: Option<PrecomputedSelectionSnapshot>,
    ) -> Self {
        let selection = clamp_rect(initial, virtual_rect);
        let frozen_desktop = frozen_frame.and_then(frozen_desktop_ref_from_capture_frame);
        let selection_snapshot = precomputed_selection_snapshot
            .and_then(selection_snapshot_from_precomputed)
            .or_else(|| capture_selection_snapshot(selection, frozen_desktop));
        Self {
            virtual_rect,
            selection,
            keybindings,
            text_commit_feedback_color: rgb(
                text_commit_feedback_color[0],
                text_commit_feedback_color[1],
                text_commit_feedback_color[2],
            ),
            radial_animation_duration_ms: radial_menu_animation_speed.duration_ms(),
            annotation_colors: annotation_palette,
            aa_scratch: RefCell::new(AaScratch::default()),
            radial_color_picker: None,
            drag: None,
            tool: Tool::Select,
            chrome_hwnd: HWND::default(),
            selection_snapshot,
            frozen_desktop,
            toolbar_icons: load_toolbar_icons(),
            slint_toolbar: RefCell::new(None),
            annotations: Vec::new(),
            redo: Vec::new(),
            selected_annotation: None,
            text_commit_feedback: None,
            editing_text: None,
            toolbar_hover: None,
            toolbar_pressed: None,
            stroke_color_idx: DEFAULT_COLOR_INDEX,
            stroke_thickness_idx: DEFAULT_THICKNESS_INDEX,
            output_action: None,
            done: false,
            canceled: false,
        }
    }

    fn stroke_color(&self) -> [u8; 4] {
        self.annotation_colors[self.stroke_color_idx]
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

    fn pixelate_block_size(&self) -> i32 {
        PIXELATE_BLOCK_STEPS[self.stroke_thickness_idx]
    }

    fn blur_radius(&self) -> i32 {
        BLUR_RADIUS_STEPS[self.stroke_thickness_idx]
    }

    fn set_stroke_color(&mut self, idx: usize) -> bool {
        if idx >= self.annotation_colors.len() || self.stroke_color_idx == idx {
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

    fn ensure_selection_snapshot(&mut self) {
        if self.selection_snapshot.is_none() {
            self.refresh_selection_snapshot();
        }
    }

    fn refresh_selection_snapshot(&mut self) {
        self.selection_snapshot = capture_selection_snapshot(self.selection, self.frozen_desktop);
    }

    fn set_selection(&mut self, next: RectPx) -> bool {
        if !rect_changed(self.selection, next) {
            return false;
        }
        self.selection = next;
        self.selection_snapshot = None;
        self.selected_annotation = None;
        self.text_commit_feedback = None;
        self.editing_text = None;
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
            Drag::ResizeAnnotation {
                index,
                handle,
                start_rect,
                start_point,
            } => self.resize_annotation(index, handle, start_rect, start_point, abs, shift_down),
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
                let next = if shift_down {
                    snap_point_to_angle_increment(start, abs, SNAP_ANGLE_INCREMENT_DEGREES)
                } else {
                    abs
                };
                if current.x == next.x && current.y == next.y {
                    return false;
                }
                self.drag = Some(Drag::DrawLine {
                    start,
                    current: next,
                    arrow,
                });
                true
            }
            Drag::DrawMarker { start, current } => {
                let next = if shift_down {
                    snap_point_to_angle_increment(start, abs, SNAP_ANGLE_INCREMENT_DEGREES)
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
            Drag::DrawText { start, current } => {
                if current.x == abs.x && current.y == abs.y {
                    return false;
                }
                self.drag = Some(Drag::DrawText {
                    start,
                    current: abs,
                });
                true
            }
            Drag::DrawPixelate { start, current } => {
                if current.x == abs.x && current.y == abs.y {
                    return false;
                }
                self.drag = Some(Drag::DrawPixelate {
                    start,
                    current: abs,
                });
                true
            }
            Drag::DrawBlur { start, current } => {
                if current.x == abs.x && current.y == abs.y {
                    return false;
                }
                self.drag = Some(Drag::DrawBlur {
                    start,
                    current: abs,
                });
                true
            }
            Drag::MoveLineEndpoint { index, endpoint } => {
                self.move_line_endpoint(index, endpoint, abs)
            }
            Drag::MoveMarkerEndpoint { index, endpoint } => {
                self.move_marker_endpoint(index, endpoint, abs)
            }
            Drag::MoveAnnotation { index, last_point } => {
                let dx = abs.x - last_point.x;
                let dy = abs.y - last_point.y;
                if dx == 0 && dy == 0 {
                    return false;
                }
                if !self.move_annotation(index, dx, dy) {
                    return false;
                }
                self.drag = Some(Drag::MoveAnnotation {
                    index,
                    last_point: abs,
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

    fn pending_text(&self) -> Option<RectPx> {
        let Drag::DrawText { start, current } = self.drag? else {
            return None;
        };
        Some(normalize_abs(start, current, self.selection))
    }

    fn pending_pixelate(&self) -> Option<RectPx> {
        let Drag::DrawPixelate { start, current } = self.drag? else {
            return None;
        };
        Some(normalize_abs(start, current, self.selection))
    }

    fn pending_blur(&self) -> Option<RectPx> {
        let Drag::DrawBlur { start, current } = self.drag? else {
            return None;
        };
        Some(normalize_abs(start, current, self.selection))
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
            Some(Drag::DrawText { start, current }) => {
                let rect = normalize_abs(start, current, self.selection);
                if rect.width() < MIN_RECT || rect.height() < MIN_RECT {
                    return false;
                }
                self.annotations.push(Annotation::Text(TextAnn {
                    rect_abs: rect,
                    text: String::new(),
                    color: stroke_color,
                }));
                self.redo.clear();
                self.selected_annotation = Some(self.annotations.len() - 1);
                self.editing_text = Some(self.annotations.len() - 1);
                true
            }
            Some(Drag::DrawPixelate { start, current }) => {
                let rect = normalize_abs(start, current, self.selection);
                if rect.width() < MIN_RECT || rect.height() < MIN_RECT {
                    return false;
                }
                self.annotations.push(Annotation::Pixelate(PixelateAnn {
                    rect_abs: rect,
                    block: self.pixelate_block_size(),
                }));
                self.redo.clear();
                self.selected_annotation = Some(self.annotations.len() - 1);
                true
            }
            Some(Drag::DrawBlur { start, current }) => {
                let rect = normalize_abs(start, current, self.selection);
                if rect.width() < MIN_RECT || rect.height() < MIN_RECT {
                    return false;
                }
                self.annotations.push(Annotation::Blur(BlurAnn {
                    rect_abs: rect,
                    radius: self.blur_radius(),
                }));
                self.redo.clear();
                self.selected_annotation = Some(self.annotations.len() - 1);
                true
            }
            Some(Drag::MoveAnnotation { .. })
            | Some(Drag::ResizeAnnotation { .. })
            | Some(Drag::MoveLineEndpoint { .. })
            | Some(Drag::MoveMarkerEndpoint { .. }) => false,
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
        self.editing_text = None;
        true
    }

    fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.annotations.push(next);
        self.selected_annotation = None;
        self.editing_text = None;
        true
    }

    fn clear_drag_state(&mut self) {
        self.drag = None;
    }

    fn delete_selected_annotation(&mut self) -> bool {
        let Some(idx) = self.selected_annotation else {
            return false;
        };
        if idx >= self.annotations.len() {
            self.selected_annotation = None;
            return false;
        }
        remove_annotation_at(self, idx);
        true
    }

    fn move_annotation(&mut self, index: usize, raw_dx: i32, raw_dy: i32) -> bool {
        if index >= self.annotations.len() {
            return false;
        }
        let Some(bounds) = annotation_bounds_abs(&self.annotations[index]) else {
            return false;
        };
        let dx = raw_dx.clamp(
            self.selection.left - bounds.left,
            self.selection.right - bounds.right,
        );
        let dy = raw_dy.clamp(
            self.selection.top - bounds.top,
            self.selection.bottom - bounds.bottom,
        );
        if dx == 0 && dy == 0 {
            return false;
        }

        match &mut self.annotations[index] {
            Annotation::Rectangle(rect) => {
                rect.rect_abs = translate_rect(rect.rect_abs, dx, dy);
            }
            Annotation::Ellipse(ellipse) => {
                ellipse.rect_abs = translate_rect(ellipse.rect_abs, dx, dy);
            }
            Annotation::Line(line) => {
                line.start_abs = translate_point(line.start_abs, dx, dy);
                line.end_abs = translate_point(line.end_abs, dx, dy);
            }
            Annotation::Marker(marker) => {
                for point in &mut marker.points_abs {
                    *point = translate_point(*point, dx, dy);
                }
            }
            Annotation::Text(text) => {
                text.rect_abs = translate_rect(text.rect_abs, dx, dy);
            }
            Annotation::Pixelate(pixelate) => {
                pixelate.rect_abs = translate_rect(pixelate.rect_abs, dx, dy);
            }
            Annotation::Blur(blur) => {
                blur.rect_abs = translate_rect(blur.rect_abs, dx, dy);
            }
        }
        true
    }

    fn resize_annotation(
        &mut self,
        index: usize,
        handle: ResizeHandle,
        start_rect: RectPx,
        start_point: POINT,
        current: POINT,
        preserve_aspect: bool,
    ) -> bool {
        if index >= self.annotations.len() {
            return false;
        }
        let mut next = resize_selection(handle, start_rect, start_point, current, self.selection);
        if preserve_aspect {
            next = constrain_resize_aspect(handle, start_rect, next, self.selection);
        }
        if next.width() < MIN_RECT || next.height() < MIN_RECT {
            return false;
        }
        if !set_annotation_resize_rect(&mut self.annotations[index], next) {
            return false;
        }
        self.drag = Some(Drag::ResizeAnnotation {
            index,
            handle,
            start_rect,
            start_point,
        });
        true
    }

    fn move_line_endpoint(&mut self, index: usize, endpoint: LineEndpoint, point: POINT) -> bool {
        let Some(Annotation::Line(line)) = self.annotations.get_mut(index) else {
            return false;
        };
        let clamped = clamp_point(point, self.selection);
        match endpoint {
            LineEndpoint::Start => {
                if line.start_abs.x == clamped.x && line.start_abs.y == clamped.y {
                    return false;
                }
                line.start_abs = clamped;
            }
            LineEndpoint::End => {
                if line.end_abs.x == clamped.x && line.end_abs.y == clamped.y {
                    return false;
                }
                line.end_abs = clamped;
            }
        }
        self.drag = Some(Drag::MoveLineEndpoint { index, endpoint });
        true
    }

    fn move_marker_endpoint(&mut self, index: usize, endpoint: LineEndpoint, point: POINT) -> bool {
        let Some(Annotation::Marker(marker)) = self.annotations.get_mut(index) else {
            return false;
        };
        if marker.points_abs.is_empty() {
            return false;
        }
        let clamped = clamp_point(point, self.selection);
        let target_idx = match endpoint {
            LineEndpoint::Start => 0,
            LineEndpoint::End => marker.points_abs.len() - 1,
        };
        let current = marker.points_abs[target_idx];
        if current.x == clamped.x && current.y == clamped.y {
            return false;
        }
        marker.points_abs[target_idx] = clamped;
        self.drag = Some(Drag::MoveMarkerEndpoint { index, endpoint });
        true
    }
}

pub fn edit_region(
    initial_selection: RectPx,
    keybindings: &EditorKeybindings,
    text_commit_feedback_color: [u8; 3],
    radial_menu_animation_speed: RadialMenuAnimationSpeed,
    annotation_palette: [[u8; 4]; ANNOTATION_PALETTE_SIZE],
    frozen_frame: Option<&capture::CaptureFrame>,
    precomputed_selection_snapshot: Option<PrecomputedSelectionSnapshot>,
) -> Result<RegionEditOutcome> {
    let virtual_rect = virtual_screen_rect();
    if virtual_rect.width() <= 0 || virtual_rect.height() <= 0 {
        bail!("invalid virtual desktop size");
    }

    let hmodule = unsafe { GetModuleHandleW(PCWSTR::null()).map_err(anyhow::Error::from)? };
    let hinstance = HINSTANCE(hmodule.0);
    register_editor_class(hinstance);

    let state_ptr = Box::into_raw(Box::new(State::new(
        initial_selection,
        virtual_rect,
        keybindings.clone(),
        text_commit_feedback_color,
        radial_menu_animation_speed,
        annotation_palette,
        frozen_frame,
        precomputed_selection_snapshot,
    )));
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
        let _ = SetForegroundWindow(hwnd);
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
        output_action: state.output_action,
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
            Annotation::Text(text) => {
                let local = RectPx {
                    left: text.rect_abs.left - result.bounds.left,
                    top: text.rect_abs.top - result.bounds.top,
                    right: text.rect_abs.right - result.bounds.left,
                    bottom: text.rect_abs.bottom - result.bounds.top,
                };
                draw_text_raster(image, local, &text.text, text.color);
            }
            Annotation::Pixelate(pixelate) => {
                let local = RectPx {
                    left: pixelate.rect_abs.left - result.bounds.left,
                    top: pixelate.rect_abs.top - result.bounds.top,
                    right: pixelate.rect_abs.right - result.bounds.left,
                    bottom: pixelate.rect_abs.bottom - result.bounds.top,
                };
                draw_pixelate_raster(image, local, pixelate.block);
            }
            Annotation::Blur(blur) => {
                let local = RectPx {
                    left: blur.rect_abs.left - result.bounds.left,
                    top: blur.rect_abs.top - result.bounds.top,
                    right: blur.rect_abs.right - result.bounds.left,
                    bottom: blur.rect_abs.bottom - result.bounds.top,
                };
                draw_blur_raster(image, local, blur.radius);
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
            SWP_SHOWWINDOW | SWP_NOACTIVATE,
        )
        .map_err(anyhow::Error::from)?;
        let _ = ShowWindow(chrome, SW_SHOWNOACTIVATE);
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
        WM_CHAR => on_char(hwnd, wparam),
        WM_RBUTTONDOWN => on_mouse_right_down(hwnd, lparam),
        WM_RBUTTONUP => on_mouse_right_up(hwnd, lparam),
        WM_LBUTTONDOWN => on_mouse_down(hwnd, lparam),
        WM_MOUSEMOVE => on_mouse_move(hwnd, lparam),
        WM_LBUTTONUP => on_mouse_up(hwnd, lparam),
        WM_MOUSEWHEEL => on_mouse_wheel(hwnd, wparam),
        WM_TIMER => on_timer(hwnd, wparam),
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
    let shift_down = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
    let alt_down = unsafe { GetKeyState(VK_MENU.0 as i32) } < 0;

    if let Some(state) = unsafe { state_mut(hwnd) }
        && state.editing_text.is_some()
    {
        if key == VK_RETURN.0 as u32 {
            if shift_down {
                let mut changed = false;
                if let Some(idx) = state.editing_text
                    && let Some(Annotation::Text(text)) = state.annotations.get_mut(idx)
                {
                    text.text.push('\n');
                    ensure_text_bounds(state, idx);
                    changed = true;
                }
                if changed {
                    invalidate_all(hwnd);
                }
                return LRESULT(0);
            }
            if let Some(index) = finish_text_edit(state) {
                set_text_commit_feedback(hwnd, state, index);
            }
            invalidate_all(hwnd);
            return LRESULT(0);
        }
        if key == VK_ESCAPE.0 as u32 {
            if let Some(index) = finish_text_edit(state) {
                set_text_commit_feedback(hwnd, state, index);
            }
            invalidate_all(hwnd);
            return LRESULT(0);
        }
        // While typing text, suppress global editor shortcuts/tool switches.
        return LRESULT(0);
    }

    if let Some(state) = unsafe { state_mut(hwnd) } {
        if key == VK_ESCAPE.0 as u32
            && let Some(picker) = state.radial_color_picker.as_mut()
        {
            if picker.phase != RadialColorPhase::Closing {
                picker.begin_close(None);
                if picker.animation_duration_ms == 0 {
                    let _ = on_timer(hwnd, WPARAM(RADIAL_COLOR_TIMER_ID));
                } else {
                    unsafe {
                        let _ = SetTimer(hwnd, RADIAL_COLOR_TIMER_ID, RADIAL_ANIM_FRAME_MS, None);
                    }
                }
            }
            invalidate_chrome(hwnd);
            return LRESULT(0);
        }
    }

    if key == VK_ESCAPE.0 as u32 {
        cancel(hwnd);
        return LRESULT(0);
    }

    if let Some(state) = unsafe { state_mut(hwnd) } {
        if state
            .keybindings
            .undo
            .matches(key, ctrl_down, shift_down, alt_down)
        {
            if state.undo() {
                sync_layer_mode(hwnd);
                invalidate_all(hwnd);
            }
            return LRESULT(0);
        }

        if state
            .keybindings
            .redo
            .matches(key, ctrl_down, shift_down, alt_down)
        {
            if state.redo() {
                sync_layer_mode(hwnd);
                invalidate_all(hwnd);
            }
            return LRESULT(0);
        }

        if state
            .keybindings
            .delete_selected
            .matches(key, ctrl_down, shift_down, alt_down)
        {
            if state.delete_selected_annotation() {
                sync_layer_mode(hwnd);
                invalidate_all(hwnd);
            }
            return LRESULT(0);
        }

        if let Some(action) = state
            .keybindings
            .output_action_for_key(key, ctrl_down, shift_down, alt_down)
        {
            if state.selection.width() >= MIN_SELECTION && state.selection.height() >= MIN_SELECTION
            {
                state.output_action = Some(action);
                state.done = true;
            }
            return LRESULT(0);
        }

        if !ctrl_down && !alt_down {
            if key == 0xDB && state.adjust_stroke_thickness(-1) {
                invalidate_all(hwnd);
                return LRESULT(0);
            }

            if key == 0xDD && state.adjust_stroke_thickness(1) {
                invalidate_all(hwnd);
                return LRESULT(0);
            }

            let color_idx = if (0x31..=0x38).contains(&key) {
                Some((key - 0x31) as usize)
            } else if (0x61..=0x68).contains(&key) {
                Some((key - 0x61) as usize)
            } else {
                None
            };
            if let Some(idx) = color_idx
                && state.set_stroke_color(idx)
            {
                invalidate_all(hwnd);
                return LRESULT(0);
            }
        }

        if let Some(tool) = state
            .keybindings
            .tool_for_key(key, ctrl_down, shift_down, alt_down)
            && state.tool != tool
        {
            let prev_tool = state.tool;
            state.tool = tool;
            if tool != Tool::Select {
                state.ensure_selection_snapshot();
            }
            state.clear_drag_state();
            let has_annotations = !state.annotations.is_empty();
            if tool_switch_needs_prepaint(prev_tool, state.tool, has_annotations) {
                invalidate_all(hwnd);
                unsafe {
                    let _ = UpdateWindow(hwnd);
                }
            }
            unsafe {
                let _ = set_layer_mode(hwnd, state.tool);
            }
            invalidate_all(hwnd);
            return LRESULT(0);
        }
    }

    if key == VK_RETURN.0 as u32 {
        if let Some(state) = unsafe { state_mut(hwnd) }
            && state.selection.width() >= MIN_SELECTION
            && state.selection.height() >= MIN_SELECTION
        {
            state.output_action = None;
            state.done = true;
        }
        return LRESULT(0);
    }

    unsafe { DefWindowProcW(hwnd, WM_KEYDOWN, wparam, LPARAM(0)) }
}

fn on_char(hwnd: HWND, wparam: WPARAM) -> LRESULT {
    let mut changed = false;
    if let Some(state) = unsafe { state_mut(hwnd) }
        && let Some(idx) = state.editing_text
        && let Some(Annotation::Text(text)) = state.annotations.get_mut(idx)
    {
        let code = wparam.0 as u32;
        if code == 0x08 {
            if !text.text.is_empty() {
                text.text.pop();
                changed = true;
            }
        } else if code >= 0x20
            && code != 0x7F
            && let Some(ch) = char::from_u32(code)
        {
            text.text.push(ch);
            changed = true;
        }
        if changed {
            ensure_text_bounds(state, idx);
        }
    }
    if changed {
        invalidate_all(hwnd);
    }
    LRESULT(0)
}

fn finish_text_edit(state: &mut State) -> Option<usize> {
    let Some(idx) = state.editing_text.take() else {
        return None;
    };
    if idx >= state.annotations.len() {
        return None;
    }
    let is_empty = matches!(
        state.annotations.get(idx),
        Some(Annotation::Text(text)) if text.text.trim().is_empty()
    );
    if is_empty {
        remove_annotation_at(state, idx);
        None
    } else {
        state.selected_annotation = Some(idx);
        Some(idx)
    }
}

fn set_text_commit_feedback(hwnd: HWND, state: &mut State, index: usize) {
    state.text_commit_feedback = Some(index);
    unsafe {
        let _ = KillTimer(hwnd, TEXT_COMMIT_FEEDBACK_TIMER_ID);
        let _ = SetTimer(
            hwnd,
            TEXT_COMMIT_FEEDBACK_TIMER_ID,
            TEXT_COMMIT_FEEDBACK_MS,
            None,
        );
    }
}

fn remove_annotation_at(state: &mut State, idx: usize) {
    if idx >= state.annotations.len() {
        return;
    }
    state.annotations.remove(idx);
    state.redo.clear();
    state.selected_annotation = match state.selected_annotation {
        Some(sel) if sel == idx => None,
        Some(sel) if sel > idx => Some(sel - 1),
        other => other,
    };
    state.text_commit_feedback = match state.text_commit_feedback {
        Some(flash) if flash == idx => None,
        Some(flash) if flash > idx => Some(flash - 1),
        other => other,
    };
    state.editing_text = match state.editing_text {
        Some(edit) if edit == idx => None,
        Some(edit) if edit > idx => Some(edit - 1),
        other => other,
    };
}

fn ensure_text_bounds(state: &mut State, idx: usize) {
    let Some(Annotation::Text(text)) = state.annotations.get_mut(idx) else {
        return;
    };
    let (need_w, need_h) = text_required_size(&text.text);
    let current_w = text.rect_abs.width();
    let current_h = text.rect_abs.height();
    if current_w >= need_w && current_h >= need_h {
        return;
    }

    let target_w = current_w.max(need_w).min(state.selection.width());
    let target_h = current_h.max(need_h).min(state.selection.height());
    let mut left = text.rect_abs.left;
    let mut top = text.rect_abs.top;
    if left + target_w > state.selection.right {
        left = state.selection.right - target_w;
    }
    if top + target_h > state.selection.bottom {
        top = state.selection.bottom - target_h;
    }
    left = left.clamp(state.selection.left, state.selection.right - target_w);
    top = top.clamp(state.selection.top, state.selection.bottom - target_h);
    text.rect_abs = RectPx {
        left,
        top,
        right: left + target_w,
        bottom: top + target_h,
    };
}

fn on_mouse_right_down(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let client = point_from_lparam(lparam);
    let mut opened = false;
    if let Some(state) = unsafe { state_mut(hwnd) }
        && state.drag.is_none()
    {
        let center = clamp_radial_center(client, client_rect(state.virtual_rect));
        let hover = radial_color_hit_test(center, client, 1.0);
        let picker = RadialColorPicker::opening(center, hover, state.radial_animation_duration_ms);
        let needs_anim_timer = picker.phase == RadialColorPhase::Opening;
        state.radial_color_picker = Some(picker);
        if needs_anim_timer {
            unsafe {
                let _ = SetTimer(hwnd, RADIAL_COLOR_TIMER_ID, RADIAL_ANIM_FRAME_MS, None);
            }
        }
        opened = true;
    }

    if opened {
        unsafe {
            let _ = SetCapture(hwnd);
        }
        invalidate_chrome(hwnd);
    }
    update_cursor(hwnd, client);
    LRESULT(0)
}

fn on_mouse_right_up(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let client = point_from_lparam(lparam);
    let mut consumed = false;
    let mut started_closing = false;
    let mut instant_close = false;
    if let Some(state) = unsafe { state_mut(hwnd) } {
        if let Some(picker) = state.radial_color_picker.as_mut() {
            if picker.phase != RadialColorPhase::Closing {
                let hovered = radial_color_hit_test(picker.center, client, picker.visual_scale());
                picker.begin_close(hovered);
                started_closing = true;
                instant_close = picker.animation_duration_ms == 0;
            }
            consumed = true;
        }
    }

    if consumed {
        if started_closing {
            if instant_close {
                let _ = on_timer(hwnd, WPARAM(RADIAL_COLOR_TIMER_ID));
            } else {
                unsafe {
                    let _ = SetTimer(hwnd, RADIAL_COLOR_TIMER_ID, RADIAL_ANIM_FRAME_MS, None);
                }
            }
        }
        invalidate_chrome(hwnd);
    }
    update_cursor(hwnd, client);
    LRESULT(0)
}

fn on_mouse_down(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let client = point_from_lparam(lparam);
    let mut started_drag = false;

    if let Some(state) = unsafe { state_mut(hwnd) } {
        if let Some(picker) = state.radial_color_picker.as_mut() {
            if picker.phase != RadialColorPhase::Closing {
                let selected = radial_color_hit_test(picker.center, client, picker.visual_scale());
                picker.begin_close(selected);
                if picker.animation_duration_ms == 0 {
                    let _ = on_timer(hwnd, WPARAM(RADIAL_COLOR_TIMER_ID));
                } else {
                    unsafe {
                        let _ = SetTimer(hwnd, RADIAL_COLOR_TIMER_ID, RADIAL_ANIM_FRAME_MS, None);
                    }
                }
            }
            state.toolbar_hover = None;
            state.toolbar_pressed = None;
            invalidate_chrome(hwnd);
            update_cursor(hwnd, client);
            return LRESULT(0);
        }

        let selection_client = to_client_rect(state.selection, state.virtual_rect);
        let bar = toolbar_layout(selection_client, client_rect(state.virtual_rect));
        if let Some(hit) = toolbar_hit(bar, client) {
            let visual_hit = hoverable_toolbar_hit(Some(hit));
            state.toolbar_pressed = visual_hit;
            state.toolbar_hover = visual_hit;
            let prev_tool = state.tool;
            let mut changed = false;
            let mut layer_changed = false;
            if state.editing_text.is_some() {
                if let Some(index) = finish_text_edit(state) {
                    set_text_commit_feedback(hwnd, state, index);
                }
                changed = true;
            }
            match hit {
                ToolbarHit::Select => {
                    if state.tool != Tool::Select {
                        state.tool = Tool::Select;
                        changed = true;
                        layer_changed = true;
                    }
                }
                ToolbarHit::Rect => {
                    if state.tool != Tool::Rectangle {
                        state.ensure_selection_snapshot();
                        state.tool = Tool::Rectangle;
                        changed = true;
                        layer_changed = true;
                    }
                }
                ToolbarHit::Ellipse => {
                    if state.tool != Tool::Ellipse {
                        state.ensure_selection_snapshot();
                        state.tool = Tool::Ellipse;
                        changed = true;
                        layer_changed = true;
                    }
                }
                ToolbarHit::Line => {
                    if state.tool != Tool::Line {
                        state.ensure_selection_snapshot();
                        state.tool = Tool::Line;
                        changed = true;
                        layer_changed = true;
                    }
                }
                ToolbarHit::Arrow => {
                    if state.tool != Tool::Arrow {
                        state.ensure_selection_snapshot();
                        state.tool = Tool::Arrow;
                        changed = true;
                        layer_changed = true;
                    }
                }
                ToolbarHit::Marker => {
                    if state.tool != Tool::Marker {
                        state.ensure_selection_snapshot();
                        state.tool = Tool::Marker;
                        changed = true;
                        layer_changed = true;
                    }
                }
                ToolbarHit::Text => {
                    if state.tool != Tool::Text {
                        state.ensure_selection_snapshot();
                        state.tool = Tool::Text;
                        changed = true;
                        layer_changed = true;
                    }
                }
                ToolbarHit::Pixelate => {
                    if state.tool != Tool::Pixelate {
                        state.ensure_selection_snapshot();
                        state.tool = Tool::Pixelate;
                        changed = true;
                        layer_changed = true;
                    }
                }
                ToolbarHit::Blur => {
                    if state.tool != Tool::Blur {
                        state.ensure_selection_snapshot();
                        state.tool = Tool::Blur;
                        changed = true;
                        layer_changed = true;
                    }
                }
                ToolbarHit::Copy => {
                    state.output_action = Some(EditorOutputAction::Copy);
                    state.done = true;
                }
                ToolbarHit::Save => {
                    state.output_action = Some(EditorOutputAction::Save);
                    state.done = true;
                }
                ToolbarHit::CopyAndSave => {
                    state.output_action = Some(EditorOutputAction::CopyAndSave);
                    state.done = true;
                }
                ToolbarHit::Pin => {
                    state.output_action = Some(EditorOutputAction::Pin);
                    state.done = true;
                }
                ToolbarHit::Panel => {}
            }
            state.clear_drag_state();
            if layer_changed {
                let has_annotations = !state.annotations.is_empty();
                if tool_switch_needs_prepaint(prev_tool, state.tool, has_annotations) {
                    invalidate_all(hwnd);
                    unsafe {
                        let _ = UpdateWindow(hwnd);
                    }
                }
                unsafe {
                    let _ = set_layer_mode(hwnd, state.tool);
                }
            }
            if changed {
                invalidate_all(hwnd);
            } else if visual_hit.is_some() {
                invalidate_chrome(hwnd);
            }
            update_cursor(hwnd, client);
            return LRESULT(0);
        }
        if state.toolbar_pressed.take().is_some() || state.toolbar_hover.take().is_some() {
            invalidate_chrome(hwnd);
        }

        let abs = clamp_point(
            client_to_abs(client, state.virtual_rect),
            state.virtual_rect,
        );
        if let Some(edit_idx) = state.editing_text {
            let keep_editing = matches!(state.annotations.get(edit_idx), Some(Annotation::Text(text)) if point_in_abs(abs, text.rect_abs));
            if !keep_editing {
                if let Some(index) = finish_text_edit(state) {
                    set_text_commit_feedback(hwnd, state, index);
                }
                invalidate_all(hwnd);
            }
            // Consume the first click while editing text: inside keeps focus,
            // outside commits the text box. A second click can start a new action.
            update_cursor(hwnd, client);
            return LRESULT(0);
        }
        match state.tool {
            Tool::Select => {
                if let Some(hit) = selected_annotation_handle_hit(state, client) {
                    match hit {
                        AnnotationHandleHit::Resize {
                            index,
                            bounds,
                            handle,
                        } => {
                            state.drag = Some(Drag::ResizeAnnotation {
                                index,
                                handle,
                                start_rect: bounds,
                                start_point: abs,
                            });
                        }
                        AnnotationHandleHit::LineEndpoint { index, endpoint } => {
                            state.drag = Some(Drag::MoveLineEndpoint { index, endpoint });
                        }
                        AnnotationHandleHit::MarkerEndpoint { index, endpoint } => {
                            state.drag = Some(Drag::MoveMarkerEndpoint { index, endpoint });
                        }
                    }
                    started_drag = true;
                } else if let Some(handle) = hit_handle(selection_client, client) {
                    state.drag = Some(Drag::Resize {
                        handle,
                        start_rect: state.selection,
                        start_point: abs,
                    });
                    started_drag = true;
                } else if let Some(idx) = hit_annotation(&state.annotations, abs) {
                    let selection_changed = state.selected_annotation != Some(idx);
                    state.selected_annotation = Some(idx);
                    state.drag = Some(Drag::MoveAnnotation {
                        index: idx,
                        last_point: abs,
                    });
                    started_drag = true;
                    if selection_changed {
                        invalidate_all(hwnd);
                    }
                } else if point_in(client, selection_client) {
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
                    sync_layer_mode(hwnd);
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
            Tool::Text => {
                if point_in(client, selection_client) {
                    state.selected_annotation = None;
                    let shift_down = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
                    if shift_down {
                        state.drag = Some(Drag::DrawText {
                            start: abs,
                            current: abs,
                        });
                    } else {
                        state.annotations.push(Annotation::Text(TextAnn {
                            rect_abs: default_text_rect_at(abs, state.selection),
                            text: String::new(),
                            color: state.stroke_color(),
                        }));
                        state.redo.clear();
                        let index = state.annotations.len() - 1;
                        state.selected_annotation = Some(index);
                        state.editing_text = Some(index);
                        state.drag = Some(Drag::MoveAnnotation {
                            index,
                            last_point: abs,
                        });
                    }
                    started_drag = true;
                }
            }
            Tool::Pixelate => {
                if point_in(client, selection_client) {
                    state.selected_annotation = None;
                    state.drag = Some(Drag::DrawPixelate {
                        start: abs,
                        current: abs,
                    });
                    started_drag = true;
                }
            }
            Tool::Blur => {
                if point_in(client, selection_client) {
                    state.selected_annotation = None;
                    state.drag = Some(Drag::DrawBlur {
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
    let mut refresh_layer = false;
    let mut radial_hover_changed = false;
    let mut toolbar_hover_changed = false;
    if let Some(state) = unsafe { state_mut(hwnd) } {
        if let Some(picker) = state.radial_color_picker.as_mut() {
            if picker.phase != RadialColorPhase::Closing {
                let hover = radial_color_hit_test(picker.center, client, picker.visual_scale());
                if hover != picker.hover_color {
                    picker.hover_color = hover;
                    radial_hover_changed = true;
                }
            }
            if state.toolbar_hover.is_some() {
                state.toolbar_hover = None;
                toolbar_hover_changed = true;
            }
        } else {
            let selection = to_client_rect(state.selection, state.virtual_rect);
            let bar = toolbar_layout(selection, client_rect(state.virtual_rect));
            let next_hover = hoverable_toolbar_hit(toolbar_hit(bar, client));
            if state.toolbar_hover != next_hover {
                state.toolbar_hover = next_hover;
                toolbar_hover_changed = true;
            }

            if state.drag.is_some() {
                let shift_down = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
                let abs = clamp_point(
                    client_to_abs(client, state.virtual_rect),
                    state.virtual_rect,
                );
                if state.update_drag(abs, shift_down) {
                    changed_tool = Some(state.tool);
                    if state.tool == Tool::Select {
                        refresh_layer = true;
                    }
                }
            }
        }
    }
    if radial_hover_changed || toolbar_hover_changed {
        invalidate_chrome(hwnd);
    }
    if refresh_layer {
        sync_layer_mode(hwnd);
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
    let mut clear_toolbar_pressed = false;
    if let Some(state) = unsafe { state_mut(hwnd) } {
        clear_toolbar_pressed = state.toolbar_pressed.take().is_some();
        if state.drag.is_some() {
            tool_for_repaint = state.tool;
            let had_size_badge_drag = matches!(
                state.drag,
                Some(Drag::NewSelection { .. } | Drag::Resize { .. })
            );
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
                if had_size_badge_drag {
                    repaint = true;
                }
            }
            if state.tool == Tool::Select && repaint && !finalized_annotation {
                state.refresh_selection_snapshot();
            }
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
    if clear_toolbar_pressed {
        invalidate_chrome(hwnd);
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

fn on_timer(hwnd: HWND, wparam: WPARAM) -> LRESULT {
    if wparam.0 == TEXT_COMMIT_FEEDBACK_TIMER_ID {
        if let Some(state) = unsafe { state_mut(hwnd) } {
            state.text_commit_feedback = None;
        }
        unsafe {
            let _ = KillTimer(hwnd, TEXT_COMMIT_FEEDBACK_TIMER_ID);
        }
        invalidate_chrome(hwnd);
        return LRESULT(0);
    }

    if wparam.0 == RADIAL_COLOR_TIMER_ID {
        let mut finalize_close = false;
        let mut stop_timer = false;
        let mut repaint_chrome = false;
        if let Some(state) = unsafe { state_mut(hwnd) }
            && let Some(picker) = state.radial_color_picker.as_mut()
        {
            match picker.phase {
                RadialColorPhase::Opening => {
                    repaint_chrome = true;
                    if picker.progress() >= 1.0 {
                        picker.phase = RadialColorPhase::Open;
                        stop_timer = true;
                    }
                }
                RadialColorPhase::Open => {
                    stop_timer = true;
                }
                RadialColorPhase::Closing => {
                    repaint_chrome = true;
                    if picker.progress() >= 1.0 {
                        finalize_close = true;
                    }
                }
            }
        } else {
            stop_timer = true;
        }

        let mut changed_color = false;
        if finalize_close
            && let Some(state) = unsafe { state_mut(hwnd) }
            && let Some(picker) = state.radial_color_picker.take()
            && let Some(idx) = picker.pending_color
        {
            changed_color = state.set_stroke_color(idx);
        }

        if stop_timer || finalize_close {
            unsafe {
                let _ = KillTimer(hwnd, RADIAL_COLOR_TIMER_ID);
            }
        }
        if finalize_close {
            unsafe {
                let _ = ReleaseCapture();
            }
        }

        if changed_color {
            invalidate_all(hwnd);
        } else if repaint_chrome || finalize_close {
            invalidate_chrome(hwnd);
        }
        return LRESULT(0);
    }

    unsafe { DefWindowProcW(hwnd, WM_TIMER, wparam, LPARAM(0)) }
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
        Tool::Rectangle
        | Tool::Ellipse
        | Tool::Line
        | Tool::Arrow
        | Tool::Text
        | Tool::Pixelate
        | Tool::Blur => invalidate_chrome(hwnd),
    }
}

fn sync_layer_mode(hwnd: HWND) {
    if let Some(state) = unsafe { state_ref(hwnd) } {
        unsafe {
            let _ = set_layer_mode(hwnd, state.tool);
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
    let render_selection_in_chrome = should_render_selection_in_chrome(state);
    let has_annotations = !state.annotations.is_empty();
    let use_color_key = should_use_color_key(state.tool, has_annotations);
    unsafe {
        if state.tool == Tool::Select && use_color_key {
            let _ = FillRect(mem_dc, &selection, transparent);
        } else if render_selection_in_chrome {
            let _ = FillRect(mem_dc, &selection, selection_fill);
        } else if let Some(snapshot) = state.selection_snapshot.as_ref() {
            draw_selection_snapshot(mem_dc, snapshot, selection);
        } else {
            let _ = FillRect(mem_dc, &selection, selection_fill);
        }
    }

    let mut aa_scratch = state.aa_scratch.borrow_mut();
    for ann in &state.annotations {
        if let Annotation::Marker(marker) = ann {
            draw_marker_overlay_aa(
                mem_dc,
                &mut aa_scratch,
                marker
                    .points_abs
                    .iter()
                    .copied()
                    .map(|p| to_client_point(p, state.virtual_rect))
                    .map(|p| offset_point(p, dirty.left, dirty.top)),
                marker.color,
                marker.thickness,
            );
        }
    }
    if let Some((start, end)) = state.pending_marker() {
        draw_marker_overlay_aa(
            mem_dc,
            &mut aa_scratch,
            [start, end]
                .into_iter()
                .map(|p| to_client_point(p, state.virtual_rect))
                .map(|p| offset_point(p, dirty.left, dirty.top)),
            state.marker_color(),
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
    let dirty = ps.rcPaint;
    let dirty_width = dirty.right - dirty.left;
    let dirty_height = dirty.bottom - dirty.top;
    if dirty_width <= 0 || dirty_height <= 0 {
        unsafe {
            let _ = EndPaint(hwnd, &ps);
        }
        return;
    }

    let mut client_full = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut client_full);
    }

    let clear = unsafe { CreateSolidBrush(OVERLAY_KEY) };
    let sel_brush = unsafe { CreateSolidBrush(SELECTION_COLOR) };

    let mem_dc = unsafe { CreateCompatibleDC(hdc) };
    if mem_dc.0.is_null() {
        cleanup_paint_objects(&[clear, sel_brush]);
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
        cleanup_paint_objects(&[clear, sel_brush]);
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
        let _ = FillRect(mem_dc, &dirty_local, clear);
    }
    let local_virtual_rect = RectPx {
        left: state.virtual_rect.left + dirty.left,
        top: state.virtual_rect.top + dirty.top,
        right: state.virtual_rect.right + dirty.left,
        bottom: state.virtual_rect.bottom + dirty.top,
    };

    let selection = to_client_rect(state.selection, local_virtual_rect);
    if should_render_selection_in_chrome(state)
        && let Some(snapshot) = state.selection_snapshot.as_ref()
    {
        draw_selection_snapshot(mem_dc, snapshot, selection);
    }
    let stroke_color = rgba_to_colorref(state.stroke_color());
    let stroke_thickness = state.stroke_thickness();
    let mut aa_scratch = state.aa_scratch.borrow_mut();
    for ann in &state.annotations {
        match ann {
            Annotation::Rectangle(rect) => {
                frame_thick_color(
                    mem_dc,
                    to_client_rect(rect.rect_abs, local_virtual_rect),
                    rgba_to_colorref(rect.color),
                    rect.thickness,
                );
            }
            Annotation::Ellipse(ellipse) => {
                draw_ellipse_outline_overlay(
                    mem_dc,
                    to_client_rect(ellipse.rect_abs, local_virtual_rect),
                    rgba_to_colorref(ellipse.color),
                    ellipse.thickness,
                );
            }
            Annotation::Line(line) => {
                if line.arrow {
                    draw_arrow_overlay_aa(
                        mem_dc,
                        &mut aa_scratch,
                        to_client_point(line.start_abs, local_virtual_rect),
                        to_client_point(line.end_abs, local_virtual_rect),
                        line.color,
                        line.thickness,
                    );
                } else {
                    draw_line_overlay_aa(
                        mem_dc,
                        &mut aa_scratch,
                        to_client_point(line.start_abs, local_virtual_rect),
                        to_client_point(line.end_abs, local_virtual_rect),
                        line.color,
                        line.thickness,
                    );
                }
            }
            Annotation::Marker(_) => {}
            Annotation::Text(text) => {
                let rect = to_client_rect(text.rect_abs, local_virtual_rect);
                frame_thick_color(mem_dc, rect, rgba_to_colorref(text.color), 1);
                draw_text_overlay(mem_dc, rect, &text.text, rgba_to_colorref(text.color));
            }
            Annotation::Pixelate(pixelate) => {
                if let Some(snapshot) = state.selection_snapshot.as_ref() {
                    draw_pixelate_overlay(
                        mem_dc,
                        snapshot,
                        state.selection,
                        pixelate.rect_abs,
                        local_virtual_rect,
                        pixelate.block,
                    );
                }
                frame_thick_color(
                    mem_dc,
                    to_client_rect(pixelate.rect_abs, local_virtual_rect),
                    rgb(255, 255, 255),
                    1,
                );
            }
            Annotation::Blur(blur) => {
                if let Some(snapshot) = state.selection_snapshot.as_ref() {
                    draw_blur_overlay(
                        mem_dc,
                        snapshot,
                        state.selection,
                        blur.rect_abs,
                        local_virtual_rect,
                        blur.radius,
                    );
                }
                frame_thick_color(
                    mem_dc,
                    to_client_rect(blur.rect_abs, local_virtual_rect),
                    rgb(255, 255, 255),
                    1,
                );
            }
        }
    }
    if let Some(pending) = state.pending_rect() {
        frame_thick_color(
            mem_dc,
            to_client_rect(pending, local_virtual_rect),
            stroke_color,
            stroke_thickness,
        );
    }
    if let Some((start, end, arrow)) = state.pending_line() {
        let start_client = to_client_point(start, local_virtual_rect);
        let end_client = to_client_point(end, local_virtual_rect);
        if arrow {
            draw_arrow_overlay_aa(
                mem_dc,
                &mut aa_scratch,
                start_client,
                end_client,
                state.stroke_color(),
                stroke_thickness,
            );
        } else {
            draw_line_overlay_aa(
                mem_dc,
                &mut aa_scratch,
                start_client,
                end_client,
                state.stroke_color(),
                stroke_thickness,
            );
        }
    }
    if let Some(pending) = state.pending_ellipse() {
        draw_ellipse_outline_overlay(
            mem_dc,
            to_client_rect(pending, local_virtual_rect),
            stroke_color,
            stroke_thickness,
        );
    }
    if let Some(pending) = state.pending_text() {
        let rect = to_client_rect(pending, local_virtual_rect);
        frame_thick_color(mem_dc, rect, stroke_color, 1);
        draw_text_overlay(mem_dc, rect, "Sample text", stroke_color);
    }
    if let Some(pending) = state.pending_pixelate() {
        if let Some(snapshot) = state.selection_snapshot.as_ref() {
            draw_pixelate_overlay(
                mem_dc,
                snapshot,
                state.selection,
                pending,
                local_virtual_rect,
                state.pixelate_block_size(),
            );
        }
        frame_thick_color(
            mem_dc,
            to_client_rect(pending, local_virtual_rect),
            stroke_color,
            1,
        );
    }
    if let Some(pending) = state.pending_blur() {
        if let Some(snapshot) = state.selection_snapshot.as_ref() {
            draw_blur_overlay(
                mem_dc,
                snapshot,
                state.selection,
                pending,
                local_virtual_rect,
                state.blur_radius(),
            );
        }
        frame_thick_color(
            mem_dc,
            to_client_rect(pending, local_virtual_rect),
            stroke_color,
            1,
        );
    }
    if let Some(selected_idx) = state.selected_annotation
        && let Some(bounds) = state
            .annotations
            .get(selected_idx)
            .and_then(annotation_bounds_abs)
    {
        let selected_rect = to_client_rect(bounds, local_virtual_rect);
        if state.text_commit_feedback == Some(selected_idx) {
            let flash_rect = RECT {
                left: selected_rect.left - 2,
                top: selected_rect.top - 2,
                right: selected_rect.right + 2,
                bottom: selected_rect.bottom + 2,
            };
            frame_thick_color(mem_dc, flash_rect, state.text_commit_feedback_color, 2);
        }
        frame_thick_color(mem_dc, selected_rect, rgb(255, 255, 255), 1);
        if let Some(resize_bounds) = state
            .annotations
            .get(selected_idx)
            .and_then(annotation_resize_rect_abs)
        {
            for (_, h) in handle_rects(to_client_rect(resize_bounds, local_virtual_rect)) {
                draw_handle_overlay_aa(mem_dc, &mut aa_scratch, h);
            }
        }
        if let Some(Annotation::Line(line)) = state.annotations.get(selected_idx) {
            let start = to_client_point(line.start_abs, local_virtual_rect);
            let end = to_client_point(line.end_abs, local_virtual_rect);
            for h in [handle_rect(start.x, start.y), handle_rect(end.x, end.y)] {
                draw_handle_overlay_aa(mem_dc, &mut aa_scratch, h);
            }
        }
        if let Some(Annotation::Marker(marker)) = state.annotations.get(selected_idx)
            && let (Some(start), Some(end)) = (
                marker.points_abs.first().copied(),
                marker.points_abs.last().copied(),
            )
        {
            let start_client = to_client_point(start, local_virtual_rect);
            let end_client = to_client_point(end, local_virtual_rect);
            for h in [
                handle_rect(start_client.x, start_client.y),
                handle_rect(end_client.x, end_client.y),
            ] {
                draw_handle_overlay_aa(mem_dc, &mut aa_scratch, h);
            }
        }
    }

    frame_thick(mem_dc, selection, sel_brush, 2);
    for (_, h) in handle_rects(selection) {
        draw_handle_overlay_aa(mem_dc, &mut aa_scratch, h);
    }

    let selection_full = to_client_rect(state.selection, state.virtual_rect);
    let bar = offset_toolbar_layout(
        toolbar_layout(selection_full, client_full),
        dirty.left,
        dirty.top,
    );
    let mut slint_toolbar = state.slint_toolbar.borrow_mut();
    if slint_toolbar.is_none() {
        *slint_toolbar = ToolbarSlintRenderer::new();
    }
    if let Some(toolbar) = slint_toolbar.as_mut() {
        toolbar.draw(
            mem_dc,
            bar.panel,
            state.toolbar_hover,
            state.toolbar_pressed,
            toolbar_footer_text(state),
        );
    }
    if let Some(picker) = state.radial_color_picker {
        let mut local_picker = picker;
        local_picker.center = offset_point(picker.center, dirty.left, dirty.top);
        draw_radial_color_picker(
            mem_dc,
            local_picker,
            state.stroke_color_idx,
            &state.annotation_colors,
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
    cleanup_paint_objects(&[clear, sel_brush]);
}

fn cleanup_paint_objects(brushes: &[windows::Win32::Graphics::Gdi::HBRUSH]) {
    for brush in brushes {
        unsafe {
            let _ = DeleteObject(*brush);
        }
    }
}

unsafe fn set_layer_mode(hwnd: HWND, tool: Tool) -> windows::core::Result<()> {
    let has_annotations = unsafe {
        state_ref(hwnd)
            .map(|state| !state.annotations.is_empty())
            .unwrap_or(false)
    };
    let use_color_key = should_use_color_key(tool, has_annotations);
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

fn should_use_color_key(tool: Tool, has_annotations: bool) -> bool {
    let _ = (tool, has_annotations);
    false
}

fn tool_switch_needs_prepaint(from: Tool, to: Tool, has_annotations: bool) -> bool {
    should_use_color_key(from, has_annotations) && !should_use_color_key(to, has_annotations)
}

fn should_render_selection_in_chrome(state: &State) -> bool {
    if state.frozen_desktop.is_none() || state.selection_snapshot.is_none() {
        return false;
    }
    if state.tool == Tool::Marker || state.pending_marker().is_some() {
        return false;
    }
    !state
        .annotations
        .iter()
        .any(|annotation| matches!(annotation, Annotation::Marker(_)))
}

fn should_show_selection_size_badge(_state: &State) -> bool {
    false
}

fn draw_selection_size_badge(
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

    let label = format!("{width:04} x {height:04}");
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
            DT_CALCRECT | DT_SINGLELINE | DT_LEFT,
        );
    }

    let text_w = (text_rect.right - text_rect.left).max(1);
    let text_h = (text_rect.bottom - text_rect.top + SIZE_BADGE_TEXT_EXTRA_H).max(1);
    let accent_w = (text_h + 14).max(24);
    let badge_w = text_w + accent_w + (SIZE_BADGE_PAD_X * 2) + 8;
    let badge_h = text_h + (SIZE_BADGE_PAD_Y * 2);
    let center_x = selection.left + (selection_w / 2);
    let center_y = selection.top + (selection_h / 2);
    let badge = RECT {
        left: center_x - (badge_w / 2),
        top: center_y - (badge_h / 2),
        right: center_x + ((badge_w + 1) / 2),
        bottom: center_y + ((badge_h + 1) / 2),
    };
    draw_lcars_hbar(hdc, badge, SIZE_BADGE_BG, SIZE_BADGE_BORDER, true, false);

    let accent = RECT {
        left: badge.left + 4,
        top: badge.top + 4,
        right: (badge.left + 4 + accent_w).min(badge.right - SIZE_BADGE_PAD_X - 20),
        bottom: badge.bottom - 4,
    };
    draw_lcars_vbar(hdc, accent, LCARS_ROSE, SIZE_BADGE_BORDER, true, true);

    let mut accent_text = "SZ".encode_utf16().collect::<Vec<u16>>();
    let mut accent_rect = accent;
    unsafe {
        let _ = SetTextColor(hdc, LCARS_TEXT_DARK);
        let _ = DrawTextW(
            hdc,
            &mut accent_text,
            &mut accent_rect,
            DT_CENTER | DT_SINGLELINE | DT_VCENTER,
        );
    }

    let mut draw_rect = RECT {
        left: accent.right + 8,
        top: badge.top + SIZE_BADGE_PAD_Y,
        right: badge.right - SIZE_BADGE_PAD_X,
        bottom: badge.bottom - SIZE_BADGE_PAD_Y,
    };
    let mut wide_draw = label.encode_utf16().collect::<Vec<u16>>();
    unsafe {
        let _ = SetTextColor(hdc, LCARS_TEXT_DARK);
        let _ = DrawTextW(
            hdc,
            &mut wide_draw,
            &mut draw_rect,
            DT_RIGHT | DT_SINGLELINE | DT_VCENTER,
        );
    }
}

fn capture_selection_snapshot(
    selection: RectPx,
    frozen_desktop: Option<FrozenDesktopRef>,
) -> Option<SelectionSnapshot> {
    if let Some(frozen) = frozen_desktop {
        return selection_snapshot_from_frozen(selection, &frozen);
    }

    let frame = capture::capture_rect(selection).ok()?;
    selection_snapshot_from_capture_frame(&frame)
}

fn selection_snapshot_from_capture_frame(
    frame: &capture::CaptureFrame,
) -> Option<SelectionSnapshot> {
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

fn selection_snapshot_from_precomputed(
    snapshot: PrecomputedSelectionSnapshot,
) -> Option<SelectionSnapshot> {
    if snapshot.width <= 0 || snapshot.height <= 0 {
        return None;
    }
    let expected = snapshot.width as usize * snapshot.height as usize * 4;
    if snapshot.bgra_pixels.len() != expected {
        return None;
    }
    Some(SelectionSnapshot {
        width: snapshot.width,
        height: snapshot.height,
        bgra_pixels: snapshot.bgra_pixels,
    })
}

fn frozen_desktop_ref_from_capture_frame(
    frame: &capture::CaptureFrame,
) -> Option<FrozenDesktopRef> {
    let width = i32::try_from(frame.image.width()).ok()?;
    let height = i32::try_from(frame.image.height()).ok()?;
    if width <= 0 || height <= 0 {
        return None;
    }
    let rgba = frame.image.as_raw();
    let expected_len = width as usize * height as usize * 4;
    if rgba.len() != expected_len {
        return None;
    }
    Some(FrozenDesktopRef {
        bounds: frame.bounds,
        width,
        height,
        rgba_ptr: rgba.as_ptr(),
        rgba_len: rgba.len(),
    })
}

fn selection_snapshot_from_frozen(
    selection: RectPx,
    frozen: &FrozenDesktopRef,
) -> Option<SelectionSnapshot> {
    if selection.left < frozen.bounds.left
        || selection.top < frozen.bounds.top
        || selection.right > frozen.bounds.right
        || selection.bottom > frozen.bounds.bottom
    {
        return None;
    }

    let width = selection.width();
    let height = selection.height();
    if width <= 0 || height <= 0 {
        return None;
    }

    let expected_len = frozen.width as usize * frozen.height as usize * 4;
    if frozen.rgba_ptr.is_null() || frozen.rgba_len < expected_len {
        return None;
    }
    let src_left = selection.left - frozen.bounds.left;
    let src_top = selection.top - frozen.bounds.top;
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let src_stride = frozen.width as usize * 4;
    let row_bytes = width as usize * 4;
    // Safety: `rgba_ptr` points into the frozen frame image buffer, which
    // lives for the full duration of `edit_region` while this snapshot is used.
    let rgba = unsafe { std::slice::from_raw_parts(frozen.rgba_ptr, frozen.rgba_len) };
    for row in 0..height as usize {
        let src_row = (src_top as usize + row) * src_stride;
        let src_off = src_row + (src_left as usize * 4);
        let dst_off = row * row_bytes;
        let src_slice = &rgba[src_off..src_off + row_bytes];
        let dst_slice = &mut pixels[dst_off..dst_off + row_bytes];
        for (src_px, dst_px) in src_slice.chunks_exact(4).zip(dst_slice.chunks_exact_mut(4)) {
            dst_px[0] = src_px[2];
            dst_px[1] = src_px[1];
            dst_px[2] = src_px[0];
            dst_px[3] = 255;
        }
    }

    Some(SelectionSnapshot {
        width,
        height,
        bgra_pixels: pixels,
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

fn ensure_region_editor_slint_platform() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let _ = slint::platform::set_platform(Box::new(RegionEditorSlintPlatform));
    });
}


fn toolbar_button_base_rgb(hit: ToolbarHit) -> [u8; 3] {
    match hit {
        ToolbarHit::Select => UI_BUTTON_ORANGE_RGB,
        ToolbarHit::Rect => UI_BUTTON_BLUE_RGB,
        ToolbarHit::Ellipse => UI_BUTTON_ORANGE_RGB,
        ToolbarHit::Line => UI_BUTTON_ORANGE_RGB,
        ToolbarHit::Arrow => UI_BUTTON_BLUE_RGB,
        ToolbarHit::Marker => UI_BUTTON_PINK_RGB,
        ToolbarHit::Text => UI_BUTTON_PINK_RGB,
        ToolbarHit::Pixelate => UI_BUTTON_PINK_RGB,
        ToolbarHit::Blur => UI_BUTTON_BLUE_RGB,
        ToolbarHit::Copy => UI_BUTTON_BLUE_RGB,
        ToolbarHit::Save => UI_BUTTON_ORANGE_RGB,
        ToolbarHit::CopyAndSave => UI_BUTTON_ORANGE_RGB,
        ToolbarHit::Pin => UI_BUTTON_PINK_RGB,
        ToolbarHit::Panel => [0, 0, 0],
    }
}

fn toolbar_button_fill_color(
    hit: ToolbarHit,
    hovered: Option<ToolbarHit>,
    pressed: Option<ToolbarHit>,
) -> Color {
    let base = toolbar_button_base_rgb(hit);
    let fill = if pressed == Some(hit) {
        mix_rgb(base, [255, 255, 255], 0.16)
    } else if hovered == Some(hit) {
        mix_rgb(base, [255, 255, 255], 0.10)
    } else {
        base
    };
    Color::from_rgb_u8(fill[0], fill[1], fill[2])
}

fn toolbar_footer_text(state: &State) -> slint::SharedString {
    format!(
        "{} / {} x {}",
        tool_display_name(state.tool),
        state.selection.width(),
        state.selection.height()
    )
    .into()
}

fn premultiplied_rgba_to_bgra_bytes(pixels: &[PremultipliedRgbaColor], bgra: &mut Vec<u8>) {
    bgra.clear();
    bgra.reserve(pixels.len() * 4);
    for pixel in pixels {
        bgra.push(pixel.blue);
        bgra.push(pixel.green);
        bgra.push(pixel.red);
        bgra.push(pixel.alpha);
    }
}

fn alpha_blit_premultiplied_rgba(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    pixels: &[PremultipliedRgbaColor],
    bgra: &mut Vec<u8>,
) {
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return;
    }
    let expected_len = (width as usize).saturating_mul(height as usize);
    if pixels.len() < expected_len {
        return;
    }

    premultiplied_rgba_to_bgra_bytes(&pixels[..expected_len], bgra);

    let source_dc = unsafe { CreateCompatibleDC(hdc) };
    if source_dc.0.is_null() {
        return;
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
    let Ok(dib) =
        (unsafe { CreateDIBSection(source_dc, &bitmap, DIB_RGB_COLORS, &mut bits, None, 0) })
    else {
        unsafe {
            let _ = DeleteDC(source_dc);
        }
        return;
    };
    if dib.0.is_null() || bits.is_null() {
        unsafe {
            let _ = DeleteObject(dib);
            let _ = DeleteDC(source_dc);
        }
        return;
    }

    unsafe {
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits.cast::<u8>(), bgra.len());
        let old_bitmap = SelectObject(source_dc, dib);
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = AlphaBlend(
            hdc, rect.left, rect.top, width, height, source_dc, 0, 0, width, height, blend,
        );
        let _ = SelectObject(source_dc, old_bitmap);
        let _ = DeleteObject(dib);
        let _ = DeleteDC(source_dc);
    }
}

impl ToolbarSlintRenderer {
    fn new() -> Option<Self> {
        ensure_region_editor_slint_platform();
        // This toolbar is composited into a fresh GDI bitmap every paint. Use a
        // full Slint redraw as well; re-used-buffer partial repainting causes
        // unchanged regions to disappear when we clear the offscreen surface.
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        PENDING_SLINT_WINDOW_ADAPTER.with(|slot| {
            *slot.borrow_mut() = Some(window.clone());
        });
        let ui = ChromeToolbarUi::new().ok()?;
        ui.show().ok()?;

        Some(Self {
            window,
            ui,
            pixels: Vec::new(),
            bgra: Vec::new(),
            width: 0,
            height: 0,
        })
    }

    fn draw(
        &mut self,
        hdc: windows::Win32::Graphics::Gdi::HDC,
        rect: RECT,
        hovered: Option<ToolbarHit>,
        pressed: Option<ToolbarHit>,
        footer_text: slint::SharedString,
    ) {
        let width = (rect.right - rect.left).max(1) as u32;
        let height = (rect.bottom - rect.top).max(1) as u32;
        let pixel_count = (width as usize).saturating_mul(height as usize);
        if self.width != width || self.height != height {
            self.width = width;
            self.height = height;
            self.window
                .set_size(slint::PhysicalSize::new(width, height));
            self.pixels
                .resize(pixel_count, PremultipliedRgbaColor::default());
        } else if self.pixels.len() != pixel_count {
            self.pixels
                .resize(pixel_count, PremultipliedRgbaColor::default());
        }

        self.ui
            .set_ui_scale(width as f32 / SLINT_TOOLBAR_BASE_W as f32);
        self.ui.set_footer_text(footer_text);
        self.ui.set_select_fill(toolbar_button_fill_color(
            ToolbarHit::Select,
            hovered,
            pressed,
        ));
        self.ui.set_rect_fill(toolbar_button_fill_color(
            ToolbarHit::Rect,
            hovered,
            pressed,
        ));
        self.ui.set_ellipse_fill(toolbar_button_fill_color(
            ToolbarHit::Ellipse,
            hovered,
            pressed,
        ));
        self.ui.set_line_fill(toolbar_button_fill_color(
            ToolbarHit::Line,
            hovered,
            pressed,
        ));
        self.ui.set_arrow_fill(toolbar_button_fill_color(
            ToolbarHit::Arrow,
            hovered,
            pressed,
        ));
        self.ui.set_marker_fill(toolbar_button_fill_color(
            ToolbarHit::Marker,
            hovered,
            pressed,
        ));
        self.ui.set_text_fill(toolbar_button_fill_color(
            ToolbarHit::Text,
            hovered,
            pressed,
        ));
        self.ui.set_pixelate_fill(toolbar_button_fill_color(
            ToolbarHit::Pixelate,
            hovered,
            pressed,
        ));
        self.ui.set_blur_fill(toolbar_button_fill_color(
            ToolbarHit::Blur,
            hovered,
            pressed,
        ));
        self.ui.set_copy_fill(toolbar_button_fill_color(
            ToolbarHit::Copy,
            hovered,
            pressed,
        ));
        self.ui.set_save_fill(toolbar_button_fill_color(
            ToolbarHit::Save,
            hovered,
            pressed,
        ));
        self.ui.set_copy_save_fill(toolbar_button_fill_color(
            ToolbarHit::CopyAndSave,
            hovered,
            pressed,
        ));
        self.ui
            .set_pin_fill(toolbar_button_fill_color(ToolbarHit::Pin, hovered, pressed));

        let _ = self.window.draw_if_needed(|renderer| {
            self.pixels.fill(PremultipliedRgbaColor::default());
            renderer.render(self.pixels.as_mut_slice(), width as usize);
        });

        alpha_blit_premultiplied_rgba(hdc, rect, &self.pixels, &mut self.bgra);
    }
}

fn load_toolbar_icons() -> ToolbarIcons {
    static ICONS: OnceLock<ToolbarIcons> = OnceLock::new();
    ICONS
        .get_or_init(|| ToolbarIcons {
            select: render_toolbar_icon(
                include_str!("assets/icons/tool-select.svg"),
                TOOL_ICON_SIZE,
            ),
            rectangle: render_toolbar_icon(
                include_str!("assets/icons/tool-rectangle.svg"),
                TOOL_ICON_SIZE,
            ),
            ellipse: render_toolbar_icon(
                include_str!("assets/icons/tool-ellipse.svg"),
                TOOL_ICON_SIZE,
            ),
            line: render_toolbar_icon(include_str!("assets/icons/tool-line.svg"), TOOL_ICON_SIZE),
            arrow: render_toolbar_icon(include_str!("assets/icons/tool-arrow.svg"), TOOL_ICON_SIZE),
            marker: render_toolbar_icon(
                include_str!("assets/icons/tool-marker.svg"),
                TOOL_ICON_SIZE,
            ),
            text: render_toolbar_icon(include_str!("assets/icons/tool-text.svg"), TOOL_ICON_SIZE),
            pixelate: render_toolbar_icon(
                include_str!("assets/icons/tool-pixelate.svg"),
                TOOL_ICON_SIZE,
            ),
            blur: render_toolbar_icon(include_str!("assets/icons/tool-blur.svg"), TOOL_ICON_SIZE),
            copy: render_toolbar_icon(
                include_str!("assets/icons/action-copy.svg"),
                ACTION_ICON_SIZE,
            ),
            save: render_toolbar_icon(
                include_str!("assets/icons/action-save.svg"),
                ACTION_ICON_SIZE,
            ),
            copy_save: render_toolbar_icon(
                include_str!("assets/icons/action-copy-save.svg"),
                ACTION_ICON_SIZE,
            ),
            pin: render_toolbar_icon(
                include_str!("assets/icons/action-pin.svg"),
                ACTION_ICON_SIZE,
            ),
        })
        .clone()
}

fn render_toolbar_icon(svg: &str, size: u32) -> Option<IconMask> {
    if size == 0 || svg.trim().is_empty() {
        return None;
    }
    let image = slint::Image::load_from_svg_data(svg.as_bytes()).ok()?;
    let rgba = image.to_rgba8()?;
    let src_w = rgba.width() as usize;
    let src_h = rgba.height() as usize;
    if src_w == 0 || src_h == 0 {
        return None;
    }
    let mut src_alpha = Vec::with_capacity(src_w * src_h);
    for px in rgba.as_slice().iter() {
        src_alpha.push(px.a);
    }

    let target_w = size as usize;
    let target_h = size as usize;
    let mut alpha = Vec::with_capacity(target_w * target_h);
    for y in 0..target_h {
        let src_y = ((y as f32 / target_h as f32) * src_h as f32)
            .floor()
            .clamp(0.0, (src_h.saturating_sub(1)) as f32) as usize;
        for x in 0..target_w {
            let src_x = ((x as f32 / target_w as f32) * src_w as f32)
                .floor()
                .clamp(0.0, (src_w.saturating_sub(1)) as f32) as usize;
            alpha.push(src_alpha[src_y * src_w + src_x]);
        }
    }
    Some(IconMask {
        width: target_w as i32,
        height: target_h as i32,
        alpha,
    })
}

fn draw_icon_button(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    icon: Option<&IconMask>,
    caption: &str,
    fill: COLORREF,
    border: COLORREF,
    icon_color: COLORREF,
) {
    let width = rect.right - rect.left;
    let wide_button = width >= 110;
    let mut text_rect = RECT {
        left: rect.left + 12,
        top: rect.top + 1,
        right: rect.right - 10,
        bottom: rect.bottom,
    };

    if wide_button {
        draw_lcars_hbar(hdc, rect, fill, border, true, false);
        let shoulder = RECT {
            left: rect.left + 6,
            top: rect.top + 4,
            right: (rect.left + 22).min(rect.right - 48),
            bottom: rect.bottom - 4,
        };
        let stripe = RECT {
            left: shoulder.right + 6,
            top: rect.top + 4,
            right: rect.right - 40,
            bottom: (rect.top + 9).min(rect.bottom - 6),
        };
        let icon_chip = RECT {
            left: rect.right - 34,
            top: rect.top + 4,
            right: rect.right - 4,
            bottom: rect.bottom - 4,
        };
        draw_lcars_vbar(hdc, shoulder, lighten_color(fill, 0.16), border, true, true);
        if stripe.right > stripe.left {
            draw_lcars_hbar(hdc, stripe, darken_color(fill, 0.16), border, false, true);
        }
        draw_lcars_hbar(
            hdc,
            icon_chip,
            darken_color(fill, 0.34),
            border,
            false,
            true,
        );
        if let Some(icon) = icon {
            draw_icon_mask(hdc, icon_chip, icon, LCARS_TEXT_LIGHT);
        }
        text_rect = RECT {
            left: shoulder.right + 8,
            top: rect.top + 1,
            right: icon_chip.left - 8,
            bottom: rect.bottom,
        };
    } else {
        draw_lcars_hbar(hdc, rect, fill, border, true, false);
        let stripe = RECT {
            left: rect.left + 16,
            top: rect.top + 4,
            right: rect.right - 10,
            bottom: (rect.top + 9).min(rect.bottom - 6),
        };
        if stripe.right > stripe.left {
            draw_lcars_hbar(hdc, stripe, darken_color(fill, 0.14), border, false, true);
        }
    }

    if !caption.is_empty() {
        let mut wide = caption.encode_utf16().collect::<Vec<u16>>();
        unsafe {
            let _ = SetTextColor(hdc, icon_color);
            let _ = DrawTextW(
                hdc,
                &mut wide,
                &mut text_rect,
                DT_LEFT | DT_SINGLELINE | DT_VCENTER,
            );
        }
    }
}

fn draw_icon_mask(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    icon: &IconMask,
    color: COLORREF,
) {
    if icon.width <= 0 || icon.height <= 0 || icon.alpha.is_empty() {
        return;
    }
    let draw_x = rect.left + (((rect.right - rect.left) - icon.width).max(0) / 2);
    let draw_y = rect.top + (((rect.bottom - rect.top) - icon.height).max(0) / 2);

    let red = (color.0 & 0xFF) as u8;
    let green = ((color.0 >> 8) & 0xFF) as u8;
    let blue = ((color.0 >> 16) & 0xFF) as u8;

    let px_count = (icon.width as usize) * (icon.height as usize);
    let mut bgra = vec![0u8; px_count * 4];
    for (i, alpha) in icon.alpha.iter().copied().enumerate().take(px_count) {
        let a = u16::from(alpha);
        let idx = i * 4;
        bgra[idx] = ((u16::from(blue) * a) / 255) as u8;
        bgra[idx + 1] = ((u16::from(green) * a) / 255) as u8;
        bgra[idx + 2] = ((u16::from(red) * a) / 255) as u8;
        bgra[idx + 3] = alpha;
    }

    let mem_dc = unsafe { CreateCompatibleDC(hdc) };
    if mem_dc.0.is_null() {
        return;
    }

    let mut bitmap = BITMAPINFO::default();
    bitmap.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: icon.width,
        biHeight: -icon.height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut::<c_void>();
    let dib = unsafe { CreateDIBSection(mem_dc, &bitmap, DIB_RGB_COLORS, &mut bits, None, 0) };
    let Ok(dib) = dib else {
        unsafe {
            let _ = DeleteDC(mem_dc);
        }
        return;
    };
    if dib.0.is_null() || bits.is_null() {
        unsafe {
            let _ = DeleteObject(dib);
            let _ = DeleteDC(mem_dc);
        }
        return;
    }

    unsafe {
        ptr::copy_nonoverlapping(bgra.as_ptr(), bits.cast::<u8>(), bgra.len());
    }

    let old_bitmap = unsafe { SelectObject(mem_dc, dib) };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    unsafe {
        let _ = AlphaBlend(
            hdc,
            draw_x,
            draw_y,
            icon.width,
            icon.height,
            mem_dc,
            0,
            0,
            icon.width,
            icon.height,
            blend,
        );
        let _ = SelectObject(mem_dc, old_bitmap);
        let _ = DeleteObject(dib);
        let _ = DeleteDC(mem_dc);
    }
}

fn draw_gradient_button(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    fill: COLORREF,
    border: COLORREF,
    radius: i32,
) {
    let (start, end) = button_gradient_stops(fill);
    if !draw_gradient_rounded_box(hdc, rect, start, end, border, radius) {
        draw_rounded_box(hdc, rect, fill, border, radius);
    }
}

fn draw_gradient_rect(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    fill: COLORREF,
    border: COLORREF,
) {
    let (start, end) = button_gradient_stops(fill);
    if !draw_gradient_fill_rect(hdc, rect, start, end, border) {
        draw_rounded_box(hdc, rect, fill, border, 2);
    }
}

fn draw_lcars_hbar(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    fill: COLORREF,
    border: COLORREF,
    round_left: bool,
    round_right: bool,
) {
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return;
    }

    let cap = height.min(width).max(2);
    let overlap = (cap / 2).max(1);
    match (round_left, round_right) {
        (true, true) => draw_gradient_button(hdc, rect, fill, border, cap),
        (false, false) => draw_gradient_rect(hdc, rect, fill, border),
        (true, false) => {
            let body = RECT {
                left: rect.left + overlap,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            };
            if body.right > body.left {
                draw_gradient_rect(hdc, body, fill, border);
            }
            let left_cap = RECT {
                left: rect.left,
                top: rect.top,
                right: (rect.left + cap).min(rect.right),
                bottom: rect.bottom,
            };
            draw_gradient_button(hdc, left_cap, fill, border, cap);
        }
        (false, true) => {
            let body = RECT {
                left: rect.left,
                top: rect.top,
                right: rect.right - overlap,
                bottom: rect.bottom,
            };
            if body.right > body.left {
                draw_gradient_rect(hdc, body, fill, border);
            }
            let right_cap = RECT {
                left: (rect.right - cap).max(rect.left),
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            };
            draw_gradient_button(hdc, right_cap, fill, border, cap);
        }
    }
}

fn draw_lcars_vbar(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    fill: COLORREF,
    border: COLORREF,
    round_top: bool,
    round_bottom: bool,
) {
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return;
    }

    let cap = width.min(height).max(2);
    let overlap = (cap / 2).max(1);
    match (round_top, round_bottom) {
        (true, true) => draw_gradient_button(hdc, rect, fill, border, cap),
        (false, false) => draw_gradient_rect(hdc, rect, fill, border),
        (true, false) => {
            let body = RECT {
                left: rect.left,
                top: rect.top + overlap,
                right: rect.right,
                bottom: rect.bottom,
            };
            if body.bottom > body.top {
                draw_gradient_rect(hdc, body, fill, border);
            }
            let top_cap = RECT {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: (rect.top + cap).min(rect.bottom),
            };
            draw_gradient_button(hdc, top_cap, fill, border, cap);
        }
        (false, true) => {
            let body = RECT {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom - overlap,
            };
            if body.bottom > body.top {
                draw_gradient_rect(hdc, body, fill, border);
            }
            let bottom_cap = RECT {
                left: rect.left,
                top: (rect.bottom - cap).max(rect.top),
                right: rect.right,
                bottom: rect.bottom,
            };
            draw_gradient_button(hdc, bottom_cap, fill, border, cap);
        }
    }
}

fn draw_gradient_fill_rect(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    start: COLORREF,
    end: COLORREF,
    border: COLORREF,
) -> bool {
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return false;
    }

    let bgra = build_diagonal_gradient_bgra(width, height, start, end);
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

    unsafe {
        let _ = StretchDIBits(
            hdc,
            rect.left,
            rect.top,
            width,
            height,
            0,
            0,
            width,
            height,
            Some(bgra.as_ptr().cast::<c_void>()),
            &bitmap,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }

    let border_brush = unsafe { CreateSolidBrush(border) };
    if !border_brush.0.is_null() {
        unsafe {
            let _ = FrameRect(hdc, &rect, border_brush);
            let _ = DeleteObject(border_brush);
        }
    }
    true
}

fn button_gradient_stops(fill: COLORREF) -> (COLORREF, COLORREF) {
    let base = colorref_to_rgb(fill);
    let luminance =
        ((base[0] as f32 * 0.299) + (base[1] as f32 * 0.587) + (base[2] as f32 * 0.114)) / 255.0;
    let highlight_mix = if luminance > 0.65 { 0.12 } else { 0.20 };
    let shadow_mix = if luminance > 0.65 { 0.18 } else { 0.28 };
    (
        rgb_from_array(mix_rgb(base, [255, 255, 255], highlight_mix)),
        rgb_from_array(mix_rgb(base, [0, 0, 0], shadow_mix)),
    )
}

fn draw_gradient_rounded_box(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    start: COLORREF,
    end: COLORREF,
    border: COLORREF,
    radius: i32,
) -> bool {
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return false;
    }

    let bgra = build_diagonal_gradient_bgra(width, height, start, end);
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

    let round = radius.max(2);
    let clip =
        unsafe { CreateRoundRectRgn(rect.left, rect.top, rect.right, rect.bottom, round, round) };
    if clip.0.is_null() {
        return false;
    }

    let saved_dc = unsafe { SaveDC(hdc) };
    if saved_dc == 0 {
        unsafe {
            let _ = DeleteObject(clip);
        }
        return false;
    }

    unsafe {
        let _ = SelectClipRgn(hdc, clip);
        let _ = StretchDIBits(
            hdc,
            rect.left,
            rect.top,
            width,
            height,
            0,
            0,
            width,
            height,
            Some(bgra.as_ptr().cast::<c_void>()),
            &bitmap,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }

    let border_brush = unsafe { CreateSolidBrush(border) };
    if !border_brush.0.is_null() {
        unsafe {
            let _ = FrameRgn(hdc, clip, border_brush, 1, 1);
            let _ = DeleteObject(border_brush);
        }
    }

    unsafe {
        let _ = RestoreDC(hdc, saved_dc);
        let _ = DeleteObject(clip);
    }
    true
}

fn build_diagonal_gradient_bgra(
    width: i32,
    height: i32,
    start: COLORREF,
    end: COLORREF,
) -> Vec<u8> {
    let width_usize = width.max(0) as usize;
    let height_usize = height.max(0) as usize;
    let mut bgra = vec![0u8; width_usize * height_usize * 4];
    let [start_r, start_g, start_b] = colorref_to_rgb(start);
    let [end_r, end_g, end_b] = colorref_to_rgb(end);
    let denom = ((width - 1).max(0) + (height - 1).max(0)).max(1) as f32;

    for y in 0..height_usize {
        for x in 0..width_usize {
            let t = ((x + y) as f32 / denom).clamp(0.0, 1.0);
            let idx = (y * width_usize + x) * 4;
            bgra[idx] = lerp_channel(start_b, end_b, t);
            bgra[idx + 1] = lerp_channel(start_g, end_g, t);
            bgra[idx + 2] = lerp_channel(start_r, end_r, t);
            bgra[idx + 3] = 255;
        }
    }

    bgra
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
fn create_overlay_font(height: i32, weight: i32) -> windows::Win32::Graphics::Gdi::HFONT {
    for face in [
        w!("Segoe UI Variable Text"),
        w!("Segoe UI Variable"),
        w!("Segoe UI"),
    ] {
        let font = create_overlay_font_for_face(face, height, weight);
        if !font.0.is_null() {
            return font;
        }
    }
    windows::Win32::Graphics::Gdi::HFONT::default()
}

fn create_overlay_font_for_face(
    face: PCWSTR,
    height: i32,
    weight: i32,
) -> windows::Win32::Graphics::Gdi::HFONT {
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
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

fn draw_line_overlay_aa(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    scratch: &mut AaScratch,
    start: POINT,
    end: POINT,
    color: [u8; 4],
    thickness: i32,
) {
    draw_segments_overlay_aa(hdc, scratch, &[(start, end)], color, thickness);
}

fn draw_arrow_overlay_aa(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    scratch: &mut AaScratch,
    start: POINT,
    end: POINT,
    color: [u8; 4],
    thickness: i32,
) {
    let Some((left, right)) = arrow_head_points(start, end, thickness) else {
        draw_line_overlay_aa(hdc, scratch, start, end, color, thickness);
        return;
    };
    draw_segments_overlay_aa(
        hdc,
        scratch,
        &[(start, end), (end, left), (end, right)],
        color,
        thickness,
    );
}

fn draw_segments_overlay_aa(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    scratch: &mut AaScratch,
    segments: &[(POINT, POINT)],
    color: [u8; 4],
    thickness: i32,
) {
    let Some((first_start, first_end)) = segments.first().copied() else {
        return;
    };
    let pad = thickness.max(1) + 2;
    let mut min_x = first_start.x.min(first_end.x);
    let mut min_y = first_start.y.min(first_end.y);
    let mut max_x = first_start.x.max(first_end.x);
    let mut max_y = first_start.y.max(first_end.y);
    for (start, end) in segments.iter().copied().skip(1) {
        min_x = min_x.min(start.x.min(end.x));
        min_y = min_y.min(start.y.min(end.y));
        max_x = max_x.max(start.x.max(end.x));
        max_y = max_y.max(start.y.max(end.y));
    }
    let left = min_x - pad;
    let top = min_y - pad;
    let right = max_x + pad + 1;
    let bottom = max_y + pad + 1;
    let width = right - left;
    let height = bottom - top;
    if width <= 0 || height <= 0 {
        return;
    }

    if !scratch.prepare(width, height) {
        return;
    }

    for (start, end) in segments {
        draw_line(
            &mut scratch.image,
            (start.x - left, start.y - top),
            (end.x - left, end.y - top),
            color,
            thickness,
        );
    }
    scratch.blit(hdc, left, top);
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

fn draw_marker_overlay_aa(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    scratch: &mut AaScratch,
    points: impl IntoIterator<Item = POINT>,
    color: [u8; 4],
    thickness: i32,
) {
    let points = points.into_iter().collect::<Vec<_>>();
    if points.len() < 2 {
        return;
    }

    let pad = thickness.max(1) + 2;
    let mut min_x = points[0].x;
    let mut min_y = points[0].y;
    let mut max_x = points[0].x;
    let mut max_y = points[0].y;
    for point in points.iter().copied().skip(1) {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }

    let left = min_x - pad;
    let top = min_y - pad;
    let right = max_x + pad + 1;
    let bottom = max_y + pad + 1;
    let width = right - left;
    let height = bottom - top;
    if width <= 0 || height <= 0 {
        return;
    }

    if !scratch.prepare(width, height) {
        return;
    }

    let mut last = points[0];
    for point in points.iter().copied().skip(1) {
        draw_line(
            &mut scratch.image,
            (last.x - left, last.y - top),
            (point.x - left, point.y - top),
            color,
            thickness,
        );
        last = point;
    }

    scratch.blit(hdc, left, top);
}

fn draw_handle_overlay_aa(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    scratch: &mut AaScratch,
    rect: RECT,
) {
    let base_w = rect.right - rect.left;
    let base_h = rect.bottom - rect.top;
    if base_w <= 0 || base_h <= 0 {
        return;
    }

    let pad = 2;
    let left = rect.left - pad;
    let top = rect.top - pad;
    let width = base_w + (pad * 2);
    let height = base_h + (pad * 2);
    if !scratch.prepare(width, height) {
        return;
    }

    let cx = (rect.left + rect.right) as f32 * 0.5 - left as f32;
    let cy = (rect.top + rect.bottom) as f32 * 0.5 - top as f32;
    let radius = (base_w.min(base_h) as f32 * 0.5).max(2.0) - 0.35;
    let border_width = 1.1f32;
    let outer_max = radius + 0.9;
    let inner_radius = (radius - border_width).max(0.5);

    for y in 0..height {
        for x in 0..width {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let dx = px - cx;
            let dy = py - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > outer_max + 0.8 {
                continue;
            }
            let outer = (outer_max - dist).clamp(0.0, 1.0);
            if outer <= 0.0 {
                continue;
            }
            let inner = (inner_radius + 0.9 - dist).clamp(0.0, 1.0);
            let border_cov = (outer - inner).clamp(0.0, 1.0);
            if border_cov > 0.0 {
                blend_pixel(&mut scratch.image, x, y, HANDLE_BORDER_COLOR, border_cov);
            }
            if inner > 0.0 {
                blend_pixel(&mut scratch.image, x, y, HANDLE_FILL_COLOR, inner);
            }
        }
    }

    scratch.blit(hdc, left, top);
}

fn draw_pixelate_overlay(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    snapshot: &SelectionSnapshot,
    selection_abs: RectPx,
    rect_abs: RectPx,
    virtual_rect: RectPx,
    block: i32,
) {
    let left = rect_abs.left.clamp(selection_abs.left, selection_abs.right);
    let top = rect_abs.top.clamp(selection_abs.top, selection_abs.bottom);
    let right = rect_abs
        .right
        .clamp(selection_abs.left, selection_abs.right);
    let bottom = rect_abs
        .bottom
        .clamp(selection_abs.top, selection_abs.bottom);
    if right <= left || bottom <= top {
        return;
    }

    let src_left = (left - selection_abs.left).clamp(0, snapshot.width);
    let src_top = (top - selection_abs.top).clamp(0, snapshot.height);
    let src_right = (right - selection_abs.left).clamp(0, snapshot.width);
    let src_bottom = (bottom - selection_abs.top).clamp(0, snapshot.height);
    let src_w = src_right - src_left;
    let src_h = src_bottom - src_top;
    if src_w <= 0 || src_h <= 0 {
        return;
    }

    let width_usize = src_w as usize;
    let height_usize = src_h as usize;
    let Some(pixel_len) = width_usize
        .checked_mul(height_usize)
        .and_then(|v| v.checked_mul(4))
    else {
        return;
    };
    let mut pixelated = vec![0u8; pixel_len];
    let snap_width = snapshot.width as usize;
    let block = block.max(1) as usize;

    for by in (0..height_usize).step_by(block) {
        let bh = block.min(height_usize - by);
        for bx in (0..width_usize).step_by(block) {
            let bw = block.min(width_usize - bx);

            let mut sum_b = 0u64;
            let mut sum_g = 0u64;
            let mut sum_r = 0u64;
            let mut sum_a = 0u64;
            let mut count = 0u64;

            for sy in 0..bh {
                let src_y = src_top as usize + by + sy;
                let row_start = ((src_y * snap_width) + src_left as usize + bx) * 4;
                for sx in 0..bw {
                    let idx = row_start + (sx * 4);
                    sum_b += snapshot.bgra_pixels[idx] as u64;
                    sum_g += snapshot.bgra_pixels[idx + 1] as u64;
                    sum_r += snapshot.bgra_pixels[idx + 2] as u64;
                    sum_a += snapshot.bgra_pixels[idx + 3] as u64;
                    count += 1;
                }
            }
            if count == 0 {
                continue;
            }
            let avg_b = (sum_b / count) as u8;
            let avg_g = (sum_g / count) as u8;
            let avg_r = (sum_r / count) as u8;
            let avg_a = (sum_a / count) as u8;

            for sy in 0..bh {
                let dst_row = ((by + sy) * width_usize + bx) * 4;
                for sx in 0..bw {
                    let idx = dst_row + (sx * 4);
                    pixelated[idx] = avg_b;
                    pixelated[idx + 1] = avg_g;
                    pixelated[idx + 2] = avg_r;
                    pixelated[idx + 3] = avg_a;
                }
            }
        }
    }

    let mut bitmap = BITMAPINFO::default();
    bitmap.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: src_w,
        biHeight: -src_h,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let dest = to_client_rect(
        RectPx {
            left,
            top,
            right,
            bottom,
        },
        virtual_rect,
    );
    unsafe {
        let _ = StretchDIBits(
            hdc,
            dest.left,
            dest.top,
            src_w,
            src_h,
            0,
            0,
            src_w,
            src_h,
            Some(pixelated.as_ptr().cast::<c_void>()),
            &bitmap,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }
}

fn draw_blur_overlay(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    snapshot: &SelectionSnapshot,
    selection_abs: RectPx,
    rect_abs: RectPx,
    virtual_rect: RectPx,
    radius: i32,
) {
    let left = rect_abs.left.clamp(selection_abs.left, selection_abs.right);
    let top = rect_abs.top.clamp(selection_abs.top, selection_abs.bottom);
    let right = rect_abs
        .right
        .clamp(selection_abs.left, selection_abs.right);
    let bottom = rect_abs
        .bottom
        .clamp(selection_abs.top, selection_abs.bottom);
    if right <= left || bottom <= top {
        return;
    }

    let src_left = (left - selection_abs.left).clamp(0, snapshot.width);
    let src_top = (top - selection_abs.top).clamp(0, snapshot.height);
    let src_right = (right - selection_abs.left).clamp(0, snapshot.width);
    let src_bottom = (bottom - selection_abs.top).clamp(0, snapshot.height);
    let src_w = src_right - src_left;
    let src_h = src_bottom - src_top;
    if src_w <= 0 || src_h <= 0 {
        return;
    }

    let width_usize = src_w as usize;
    let height_usize = src_h as usize;
    let Some(pixel_len) = width_usize
        .checked_mul(height_usize)
        .and_then(|v| v.checked_mul(4))
    else {
        return;
    };
    let mut source = vec![0u8; pixel_len];
    let snap_width = snapshot.width as usize;
    for y in 0..height_usize {
        let src_row = (src_top as usize + y) * snap_width;
        let src_off = (src_row + src_left as usize) * 4;
        let dst_off = y * width_usize * 4;
        source[dst_off..(dst_off + (width_usize * 4))]
            .copy_from_slice(&snapshot.bgra_pixels[src_off..(src_off + (width_usize * 4))]);
    }
    let blurred = blur_buffer_4ch(&source, width_usize, height_usize, radius.max(1) as usize);

    let mut bitmap = BITMAPINFO::default();
    bitmap.bmiHeader = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: src_w,
        biHeight: -src_h,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let dest = to_client_rect(
        RectPx {
            left,
            top,
            right,
            bottom,
        },
        virtual_rect,
    );
    unsafe {
        let _ = StretchDIBits(
            hdc,
            dest.left,
            dest.top,
            src_w,
            src_h,
            0,
            0,
            src_w,
            src_h,
            Some(blurred.as_ptr().cast::<c_void>()),
            &bitmap,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
    }
}

fn draw_text_overlay(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    text: &str,
    color: COLORREF,
) {
    if text.is_empty() {
        return;
    }
    if rect.right - rect.left < 4 || rect.bottom - rect.top < 4 {
        return;
    }
    let mut left = rect.left + TEXT_PAD;
    let mut top = rect.top + TEXT_PAD;
    let right = rect.right - TEXT_PAD;
    let bottom = rect.bottom - TEXT_PAD;
    if right <= left || bottom <= top {
        return;
    }

    let brush = unsafe { CreateSolidBrush(color) };
    if brush.0.is_null() {
        return;
    }

    unsafe {
        for ch in text.chars() {
            if ch == '\r' {
                continue;
            }
            if ch == '\n' {
                left = rect.left + TEXT_PAD;
                top += TEXT_GLYPH_H + TEXT_LINE_GAP;
                if top + TEXT_GLYPH_H > bottom {
                    break;
                }
                continue;
            }

            let advance = if ch == ' ' {
                TEXT_SPACE_ADVANCE
            } else {
                TEXT_GLYPH_ADVANCE
            };
            if left + TEXT_GLYPH_W > right {
                continue;
            }
            if top + TEXT_GLYPH_H > bottom {
                break;
            }

            if ch != ' ' {
                let glyph = glyph_5x7(ch);
                for (row, bits) in glyph.into_iter().enumerate() {
                    for col in 0..5 {
                        if (bits & (1 << (4 - col))) == 0 {
                            continue;
                        }
                        let x0 = left + (col * TEXT_SCALE);
                        let y0 = top + (row as i32 * TEXT_SCALE);
                        let px_rect = RECT {
                            left: x0,
                            top: y0,
                            right: x0 + TEXT_SCALE,
                            bottom: y0 + TEXT_SCALE,
                        };
                        let _ = FillRect(hdc, &px_rect, brush);
                    }
                }
            }

            left += advance;
            if left >= right {
                left = right;
            }
        }
        let _ = DeleteObject(brush);
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

fn constrain_resize_aspect(
    handle: ResizeHandle,
    start: RectPx,
    raw: RectPx,
    bounds: RectPx,
) -> RectPx {
    let start_w = start.width().max(MIN_RECT);
    let start_h = start.height().max(MIN_RECT);
    let ratio = start_w as f32 / start_h as f32;
    if !ratio.is_finite() || ratio <= 0.0 {
        return raw;
    }

    match handle {
        ResizeHandle::NW | ResizeHandle::NE | ResizeHandle::SW | ResizeHandle::SE => {
            let (anchor_x, x_dir) = match handle {
                ResizeHandle::NW | ResizeHandle::SW => (start.right, -1),
                ResizeHandle::NE | ResizeHandle::SE => (start.left, 1),
                _ => unreachable!(),
            };
            let (anchor_y, y_dir) = match handle {
                ResizeHandle::NW | ResizeHandle::NE => (start.bottom, -1),
                ResizeHandle::SW | ResizeHandle::SE => (start.top, 1),
                _ => unreachable!(),
            };

            let max_w = if x_dir < 0 {
                anchor_x - bounds.left
            } else {
                bounds.right - anchor_x
            };
            let max_h = if y_dir < 0 {
                anchor_y - bounds.top
            } else {
                bounds.bottom - anchor_y
            };
            if max_w < MIN_RECT || max_h < MIN_RECT {
                return raw;
            }

            let (mut w, mut h) = fit_dims_aspect(raw.width(), raw.height(), ratio);
            if w > max_w {
                w = max_w;
                h = ((w as f32) / ratio).round() as i32;
            }
            if h > max_h {
                h = max_h;
                w = ((h as f32) * ratio).round() as i32;
            }
            w = w.clamp(MIN_RECT, max_w);
            h = h.clamp(MIN_RECT, max_h);

            let (left, right) = if x_dir < 0 {
                (anchor_x - w, anchor_x)
            } else {
                (anchor_x, anchor_x + w)
            };
            let (top, bottom) = if y_dir < 0 {
                (anchor_y - h, anchor_y)
            } else {
                (anchor_y, anchor_y + h)
            };
            RectPx {
                left,
                top,
                right,
                bottom,
            }
        }
        ResizeHandle::E | ResizeHandle::W => {
            let (anchor_x, x_dir) = match handle {
                ResizeHandle::E => (start.left, 1),
                ResizeHandle::W => (start.right, -1),
                _ => unreachable!(),
            };
            let max_w = if x_dir < 0 {
                anchor_x - bounds.left
            } else {
                bounds.right - anchor_x
            };
            if max_w < MIN_RECT {
                return raw;
            }

            let mut w = raw.width().clamp(MIN_RECT, max_w);
            let max_h = bounds.height().max(MIN_RECT);
            let mut h = ((w as f32) / ratio).round() as i32;
            if h > max_h {
                h = max_h;
                w = ((h as f32) * ratio).round() as i32;
                w = w.clamp(MIN_RECT, max_w);
            } else {
                h = h.max(MIN_RECT);
            }

            let center_y = start.top + (start.height() / 2);
            let mut top = center_y - (h / 2);
            let mut bottom = top + h;
            if top < bounds.top {
                top = bounds.top;
                bottom = top + h;
            }
            if bottom > bounds.bottom {
                bottom = bounds.bottom;
                top = bottom - h;
            }

            let (left, right) = if x_dir < 0 {
                (anchor_x - w, anchor_x)
            } else {
                (anchor_x, anchor_x + w)
            };
            RectPx {
                left,
                top,
                right,
                bottom,
            }
        }
        ResizeHandle::N | ResizeHandle::S => {
            let (anchor_y, y_dir) = match handle {
                ResizeHandle::N => (start.bottom, -1),
                ResizeHandle::S => (start.top, 1),
                _ => unreachable!(),
            };
            let max_h = if y_dir < 0 {
                anchor_y - bounds.top
            } else {
                bounds.bottom - anchor_y
            };
            if max_h < MIN_RECT {
                return raw;
            }

            let mut h = raw.height().clamp(MIN_RECT, max_h);
            let max_w = bounds.width().max(MIN_RECT);
            let mut w = ((h as f32) * ratio).round() as i32;
            if w > max_w {
                w = max_w;
                h = ((w as f32) / ratio).round() as i32;
                h = h.clamp(MIN_RECT, max_h);
            } else {
                w = w.max(MIN_RECT);
            }

            let center_x = start.left + (start.width() / 2);
            let mut left = center_x - (w / 2);
            let mut right = left + w;
            if left < bounds.left {
                left = bounds.left;
                right = left + w;
            }
            if right > bounds.right {
                right = bounds.right;
                left = right - w;
            }

            let (top, bottom) = if y_dir < 0 {
                (anchor_y - h, anchor_y)
            } else {
                (anchor_y, anchor_y + h)
            };
            RectPx {
                left,
                top,
                right,
                bottom,
            }
        }
    }
}

fn fit_dims_aspect(width: i32, height: i32, ratio: f32) -> (i32, i32) {
    let mut w = width.max(MIN_RECT) as f32;
    let mut h = height.max(MIN_RECT) as f32;
    if (w / h) > ratio {
        w = h * ratio;
    } else {
        h = w / ratio;
    }
    let w = (w.round() as i32).max(MIN_RECT);
    let h = (h.round() as i32).max(MIN_RECT);
    (w, h)
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

fn default_text_rect_at(origin: POINT, bounds: RectPx) -> RectPx {
    let (fit_w, fit_h) = text_required_size("");
    let width = fit_w.max(TEXT_DEFAULT_W).min(bounds.width().max(MIN_RECT));
    let height = fit_h.max(TEXT_DEFAULT_H).min(bounds.height().max(MIN_RECT));
    let left = origin.x.clamp(bounds.left, bounds.right - width);
    let top = origin.y.clamp(bounds.top, bounds.bottom - height);
    RectPx {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

fn client_to_abs(client: POINT, bounds: RectPx) -> POINT {
    POINT {
        x: bounds.left + client.x,
        y: bounds.top + client.y,
    }
}

fn weighted_row_rects(
    left: i32,
    top: i32,
    height: i32,
    width: i32,
    gap: i32,
    weights: &[i32],
) -> Vec<RECT> {
    if weights.is_empty() {
        return Vec::new();
    }

    let gap_total = gap * weights.len().saturating_sub(1) as i32;
    let usable_width = (width - gap_total).max(weights.len() as i32);
    let total_weight = weights.iter().copied().sum::<i32>().max(1);
    let mut rects = Vec::with_capacity(weights.len());
    let mut x = left;
    let mut consumed = 0;

    for (idx, weight) in weights.iter().copied().enumerate() {
        let button_w = if idx == weights.len() - 1 {
            usable_width - consumed
        } else {
            ((usable_width * weight) / total_weight).max(1)
        };
        rects.push(RECT {
            left: x,
            top,
            right: x + button_w,
            bottom: top + height,
        });
        x += button_w + gap;
        consumed += button_w;
    }

    rects
}

fn scaled_toolbar_rect(
    origin: POINT,
    scale: f32,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
) -> RECT {
    let scaled_left = origin.x + ((left as f32) * scale).round() as i32;
    let scaled_top = origin.y + ((top as f32) * scale).round() as i32;
    let scaled_width = ((width as f32) * scale).round() as i32;
    let scaled_height = ((height as f32) * scale).round() as i32;
    RECT {
        left: scaled_left,
        top: scaled_top,
        right: scaled_left + scaled_width,
        bottom: scaled_top + scaled_height,
    }
}

fn toolbar_layout(selection: RECT, client: RECT) -> ToolbarLayout {
    let avail_w = (client.right - client.left - (BAR_MARGIN * 2)).max(1);
    let width_scale = (avail_w as f32 / SLINT_TOOLBAR_BASE_W as f32).min(1.0);
    let scale = if width_scale >= SLINT_TOOLBAR_MIN_SCALE {
        width_scale
    } else {
        width_scale.max(0.58)
    };
    let width = ((SLINT_TOOLBAR_BASE_W as f32) * scale).round() as i32;
    let panel_h = ((SLINT_TOOLBAR_BASE_H as f32) * scale).round() as i32;

    let center_x = selection.left + ((selection.right - selection.left) / 2);
    let min_left = client.left + BAR_MARGIN;
    let max_left = client.right - BAR_MARGIN - width;
    let left = if max_left < min_left {
        min_left
    } else {
        (center_x - (width / 2)).clamp(min_left, max_left)
    };
    let toolbar_above = selection.top - BAR_GAP - panel_h >= client.top + BAR_MARGIN;
    let toolbar_below = selection.bottom + BAR_GAP + panel_h <= client.bottom - BAR_MARGIN;
    let top = if toolbar_above {
        selection.top - BAR_GAP - panel_h
    } else if toolbar_below {
        selection.bottom + BAR_GAP
    } else {
        (client.top + BAR_MARGIN).min(client.bottom - BAR_MARGIN - panel_h)
    };

    let origin = POINT { x: left, y: top };
    ToolbarLayout {
        panel: scaled_toolbar_rect(
            origin,
            scale,
            0,
            0,
            SLINT_TOOLBAR_BASE_W,
            SLINT_TOOLBAR_BASE_H,
        ),
        lcars_cap: scaled_toolbar_rect(origin, scale, 0, 0, 177, 97),
        cap_footer: scaled_toolbar_rect(origin, scale, 0, 110, 160, 35),
        top_band: scaled_toolbar_rect(origin, scale, 177, 0, 507, 25),
        readout: scaled_toolbar_rect(origin, scale, 616, 120, 126, 16),
        tools_group: scaled_toolbar_rect(origin, scale, 116, 25, 332, 68),
        tools_tag: scaled_toolbar_rect(origin, scale, 122, 29, 80, 15),
        actions_group: scaled_toolbar_rect(origin, scale, 472, 25, 270, 68),
        actions_tag: scaled_toolbar_rect(origin, scale, 472, 29, 90, 15),
        select_btn: scaled_toolbar_rect(origin, scale, 120, 44, 64, 25),
        rect_btn: scaled_toolbar_rect(origin, scale, 120, 71, 64, 25),
        ellipse_btn: scaled_toolbar_rect(origin, scale, 186, 44, 64, 25),
        line_btn: scaled_toolbar_rect(origin, scale, 186, 71, 64, 25),
        arrow_btn: scaled_toolbar_rect(origin, scale, 252, 44, 64, 25),
        marker_btn: scaled_toolbar_rect(origin, scale, 252, 71, 64, 25),
        text_btn: scaled_toolbar_rect(origin, scale, 318, 44, 64, 25),
        pixelate_btn: scaled_toolbar_rect(origin, scale, 318, 71, 64, 25),
        blur_btn: scaled_toolbar_rect(origin, scale, 384, 44, 64, 25),
        copy_btn: scaled_toolbar_rect(origin, scale, 474, 43, 64, 55),
        save_btn: scaled_toolbar_rect(origin, scale, 544, 43, 64, 55),
        copy_save_btn: scaled_toolbar_rect(origin, scale, 614, 43, 64, 55),
        pin_btn: scaled_toolbar_rect(origin, scale, 687, 43, 54, 55),
    }
}
fn offset_toolbar_layout(layout: ToolbarLayout, offset_x: i32, offset_y: i32) -> ToolbarLayout {
    ToolbarLayout {
        panel: offset_rect(layout.panel, offset_x, offset_y),
        lcars_cap: offset_rect(layout.lcars_cap, offset_x, offset_y),
        cap_footer: offset_rect(layout.cap_footer, offset_x, offset_y),
        top_band: offset_rect(layout.top_band, offset_x, offset_y),
        readout: offset_rect(layout.readout, offset_x, offset_y),
        tools_group: offset_rect(layout.tools_group, offset_x, offset_y),
        tools_tag: offset_rect(layout.tools_tag, offset_x, offset_y),
        actions_group: offset_rect(layout.actions_group, offset_x, offset_y),
        actions_tag: offset_rect(layout.actions_tag, offset_x, offset_y),
        select_btn: offset_rect(layout.select_btn, offset_x, offset_y),
        rect_btn: offset_rect(layout.rect_btn, offset_x, offset_y),
        ellipse_btn: offset_rect(layout.ellipse_btn, offset_x, offset_y),
        line_btn: offset_rect(layout.line_btn, offset_x, offset_y),
        arrow_btn: offset_rect(layout.arrow_btn, offset_x, offset_y),
        marker_btn: offset_rect(layout.marker_btn, offset_x, offset_y),
        text_btn: offset_rect(layout.text_btn, offset_x, offset_y),
        pixelate_btn: offset_rect(layout.pixelate_btn, offset_x, offset_y),
        blur_btn: offset_rect(layout.blur_btn, offset_x, offset_y),
        copy_btn: offset_rect(layout.copy_btn, offset_x, offset_y),
        save_btn: offset_rect(layout.save_btn, offset_x, offset_y),
        copy_save_btn: offset_rect(layout.copy_save_btn, offset_x, offset_y),
        pin_btn: offset_rect(layout.pin_btn, offset_x, offset_y),
    }
}
fn clamp_radial_center(point: POINT, client: RECT) -> POINT {
    let min_x = client.left + RADIAL_MARGIN;
    let max_x = (client.right - RADIAL_MARGIN - 1).max(min_x);
    let min_y = client.top + RADIAL_MARGIN;
    let max_y = (client.bottom - RADIAL_MARGIN - 1).max(min_y);
    POINT {
        x: point.x.clamp(min_x, max_x),
        y: point.y.clamp(min_y, max_y),
    }
}

fn radial_swatch_centers(center: POINT, scale: f32) -> [POINT; ANNOTATION_PALETTE_SIZE] {
    let scaled_radius = (RADIAL_MENU_RADIUS as f32 * scale.clamp(0.0, 1.0))
        .round()
        .max(0.0);
    std::array::from_fn(|idx| {
        let angle = (-std::f32::consts::FRAC_PI_2)
            + ((idx as f32) * (std::f32::consts::TAU / ANNOTATION_PALETTE_SIZE as f32));
        POINT {
            x: center.x + (angle.cos() * scaled_radius).round() as i32,
            y: center.y + (angle.sin() * scaled_radius).round() as i32,
        }
    })
}

fn radial_color_hit_test(center: POINT, point: POINT, scale: f32) -> Option<usize> {
    let scaled_radius =
        ((RADIAL_SWATCH_RADIUS as f32) * (0.35 + (0.65 * scale.clamp(0.0, 1.0)))).round() as i32;
    let radius_sq = (scaled_radius.max(2) + 5).pow(2);
    radial_swatch_centers(center, scale)
        .iter()
        .enumerate()
        .find_map(|(idx, swatch)| {
            let dx = point.x - swatch.x;
            let dy = point.y - swatch.y;
            ((dx * dx) + (dy * dy) <= radius_sq).then_some(idx)
        })
}

fn draw_radial_color_picker(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    picker: RadialColorPicker,
    selected_color: usize,
    colors: &[[u8; 4]; ANNOTATION_PALETTE_SIZE],
) {
    let scale = picker.visual_scale();
    let swatch_radius =
        ((RADIAL_SWATCH_RADIUS as f32) * (0.35 + (0.65 * scale.clamp(0.0, 1.0)))).round() as i32;

    for (idx, center) in radial_swatch_centers(picker.center, scale)
        .iter()
        .copied()
        .enumerate()
    {
        let border = if picker.hover_color == Some(idx) {
            LCARS_TEXT_LIGHT
        } else if idx == selected_color {
            LCARS_GOLD
        } else {
            BTN_BORDER
        };
        let border_width = if picker.hover_color == Some(idx) {
            3
        } else {
            2
        };
        draw_circle(
            hdc,
            center,
            swatch_radius.max(2),
            rgba_to_colorref(colors[idx]),
            border,
            border_width,
        );
    }
}

fn draw_circle(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    center: POINT,
    radius: i32,
    fill: COLORREF,
    border: COLORREF,
    border_width: i32,
) {
    if radius <= 0 {
        return;
    }
    let brush = unsafe { CreateSolidBrush(fill) };
    if brush.0.is_null() {
        return;
    }
    let pen = unsafe { CreatePen(PS_SOLID, border_width.max(1), border) };
    if pen.0.is_null() {
        unsafe {
            let _ = DeleteObject(brush);
        }
        return;
    }

    unsafe {
        let old_brush = SelectObject(hdc, brush);
        let old_pen = SelectObject(hdc, pen);
        let _ = Ellipse(
            hdc,
            center.x - radius,
            center.y - radius,
            center.x + radius + 1,
            center.y + radius + 1,
        );
        let _ = SelectObject(hdc, old_brush);
        let _ = SelectObject(hdc, old_pen);
        let _ = DeleteObject(brush);
        let _ = DeleteObject(pen);
    }
}

fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t.powi(3)
}

fn inverse_close_progress_from_scale(scale: f32) -> f32 {
    let remaining = (1.0 - scale.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    remaining.cbrt()
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
    if point_in(p, layout.text_btn) {
        return Some(ToolbarHit::Text);
    }
    if point_in(p, layout.pixelate_btn) {
        return Some(ToolbarHit::Pixelate);
    }
    if point_in(p, layout.blur_btn) {
        return Some(ToolbarHit::Blur);
    }
    if point_in(p, layout.copy_btn) {
        return Some(ToolbarHit::Copy);
    }
    if point_in(p, layout.save_btn) {
        return Some(ToolbarHit::Save);
    }
    if point_in(p, layout.copy_save_btn) {
        return Some(ToolbarHit::CopyAndSave);
    }
    if point_in(p, layout.pin_btn) {
        return Some(ToolbarHit::Pin);
    }
    if point_in(p, layout.panel) {
        return Some(ToolbarHit::Panel);
    }
    None
}

fn hoverable_toolbar_hit(hit: Option<ToolbarHit>) -> Option<ToolbarHit> {
    match hit {
        Some(ToolbarHit::Panel) | None => None,
        value => value,
    }
}

fn hit_handle(selection: RECT, p: POINT) -> Option<ResizeHandle> {
    for (handle, rect) in handle_rects(selection) {
        if point_in(p, rect) {
            return Some(handle);
        }
    }
    None
}

fn selected_annotation_handle_hit(state: &State, client: POINT) -> Option<AnnotationHandleHit> {
    let idx = state.selected_annotation?;
    let ann = state.annotations.get(idx)?;
    if let Some(bounds) = annotation_resize_rect_abs(ann) {
        let client_rect = to_client_rect(bounds, state.virtual_rect);
        if let Some(handle) = hit_handle(client_rect, client) {
            return Some(AnnotationHandleHit::Resize {
                index: idx,
                bounds,
                handle,
            });
        }
    }
    if let Annotation::Line(line) = ann {
        let end = to_client_point(line.end_abs, state.virtual_rect);
        if point_in(client, handle_rect(end.x, end.y)) {
            return Some(AnnotationHandleHit::LineEndpoint {
                index: idx,
                endpoint: LineEndpoint::End,
            });
        }
        let start = to_client_point(line.start_abs, state.virtual_rect);
        if point_in(client, handle_rect(start.x, start.y)) {
            return Some(AnnotationHandleHit::LineEndpoint {
                index: idx,
                endpoint: LineEndpoint::Start,
            });
        }
    }
    if let Annotation::Marker(marker) = ann
        && let (Some(start), Some(end)) = (
            marker.points_abs.first().copied(),
            marker.points_abs.last().copied(),
        )
    {
        let end_client = to_client_point(end, state.virtual_rect);
        if point_in(client, handle_rect(end_client.x, end_client.y)) {
            return Some(AnnotationHandleHit::MarkerEndpoint {
                index: idx,
                endpoint: LineEndpoint::End,
            });
        }
        let start_client = to_client_point(start, state.virtual_rect);
        if point_in(client, handle_rect(start_client.x, start_client.y)) {
            return Some(AnnotationHandleHit::MarkerEndpoint {
                index: idx,
                endpoint: LineEndpoint::Start,
            });
        }
    }
    None
}

fn update_cursor(hwnd: HWND, client: POINT) {
    let Some(state) = (unsafe { state_ref(hwnd) }) else {
        return;
    };
    if let Some(picker) = state.radial_color_picker {
        let cursor_id =
            if radial_color_hit_test(picker.center, client, picker.visual_scale()).is_some() {
                IDC_HAND
            } else {
                IDC_ARROW
            };
        unsafe {
            if let Ok(cursor) = LoadCursorW(HINSTANCE::default(), cursor_id) {
                let _ = SetCursor(cursor);
            }
        }
        return;
    }
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
            | ToolbarHit::Text
            | ToolbarHit::Pixelate
            | ToolbarHit::Blur
            | ToolbarHit::Copy
            | ToolbarHit::Save
            | ToolbarHit::CopyAndSave
            | ToolbarHit::Pin => IDC_HAND,
            ToolbarHit::Panel => IDC_ARROW,
        }
    } else {
        match state.tool {
            Tool::Select => {
                let annotation_handle_cursor = match state.drag {
                    Some(Drag::ResizeAnnotation { handle, .. }) => Some(cursor_for_handle(handle)),
                    Some(Drag::MoveLineEndpoint { .. }) => Some(IDC_SIZEALL),
                    Some(Drag::MoveMarkerEndpoint { .. }) => Some(IDC_SIZEALL),
                    _ => selected_annotation_handle_hit(state, client).map(|hit| match hit {
                        AnnotationHandleHit::Resize { handle, .. } => cursor_for_handle(handle),
                        AnnotationHandleHit::LineEndpoint { .. } => IDC_SIZEALL,
                        AnnotationHandleHit::MarkerEndpoint { .. } => IDC_SIZEALL,
                    }),
                };
                if let Some(cursor) = annotation_handle_cursor {
                    cursor
                } else if let Some(h) = match state.drag {
                    Some(Drag::Resize { handle, .. }) => Some(handle),
                    _ => hit_handle(selection, client),
                } {
                    cursor_for_handle(h)
                } else if matches!(
                    state.drag,
                    Some(Drag::Move { .. })
                        | Some(Drag::MoveAnnotation { .. })
                        | Some(Drag::MoveLineEndpoint { .. })
                        | Some(Drag::MoveMarkerEndpoint { .. })
                ) || point_in(client, selection)
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
            Tool::Text => {
                if point_in(client, selection) {
                    IDC_CROSS
                } else {
                    IDC_ARROW
                }
            }
            Tool::Pixelate => {
                if point_in(client, selection) {
                    IDC_CROSS
                } else {
                    IDC_ARROW
                }
            }
            Tool::Blur => {
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

fn translate_rect(rect: RectPx, dx: i32, dy: i32) -> RectPx {
    RectPx {
        left: rect.left + dx,
        top: rect.top + dy,
        right: rect.right + dx,
        bottom: rect.bottom + dy,
    }
}

fn translate_point(point: POINT, dx: i32, dy: i32) -> POINT {
    POINT {
        x: point.x + dx,
        y: point.y + dy,
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

fn draw_pixelate_raster(image: &mut RgbaImage, rect: RectPx, block: i32) {
    let width = image.width() as i32;
    let height = image.height() as i32;
    if width <= 0 || height <= 0 {
        return;
    }
    let left = rect.left.clamp(0, width);
    let top = rect.top.clamp(0, height);
    let right = rect.right.clamp(0, width);
    let bottom = rect.bottom.clamp(0, height);
    if right <= left || bottom <= top {
        return;
    }

    let block = block.max(1);
    for by in (top..bottom).step_by(block as usize) {
        let y1 = (by + block).min(bottom);
        for bx in (left..right).step_by(block as usize) {
            let x1 = (bx + block).min(right);
            let mut sum = [0u64; 4];
            let mut count = 0u64;
            for y in by..y1 {
                for x in bx..x1 {
                    let px = image.get_pixel(x as u32, y as u32).0;
                    sum[0] += px[0] as u64;
                    sum[1] += px[1] as u64;
                    sum[2] += px[2] as u64;
                    sum[3] += px[3] as u64;
                    count += 1;
                }
            }
            if count == 0 {
                continue;
            }
            let avg = [
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
                (sum[3] / count) as u8,
            ];
            for y in by..y1 {
                for x in bx..x1 {
                    image.put_pixel(x as u32, y as u32, Rgba(avg));
                }
            }
        }
    }
}

fn draw_blur_raster(image: &mut RgbaImage, rect: RectPx, radius: i32) {
    let width = image.width() as i32;
    let height = image.height() as i32;
    if width <= 0 || height <= 0 {
        return;
    }
    let left = rect.left.clamp(0, width);
    let top = rect.top.clamp(0, height);
    let right = rect.right.clamp(0, width);
    let bottom = rect.bottom.clamp(0, height);
    if right <= left || bottom <= top {
        return;
    }

    let patch_w = (right - left) as usize;
    let patch_h = (bottom - top) as usize;
    let Some(pixel_len) = patch_w.checked_mul(patch_h).and_then(|v| v.checked_mul(4)) else {
        return;
    };
    let mut patch = vec![0u8; pixel_len];
    for y in 0..patch_h {
        for x in 0..patch_w {
            let src = image
                .get_pixel((left as usize + x) as u32, (top as usize + y) as u32)
                .0;
            let idx = (y * patch_w + x) * 4;
            patch[idx] = src[0];
            patch[idx + 1] = src[1];
            patch[idx + 2] = src[2];
            patch[idx + 3] = src[3];
        }
    }

    let blurred = blur_buffer_4ch(&patch, patch_w, patch_h, radius.max(1) as usize);
    for y in 0..patch_h {
        for x in 0..patch_w {
            let idx = (y * patch_w + x) * 4;
            let px = Rgba([
                blurred[idx],
                blurred[idx + 1],
                blurred[idx + 2],
                blurred[idx + 3],
            ]);
            image.put_pixel((left as usize + x) as u32, (top as usize + y) as u32, px);
        }
    }
}

fn blur_buffer_4ch(src: &[u8], width: usize, height: usize, radius: usize) -> Vec<u8> {
    if src.is_empty() || width == 0 || height == 0 || radius == 0 {
        return src.to_vec();
    }

    let channels = 4usize;
    let kernel = (radius * 2 + 1) as i32;
    let mut horizontal = vec![0u8; src.len()];
    let mut output = vec![0u8; src.len()];

    for y in 0..height {
        let row = y * width;
        let mut sums = [0i32; 4];
        for i in 0..=(radius * 2) {
            let x = i.saturating_sub(radius).min(width - 1);
            let idx = (row + x) * channels;
            for c in 0..channels {
                sums[c] += src[idx + c] as i32;
            }
        }
        for x in 0..width {
            let dst = (row + x) * channels;
            for c in 0..channels {
                horizontal[dst + c] = (sums[c] / kernel).clamp(0, 255) as u8;
            }

            let out_x = x.saturating_sub(radius);
            let in_x = (x + radius + 1).min(width - 1);
            let out_idx = (row + out_x) * channels;
            let in_idx = (row + in_x) * channels;
            for c in 0..channels {
                sums[c] += src[in_idx + c] as i32;
                sums[c] -= src[out_idx + c] as i32;
            }
        }
    }

    for x in 0..width {
        let mut sums = [0i32; 4];
        for i in 0..=(radius * 2) {
            let y = i.saturating_sub(radius).min(height - 1);
            let idx = (y * width + x) * channels;
            for c in 0..channels {
                sums[c] += horizontal[idx + c] as i32;
            }
        }
        for y in 0..height {
            let dst = (y * width + x) * channels;
            for c in 0..channels {
                output[dst + c] = (sums[c] / kernel).clamp(0, 255) as u8;
            }

            let out_y = y.saturating_sub(radius);
            let in_y = (y + radius + 1).min(height - 1);
            let out_idx = (out_y * width + x) * channels;
            let in_idx = (in_y * width + x) * channels;
            for c in 0..channels {
                sums[c] += horizontal[in_idx + c] as i32;
                sums[c] -= horizontal[out_idx + c] as i32;
            }
        }
    }

    output
}

fn draw_text_raster(image: &mut RgbaImage, rect: RectPx, text: &str, color: [u8; 4]) {
    if text.is_empty() {
        return;
    }
    let width = image.width() as i32;
    let height = image.height() as i32;
    if width <= 0 || height <= 0 {
        return;
    }

    let mut left = rect.left.clamp(0, width);
    let mut top = rect.top.clamp(0, height);
    let mut right = rect.right.clamp(0, width);
    let mut bottom = rect.bottom.clamp(0, height);
    if right <= left || bottom <= top {
        return;
    }
    // Keep a small inset to avoid drawing against border lines.
    left += TEXT_PAD;
    top += TEXT_PAD;
    right -= TEXT_PAD;
    bottom -= TEXT_PAD;
    if right <= left || bottom <= top {
        return;
    }

    let mut x = left;
    let mut y = top;
    let max_x = right;
    let max_y = bottom;

    for ch in text.chars() {
        if ch == '\r' {
            continue;
        }
        if ch == '\n' {
            x = left;
            y += TEXT_GLYPH_H + TEXT_LINE_GAP;
            if y + TEXT_GLYPH_H > max_y {
                break;
            }
            continue;
        }
        let advance = if ch == ' ' {
            TEXT_SPACE_ADVANCE
        } else {
            TEXT_GLYPH_ADVANCE
        };
        if x + TEXT_GLYPH_W > max_x {
            continue;
        }
        if y + TEXT_GLYPH_H > max_y {
            break;
        }
        if ch != ' ' {
            let glyph = glyph_5x7(ch);
            for (row, bits) in glyph.into_iter().enumerate() {
                for col in 0..5 {
                    if (bits & (1 << (4 - col))) == 0 {
                        continue;
                    }
                    let px0 = x + (col * TEXT_SCALE);
                    let py0 = y + (row as i32 * TEXT_SCALE);
                    for sy in 0..TEXT_SCALE {
                        for sx in 0..TEXT_SCALE {
                            let px = px0 + sx;
                            let py = py0 + sy;
                            if px >= 0 && px < width && py >= 0 && py < height {
                                blend_pixel(image, px, py, color, 1.0);
                            }
                        }
                    }
                }
            }
        }
        x += advance;
        if x >= max_x {
            x = max_x;
        }
    }
}

fn text_required_size(text: &str) -> (i32, i32) {
    let mut max_line_w = TEXT_GLYPH_W;
    let mut line_w = 0;
    let mut line_count = 1;

    for ch in text.chars() {
        if ch == '\r' {
            continue;
        }
        if ch == '\n' {
            max_line_w = max_line_w.max(line_w.max(TEXT_GLYPH_W));
            line_w = 0;
            line_count += 1;
            continue;
        }
        line_w += if ch == ' ' {
            TEXT_SPACE_ADVANCE
        } else {
            TEXT_GLYPH_ADVANCE
        };
    }
    max_line_w = max_line_w.max(line_w.max(TEXT_GLYPH_W));

    let text_h = (line_count * TEXT_GLYPH_H) + ((line_count - 1) * TEXT_LINE_GAP);
    let width = max_line_w + (TEXT_PAD * 2);
    let height = text_h + (TEXT_PAD * 2);
    (width.max(TEXT_DEFAULT_W), height.max(TEXT_DEFAULT_H))
}

fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_lowercase() {
        'a' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'b' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'c' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'd' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'e' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'f' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'g' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'h' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'i' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'j' => [
            0b11111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        'k' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'l' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'm' => [
            0b10001, 0b11011, 0b10101, 0b10001, 0b10001, 0b10001, 0b10001,
        ],
        'n' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'o' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'p' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'r' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        's' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        't' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'u' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'v' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'w' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10101, 0b11011, 0b10001,
        ],
        'x' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b10000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        _ => [
            0b11111, 0b10001, 0b00100, 0b00100, 0b00100, 0b10001, 0b11111,
        ],
    }
}

fn draw_line(
    image: &mut RgbaImage,
    start: (i32, i32),
    end: (i32, i32),
    color: [u8; 4],
    thickness: i32,
) {
    let width = image.width() as i32;
    let height = image.height() as i32;
    if width <= 0 || height <= 0 {
        return;
    }

    let sx = start.0 as f32;
    let sy = start.1 as f32;
    let ex = end.0 as f32;
    let ey = end.1 as f32;
    let radius = (thickness.max(1) as f32) * 0.5;

    let x_min = (sx.min(ex) - radius - 1.0).floor().max(0.0) as i32;
    let x_max = (sx.max(ex) + radius + 1.0).ceil().min((width - 1) as f32) as i32;
    let y_min = (sy.min(ey) - radius - 1.0).floor().max(0.0) as i32;
    let y_max = (sy.max(ey) + radius + 1.0).ceil().min((height - 1) as f32) as i32;
    let dx = ex - sx;
    let dy = ey - sy;
    let len_sq = (dx * dx) + (dy * dy);
    let aa_max_dist = radius + LINE_AA_EDGE_WIDTH;
    let reject_margin = aa_max_dist + 0.8;

    for py in y_min..=y_max {
        for px in x_min..=x_max {
            let cx = px as f32 + 0.5;
            let cy = py as f32 + 0.5;
            let center_dist = point_to_segment_distance_precomputed(cx, cy, sx, sy, dx, dy, len_sq);
            if center_dist > reject_margin {
                continue;
            }

            let coverage = if center_dist <= (radius - LINE_AA_EDGE_WIDTH).max(0.0) {
                1.0
            } else {
                let mut accum = 0.0f32;
                for (ox, oy) in LINE_AA_SUBSAMPLES {
                    let sample_x = px as f32 + ox;
                    let sample_y = py as f32 + oy;
                    let dist = point_to_segment_distance_precomputed(
                        sample_x, sample_y, sx, sy, dx, dy, len_sq,
                    );
                    accum += (aa_max_dist - dist).clamp(0.0, 1.0);
                }
                accum / LINE_AA_SUBSAMPLES.len() as f32
            };
            if coverage > 0.0 {
                blend_pixel(image, px, py, color, coverage);
            }
        }
    }
}

fn point_to_segment_distance_precomputed(
    px: f32,
    py: f32,
    ax: f32,
    ay: f32,
    dx: f32,
    dy: f32,
    len_sq: f32,
) -> f32 {
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

fn draw_arrow_head(
    image: &mut RgbaImage,
    start: (i32, i32),
    end: (i32, i32),
    color: [u8; 4],
    thickness: i32,
) {
    let start_point = POINT {
        x: start.0,
        y: start.1,
    };
    let end_point = POINT { x: end.0, y: end.1 };
    let Some((left, right)) = arrow_head_points(start_point, end_point, thickness) else {
        return;
    };
    draw_line(image, end, (left.x, left.y), color, thickness);
    draw_line(image, end, (right.x, right.y), color, thickness);
}

fn arrow_head_points(start: POINT, end: POINT, thickness: i32) -> Option<(POINT, POINT)> {
    let dx = (end.x - start.x) as f32;
    let dy = (end.y - start.y) as f32;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return None;
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
    Some((left, right))
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

fn snap_point_to_angle_increment(origin: POINT, raw: POINT, degrees: f32) -> POINT {
    let dx = (raw.x - origin.x) as f32;
    let dy = (raw.y - origin.y) as f32;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.5 {
        return origin;
    }
    let step_radians = degrees.to_radians();
    if !step_radians.is_finite() || step_radians <= 0.0 {
        return raw;
    }
    let angle = dy.atan2(dx);
    let snapped = (angle / step_radians).round() * step_radians;
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
        Annotation::Text(text) => Some(text.rect_abs),
        Annotation::Pixelate(pixelate) => Some(pixelate.rect_abs),
        Annotation::Blur(blur) => Some(blur.rect_abs),
    }
}

fn annotation_resize_rect_abs(annotation: &Annotation) -> Option<RectPx> {
    match annotation {
        Annotation::Rectangle(rect) => Some(rect.rect_abs),
        Annotation::Ellipse(ellipse) => Some(ellipse.rect_abs),
        Annotation::Text(text) => Some(text.rect_abs),
        Annotation::Pixelate(pixelate) => Some(pixelate.rect_abs),
        Annotation::Blur(blur) => Some(blur.rect_abs),
        Annotation::Line(_) | Annotation::Marker(_) => None,
    }
}

fn set_annotation_resize_rect(annotation: &mut Annotation, rect: RectPx) -> bool {
    match annotation {
        Annotation::Rectangle(ann) => {
            if !rect_changed(ann.rect_abs, rect) {
                return false;
            }
            ann.rect_abs = rect;
            true
        }
        Annotation::Ellipse(ann) => {
            if !rect_changed(ann.rect_abs, rect) {
                return false;
            }
            ann.rect_abs = rect;
            true
        }
        Annotation::Text(ann) => {
            if !rect_changed(ann.rect_abs, rect) {
                return false;
            }
            ann.rect_abs = rect;
            true
        }
        Annotation::Pixelate(ann) => {
            if !rect_changed(ann.rect_abs, rect) {
                return false;
            }
            ann.rect_abs = rect;
            true
        }
        Annotation::Blur(ann) => {
            if !rect_changed(ann.rect_abs, rect) {
                return false;
            }
            ann.rect_abs = rect;
            true
        }
        Annotation::Line(_) | Annotation::Marker(_) => false,
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
        Annotation::Rectangle(rect) => {
            point_in_abs(point_abs, rect.rect_abs)
                || point_near_rect_outline(
                    point_abs,
                    rect.rect_abs,
                    tolerance + rect.thickness as f32 * 0.5,
                )
        }
        Annotation::Ellipse(ellipse) => {
            point_in_ellipse(point_abs, ellipse.rect_abs)
                || point_near_ellipse_outline(
                    point_abs,
                    ellipse.rect_abs,
                    tolerance + ellipse.thickness as f32 * 0.5,
                )
        }
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
        Annotation::Text(text) => {
            point_abs.x >= text.rect_abs.left
                && point_abs.x < text.rect_abs.right
                && point_abs.y >= text.rect_abs.top
                && point_abs.y < text.rect_abs.bottom
        }
        Annotation::Pixelate(pixelate) => point_in_abs(point_abs, pixelate.rect_abs),
        Annotation::Blur(blur) => point_in_abs(point_abs, blur.rect_abs),
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

fn point_in_ellipse(point: POINT, rect: RectPx) -> bool {
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
    (nx * nx + ny * ny) <= 1.0
}

fn point_to_segment_distance(point: POINT, a: POINT, b: POINT) -> f32 {
    point_to_segment_distance_f32(
        point.x as f32,
        point.y as f32,
        a.x as f32,
        a.y as f32,
        b.x as f32,
        b.y as f32,
    )
}

fn point_to_segment_distance_f32(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
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

fn point_in_abs(p: POINT, r: RectPx) -> bool {
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

fn tool_display_name(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "SELECT",
        Tool::Rectangle => "RECT",
        Tool::Ellipse => "CIRCLE",
        Tool::Line => "LINE",
        Tool::Arrow => "ARROW",
        Tool::Marker => "MARKER",
        Tool::Text => "TEXT",
        Tool::Pixelate => "CENSOR",
        Tool::Blur => "BLUR",
    }
}

fn toolbar_hit_base_color(hit: ToolbarHit) -> COLORREF {
    match hit {
        ToolbarHit::Select => LCARS_LILAC,
        ToolbarHit::Rect => LCARS_PEACH,
        ToolbarHit::Ellipse => LCARS_GOLD,
        ToolbarHit::Line => LCARS_SKY,
        ToolbarHit::Arrow => LCARS_PURPLE,
        ToolbarHit::Marker => LCARS_CORAL,
        ToolbarHit::Text => LCARS_ROSE,
        ToolbarHit::Pixelate => LCARS_GOLD,
        ToolbarHit::Blur => LCARS_LILAC,
        ToolbarHit::Copy => LCARS_GOLD,
        ToolbarHit::Save => LCARS_PEACH,
        ToolbarHit::CopyAndSave => LCARS_ROSE,
        ToolbarHit::Pin => LCARS_SKY,
        ToolbarHit::Panel => LCARS_PANEL_MID,
    }
}

fn lighten_color(color: COLORREF, amount: f32) -> COLORREF {
    rgb_from_array(mix_rgb(colorref_to_rgb(color), [255, 255, 255], amount))
}

fn darken_color(color: COLORREF, amount: f32) -> COLORREF {
    rgb_from_array(mix_rgb(colorref_to_rgb(color), [0, 0, 0], amount))
}

fn inset_rect(rect: RECT, dx: i32, dy: i32) -> RECT {
    RECT {
        left: rect.left + dx,
        top: rect.top + dy,
        right: rect.right - dx,
        bottom: rect.bottom - dy,
    }
}

fn colorref_to_rgb(color: COLORREF) -> [u8; 3] {
    [
        (color.0 & 0xFF) as u8,
        ((color.0 >> 8) & 0xFF) as u8,
        ((color.0 >> 16) & 0xFF) as u8,
    ]
}

fn rgb_from_array(color: [u8; 3]) -> COLORREF {
    rgb(color[0], color[1], color[2])
}

fn mix_rgb(base: [u8; 3], target: [u8; 3], amount: f32) -> [u8; 3] {
    [
        lerp_channel(base[0], target[0], amount),
        lerp_channel(base[1], target[1], amount),
        lerp_channel(base[2], target[2], amount),
    ]
}

fn lerp_channel(start: u8, end: u8, t: f32) -> u8 {
    let amount = t.clamp(0.0, 1.0);
    ((start as f32) + ((end as f32) - (start as f32)) * amount).round() as u8
}

const fn rgba_to_colorref(color: [u8; 4]) -> COLORREF {
    rgb(color[0], color[1], color[2])
}

const fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF((red as u32) | ((green as u32) << 8) | ((blue as u32) << 16))
}
