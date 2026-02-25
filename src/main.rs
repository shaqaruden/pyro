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
mod platform_windows;
#[cfg(target_os = "windows")]
mod region_editor;
#[cfg(target_os = "windows")]
mod region_overlay;
#[cfg(target_os = "windows")]
mod tray;

#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    logging::init();
    platform_windows::init_process_dpi_awareness();
    app::run()
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("Pyro currently supports Windows only.");
}
