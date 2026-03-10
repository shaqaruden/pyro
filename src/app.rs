use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{ArgAction, Args, Parser, Subcommand};
use image::{RgbaImage, imageops};
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostQuitMessage, TranslateMessage, WM_HOTKEY,
};

use crate::capture::{self, CaptureFrame, CaptureTarget};
use crate::config::{
    ANNOTATION_PALETTE_SIZE, AppConfig, RadialMenuAnimationSpeed, load_config_from_path,
    load_or_create_config,
};
use crate::hotkey::parse_hotkey;
use crate::output::{copy_to_clipboard, save_png};
use crate::pinned_capture;
use crate::platform_windows::{MonitorDescriptor, RectPx, monitor_count, virtual_screen_rect};
use crate::region_editor::{self, EditorOutputAction, RegionEditOutcome};
use crate::region_overlay;
use crate::settings_ui;
use crate::tray::{TRAY_ACTION_MESSAGE, TrayAction, TrayHost};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum AppMode {
    Idle,
    Capture,
    Edit,
}

const HOTKEY_ID: i32 = 1;
const HOTKEY_RELOAD_MESSAGE: u32 = TRAY_ACTION_MESSAGE + 1;

#[derive(Debug)]
struct AppState {
    mode: AppMode,
}

#[derive(Debug)]
struct CapturedFrame {
    frame: CaptureFrame,
    output_action: Option<EditorOutputAction>,
}

#[derive(Debug, Clone, Copy)]
struct OutputPlan {
    copy: bool,
    save: bool,
    pin: bool,
}

#[derive(Debug, Clone)]
struct EditorRuntimeOptions {
    keybindings: region_editor::EditorKeybindings,
    text_commit_feedback_color: [u8; 3],
    radial_menu_animation_speed: RadialMenuAnimationSpeed,
    annotation_palette: [[u8; 4]; ANNOTATION_PALETTE_SIZE],
}

#[derive(Debug, Parser)]
#[command(
    name = "pyro",
    version,
    about = "Windows screenshot utility (Phase 0/1 foundation)"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start background process and listen for the configured global capture hotkey
    Run,
    /// Capture a screenshot now
    Capture(CaptureArgs),
    /// Open the settings window
    Settings,
    /// Print monitor and DPI metadata
    Monitors(MonitorArgs),
}

#[derive(Debug, Args)]
struct CaptureArgs {
    /// Capture target
    #[arg(long, value_enum)]
    target: Option<CaptureTarget>,
    /// Delay before capture in milliseconds
    #[arg(long)]
    delay_ms: Option<u64>,
    /// Write PNG output to this path
    #[arg(long)]
    output: Option<PathBuf>,
    /// Force copy image to clipboard
    #[arg(long, action = ArgAction::SetTrue)]
    clipboard: bool,
    /// Force-disable clipboard copy
    #[arg(long, action = ArgAction::SetTrue)]
    no_clipboard: bool,
    /// Open region edit UI before outputting (region target only)
    #[arg(long, action = ArgAction::SetTrue)]
    edit: bool,
    /// Skip region edit UI and output capture immediately
    #[arg(long, action = ArgAction::SetTrue)]
    no_edit: bool,
}

#[derive(Debug, Args, Clone)]
struct MonitorArgs {
    /// Output monitor metadata as JSON
    #[arg(long, action = ArgAction::SetTrue)]
    json: bool,
    /// Return non-zero exit code when monitor diagnostics fail
    #[arg(long, action = ArgAction::SetTrue)]
    validate: bool,
    /// Validate the expected monitor count
    #[arg(long)]
    expect_count: Option<usize>,
    /// Write full monitor report JSON to file
    #[arg(long)]
    report: Option<PathBuf>,
    /// Compare current layout against a previously saved monitor report
    #[arg(long)]
    compare_report: Option<PathBuf>,
    /// Fail validation if virtual desktop uncovered gap area exceeds this value
    #[arg(long)]
    max_gap_px: Option<u64>,
    /// Fail validation if virtual desktop overlap area exceeds this value
    #[arg(long)]
    max_overlap_px: Option<u64>,
}

impl MonitorArgs {
    fn validation_enabled(&self) -> bool {
        self.validate
            || self.expect_count.is_some()
            || self.compare_report.is_some()
            || self.max_gap_px.is_some()
            || self.max_overlap_px.is_some()
    }
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let loaded = load_or_create_config()?;

    match cli.command.unwrap_or(Command::Run) {
        Command::Run => run_hotkey_listener(&loaded),
        Command::Capture(args) => run_capture(args, &loaded),
        Command::Settings => {
            settings_ui::launch_settings_window(&loaded.path)?;
            Ok(())
        }
        Command::Monitors(args) => print_monitor_metadata(args),
    }
}

