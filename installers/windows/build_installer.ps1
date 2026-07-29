#!/usr/bin/env pwsh
# Build the buttre MSI installer via cargo-wix.
# Usage: ./build_installer.ps1 [-Version 0.6.3-alpha]
param(
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path "$PSScriptRoot\..\.."

# cargo-wix 0.3.x drives WiX v3's candle/light. WiX v3 is free (MS-RL) — the
# paid product is FireGiant's v4+, which cargo-wix cannot use anyway: v4
# replaced candle/light with a single `wix` CLI. Do not "upgrade" past v3 here.
function Resolve-WixBinPath {
    if (Get-Command candle.exe -ErrorAction SilentlyContinue) { return $null }  # already on PATH

    $candidates = @()
    if ($env:WIX) { $candidates += (Join-Path $env:WIX "bin") }
    $candidates += @(
        "${env:ProgramFiles(x86)}\WiX Toolset v3.14\bin"
        "${env:ProgramFiles(x86)}\WiX Toolset v3.11\bin"
        "$env:ProgramFiles\WiX Toolset v3.14\bin"
        "$env:ProgramFiles\WiX Toolset v3.11\bin"
    )
    foreach ($dir in $candidates) {
        if ($dir -and (Test-Path (Join-Path $dir "candle.exe"))) { return $dir }
    }

    # Guidance goes to the host, not into the exception: PowerShell collapses a
    # multi-line throw message into one unreadable line.
    Write-Host ""
    Write-Host "  WiX Toolset v3 not found - cargo-wix needs its candle.exe/light.exe." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "      winget install --id WiXToolset.WiXToolset" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "  WiX v3 is free and open source; only FireGiant's v4+ is paid, and" -ForegroundColor Gray
    Write-Host "  cargo-wix 0.3.x cannot use v4 regardless. Open a new shell after" -ForegroundColor Gray
    Write-Host "  installing so the WIX environment variable is visible." -ForegroundColor Gray
    Write-Host ""
    throw "WiX Toolset v3 not found"
}

Push-Location $repoRoot
try {
    Write-Host "==> Building buttre-platform release..."
    cargo build -p buttre-platform --release

    $targetDir = Join-Path $repoRoot "target\release"
    $nomDb     = Join-Path $targetDir "buttre_nom.db"

    # cargo-wix only forwards preprocessor defines to candle via -C/--compiler-arg;
    # args after `--` are NOT passed through.
    $wixArgs = @()
    $wixBin = Resolve-WixBinPath
    if ($wixBin) {
        Write-Host "==> WiX v3: $wixBin"
        $wixArgs += @("-b", $wixBin)
    }
    if (Test-Path $nomDb) {
        Write-Host "==> Nom DB found, including in MSI"
        $wixArgs += @("-C", "-dIncludeNomDb=1")
    } else {
        Write-Host "==> Nom DB not found, MSI will ship without it"
    }

    if ($Version -eq "") {
        # cargo pkgid returns something like path+file:///...#0.6.3-alpha
        $Version = (cargo pkgid -p buttre-platform) -replace '.*#', ''
    }

    # --install-version takes SemVer 3-part (strips pre-release suffix).
    # cargo-wix automatically defines $(var.Version) = this value for product.wxs.
    # Do NOT also pass -C "-dVersion=..." — candle rejects duplicate variable declarations.
    $semVer = $Version -replace '-.*$', ''  # e.g. "0.7.0"

    # cargo-wix resolves `include` paths relative to cwd, so run from the crate directory
    # where include = "../../installers/windows/product.wxs" resolves correctly.
    $crateDir  = Join-Path $repoRoot "crates\buttre-platform"
    $outputAbs = Join-Path $repoRoot "target\wix\buttre-$Version-x86_64.msi"
    New-Item -ItemType Directory -Force (Join-Path $repoRoot "target\wix") | Out-Null

    Push-Location $crateDir
    try {
        Write-Host "==> Building MSI v$Version (install-version: $semVer)..."
        cargo wix `
            --package buttre-platform `
            --nocapture `
            --output $outputAbs `
            --install-version $semVer `
            @wixArgs
        # $ErrorActionPreference does not cover native exit codes, and cargo-wix
        # reports candle/light failures that way — without this the script
        # announced an MSI it had not built.
        if ($LASTEXITCODE -ne 0) { throw "cargo wix failed with exit code $LASTEXITCODE" }
    }
    finally {
        Pop-Location
    }

    $msiPath = "target\wix\buttre-$Version-x86_64.msi"
    Write-Host ""
    Write-Host "==> MSI: $msiPath"
    Get-Item $msiPath | Select-Object Name, @{N='Size';E={"{0:N0} KB" -f ($_.Length / 1KB)}}
}
finally {
    Pop-Location
}
