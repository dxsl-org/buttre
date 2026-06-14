#!/usr/bin/env pwsh
# Build the buttre Windows hook-only ZIP (no TSF, no installer required — just run buttre.exe).
# Usage: ./build_hook_zip.ps1 [-Version 0.6.3-alpha]
param(
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path "$PSScriptRoot\..\.."

Push-Location $repoRoot
try {
    Write-Host "==> Building buttre-platform release..."
    cargo build -p buttre-platform --release

    if ($Version -eq "") {
        $Version = (cargo pkgid -p buttre-platform) -replace '.*#', ''
    }

    $targetDir  = Join-Path $repoRoot "target\release"
    $stagingDir = Join-Path $repoRoot "target\hook-zip\buttre"
    $outDir     = Join-Path $repoRoot "target\hook-zip"
    $zipPath    = Join-Path $outDir "buttre-$Version-windows-x86_64-hook.zip"

    # Clean + stage
    if (Test-Path $stagingDir) { Remove-Item $stagingDir -Recurse -Force }
    New-Item -ItemType Directory -Force $stagingDir | Out-Null

    Copy-Item "$targetDir\buttre.exe" $stagingDir

    $nomDb = Join-Path $targetDir "buttre_nom.db"
    if (Test-Path $nomDb) {
        Copy-Item $nomDb $stagingDir
        Write-Host "==> Included buttre_nom.db (Nôm input support)"
    } else {
        Write-Host "==> buttre_nom.db not found — Nôm input will be unavailable"
    }

    # Bundle keyboard configs if present
    $kbDir = Join-Path $repoRoot "keyboards"
    if (Test-Path $kbDir) {
        $kbDest = Join-Path $stagingDir "keyboards"
        Copy-Item $kbDir $kbDest -Recurse
        Write-Host "==> Included keyboards/ configs"
    }

    Write-Host "==> Creating ZIP..."
    Compress-Archive -Path "$stagingDir\*" -DestinationPath $zipPath -Force

    Write-Host ""
    Write-Host "==> ZIP: $zipPath"
    Get-Item $zipPath | Select-Object Name, @{N='Size';E={"{0:N0} KB" -f ($_.Length / 1KB)}}
    Write-Host ""
    Write-Host "Usage: extract zip, run buttre.exe — uses keyboard hook (no install needed)."
    Write-Host "Note:  TSF (system-wide IME) requires the MSI installer."
}
finally {
    Pop-Location
}
