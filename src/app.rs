use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{ArgAction, Args, Parser, Subcommand};
use image::RgbaImage;
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
use crate::platform_windows::monitor_count;
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

    const HOTKEY_ID: i32 = 1;
    unsafe {
        RegisterHotKey(tray.hwnd(), HOTKEY_ID, hotkey.modifiers, hotkey.vk)
            .context("register global hotkey failed")?;
    }
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

        if msg.message == WM_HOTKEY && msg.wParam.0 == HOTKEY_ID as usize {
            refresh_runtime_config(&config_path, &mut app_config, &mut editor_options);
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
                    refresh_runtime_config(&config_path, &mut app_config, &mut editor_options);
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
                    refresh_runtime_config(&config_path, &mut app_config, &mut editor_options);
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
                    refresh_runtime_config(&config_path, &mut app_config, &mut editor_options);
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
                    refresh_runtime_config(&config_path, &mut app_config, &mut editor_options);
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
    )?;
    let _ = AppMode::Edit;

    Ok(())
}

fn print_monitor_metadata() -> Result<()> {
    let monitors = capture::enumerate_monitors()?;
    println!("Detected {} monitor(s)", monitors.len());
    for monitor in monitors {
        println!(
            "{}: rect=({}, {}) {}x{} dpi={}x{} primary={}",
            monitor.device_name,
            monitor.rect.left,
            monitor.rect.top,
            monitor.rect.width(),
            monitor.rect.height(),
            monitor.dpi_x,
            monitor.dpi_y,
            monitor.is_primary
        );
    }
    Ok(())
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
    if should_region_edit && target == CaptureTarget::Region {
        if delay_ms > 0 {
            thread::sleep(Duration::from_millis(delay_ms));
        }

        let Some(initial_region) = region_overlay::select_region_immediate()? else {
            return Ok(None);
        };

        let edit_result = match region_editor::edit_region(
            initial_region,
            &editor_options.keybindings,
            editor_options.text_commit_feedback_color,
            editor_options.radial_menu_animation_speed,
            editor_options.annotation_palette,
        )? {
            RegionEditOutcome::Apply(result) => result,
            RegionEditOutcome::Cancel => return Ok(None),
        };

        let mut frame = capture::capture_rect(edit_result.bounds())?;
        region_editor::apply_annotations(&mut frame.image, &edit_result);
        return Ok(Some(CapturedFrame {
            frame,
            output_action: edit_result.output_action(),
        }));
    }

    let frame = capture::capture_target_with_delay(target, delay_ms)?;
    Ok(Some(CapturedFrame {
        frame,
        output_action: None,
    }))
}

fn refresh_runtime_config(
    config_path: &Path,
    app_config: &mut AppConfig,
    editor_options: &mut EditorRuntimeOptions,
) {
    let Ok(next_config) = load_config_from_path(config_path) else {
        return;
    };
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
    };
    match editor_action {
        None => default,
        Some(EditorOutputAction::Copy) => OutputPlan {
            copy: true,
            // Preserve explicit output path behavior when present.
            save: output.is_some(),
        },
        Some(EditorOutputAction::Save) => OutputPlan {
            copy: false,
            save: true,
        },
        Some(EditorOutputAction::CopyAndSave) => OutputPlan {
            copy: true,
            save: true,
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
) -> Result<()> {
    if plan.copy {
        copy_to_clipboard(image)?;
        println!(
            "Copied to clipboard ({}x{}).",
            image.width(),
            image.height()
        );
    }

    if plan.save {
        if let Some(path) = save_png(image, output, save_dir)? {
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
