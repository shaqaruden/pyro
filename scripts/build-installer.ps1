param(
    [string]$Version,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

function Write-Banner {
    param([string]$Message)
    Write-Host ""
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Normalize-InstallerVersion {
    param([string]$RawVersion)
    $v = $RawVersion.Trim()
    if ($v.StartsWith("v")) {
        $v = $v.Substring(1)
    }
    $v = ($v -split "-", 2)[0]

    if ($v -notmatch "^\d+\.\d+\.\d+(\.\d+)?$") {
        throw "Installer version must be numeric semver style (for example: 0.1.0 or 1.2.3.4). Got: $RawVersion"
    }

    $parts = $v.Split(".")
    if ($parts.Count -eq 3) {
        return "$($parts[0]).$($parts[1]).$($parts[2])"
    }
    return "$($parts[0]).$($parts[1]).$($parts[2]).$($parts[3])"
}

function Resolve-WixCommand {
    function Test-DotnetToolRunWix {
        cmd /c "dotnet tool run wix --version >nul 2>nul" | Out-Null
        return ($LASTEXITCODE -eq 0)
    }

    $wixCmd = Get-Command wix -ErrorAction SilentlyContinue
    if ($null -ne $wixCmd) {
        return @{ Kind = "wix-cli"; Command = $wixCmd.Source; PrefixArgs = @() }
    }

    $globalToolPath = Join-Path $env:USERPROFILE ".dotnet\tools\wix.exe"
    if (Test-Path $globalToolPath) {
        return @{ Kind = "wix-global-tool"; Command = $globalToolPath; PrefixArgs = @() }
    }

    $toolManifest = Join-Path (Get-Location) ".config\dotnet-tools.json"
    if (Test-Path $toolManifest) {
        cmd /c "dotnet tool restore >nul 2>nul" | Out-Null
        if (Test-DotnetToolRunWix) {
            return @{ Kind = "dotnet-tool-run"; Command = "dotnet"; PrefixArgs = @("tool", "run", "wix") }
        }
    }

    Write-Host "WiX CLI not found. Attempting install via dotnet global tool..." -ForegroundColor Yellow
    $installLog = Join-Path $env:TEMP "pyro-wix-install.log"
    cmd /c "dotnet tool install --global wix > `"$installLog`" 2>&1" | Out-Null
    if ($LASTEXITCODE -eq 0) {
        if (Test-Path $globalToolPath) {
            return @{ Kind = "wix-global-tool"; Command = $globalToolPath; PrefixArgs = @() }
        }
        $wixAfterInstall = Get-Command wix -ErrorAction SilentlyContinue
        if ($null -ne $wixAfterInstall) {
            return @{ Kind = "wix-cli"; Command = $wixAfterInstall.Source; PrefixArgs = @() }
        }
    } else {
        Write-Warning "dotnet tool install --global wix failed. See log: $installLog"
    }

    throw @"
WiX CLI not found.

Try these commands and rerun:
  dotnet tool install --global wix

If already installed globally but not on PATH in this terminal:
  `$env:PATH += ";$env:USERPROFILE\.dotnet\tools"

Then run:
  powershell -ExecutionPolicy Bypass -File scripts/build-installer.ps1 -Version <version>
"@
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Push-Location $repoRoot
try {
    if ([string]::IsNullOrWhiteSpace($Version)) {
        $cargoVersionLine = Get-Content "Cargo.toml" | Where-Object { $_ -match '^\s*version\s*=' } | Select-Object -First 1
        if (-not $cargoVersionLine) {
            throw "Could not resolve version from Cargo.toml. Pass -Version explicitly."
        }
        $Version = ($cargoVersionLine -replace '^\s*version\s*=\s*"', '' -replace '"\s*$', '').Trim()
    }

    $installerVersion = Normalize-InstallerVersion -RawVersion $Version
    $displayVersion = $Version
    if ($displayVersion.StartsWith("v")) {
        $displayVersion = $displayVersion.Substring(1)
    }

    if (-not $SkipBuild) {
        Write-Banner "Building release binaries"
        & cargo build --release --locked --bin pyro --bin pyro-settings
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed"
        }
    }

    $requiredFiles = @(
        "target/release/pyro.exe",
        "target/release/pyro-settings.exe",
        "installer/pyro.wxs",
        "README.md",
        "LICENSE",
        "docs/USER_GUIDE.md"
    )
    foreach ($file in $requiredFiles) {
        if (-not (Test-Path $file)) {
            throw "Required file missing: $file"
        }
    }

    $distDir = Join-Path $repoRoot "dist"
    if (-not (Test-Path $distDir)) {
        New-Item -ItemType Directory -Path $distDir | Out-Null
    }

    $msiName = "pyro-$displayVersion-windows-x64.msi"
    $msiPath = Join-Path $distDir $msiName
    $shaPath = Join-Path $distDir "pyro-$displayVersion-windows-x64.msi.sha256"

    $wix = Resolve-WixCommand
    Write-Banner "Building MSI installer"
    $wixArgs = @()
    if ($wix.PrefixArgs.Count -gt 0) {
        $wixArgs += $wix.PrefixArgs
    }
    $wixArgs += @(
        "build",
        "installer/pyro.wxs",
        "-arch", "x64",
        "-d", "SourceRoot=$repoRoot",
        "-d", "InstallerVersion=$installerVersion",
        "-out", "$msiPath"
    )
    & $wix.Command @wixArgs
    if ($LASTEXITCODE -ne 0) {
        throw "wix build failed"
    }

    if (-not (Test-Path $msiPath)) {
        throw "MSI output missing: $msiPath"
    }

    $hash = (Get-FileHash $msiPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Content -Path $shaPath -Value "$hash  $msiName" -Encoding Ascii

    Write-Banner "Installer ready"
    Write-Host "MSI:  $msiPath"
    Write-Host "SHA:  $shaPath"
    Write-Host "MSI version: $installerVersion"
}
finally {
    Pop-Location
}