fn run_hotkey_listener(loaded: &crate::config::LoadedConfig) -> Result<()> {
    let mut app_config = loaded.data.clone();
    let mut editor_options = resolve_editor_options(loaded)?;
    let config_path = loaded.path.clone();
    let state = AppState {
        mode: AppMode::Idle,
    };
    let tray = TrayHost::create().context("initialize tray icon failed")?;

    let hotkey = parse_hotkey(&app_config.capture_hotkey).with_context(|| {
        format!(
            "invalid capture_hotkey in {}: {}",
            loaded.path.display(),
            app_config.capture_hotkey
        )
    })?;

    unsafe {
        RegisterHotKey(tray.hwnd(), HOTKEY_ID, hotkey.modifiers, hotkey.vk)
            .context("register global hotkey failed")?;
    }
    let mut registered_hotkey = hotkey;
    let _hotkey_guard = HotkeyRegistrationGuard {
        hwnd: tray.hwnd(),
        id: HOTKEY_ID,
    };

    let mut state = state;
    println!("Config: {}", loaded.path.display());
    println!("Hotkey (configured): {}", app_config.capture_hotkey);
    println!("Initial mode: {:?}", state.mode);
    println!("Detected monitors: {}", monitor_count());
    println!("Hotkey listener is active. Press Ctrl+C to quit.");
    tracing::info!("hotkey listener started");

    let mut msg = MSG::default();
    loop {
        let status = unsafe { GetMessageW(&mut msg, HWND::default(), 0, 0) }.0;
        if status == -1 {
            bail!("GetMessageW failed");
        }
        if status == 0 {
            break;
        }

        if msg.message == HOTKEY_RELOAD_MESSAGE {
            refresh_runtime_config(
                &config_path,
                &mut app_config,
                &mut editor_options,
                tray.hwnd(),
                HOTKEY_ID,
                &mut registered_hotkey,
            );
            continue;
        }

        if msg.message == WM_HOTKEY && msg.wParam.0 == HOTKEY_ID as usize {
            refresh_runtime_config(
                &config_path,
                &mut app_config,
                &mut editor_options,
                tray.hwnd(),
                HOTKEY_ID,
                &mut registered_hotkey,
            );
            if let Err(err) = trigger_capture(
                &mut state,
                app_config.default_target,
                &app_config,
                &editor_options,
            ) {
                tracing::error!("hotkey capture failed: {err:#}");
                eprintln!("Hotkey capture failed: {err:#}");
            }
            continue;
        }

        if msg.message == TRAY_ACTION_MESSAGE {
            let action = TrayAction::from_code(msg.wParam.0);
            match action {
                Some(TrayAction::CaptureDefault) => {
                    refresh_runtime_config(
                        &config_path,
                        &mut app_config,
                        &mut editor_options,
                        tray.hwnd(),
                        HOTKEY_ID,
                        &mut registered_hotkey,
                    );
                    if let Err(err) = trigger_capture(
                        &mut state,
                        app_config.default_target,
                        &app_config,
                        &editor_options,
                    ) {
                        tracing::error!("tray capture failed: {err:#}");
                        eprintln!("Tray capture failed: {err:#}");
                    }
                }
                Some(TrayAction::CapturePrimary) => {
                    refresh_runtime_config(
                        &config_path,
                        &mut app_config,
                        &mut editor_options,
                        tray.hwnd(),
                        HOTKEY_ID,
                        &mut registered_hotkey,
                    );
                    if let Err(err) = trigger_capture(
                        &mut state,
                        CaptureTarget::Primary,
                        &app_config,
                        &editor_options,
                    ) {
                        tracing::error!("tray capture failed: {err:#}");
                        eprintln!("Tray capture failed: {err:#}");
                    }
                }
                Some(TrayAction::CaptureRegion) => {
                    refresh_runtime_config(
                        &config_path,
                        &mut app_config,
                        &mut editor_options,
                        tray.hwnd(),
                        HOTKEY_ID,
                        &mut registered_hotkey,
                    );
                    if let Err(err) = trigger_capture(
                        &mut state,
                        CaptureTarget::Region,
                        &app_config,
                        &editor_options,
                    ) {
                        tracing::error!("tray capture failed: {err:#}");
                        eprintln!("Tray capture failed: {err:#}");
                    }
                }
                Some(TrayAction::CaptureAllDisplays) => {
                    refresh_runtime_config(
                        &config_path,
                        &mut app_config,
                        &mut editor_options,
                        tray.hwnd(),
                        HOTKEY_ID,
                        &mut registered_hotkey,
                    );
                    if let Err(err) = trigger_capture(
                        &mut state,
                        CaptureTarget::AllDisplays,
                        &app_config,
                        &editor_options,
                    ) {
                        tracing::error!("tray capture failed: {err:#}");
                        eprintln!("Tray capture failed: {err:#}");
                    }
                }
                Some(TrayAction::Settings) => {
                    if let Err(err) = settings_ui::launch_settings_window(&config_path) {
                        tracing::error!("open settings failed: {err:#}");
                        eprintln!("Open settings failed: {err:#}");
                    }
                }
                Some(TrayAction::Quit) => unsafe {
                    PostQuitMessage(0);
                },
                None => {}
            }
            continue;
        }

        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    Ok(())
}

fn run_capture(args: CaptureArgs, loaded: &crate::config::LoadedConfig) -> Result<()> {
    let editor_options = resolve_editor_options(loaded)?;
    if args.clipboard && args.no_clipboard {
        bail!("cannot use --clipboard and --no-clipboard together");
    }
    if args.edit && args.no_edit {
        bail!("cannot use --edit and --no-edit together");
    }

    let target = args.target.unwrap_or(loaded.data.default_target);
    let delay_ms = args.delay_ms.unwrap_or(loaded.data.default_delay_ms);
    let _mode = AppMode::Capture;
    let should_copy = if args.clipboard {
        true
    } else if args.no_clipboard {
        false
    } else {
        loaded.data.copy_to_clipboard
    };
    let should_edit = if args.edit {
        true
    } else if args.no_edit {
        false
    } else {
        loaded.data.open_editor
    };

    let should_region_edit = should_edit && target == CaptureTarget::Region;
    let Some(captured) =
        acquire_capture_frame(target, delay_ms, should_region_edit, &editor_options)?
    else {
        println!("Capture canceled.");
        return Ok(());
    };
    let plan = resolve_output_plan(should_copy, args.output.as_ref(), captured.output_action);

    emit_capture_output(
        target,
        &captured.frame.image,
        captured.frame.bounds,
        plan,
        args.output,
        &loaded.data.save_dir,
        &loaded.data.filename_template,
    )?;
    let _ = AppMode::Edit;

    Ok(())
}

fn print_monitor_metadata(args: MonitorArgs) -> Result<()> {
    let mut monitors = capture::enumerate_monitors()?;
    let virtual_rect = virtual_screen_rect();
    monitors.sort_by(|left, right| {
        left.rect
            .top
            .cmp(&right.rect.top)
            .then(left.rect.left.cmp(&right.rect.left))
            .then(right.is_primary.cmp(&left.is_primary))
            .then(left.device_name.cmp(&right.device_name))
    });

    let rows = build_monitor_rows(&monitors);
    let diagnostics = collect_monitor_diagnostics(&monitors, virtual_rect);
    let validation = build_monitor_validation(&rows, &diagnostics, &args)?;
    let report = MonitorReport {
        detected_monitors: rows.len(),
        virtual_desktop: VirtualDesktopReport {
            left: virtual_rect.left,
            top: virtual_rect.top,
            width: virtual_rect.width().max(0) as i64,
            height: virtual_rect.height().max(0) as i64,
            area_px: (virtual_rect.width().max(0) as i64) * (virtual_rect.height().max(0) as i64),
        },
        monitors: rows.clone(),
        diagnostics: diagnostics.clone(),
        validation: validation.clone(),
    };

    if let Some(path) = args.report.as_ref() {
        write_monitor_report(path, &report)?;
    }
    if args.json {
        print_monitor_metadata_json(&report)?;
    } else {
        print_monitor_metadata_text(&rows, virtual_rect, &diagnostics, &validation);
        if let Some(path) = args.report.as_ref() {
            println!();
            println!("Report written: {}", path.display());
        }
    }

    if validation.enabled && !validation.passed {
        let details = validation
            .issues
            .iter()
            .map(|issue| format!("- {issue}"))
            .collect::<Vec<_>>()
            .join("\n");
        bail!("monitor validation failed:\n{details}");
    }
    Ok(())
}

fn print_monitor_metadata_text(
    rows: &[MonitorRow],
    virtual_rect: RectPx,
    diagnostics: &MonitorDiagnostics,
    validation: &MonitorValidation,
) {
    let virtual_width = virtual_rect.width().max(0) as i64;
    let virtual_height = virtual_rect.height().max(0) as i64;
    let virtual_area = virtual_width * virtual_height;

    println!("Detected {} monitor(s)", rows.len());
    println!(
        "Virtual desktop: origin=({}, {}) size={}x{} area={} px",
        virtual_rect.left, virtual_rect.top, virtual_width, virtual_height, virtual_area
    );
    println!(
        "{:<3} {:<18} {:>11} {:>11} {:>11} {:>8} {:>8}",
        "ID", "Device", "Origin(px)", "Size(px)", "Logical", "Scale", "Primary"
    );
    for row in rows {
        println!(
            "{:<3} {:<18} {:>11} {:>11} {:>11} {:>7}% {:>8}",
            row.id,
            trim_monitor_label(&row.device_name, 18),
            format!("{},{}", row.origin_x, row.origin_y),
            format!("{}x{}", row.width, row.height),
            format!("{}x{}", row.logical_width, row.logical_height),
            row.scale_percent,
            if row.is_primary { "yes" } else { "no" }
        );
    }

    println!();
    println!("Diagnostics:");
    if diagnostics.primary_ok {
        println!("- Primary monitor: ok");
    } else {
        println!(
            "- Primary monitor: expected exactly one primary, found {}",
            diagnostics.primary_count
        );
    }

    if diagnostics.out_of_bounds.is_empty() {
        println!("- Virtual bounds containment: ok");
    } else {
        println!(
            "- Virtual bounds containment: {} monitor(s) out of bounds ({})",
            diagnostics.out_of_bounds.len(),
            diagnostics.out_of_bounds.join(", ")
        );
    }

    if diagnostics.overlaps.is_empty() {
        println!("- Pairwise overlap: none");
    } else {
        println!(
            "- Pairwise overlap: {} overlapping pair(s)",
            diagnostics.overlaps.len()
        );
        for overlap in &diagnostics.overlaps {
            println!(
                "  {} <-> {} overlap={} px",
                overlap.left_device, overlap.right_device, overlap.area_px
            );
        }
    }

    let virtual_area = (virtual_rect.width().max(0) as i64) * (virtual_rect.height().max(0) as i64);
    if virtual_area > 0 {
        println!(
            "- Virtual coverage: gap={} px ({:.2}%), overlap={} px ({:.2}%), max stack={}",
            diagnostics.gap_area_px,
            diagnostics.gap_area_percent,
            diagnostics.overlap_area_px,
            diagnostics.overlap_area_percent,
            diagnostics.max_stack
        );
    } else {
        println!(
            "- Virtual coverage: skipped (invalid virtual desktop size {}x{})",
            virtual_rect.width(),
            virtual_rect.height()
        );
    }

    if validation.enabled {
        println!();
        if validation.passed {
            println!("Validation: passed");
        } else {
            println!("Validation: failed");
            for issue in &validation.issues {
                println!("- {issue}");
            }
        }
    }
}

fn print_monitor_metadata_json(report: &MonitorReport) -> Result<()> {
    let serialized =
        serde_json::to_string_pretty(&report).context("serialize monitor metadata to JSON")?;
    println!("{serialized}");
    Ok(())
}

fn write_monitor_report(path: &Path, report: &MonitorReport) -> Result<()> {
    let serialized =
        serde_json::to_string_pretty(report).context("serialize monitor metadata report")?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create report output directory {}", parent.display()))?;
        }
    }
    std::fs::write(path, serialized)
        .with_context(|| format!("write monitor report {}", path.display()))?;
    Ok(())
}

