#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
mod app;
#[cfg(target_os = "windows")]
mod capture;
#[cfg(target_os = "windows")]
mod config;
#[cfg(target_os = "windows")]
mod hotkey;
#[cfg(target_os = "windows")]
mod logging;
#[cfg(target_os = "windows")]
mod output;
#[cfg(target_os = "windows")]
mod pinned_capture;
#[cfg(target_os = "windows")]
mod platform_windows;
#[cfg(target_os = "windows")]
mod region_editor;
#[cfg(target_os = "windows")]
mod region_overlay;
#[cfg(target_os = "windows")]
mod settings_ui;
#[cfg(target_os = "windows")]
mod tray;

#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    attach_parent_console();
    logging::init();
    platform_windows::init_process_dpi_awareness();
    app::run()
}

#[cfg(target_os = "windows")]
fn attach_parent_console() {
    use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("Pyro currently supports Windows only.");
}
