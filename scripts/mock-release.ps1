param(
    [string]$Version,
    [switch]$SkipTests,
    [switch]$NoInstaller
)

$ErrorActionPreference = "Stop"

function Require-Path {
    param([string]$Path)
    if (-not (Test-Path $Path)) {
        throw "Required file missing: $Path"
    }
}

function Write-Banner {
    param([string]$Message)
    Write-Host ""
    Write-Host "==> $Message" -ForegroundColor Cyan
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Push-Location $repoRoot
try {
    if ([string]::IsNullOrWhiteSpace($Version)) {
        $tag = (cmd /c "git describe --tags --exact-match 2>nul")
        if ($LASTEXITCODE -eq 0 -and -not [string]::IsNullOrWhiteSpace($tag)) {
            $Version = $tag.Trim()
        } else {
            $Version = "dev-{0}" -f (Get-Date -Format "yyyyMMdd-HHmmss")
        }
    }

    $distDir = Join-Path $repoRoot "dist"
    $bundleName = "pyro-$Version-windows-x64"
    $bundleDir = Join-Path $distDir $bundleName
    $zipPath = Join-Path $distDir "$bundleName.zip"
    $zipShaPath = Join-Path $distDir "$bundleName.sha256"

    if (Test-Path $distDir) {
        Remove-Item -Recurse -Force $distDir
    }
    New-Item -ItemType Directory -Path $distDir | Out-Null
    New-Item -ItemType Directory -Path $bundleDir | Out-Null

    if (-not $SkipTests) {
        Write-Banner "Running test suite"
        & cargo test --locked
        if ($LASTEXITCODE -ne 0) {
            throw "cargo test failed"
        }
    }

    Write-Banner "Building release binaries"
    & cargo build --release --locked --bin pyro --bin pyro-settings
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed"
    }

    $releaseFiles = @(
        @{ Src = "target/release/pyro.exe"; Dst = "pyro.exe" },
        @{ Src = "target/release/pyro-settings.exe"; Dst = "pyro-settings.exe" },
        @{ Src = "README.md"; Dst = "README.md" },
        @{ Src = "LICENSE"; Dst = "LICENSE" },
        @{ Src = "docs/USER_GUIDE.md"; Dst = "USER_GUIDE.md" }
    )

    Write-Banner "Staging portable bundle files"
    foreach ($entry in $releaseFiles) {
        Require-Path $entry.Src
        Copy-Item $entry.Src (Join-Path $bundleDir $entry.Dst)
    }

    Write-Banner "Creating portable zip"
    Compress-Archive -Path (Join-Path $bundleDir "*") -DestinationPath $zipPath -Force
    $zipHash = (Get-FileHash $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -Path $zipShaPath -Value "$zipHash  $(Split-Path $zipPath -Leaf)" -Encoding Ascii

    $installerBuilt = $false
    $installerPath = $null
    $installerShaPath = $null
    if (-not $NoInstaller) {
        Write-Banner "Building MSI installer"
        & powershell -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "build-installer.ps1") -Version $Version -SkipBuild
        if ($LASTEXITCODE -eq 0) {
            $displayVersion = $Version
            if ($displayVersion.StartsWith("v")) {
                $displayVersion = $displayVersion.Substring(1)
            }
            $installerPath = Join-Path $distDir "pyro-$displayVersion-windows-x64.msi"
            $installerShaPath = Join-Path $distDir "pyro-$displayVersion-windows-x64.msi.sha256"
            if ((Test-Path $installerPath) -and (Test-Path $installerShaPath)) {
                $installerBuilt = $true
            } else {
                throw "MSI installer build completed but expected files were not found."
            }
        } else {
            throw "MSI installer build failed."
        }
    }

    Write-Banner "Mock release artifacts"
    Write-Host "Bundle directory: $bundleDir"
    Write-Host "Portable zip:    $zipPath"
    Write-Host "Zip checksum:    $zipShaPath"
    if ($installerBuilt) {
        Write-Host "Installer:       $installerPath"
        Write-Host "Installer sha:   $installerShaPath"
    } else {
        Write-Host "Installer:       skipped"
    }
}
finally {
    Pop-Location
}
