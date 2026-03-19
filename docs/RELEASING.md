# Releasing Pyro

Releases are created automatically by GitHub Actions when a tag matching `v*` is pushed.

## What the release workflow does

- Runs `cargo test --locked` on `windows-latest`.
- Builds release binaries:
  - `pyro.exe`
  - `pyro-settings.exe`
- Packages a portable bundle zip named:
  - `pyro-<tag>-windows-x64.zip`
- Generates a SHA-256 checksum file:
  - `pyro-<tag>-windows-x64.sha256`
- Builds a standard Windows MSI installer:
  - `pyro-<tag>-windows-x64.msi`
- Generates MSI checksum:
  - `pyro-<tag>-windows-x64.msi.sha256`
- Publishes/updates the GitHub Release for that tag and uploads both files.

## Release steps

1. Ensure `main` is ready and tests pass locally:
   - `cargo test --locked`
2. Create and push a semantic version tag:
   - `git tag v0.1.0`
   - `git push origin v0.1.0`
3. Wait for the `Release` workflow to complete in GitHub Actions.
4. Verify release assets on the GitHub Release page.

## Local mock release build

To create local release artifacts (portable zip + MSI installer):

```powershell
powershell -ExecutionPolicy Bypass -File scripts/mock-release.ps1
```

Artifacts are written to `dist/`.

Prerequisite for MSI:

```powershell
dotnet tool restore
```

Direct MSI-only build:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/build-installer.ps1 -Version v0.1.0
```

## Notes

- Tags that include a hyphen (example: `v0.2.0-rc1`) are marked as pre-releases.
- The release workflow is defined in `.github/workflows/release.yml`.
