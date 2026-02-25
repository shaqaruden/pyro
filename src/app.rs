use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{ArgAction, Args, Parser, Subcommand};

use crate::capture::{self, CaptureTarget};
use crate::config::load_or_create_config;
use crate::output::{copy_to_clipboard, save_png};
use crate::platform_windows::monitor_count;

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
    /// Start background lifecycle skeleton (tray/hotkey to be added next)
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
        Command::Run => run_lifecycle_stub(&loaded),
        Command::Capture(args) => run_capture(args, &loaded),
        Command::Monitors => print_monitor_metadata(),
    }
}

fn run_lifecycle_stub(loaded: &crate::config::LoadedConfig) -> Result<()> {
    let state = AppState {
        mode: AppMode::Idle,
    };
    println!("Pyro lifecycle skeleton is ready.");
    println!("Config: {}", loaded.path.display());
    println!("Hotkey (configured): {}", loaded.data.capture_hotkey);
    println!("Initial mode: {:?}", state.mode);
    println!("Detected monitors: {}", monitor_count());
    println!("Use `pyro capture --target all-displays --clipboard` to test capture.");
    tracing::info!("lifecycle skeleton initialized");
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