fn build_monitor_validation(
    rows: &[MonitorRow],
    diagnostics: &MonitorDiagnostics,
    args: &MonitorArgs,
) -> Result<MonitorValidation> {
    let mut issues = Vec::new();
    if let Some(expected_count) = args.expect_count {
        if rows.len() != expected_count {
            issues.push(format!(
                "expected {expected_count} monitor(s), found {}",
                rows.len()
            ));
        }
    }
    if !diagnostics.primary_ok {
        issues.push(format!(
            "expected exactly one primary monitor, found {}",
            diagnostics.primary_count
        ));
    }
    if !diagnostics.out_of_bounds.is_empty() {
        issues.push(format!(
            "{} monitor(s) outside virtual desktop bounds",
            diagnostics.out_of_bounds.len()
        ));
    }
    if args.max_overlap_px.is_none() && !diagnostics.overlaps.is_empty() {
        issues.push(format!(
            "{} overlapping monitor pair(s) detected",
            diagnostics.overlaps.len()
        ));
    }
    if let Some(compare_path) = args.compare_report.as_ref() {
        let baseline = read_monitor_report(compare_path)?;
        compare_monitor_reports(rows, &baseline, &mut issues);
    }
    if let Some(max_gap_px) = args.max_gap_px {
        if diagnostics.gap_area_px > max_gap_px as i64 {
            issues.push(format!(
                "gap area {} px exceeds configured max {} px",
                diagnostics.gap_area_px, max_gap_px
            ));
        }
    }
    if let Some(max_overlap_px) = args.max_overlap_px {
        if diagnostics.overlap_area_px > max_overlap_px as i64 {
            issues.push(format!(
                "overlap area {} px exceeds configured max {} px",
                diagnostics.overlap_area_px, max_overlap_px
            ));
        }
    }

    Ok(MonitorValidation {
        enabled: args.validation_enabled(),
        passed: issues.is_empty(),
        issues,
    })
}

