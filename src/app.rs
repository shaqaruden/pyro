use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{ArgAction, Args, Parser, Subcommand};
use image::{RgbaImage, imageops};
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
use crate::platform_windows::{monitor_count, virtual_screen_rect};
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
    Monitors,
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
        Command::Monitors => print_monitor_metadata(),
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

fn print_monitor_metadata() -> Result<()> {
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

    let virtual_width = virtual_rect.width().max(0) as i64;
    let virtual_height = virtual_rect.height().max(0) as i64;
    let virtual_area = virtual_width * virtual_height;

    println!("Detected {} monitor(s)", monitors.len());
    println!(
        "Virtual desktop: origin=({}, {}) size={}x{} area={} px",
        virtual_rect.left, virtual_rect.top, virtual_width, virtual_height, virtual_area
    );
    println!(
        "{:<3} {:<18} {:>11} {:>11} {:>11} {:>8} {:>8}",
        "ID", "Device", "Origin(px)", "Size(px)", "Logical", "Scale", "Primary"
    );
    for (index, monitor) in monitors.iter().enumerate() {
        let dpi_x = monitor.dpi_x.max(1);
        let dpi_y = monitor.dpi_y.max(1);
        let logical_width = ((monitor.rect.width() as f32 * 96.0) / dpi_x as f32).round() as i32;
        let logical_height = ((monitor.rect.height() as f32 * 96.0) / dpi_y as f32).round() as i32;
        let scale = ((dpi_x as f32 * 100.0) / 96.0).round();
        println!(
            "{:<3} {:<18} {:>11} {:>11} {:>11} {:>7}% {:>8}",
            index + 1,
            trim_monitor_label(&monitor.device_name, 18),
            format!("{},{}", monitor.rect.left, monitor.rect.top),
            format!("{}x{}", monitor.rect.width(), monitor.rect.height()),
            format!("{}x{}", logical_width, logical_height),
            scale as i32,
            if monitor.is_primary { "yes" } else { "no" }
        );
    }

    print_monitor_diagnostics(&monitors, virtual_rect);
    Ok(())
}

fn print_monitor_diagnostics(
    monitors: &[crate::platform_windows::MonitorDescriptor],
    virtual_rect: crate::platform_windows::RectPx,
) {
    let primary_count = monitors.iter().filter(|monitor| monitor.is_primary).count();
    println!();
    println!("Diagnostics:");
    if primary_count == 1 {
        println!("- Primary monitor: ok");
    } else {
        println!(
            "- Primary monitor: expected exactly one primary, found {}",
            primary_count
        );
    }

    let mut out_of_bounds = Vec::new();
    for monitor in monitors {
        if monitor.rect.left < virtual_rect.left
            || monitor.rect.top < virtual_rect.top
            || monitor.rect.right > virtual_rect.right
            || monitor.rect.bottom > virtual_rect.bottom
        {
            out_of_bounds.push(monitor.device_name.as_str());
        }
    }
    if out_of_bounds.is_empty() {
        println!("- Virtual bounds containment: ok");
    } else {
        println!(
            "- Virtual bounds containment: {} monitor(s) out of bounds ({})",
            out_of_bounds.len(),
            out_of_bounds.join(", ")
        );
    }

    let mut overlaps = Vec::new();
    for left in 0..monitors.len() {
        for right in (left + 1)..monitors.len() {
            let area = intersection_area(monitors[left].rect, monitors[right].rect);
            if area > 0 {
                overlaps.push((left, right, area));
            }
        }
    }
    if overlaps.is_empty() {
        println!("- Pairwise overlap: none");
    } else {
        println!("- Pairwise overlap: {} overlapping pair(s)", overlaps.len());
        for (left, right, area) in overlaps {
            println!(
                "  {} <-> {} overlap={} px",
                monitors[left].device_name, monitors[right].device_name, area
            );
        }
    }

    let layout = analyze_virtual_layout(monitors, virtual_rect);
    let virtual_area = (virtual_rect.width().max(0) as i64) * (virtual_rect.height().max(0) as i64);
    if virtual_area > 0 {
        let gap_percent = (layout.gap_area as f64 / virtual_area as f64) * 100.0;
        let overlap_percent = (layout.overlap_area as f64 / virtual_area as f64) * 100.0;
        println!(
            "- Virtual coverage: gap={} px ({:.2}%), overlap={} px ({:.2}%), max stack={}",
            layout.gap_area, gap_percent, layout.overlap_area, overlap_percent, layout.max_stack
        );
    } else {
        println!(
            "- Virtual coverage: skipped (invalid virtual desktop size {}x{})",
            virtual_rect.width(),
            virtual_rect.height()
        );
    }
}

fn trim_monitor_label(input: &str, max_len: usize) -> String {
    if input.chars().count() <= max_len {
        return input.to_string();
    }
    if max_len <= 1 {
        return "…".to_string();
    }
    let mut value = input.chars().take(max_len - 1).collect::<String>();
    value.push('…');
    value
}

#[derive(Debug, Clone, Copy)]
struct VirtualLayoutStats {
    gap_area: i64,
    overlap_area: i64,
    max_stack: usize,
}

fn analyze_virtual_layout(
    monitors: &[crate::platform_windows::MonitorDescriptor],
    virtual_rect: crate::platform_windows::RectPx,
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

fn intersection_area(
    left: crate::platform_windows::RectPx,
    right: crate::platform_windows::RectPx,
) -> i64 {
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
