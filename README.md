# Pyro

Pyro is a Windows screenshot utility written in Rust, inspired by Flameshot.

It supports mixed-DPI, multi-monitor setups and an interactive region editor with annotations.

## Features

- Capture targets: `region`, `primary`, `all-displays`
- Global hotkey capture (default: `PrintScreen`)
- Region editor tools: select, rectangle, ellipse, line, arrow, marker, text, pixelate, blur
- Copy/save/copy+save actions
- Pin capture as an always-on-top floating window
- Tray icon with quick actions
- Settings window (`pyro-settings`) with live hotkey reload
- Monitor diagnostics CLI with JSON/validation/report/baseline compare

## Requirements

- Windows 10 or Windows 11 (x64)
- Rust stable toolchain (if building from source)

## Build From Source

```powershell
cargo build --release --locked --bin pyro --bin pyro-settings
```

Binaries:

- `target/release/pyro.exe`
- `target/release/pyro-settings.exe`

Build MSI installer (standard Windows installer with Installed Apps entry):

```powershell
dotnet tool restore
powershell -ExecutionPolicy Bypass -File scripts/build-installer.ps1 -Version v0.1.0
```

## Run

Start tray app:

```powershell
cargo run -- run
```

Capture once:

```powershell
cargo run -- capture --target region --edit
```

Open settings:

```powershell
cargo run -- settings
```

Monitor diagnostics:

```powershell
cargo run -- monitors --json --validate
```

## Configuration

Config file:

- `%APPDATA%\\pyro\\config.toml`

Defaults:

- capture hotkey: `PrintScreen`
- default target: `region`

## Documentation

- User Guide: `docs/USER_GUIDE.md`
- Releasing: `docs/RELEASING.md`
- Local mock release script: `scripts/mock-release.ps1`

## License

This project is licensed under **GNU GPL v2 only** (`GPL-2.0-only`), matching the Linux kernel license model.