fn compare_monitor_reports(
    current_rows: &[MonitorRow],
    baseline: &MonitorReport,
    issues: &mut Vec<String>,
) {
    if baseline.detected_monitors != current_rows.len() {
        issues.push(format!(
            "baseline report expected {} monitor(s), found {}",
            baseline.detected_monitors,
            current_rows.len()
        ));
    }

    let baseline_virtual = &baseline.virtual_desktop;
    let current_virtual = monitor_virtual_from_rows(current_rows);
    if baseline_virtual.left != current_virtual.left
        || baseline_virtual.top != current_virtual.top
        || baseline_virtual.width != current_virtual.width
        || baseline_virtual.height != current_virtual.height
    {
        issues.push(format!(
            "virtual desktop changed: baseline=({}, {}) {}x{}, current=({}, {}) {}x{}",
            baseline_virtual.left,
            baseline_virtual.top,
            baseline_virtual.width,
            baseline_virtual.height,
            current_virtual.left,
            current_virtual.top,
            current_virtual.width,
            current_virtual.height
        ));
    }

    let mut baseline_by_name = std::collections::HashMap::new();
    for baseline_row in &baseline.monitors {
        baseline_by_name.insert(baseline_row.device_name.as_str(), baseline_row);
    }

    for row in current_rows {
        let Some(base) = baseline_by_name.remove(row.device_name.as_str()) else {
            issues.push(format!("new monitor detected: {}", row.device_name));
            continue;
        };
        if row.origin_x != base.origin_x || row.origin_y != base.origin_y {
            issues.push(format!(
                "{} origin changed: baseline=({}, {}), current=({}, {})",
                row.device_name, base.origin_x, base.origin_y, row.origin_x, row.origin_y
            ));
        }
        if row.width != base.width || row.height != base.height {
            issues.push(format!(
                "{} size changed: baseline={}x{}, current={}x{}",
                row.device_name, base.width, base.height, row.width, row.height
            ));
        }
        if row.dpi_x != base.dpi_x || row.dpi_y != base.dpi_y {
            issues.push(format!(
                "{} dpi changed: baseline={}x{}, current={}x{}",
                row.device_name, base.dpi_x, base.dpi_y, row.dpi_x, row.dpi_y
            ));
        }
        if row.is_primary != base.is_primary {
            issues.push(format!(
                "{} primary flag changed: baseline={}, current={}",
                row.device_name, base.is_primary, row.is_primary
            ));
        }
    }

    for missing in baseline_by_name.keys() {
        issues.push(format!("baseline monitor missing: {}", missing));
    }
}

fn monitor_virtual_from_rows(rows: &[MonitorRow]) -> VirtualDesktopReport {
    if rows.is_empty() {
        return VirtualDesktopReport {
            left: 0,
            top: 0,
            width: 0,
            height: 0,
            area_px: 0,
        };
    }
    let left = rows.iter().map(|row| row.origin_x).min().unwrap_or(0);
    let top = rows.iter().map(|row| row.origin_y).min().unwrap_or(0);
    let right = rows
        .iter()
        .map(|row| row.origin_x.saturating_add(row.width))
        .max()
        .unwrap_or(0);
    let bottom = rows
        .iter()
        .map(|row| row.origin_y.saturating_add(row.height))
        .max()
        .unwrap_or(0);
    let width = (right - left).max(0) as i64;
    let height = (bottom - top).max(0) as i64;
    VirtualDesktopReport {
        left,
        top,
        width,
        height,
        area_px: width * height,
    }
}

fn read_monitor_report(path: &Path) -> Result<MonitorReport> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("read monitor report {}", path.display()))?;
    serde_json::from_str::<MonitorReport>(&content)
        .with_context(|| format!("parse monitor report {}", path.display()))
}

fn build_monitor_rows(monitors: &[MonitorDescriptor]) -> Vec<MonitorRow> {
    let mut rows = Vec::with_capacity(monitors.len());
    for (index, monitor) in monitors.iter().enumerate() {
        let dpi_x = monitor.dpi_x.max(1);
        let dpi_y = monitor.dpi_y.max(1);
        let logical_width = ((monitor.rect.width() as f32 * 96.0) / dpi_x as f32).round() as i32;
        let logical_height = ((monitor.rect.height() as f32 * 96.0) / dpi_y as f32).round() as i32;
        let scale_percent = ((dpi_x as f32 * 100.0) / 96.0).round() as i32;
        rows.push(MonitorRow {
            id: index + 1,
            device_name: monitor.device_name.clone(),
            origin_x: monitor.rect.left,
            origin_y: monitor.rect.top,
            width: monitor.rect.width(),
            height: monitor.rect.height(),
            logical_width,
            logical_height,
            dpi_x: monitor.dpi_x,
            dpi_y: monitor.dpi_y,
            scale_percent,
            is_primary: monitor.is_primary,
        });
    }
    rows
}

fn collect_monitor_diagnostics(
    monitors: &[MonitorDescriptor],
    virtual_rect: RectPx,
) -> MonitorDiagnostics {
    let primary_count = monitors.iter().filter(|monitor| monitor.is_primary).count();

    let mut out_of_bounds = Vec::new();
    for monitor in monitors {
        if monitor.rect.left < virtual_rect.left
            || monitor.rect.top < virtual_rect.top
            || monitor.rect.right > virtual_rect.right
            || monitor.rect.bottom > virtual_rect.bottom
        {
            out_of_bounds.push(monitor.device_name.clone());
        }
    }

    let mut overlaps = Vec::new();
    for left in 0..monitors.len() {
        for right in (left + 1)..monitors.len() {
            let area = intersection_area(monitors[left].rect, monitors[right].rect);
            if area > 0 {
                overlaps.push(MonitorOverlap {
                    left_device: monitors[left].device_name.clone(),
                    right_device: monitors[right].device_name.clone(),
                    area_px: area,
                });
            }
        }
    }

    let layout = analyze_virtual_layout(monitors, virtual_rect);
    let virtual_area = (virtual_rect.width().max(0) as i64) * (virtual_rect.height().max(0) as i64);
    let (gap_area_percent, overlap_area_percent) = if virtual_area > 0 {
        (
            (layout.gap_area as f64 / virtual_area as f64) * 100.0,
            (layout.overlap_area as f64 / virtual_area as f64) * 100.0,
        )
    } else {
        (0.0, 0.0)
    };

    MonitorDiagnostics {
        primary_count,
        primary_ok: primary_count == 1,
        out_of_bounds,
        overlaps,
        gap_area_px: layout.gap_area,
        gap_area_percent,
        overlap_area_px: layout.overlap_area,
        overlap_area_percent,
        max_stack: layout.max_stack,
    }
}

