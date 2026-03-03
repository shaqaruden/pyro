#[cfg(target_os = "windows")]
fn main() {
    if let Err(err) = embed_windows_app_icon() {
        println!("cargo:warning=failed to embed app icon: {err}");
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {}

#[cfg(target_os = "windows")]
fn embed_windows_app_icon() -> Result<(), Box<dyn std::error::Error>> {
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    let icon_source = Path::new("src/assets/app-icon.ico");
    println!("cargo:rerun-if-changed={}", icon_source.display());
    if !icon_source.exists() {
        return Err(format!("icon file not found: {}", icon_source.display()).into());
    }

    let rc_exe = find_rc_exe().ok_or("rc.exe not found (Windows SDK)")?;
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    let icon_copy = out_dir.join("pyro-app-icon.ico");
    fs::copy(icon_source, &icon_copy)?;

    let rc_file = out_dir.join("pyro-app-icon.rc");
    fs::write(&rc_file, "1 ICON \"pyro-app-icon.ico\"\n")?;

    let res_file = out_dir.join("pyro-app-icon.res");
    let fo_arg = format!("/fo{}", res_file.display());

    let output = Command::new(&rc_exe)
        .current_dir(&out_dir)
        .arg("/nologo")
        .arg(&fo_arg)
        .arg("pyro-app-icon.rc")
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "rc.exe failed (status={}): stdout=`{}` stderr=`{}`",
            output.status, stdout, stderr
        )
        .into());
    }

    if !res_file.exists() {
        return Err(format!("resource output missing: {}", res_file.display()).into());
    }

    println!("cargo:rustc-link-arg-bin=pyro={}", res_file.display());
    println!(
        "cargo:rustc-link-arg-bin=pyro-settings={}",
        res_file.display()
    );
    Ok(())
}

#[cfg(target_os = "windows")]
fn find_rc_exe() -> Option<std::path::PathBuf> {
    use std::env;
    use std::fs;
    use std::path::PathBuf;

    for var in ["PYRO_RC_EXE", "RC"] {
        if let Ok(value) = env::var(var) {
            let path = PathBuf::from(value);
            if path.is_file() {
                return Some(path);
            }
        }
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    for root in [
        PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin"),
        PathBuf::from(r"C:\Program Files\Windows Kits\10\bin"),
    ] {
        if !root.is_dir() {
            continue;
        }

        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.eq_ignore_ascii_case("x64") {
                        let direct = path.join("rc.exe");
                        if direct.is_file() {
                            candidates.push(direct);
                        }
                    } else if name.starts_with("10.") {
                        let versioned = path.join("x64").join("rc.exe");
                        if versioned.is_file() {
                            candidates.push(versioned);
                        }
                    }
                }
            }
        }
    }

    candidates.sort();
    candidates.pop()
}
