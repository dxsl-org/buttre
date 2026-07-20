#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Quick-build the Windows hook-mode buttre for local testing.

.DESCRIPTION
    Builds buttre-platform (hook exe) and stages all required files into
    target\hook-zip\buttre\ so you can run buttre.exe directly — no installer,
    no regsvr32, no admin. Much faster than the CI release script.

    By default builds the optimized RELEASE profile — the staged zip is what
    ends up installed, and a debug buttre.exe is ~5x larger and slower (a
    32MB debug build once shipped to Program Files this way). Pass -Debug
    only for quick compile-test iteration you will not install.

.EXAMPLE
    # Optimized release build (default):
    .\scripts\build-hook.ps1

    # Fast unoptimized build for local iteration only:
    .\scripts\build-hook.ps1 -Debug

    # Build and run immediately:
    .\scripts\build-hook.ps1 -Run
#>
param(
    [switch]$Debug,
    [switch]$Run,
    [switch]$NoZip
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path "$PSScriptRoot\.."

Push-Location $repoRoot
try {
    $profile_ = if ($Debug) { "debug" } else { "release" }
    $profileLabel = if ($Debug) { "debug (fast, DO NOT install)" } else { "release (optimized)" }

    Write-Host ""
    Write-Host "  buttre hook-mode quick build" -ForegroundColor Cyan
    Write-Host "  Profile: $profileLabel" -ForegroundColor Gray
    Write-Host ""

    # ── Build ────────────────────────────────────────────────────────────
    $buildArgs = @("build", "-p", "buttre-platform")
    if (-not $Debug) { $buildArgs += "--release" }

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    Write-Host "[1/3] Building..." -ForegroundColor Yellow
    & cargo @buildArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Host "ERROR: cargo build failed!" -ForegroundColor Red
        exit 1
    }
    $sw.Stop()
    Write-Host "      Build OK ($([math]::Round($sw.Elapsed.TotalSeconds, 1))s)" -ForegroundColor Green

    # ── Stage ────────────────────────────────────────────────────────────
    Write-Host "[2/3] Staging files..." -ForegroundColor Yellow
    $targetDir  = Join-Path $repoRoot "target\$profile_"
    $stagingDir = Join-Path $repoRoot "target\hook-zip\buttre"

    if (Test-Path $stagingDir) { Remove-Item $stagingDir -Recurse -Force }
    New-Item -ItemType Directory -Force $stagingDir | Out-Null

    # buttre.exe
    $exe = Join-Path $targetDir "buttre.exe"
    if (-not (Test-Path $exe)) {
        Write-Host "ERROR: buttre.exe not found at $exe" -ForegroundColor Red
        exit 1
    }
    Copy-Item $exe $stagingDir
    $exeSize = (Get-Item $exe).Length / 1MB
    Write-Host "      buttre.exe  ($([math]::Round($exeSize, 1)) MB)" -ForegroundColor Gray

    # buttre_nom.db (optional — Nôm input)
    $nomDb = Join-Path $targetDir "buttre_nom.db"
    if (Test-Path $nomDb) {
        Copy-Item $nomDb $stagingDir
        Write-Host "      buttre_nom.db (Nôm support included)" -ForegroundColor Gray
    } else {
        # Fallback: check repo root
        $nomDbRoot = Join-Path $repoRoot "buttre_nom.db"
        if (Test-Path $nomDbRoot) {
            Copy-Item $nomDbRoot $stagingDir
            Write-Host "      buttre_nom.db (from repo root)" -ForegroundColor Gray
        } else {
            Write-Host "      buttre_nom.db not found — Nôm input unavailable" -ForegroundColor DarkYellow
        }
    }

    # keyboards/ configs
    $kbDir = Join-Path $repoRoot "keyboards"
    if (Test-Path $kbDir) {
        Copy-Item $kbDir (Join-Path $stagingDir "keyboards") -Recurse
        $kbCount = (Get-ChildItem "$kbDir\*.toml").Count
        Write-Host "      keyboards/ ($kbCount layouts)" -ForegroundColor Gray
    }

    # ── Zip (optional) ───────────────────────────────────────────────────
    if (-not $NoZip) {
        Write-Host "[3/3] Creating ZIP..." -ForegroundColor Yellow
        $outDir  = Join-Path $repoRoot "target\hook-zip"
        $zipName = "buttre-hook-$profile_.zip"
        $zipPath = Join-Path $outDir $zipName
        if (Test-Path $zipPath) { Remove-Item $zipPath -Force }
        Compress-Archive -Path "$stagingDir\*" -DestinationPath $zipPath -Force
        $zipSize = (Get-Item $zipPath).Length / 1MB
        Write-Host "      $zipName ($([math]::Round($zipSize, 1)) MB)" -ForegroundColor Gray
    } else {
        Write-Host "[3/3] Skipped ZIP (-NoZip)" -ForegroundColor DarkGray
    }

    # ── Summary ──────────────────────────────────────────────────────────
    Write-Host ""
    Write-Host "  Ready!" -ForegroundColor Green
    Write-Host "  Run:  target\hook-zip\buttre\buttre.exe" -ForegroundColor Cyan
    Write-Host "  Dir:  $stagingDir" -ForegroundColor Gray
    Write-Host ""

    # ── Auto-run ─────────────────────────────────────────────────────────
    if ($Run) {
        $exePath = Join-Path $stagingDir "buttre.exe"
        Write-Host "  Launching buttre.exe..." -ForegroundColor Cyan
        & $exePath
    }
}
finally {
    Pop-Location
}
