# Pyro Settings (WinUI 3)

This project is a WinUI 3 desktop companion app used by `pyro` to edit `config.toml`.

## Build

1. Install .NET 8 SDK and WinUI workloads (Windows App SDK prerequisites).
2. From `settings/Pyro.Settings`:

```powershell
dotnet restore
dotnet build -c Debug -p:Platform=x64
```

## Launch manually

```powershell
dotnet run -- "C:\Users\<you>\AppData\Roaming\pyro\config.toml"
```

Or run from tray/menu in Pyro after building.

## Notes

- The app edits and writes `config.toml` values used by Pyro.
- Restart the tray app for global hotkey/default-target changes to take effect.
