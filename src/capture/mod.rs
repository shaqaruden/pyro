mod windows_capture;

use std::fmt::{Display, Formatter};
use std::thread;
use std::time::Duration;

use anyhow::{Result, bail};
use clap::ValueEnum;
use image::RgbaImage;
use serde::{Deserialize, Serialize};

use crate::platform_windows::{
    MonitorDescriptor, RectPx, primary_screen_rect, virtual_screen_rect,
};

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureTarget {
    Primary,
    Region,
    #[default]
    AllDisplays,
}

impl Display for CaptureTarget {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Primary => f.write_str("primary"),
            Self::Region => f.write_str("region"),
            Self::AllDisplays => f.write_str("all-displays"),
        }
    }
}

#[derive(Debug)]
pub struct CaptureFrame {
    pub bounds: RectPx,
    pub image: RgbaImage,
}

pub fn capture_target_with_delay(target: CaptureTarget, delay_ms: u64) -> Result<CaptureFrame> {
    if delay_ms > 0 {
        thread::sleep(Duration::from_millis(delay_ms));
    }
    capture_target(target)
}

pub fn capture_target(target: CaptureTarget) -> Result<CaptureFrame> {
    let bounds = match target {
        CaptureTarget::Primary => primary_screen_rect(),
        CaptureTarget::Region => crate::region_overlay::select_region()?,
        CaptureTarget::AllDisplays => virtual_screen_rect(),
    };

    capture_rect(bounds)
}

pub fn capture_rect(bounds: RectPx) -> Result<CaptureFrame> {
    if bounds.width() <= 0 || bounds.height() <= 0 {
        bail!(
            "invalid capture bounds {}x{}",
            bounds.width(),
            bounds.height()
        );
    }

    let image = windows_capture::capture_rect(bounds)?;
    Ok(CaptureFrame { bounds, image })
}

pub fn enumerate_monitors() -> Result<Vec<MonitorDescriptor>> {
    crate::platform_windows::enumerate_monitors()
}
