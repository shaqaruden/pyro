use std::mem::size_of;

use anyhow::{Result, bail};
use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, MONITORINFOF_PRIMARY, SM_CMONITORS, SM_CXSCREEN, SM_CXVIRTUALSCREEN,
    SM_CYSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SetProcessDPIAware,
};

#[derive(Debug, Clone, Copy)]
pub struct RectPx {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl RectPx {
    pub fn width(self) -> i32 {
        self.right - self.left
    }

    pub fn height(self) -> i32 {
        self.bottom - self.top
    }
}

#[derive(Debug, Clone)]
pub struct MonitorDescriptor {
    pub device_name: String,
    pub rect: RectPx,
    pub dpi_x: u32,
    pub dpi_y: u32,
    pub is_primary: bool,
}

pub fn init_process_dpi_awareness() {
    unsafe {
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_ok() {
            return;
        }

        let _ = SetProcessDPIAware();
    }
}

pub fn primary_screen_rect() -> RectPx {
    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    RectPx {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    }
}

pub fn virtual_screen_rect() -> RectPx {
    let left = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let top = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) };
    RectPx {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

pub fn monitor_count() -> usize {
    unsafe { GetSystemMetrics(SM_CMONITORS) as usize }
}

pub fn enumerate_monitors() -> Result<Vec<MonitorDescriptor>> {
    unsafe extern "system" fn callback(
        monitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let monitors = unsafe { &mut *(lparam.0 as *mut Vec<MonitorDescriptor>) };

        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if !unsafe { GetMonitorInfoW(monitor, &mut info.monitorInfo as *mut MONITORINFO) }.as_bool()
        {
            return true.into();
        }

        let mut dpi_x = 96;
        let mut dpi_y = 96;
        if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }.is_err()
        {
            dpi_x = 96;
            dpi_y = 96;
        }

        monitors.push(MonitorDescriptor {
            device_name: utf16_to_string(&info.szDevice),
            rect: RectPx {
                left: info.monitorInfo.rcMonitor.left,
                top: info.monitorInfo.rcMonitor.top,
                right: info.monitorInfo.rcMonitor.right,
                bottom: info.monitorInfo.rcMonitor.bottom,
            },
            dpi_x,
            dpi_y,
            is_primary: (info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY) != 0,
        });

        true.into()
    }

    let mut monitors = Vec::<MonitorDescriptor>::new();
    let ok = unsafe {
        EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(callback),
            LPARAM(&mut monitors as *mut Vec<MonitorDescriptor> as isize),
        )
    };
    if !ok.as_bool() {
        bail!("EnumDisplayMonitors failed");
    }
    Ok(monitors)
}

fn utf16_to_string(chars: &[u16]) -> String {
    let end = chars
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(chars.len());
    String::from_utf16_lossy(&chars[..end])
}
