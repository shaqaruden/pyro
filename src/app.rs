use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{ArgAction, Args, Parser, Subcommand};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MSG, PostQuitMessage, TranslateMessage, WM_HOTKEY,
};

use crate::capture::{self, CaptureTarget};
use crate::config::{AppConfig, load_or_create_config};
use crate::hotkey::parse_hotkey;
use crate::output::{copy_to_clipboard, save_png};
use crate::platform_windows::monitor_count;
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
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let loaded = load_or_create_config()?;

    match cli.command.unwrap_or(Command::Run) {
        Command::Run => run_hotkey_listener(&loaded),
        Command::Capture(args) => run_capture(args, &loaded),
        Command::Monitors => print_monitor_metadata(),
    }
}

fn run_hotkey_listener(loaded: &crate::config::LoadedConfig) -> Result<()> {
    let state = AppState {
        mode: AppMode::Idle,
    };
    let tray = TrayHost::create().context("initialize tray icon failed")?;

    let hotkey = parse_hotkey(&loaded.data.capture_hotkey).with_context(|| {
        format!(
            "invalid capture_hotkey in {}: {}",
            loaded.path.display(),
            loaded.data.capture_hotkey
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
    println!("Hotkey (configured): {}", loaded.data.capture_hotkey);
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
            if let Err(err) = trigger_capture(&mut state, loaded.data.default_target, &loaded.data)
            {
                tracing::error!("hotkey capture failed: {err:#}");
                eprintln!("Hotkey capture failed: {err:#}");
            }
            continue;
        }

        if msg.message == TRAY_ACTION_MESSAGE {
            let action = TrayAction::from_code(msg.wParam.0);
            match action {
                Some(TrayAction::CaptureDefault) => {
                    if let Err(err) =
                        trigger_capture(&mut state, loaded.data.default_target, &loaded.data)
                    {
                        tracing::error!("tray capture failed: {err:#}");
                        eprintln!("Tray capture failed: {err:#}");
                    }
                }
                Some(TrayAction::CapturePrimary) => {
                    if let Err(err) =
                        trigger_capture(&mut state, CaptureTarget::Primary, &loaded.data)
                    {
                        tracing::error!("tray capture failed: {err:#}");
                        eprintln!("Tray capture failed: {err:#}");
                    }
                }
                Some(TrayAction::CaptureAllDisplays) => {
                    if let Err(err) =
                        trigger_capture(&mut state, CaptureTarget::AllDisplays, &loaded.data)
                    {
                        tracing::error!("tray capture failed: {err:#}");
                        eprintln!("Tray capture failed: {err:#}");
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
    if args.clipboard && args.no_clipboard {
        bail!("cannot use --clipboard and --no-clipboard together");
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

    let frame = capture::capture_target_with_delay(target, delay_ms)?;

    if should_copy {
        copy_to_clipboard(&frame.image)?;
        println!(
            "Copied to clipboard ({}x{}).",
            frame.image.width(),
            frame.image.height()
        );
    }

    if args.output.is_some() || !should_copy {
        let path = save_png(&frame.image, args.output, &loaded.data.save_dir)?;
        println!("Saved: {}", path.display());
    }

    println!(
        "Captured {} at px rect ({}, {}) {}x{}",
        target,
        frame.bounds.left,
        frame.bounds.top,
        frame.bounds.width(),
        frame.bounds.height()
    );

    let _ = AppMode::Edit;

    Ok(())
}

fn capture_from_config_target(target: CaptureTarget, config: &AppConfig) -> Result<()> {
    let frame = capture::capture_target_with_delay(target, config.default_delay_ms)?;

    if config.copy_to_clipboard {
        copy_to_clipboard(&frame.image)?;
        println!(
            "Copied to clipboard ({}x{}).",
            frame.image.width(),
            frame.image.height()
        );
    } else {
        let path = save_png(&frame.image, None, &config.save_dir)?;
        println!("Saved: {}", path.display());
    }

    println!(
        "Captured {} at px rect ({}, {}) {}x{}",
        target,
        frame.bounds.left,
        frame.bounds.top,
        frame.bounds.width(),
        frame.bounds.height()
    );

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

fn trigger_capture(state: &mut AppState, target: CaptureTarget, config: &AppConfig) -> Result<()> {
    transition_mode(state, AppMode::Capture);
    let result = capture_from_config_target(target, config);
    transition_mode(state, AppMode::Idle);
    result
}
