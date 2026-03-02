use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};

pub fn launch_settings_window(config_path: &Path) -> Result<()> {
    if let Some(exe) = resolve_settings_exe()? {
        log_launch(format!("launch exe: {}", exe.display()));
        let mut child = Command::new(&exe)
            .arg(config_path)
            .spawn()
            .with_context(|| format!("launch settings UI {}", exe.display()))?;
        log_launch(format!("spawned exe pid={}", child.id()));
        thread::sleep(Duration::from_millis(900));
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("check settings UI status {}", exe.display()))?
        {
            log_launch(format!("exe exited immediately: {status}"));
            bail!("settings UI exited immediately with status {status}");
        }
        log_launch("exe appears running".to_string());
        return Ok(());
    }

    if let Some(manifest) = resolve_manifest_path()? {
        log_launch(format!("launch via cargo run: {}", manifest.display()));
        let mut child = Command::new("cargo")
            .arg("run")
            .arg("--manifest-path")
            .arg(&manifest)
            .arg("--bin")
            .arg("pyro-settings")
            .arg("--")
            .arg(config_path)
            .spawn()
            .with_context(|| format!("launch settings UI via cargo run {}", manifest.display()))?;
        log_launch(format!("spawned cargo pid={}", child.id()));
        thread::sleep(Duration::from_millis(1800));
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("check settings UI status {}", manifest.display()))?
        {
            log_launch(format!("cargo run exited immediately: {status}"));
            bail!("settings UI failed to start (cargo run exited with status {status})");
        }
        log_launch("cargo run process appears running".to_string());
        return Ok(());
    }

    bail!(
        "settings UI executable not found. Build `pyro-settings` with `cargo build --bin \
pyro-settings` or set PYRO_SETTINGS_EXE to the binary path."
    )
}

fn resolve_settings_exe() -> Result<Option<PathBuf>> {
    if let Some(path) = env::var_os("PYRO_SETTINGS_EXE") {
        let candidate = PathBuf::from(path);
        if candidate.exists() {
            return Ok(Some(candidate));
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
        candidates.push(dir.join("pyro-settings.exe"));
        candidates.push(dir.join("pyro-settings"));
    }

    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd.join("pyro-settings.exe"));
        candidates.push(cwd.join("target").join("debug").join("pyro-settings.exe"));
        candidates.push(cwd.join("target").join("release").join("pyro-settings.exe"));
    }

    for candidate in candidates {
        if candidate.exists() {
            return Ok(Some(candidate));
        }
    }

    Ok(None)
}

fn resolve_manifest_path() -> Result<Option<PathBuf>> {
    if let Some(path) = env::var_os("PYRO_SETTINGS_MANIFEST") {
        let candidate = PathBuf::from(path);
        if candidate.exists() {
            return Ok(Some(candidate));
        }
        bail!(
            "PYRO_SETTINGS_MANIFEST points to a missing file: {}",
            candidate.display()
        );
    }

    if let Ok(cwd) = env::current_dir() {
        let manifest = cwd.join("Cargo.toml");
        if manifest.exists() {
            return Ok(Some(manifest));
        }
    }

    if let Ok(current_exe) = env::current_exe()
        && let Some(exe_dir) = current_exe.parent()
    {
        let mut cursor = Some(exe_dir.to_path_buf());
        while let Some(dir) = cursor {
            let manifest = dir.join("Cargo.toml");
            if manifest.exists() {
                return Ok(Some(manifest));
            }
            cursor = dir.parent().map(Path::to_path_buf);
        }
    }

    Ok(None)
}

fn log_launch(message: String) {
    let path = env::temp_dir().join("pyro-settings-launcher.log");
    let line = format!("[{}] {}\n", unix_timestamp_secs(), message);
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()));
}

fn unix_timestamp_secs() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
