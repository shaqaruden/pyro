use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn launch_settings_window(config_path: &Path) -> Result<()> {
    let exe = resolve_settings_exe()?;
    Command::new(&exe)
        .arg(config_path)
        .spawn()
        .with_context(|| format!("launch settings UI {}", exe.display()))?;
    Ok(())
}

fn resolve_settings_exe() -> Result<PathBuf> {
    if let Some(path) = env::var_os("PYRO_SETTINGS_EXE") {
        let candidate = PathBuf::from(path);
        if candidate.exists() {
            return Ok(candidate);
        }
        bail!(
            "PYRO_SETTINGS_EXE points to a missing file: {}",
            candidate.display()
        );
    }

    let mut candidates = Vec::new();
    if let Ok(current_exe) = env::current_exe()
        && let Some(dir) = current_exe.parent()
    {
        candidates.push(dir.join("Pyro.Settings.exe"));
        candidates.push(dir.join("pyro-settings.exe"));
    }

    if let Ok(cwd) = env::current_dir() {
        candidates.push(
            cwd.join("settings")
                .join("Pyro.Settings")
                .join("bin")
                .join("x64")
                .join("Debug")
                .join("net8.0-windows10.0.19041.0")
                .join("win10-x64")
                .join("Pyro.Settings.exe"),
        );
        candidates.push(
            cwd.join("settings")
                .join("Pyro.Settings")
                .join("bin")
                .join("Debug")
                .join("net8.0-windows10.0.19041.0")
                .join("win10-x64")
                .join("Pyro.Settings.exe"),
        );
        candidates.push(
            cwd.join("settings")
                .join("Pyro.Settings")
                .join("Pyro.Settings.exe"),
        );
    }

    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    bail!(
        "settings UI executable not found. Build the WinUI 3 project under `settings/Pyro.Settings` \
or set PYRO_SETTINGS_EXE to the executable path."
    )
}