fn trim_monitor_label(input: &str, max_len: usize) -> String {
    if input.chars().count() <= max_len {
        return input.to_string();
    }
    if max_len <= 3 {
        return "...".to_string();
    }
    let mut value = input.chars().take(max_len - 3).collect::<String>();
    value.push_str("...");
    value
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MonitorRow {
    id: usize,
    device_name: String,
    origin_x: i32,
    origin_y: i32,
    width: i32,
    height: i32,
    logical_width: i32,
    logical_height: i32,
    dpi_x: u32,
    dpi_y: u32,
    scale_percent: i32,
    is_primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MonitorOverlap {
    left_device: String,
    right_device: String,
    area_px: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MonitorDiagnostics {
    primary_count: usize,
    primary_ok: bool,
    out_of_bounds: Vec<String>,
    overlaps: Vec<MonitorOverlap>,
    gap_area_px: i64,
    gap_area_percent: f64,
    overlap_area_px: i64,
    overlap_area_percent: f64,
    max_stack: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MonitorValidation {
    enabled: bool,
    passed: bool,
    issues: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct VirtualDesktopReport {
    left: i32,
    top: i32,
    width: i64,
    height: i64,
    area_px: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct MonitorReport {
    detected_monitors: usize,
    virtual_desktop: VirtualDesktopReport,
    monitors: Vec<MonitorRow>,
    diagnostics: MonitorDiagnostics,
    validation: MonitorValidation,
}

#[derive(Debug, Clone, Copy)]
struct VirtualLayoutStats {
    gap_area: i64,
    overlap_area: i64,
    max_stack: usize,
}

fn analyze_virtual_layout(
    monitors: &[MonitorDescriptor],
    virtual_rect: RectPx,
) -> VirtualLayoutStats {
    let mut x_breaks = vec![virtual_rect.left, virtual_rect.right];
    let mut y_breaks = vec![virtual_rect.top, virtual_rect.bottom];

    for monitor in monitors {
        let clamped_left = monitor
            .rect
            .left
            .clamp(virtual_rect.left, virtual_rect.right);
        let clamped_right = monitor
            .rect
            .right
            .clamp(virtual_rect.left, virtual_rect.right);
        let clamped_top = monitor
            .rect
            .top
            .clamp(virtual_rect.top, virtual_rect.bottom);
        let clamped_bottom = monitor
            .rect
            .bottom
            .clamp(virtual_rect.top, virtual_rect.bottom);
        x_breaks.push(clamped_left);
        x_breaks.push(clamped_right);
        y_breaks.push(clamped_top);
        y_breaks.push(clamped_bottom);
    }

    x_breaks.sort_unstable();
    x_breaks.dedup();
    y_breaks.sort_unstable();
    y_breaks.dedup();

    let mut gap_area = 0_i64;
    let mut overlap_area = 0_i64;
    let mut max_stack = 0_usize;

    if x_breaks.len() < 2 || y_breaks.len() < 2 {
        return VirtualLayoutStats {
            gap_area,
            overlap_area,
            max_stack,
        };
    }

    for x_idx in 0..(x_breaks.len() - 1) {
        let left = x_breaks[x_idx];
        let right = x_breaks[x_idx + 1];
        if right <= left {
            continue;
        }

        for y_idx in 0..(y_breaks.len() - 1) {
            let top = y_breaks[y_idx];
            let bottom = y_breaks[y_idx + 1];
            if bottom <= top {
                continue;
            }

            let area = ((right - left) as i64) * ((bottom - top) as i64);
            let mut cover_count = 0_usize;
            for monitor in monitors {
                if monitor.rect.left < right
                    && monitor.rect.right > left
                    && monitor.rect.top < bottom
                    && monitor.rect.bottom > top
                {
                    cover_count += 1;
                }
            }

            if cover_count == 0 {
                gap_area += area;
            } else if cover_count > 1 {
                overlap_area += area;
            }
            if cover_count > max_stack {
                max_stack = cover_count;
            }
        }
    }

    VirtualLayoutStats {
        gap_area,
        overlap_area,
        max_stack,
    }
}

fn intersection_area(left: RectPx, right: RectPx) -> i64 {
    let intersect_left = left.left.max(right.left);
    let intersect_top = left.top.max(right.top);
    let intersect_right = left.right.min(right.right);
    let intersect_bottom = left.bottom.min(right.bottom);
    if intersect_right <= intersect_left || intersect_bottom <= intersect_top {
        return 0;
    }
    ((intersect_right - intersect_left) as i64) * ((intersect_bottom - intersect_top) as i64)
}

struct HotkeyRegistrationGuard {
    hwnd: HWND,
    id: i32,
}

impl Drop for HotkeyRegistrationGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = UnregisterHotKey(self.hwnd, self.id);
        }
    }
}

fn transition_mode(state: &mut AppState, mode: AppMode) {
    state.mode = mode;
    tracing::debug!("mode -> {:?}", state.mode);
}

fn trigger_capture(
    state: &mut AppState,
    target: CaptureTarget,
    config: &AppConfig,
    editor_options: &EditorRuntimeOptions,
) -> Result<()> {
    transition_mode(state, AppMode::Capture);
    let result = (|| -> Result<()> {
        let should_region_edit = config.open_editor && target == CaptureTarget::Region;
        if should_region_edit {
            transition_mode(state, AppMode::Edit);
        }

        let Some(captured) = acquire_capture_frame(
            target,
            config.default_delay_ms,
            should_region_edit,
            editor_options,
        )?
        else {
            println!("Capture canceled.");
            return Ok(());
        };

        if should_region_edit {
            transition_mode(state, AppMode::Capture);
        }

        let plan = resolve_output_plan(config.copy_to_clipboard, None, captured.output_action);

        emit_capture_output(
            target,
            &captured.frame.image,
            captured.frame.bounds,
            plan,
            None,
            &config.save_dir,
            &config.filename_template,
        )
    })();
    transition_mode(state, AppMode::Idle);
    result
}

fn acquire_capture_frame(
    target: CaptureTarget,
    delay_ms: u64,
    should_region_edit: bool,
    editor_options: &EditorRuntimeOptions,
) -> Result<Option<CapturedFrame>> {
    if target == CaptureTarget::Region {
        if delay_ms > 0 {
            thread::sleep(Duration::from_millis(delay_ms));
        }

        let frozen_frame = capture::capture_rect(virtual_screen_rect())?;
        let selected = if should_region_edit {
            region_overlay::select_region_immediate_from_frame(&frozen_frame)?
        } else {
            Some(region_overlay::select_region_from_frame(&frozen_frame)?)
        };
        let Some(selection) = selected else {
            return Ok(None);
        };
        let initial_region = selection.rect;

        if should_region_edit {
            let edit_result = match region_editor::edit_region(
                initial_region,
                &editor_options.keybindings,
                editor_options.text_commit_feedback_color,
                editor_options.radial_menu_animation_speed,
                editor_options.annotation_palette,
                Some(&frozen_frame),
                selection.precomputed_snapshot,
            )? {
                RegionEditOutcome::Apply(result) => result,
                RegionEditOutcome::Cancel => return Ok(None),
            };

            let mut frame = crop_capture_frame(&frozen_frame, edit_result.bounds())?;
            region_editor::apply_annotations(&mut frame.image, &edit_result);
            return Ok(Some(CapturedFrame {
                frame,
                output_action: edit_result.output_action(),
            }));
        }

        let frame = crop_capture_frame(&frozen_frame, initial_region)?;
        return Ok(Some(CapturedFrame {
            frame,
            output_action: None,
        }));
    }

    let frame = capture::capture_target_with_delay(target, delay_ms)?;
    Ok(Some(CapturedFrame {
        frame,
        output_action: None,
    }))
}

fn crop_capture_frame(
    source: &CaptureFrame,
    bounds: crate::platform_windows::RectPx,
) -> Result<CaptureFrame> {
    if bounds.left < source.bounds.left
        || bounds.top < source.bounds.top
        || bounds.right > source.bounds.right
        || bounds.bottom > source.bounds.bottom
    {
        bail!("crop bounds are outside the source frame");
    }

    let width = bounds.width();
    let height = bounds.height();
    if width <= 0 || height <= 0 {
        bail!("invalid crop bounds {}x{}", width, height);
    }

    let src_x = (bounds.left - source.bounds.left) as u32;
    let src_y = (bounds.top - source.bounds.top) as u32;
    let cropped =
        imageops::crop_imm(&source.image, src_x, src_y, width as u32, height as u32).to_image();
    Ok(CaptureFrame {
        bounds,
        image: cropped,
    })
}

fn refresh_runtime_config(
    config_path: &Path,
    app_config: &mut AppConfig,
    editor_options: &mut EditorRuntimeOptions,
    hotkey_hwnd: HWND,
    hotkey_id: i32,
    registered_hotkey: &mut crate::hotkey::Hotkey,
) {
    let Ok(mut next_config) = load_config_from_path(config_path) else {
        return;
    };

    if next_config.capture_hotkey != app_config.capture_hotkey {
        match parse_hotkey(&next_config.capture_hotkey) {
            Ok(next_hotkey) => {
                if next_hotkey != *registered_hotkey {
                    unsafe {
                        let _ = UnregisterHotKey(hotkey_hwnd, hotkey_id);
                    }
                    match unsafe {
                        RegisterHotKey(
                            hotkey_hwnd,
                            hotkey_id,
                            next_hotkey.modifiers,
                            next_hotkey.vk,
                        )
                    } {
                        Ok(()) => {
                            *registered_hotkey = next_hotkey;
                            println!("Hotkey updated: {}", next_config.capture_hotkey);
                        }
                        Err(err) => {
                            eprintln!(
                                "Hotkey update failed for `{}`: {err}. Keeping previous hotkey `{}`.",
                                next_config.capture_hotkey, app_config.capture_hotkey
                            );
                            let _ = unsafe {
                                RegisterHotKey(
                                    hotkey_hwnd,
                                    hotkey_id,
                                    registered_hotkey.modifiers,
                                    registered_hotkey.vk,
                                )
                            };
                            next_config.capture_hotkey = app_config.capture_hotkey.clone();
                        }
                    }
                }
            }
            Err(err) => {
                eprintln!(
                    "Ignoring invalid capture_hotkey `{}` in {}: {err}",
                    next_config.capture_hotkey,
                    config_path.display()
                );
                next_config.capture_hotkey = app_config.capture_hotkey.clone();
            }
        }
    }

    let Ok(next_editor_options) = resolve_editor_options_from_data(config_path, &next_config)
    else {
        return;
    };
    *app_config = next_config;
    *editor_options = next_editor_options;
}

fn resolve_editor_options(loaded: &crate::config::LoadedConfig) -> Result<EditorRuntimeOptions> {
    resolve_editor_options_from_data(&loaded.path, &loaded.data)
}

fn resolve_editor_options_from_data(
    config_path: &Path,
    config: &AppConfig,
) -> Result<EditorRuntimeOptions> {
    let keybindings = region_editor::EditorKeybindings::from_config(&config.editor.shortcuts)
        .with_context(|| {
            format!(
                "invalid editor shortcut config in {}",
                config_path.display()
            )
        })?;
    let text_commit_feedback_color =
        region_editor::parse_hex_rgb_color(&config.editor.text_commit_feedback_color)
            .with_context(|| {
                format!(
                    "invalid editor.text_commit_feedback_color in {}",
                    config_path.display()
                )
            })?;
    Ok(EditorRuntimeOptions {
        keybindings,
        text_commit_feedback_color,
        radial_menu_animation_speed: config.editor.radial_menu_animation_speed,
        annotation_palette: parse_annotation_palette(config_path, config)?,
    })
}

fn parse_annotation_palette(
    config_path: &Path,
    config: &AppConfig,
) -> Result<[[u8; 4]; ANNOTATION_PALETTE_SIZE]> {
    let mut palette = [[0u8; 4]; ANNOTATION_PALETTE_SIZE];
    for (idx, raw) in config.editor.annotation_palette.iter().enumerate() {
        let rgb = region_editor::parse_hex_rgb_color(raw).with_context(|| {
            format!(
                "invalid editor.annotation_palette[{}] in {}",
                idx,
                config_path.display()
            )
        })?;
        palette[idx] = [rgb[0], rgb[1], rgb[2], 255];
    }
    Ok(palette)
}

fn resolve_output_plan(
    should_copy: bool,
    output: Option<&PathBuf>,
    editor_action: Option<EditorOutputAction>,
) -> OutputPlan {
    let default = OutputPlan {
        copy: should_copy,
        save: output.is_some() || !should_copy,
        pin: false,
    };
    match editor_action {
        None => default,
        Some(EditorOutputAction::Copy) => OutputPlan {
            copy: true,
            // Preserve explicit output path behavior when present.
            save: output.is_some(),
            pin: false,
        },
        Some(EditorOutputAction::Save) => OutputPlan {
            copy: false,
            save: true,
            pin: false,
        },
        Some(EditorOutputAction::CopyAndSave) => OutputPlan {
            copy: true,
            save: true,
            pin: false,
        },
        Some(EditorOutputAction::Pin) => OutputPlan {
            copy: false,
            // Preserve explicit output path behavior when present.
            save: output.is_some(),
            pin: true,
        },
    }
}

fn emit_capture_output(
    target: CaptureTarget,
    image: &RgbaImage,
    bounds: crate::platform_windows::RectPx,
    plan: OutputPlan,
    output: Option<PathBuf>,
    save_dir: &Path,
    filename_template: &str,
) -> Result<()> {
    if plan.pin {
        pinned_capture::show_pinned_capture(image, save_dir, filename_template)?;
        println!(
            "Pinned capture opened. Drag to move; wheel to zoom; right-click for actions; Esc to close."
        );
    }

    if plan.copy {
        copy_to_clipboard(image)?;
        println!(
            "Copied to clipboard ({}x{}).",
            image.width(),
            image.height()
        );
    }

    if plan.save {
        if let Some(path) = save_png(image, output, save_dir, filename_template)? {
            println!("Saved: {}", path.display());
        } else {
            println!("Save canceled.");
        }
    }

    println!(
        "Captured {} at px rect ({}, {}) {}x{}",
        target,
        bounds.left,
        bounds.top,
        bounds.width(),
        bounds.height()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn row(
        id: usize,
        device_name: &str,
        origin_x: i32,
        origin_y: i32,
        width: i32,
        height: i32,
        is_primary: bool,
    ) -> MonitorRow {
        MonitorRow {
            id,
            device_name: device_name.to_string(),
            origin_x,
            origin_y,
            width,
            height,
            logical_width: width,
            logical_height: height,
            dpi_x: 96,
            dpi_y: 96,
            scale_percent: 100,
            is_primary,
        }
    }

    fn monitor(
        device_name: &str,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        dpi_x: u32,
        dpi_y: u32,
        is_primary: bool,
    ) -> MonitorDescriptor {
        MonitorDescriptor {
            device_name: device_name.to_string(),
            rect: RectPx {
                left,
                top,
                right,
                bottom,
            },
            dpi_x,
            dpi_y,
            is_primary,
        }
    }

    fn test_args() -> MonitorArgs {
        MonitorArgs {
            json: false,
            validate: true,
            expect_count: None,
            report: None,
            compare_report: None,
            max_gap_px: None,
            max_overlap_px: None,
        }
    }

    fn diagnostics(
        primary_count: usize,
        overlaps: Vec<MonitorOverlap>,
        gap_area_px: i64,
        overlap_area_px: i64,
    ) -> MonitorDiagnostics {
        MonitorDiagnostics {
            primary_count,
            primary_ok: primary_count == 1,
            out_of_bounds: Vec::new(),
            overlaps,
            gap_area_px,
            gap_area_percent: 0.0,
            overlap_area_px,
            overlap_area_percent: 0.0,
            max_stack: 1,
        }
    }

    fn report(rows: Vec<MonitorRow>) -> MonitorReport {
        MonitorReport {
            detected_monitors: rows.len(),
            virtual_desktop: monitor_virtual_from_rows(&rows),
            monitors: rows,
            diagnostics: diagnostics(1, Vec::new(), 0, 0),
            validation: MonitorValidation {
                enabled: false,
                passed: true,
                issues: Vec::new(),
            },
        }
    }

    fn unique_temp_file(stem: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("pyro-{stem}-{}-{suffix}.json", std::process::id()))
    }

    #[test]
    fn analyze_virtual_layout_detects_gap() {
        let monitors = vec![
            monitor("DISPLAY1", 0, 0, 1920, 1080, 96, 96, true),
            monitor("DISPLAY2", 2200, 0, 4120, 1080, 96, 96, false),
        ];
        let virtual_rect = RectPx {
            left: 0,
            top: 0,
            right: 4120,
            bottom: 1080,
        };

        let stats = analyze_virtual_layout(&monitors, virtual_rect);
        assert!(stats.gap_area > 0);
        assert_eq!(stats.overlap_area, 0);
    }

    #[test]
    fn analyze_virtual_layout_detects_overlap() {
        let monitors = vec![
            monitor("DISPLAY1", 0, 0, 100, 100, 96, 96, true),
            monitor("DISPLAY2", 50, 0, 150, 100, 96, 96, false),
        ];
        let virtual_rect = RectPx {
            left: 0,
            top: 0,
            right: 150,
            bottom: 100,
        };
        let stats = analyze_virtual_layout(&monitors, virtual_rect);
        assert_eq!(stats.gap_area, 0);
        assert_eq!(stats.overlap_area, 5000);
        assert_eq!(stats.max_stack, 2);
    }

    #[test]
    fn collect_monitor_diagnostics_detects_out_of_bounds_and_overlap() {
        let monitors = vec![
            monitor("DISPLAY1", 0, 0, 100, 100, 96, 96, true),
            monitor("DISPLAY2", 80, 0, 180, 100, 96, 96, false),
            monitor("DISPLAY3", 200, 0, 300, 100, 96, 96, false),
        ];
        let virtual_rect = RectPx {
            left: 0,
            top: 0,
            right: 250,
            bottom: 100,
        };
        let result = collect_monitor_diagnostics(&monitors, virtual_rect);
        assert_eq!(result.primary_count, 1);
        assert_eq!(result.out_of_bounds, vec!["DISPLAY3".to_string()]);
        assert_eq!(result.overlaps.len(), 1);
        assert_eq!(result.overlaps[0].area_px, 2000);
    }

    #[test]
    fn compare_monitor_reports_flags_new_monitor() {
        let current_rows = vec![
            row(1, "DISPLAY1", 0, 0, 1920, 1080, true),
            row(2, "DISPLAY2", 1920, 0, 1920, 1080, false),
        ];
        let baseline = report(vec![row(1, "DISPLAY1", 0, 0, 1920, 1080, true)]);

        let mut issues = Vec::new();
        compare_monitor_reports(&current_rows, &baseline, &mut issues);
        assert!(!issues.is_empty());
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("new monitor detected"))
        );
    }

    #[test]
    fn compare_monitor_reports_flags_changed_origin_and_primary() {
        let current_rows = vec![row(1, "DISPLAY1", 100, 0, 1920, 1080, false)];
        let baseline = report(vec![row(1, "DISPLAY1", 0, 0, 1920, 1080, true)]);
        let mut issues = Vec::new();
        compare_monitor_reports(&current_rows, &baseline, &mut issues);
        assert!(
            issues.iter().any(|issue| issue.contains("origin changed")),
            "{issues:?}"
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("primary flag changed")),
            "{issues:?}"
        );
    }

    #[test]
    fn build_monitor_validation_respects_gap_threshold() {
        let rows = vec![row(1, "DISPLAY1", 0, 0, 1920, 1080, true)];
        let mut diagnostics = diagnostics(1, Vec::new(), 400, 0);
        diagnostics.gap_area_percent = 1.0;

        let mut args = test_args();
        args.max_gap_px = Some(300);
        let result = build_monitor_validation(&rows, &diagnostics, &args).expect("validation");
        assert!(!result.passed);
        assert!(result.issues.iter().any(|issue| issue.contains("gap area")));

        args.max_gap_px = Some(500);
        let result = build_monitor_validation(&rows, &diagnostics, &args).expect("validation");
        assert!(result.passed);
    }

    #[test]
    fn build_monitor_validation_overlap_default_vs_threshold() {
        let rows = vec![row(1, "DISPLAY1", 0, 0, 1920, 1080, true)];
        let overlap = MonitorOverlap {
            left_device: "DISPLAY1".to_string(),
            right_device: "DISPLAY2".to_string(),
            area_px: 200,
        };
        let diagnostics = diagnostics(1, vec![overlap], 0, 200);

        let mut args = test_args();
        let result = build_monitor_validation(&rows, &diagnostics, &args).expect("validation");
        assert!(!result.passed);
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.contains("overlapping monitor pair")),
            "{:?}",
            result.issues
        );

        args.max_overlap_px = Some(250);
        let result = build_monitor_validation(&rows, &diagnostics, &args).expect("validation");
        assert!(result.passed, "{:?}", result.issues);

        args.max_overlap_px = Some(100);
        let result = build_monitor_validation(&rows, &diagnostics, &args).expect("validation");
        assert!(!result.passed);
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.contains("overlap area")),
            "{:?}",
            result.issues
        );
    }

    #[test]
    fn validation_enabled_when_any_threshold_or_compare_is_set() {
        let mut args = test_args();
        args.validate = false;
        assert!(!args.validation_enabled());
        args.max_gap_px = Some(1);
        assert!(args.validation_enabled());
        args.max_gap_px = None;
        args.max_overlap_px = Some(1);
        assert!(args.validation_enabled());
        args.max_overlap_px = None;
        args.compare_report = Some(std::path::PathBuf::from("baseline.json"));
        assert!(args.validation_enabled());
    }

    #[test]
    fn monitor_virtual_from_rows_handles_negative_coordinates() {
        let rows = vec![
            row(1, "DISPLAY1", -1920, 0, 1920, 1080, false),
            row(2, "DISPLAY2", 0, -120, 1920, 1200, true),
        ];
        let virtual_rect = monitor_virtual_from_rows(&rows);
        assert_eq!(virtual_rect.left, -1920);
        assert_eq!(virtual_rect.top, -120);
        assert_eq!(virtual_rect.width, 3840);
        assert_eq!(virtual_rect.height, 1200);
        assert_eq!(virtual_rect.area_px, 4_608_000);
    }

    #[test]
    fn trim_monitor_label_behaves_for_short_and_long_values() {
        assert_eq!(trim_monitor_label("ABC", 5), "ABC");
        assert_eq!(trim_monitor_label("ABCDEFG", 3), "...");
        assert_eq!(trim_monitor_label("ABCDEFG", 6), "ABC...");
    }

    #[test]
    fn read_write_monitor_report_roundtrip() {
        let path = unique_temp_file("monitor-roundtrip");
        let report = report(vec![
            row(1, "DISPLAY1", 0, 0, 1920, 1080, true),
            row(2, "DISPLAY2", 1920, 0, 1920, 1080, false),
        ]);
        write_monitor_report(&path, &report).expect("write report");
        let loaded = read_monitor_report(&path).expect("read report");
        assert_eq!(loaded.detected_monitors, 2);
        assert_eq!(loaded.monitors.len(), 2);
        assert_eq!(loaded.monitors[0].device_name, "DISPLAY1");
        assert_eq!(loaded.virtual_desktop.width, 3840);
        assert_eq!(loaded.virtual_desktop.height, 1080);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn build_monitor_validation_returns_error_for_missing_compare_report() {
        let rows = vec![row(1, "DISPLAY1", 0, 0, 1920, 1080, true)];
        let diagnostics = diagnostics(1, Vec::new(), 0, 0);
        let mut args = test_args();
        args.compare_report = Some(std::path::PathBuf::from(
            "Z:\\definitely-missing-pyro-monitor-report.json",
        ));
        let result = build_monitor_validation(&rows, &diagnostics, &args);
        assert!(result.is_err());
    }
}
