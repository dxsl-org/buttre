#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Build and install the TSF text service exactly the way the release MSI
    does, for local testing before shipping.

.DESCRIPTION
    Mirrors installers\windows\product.wxs step for step: builds the release
    profile, stages the same files into the same directory, and writes the
    same registry keys. What you test is what users get.

    In particular the registration is REGISTRY-ONLY, matching the MSI — the
    MSI has no CustomAction, so it never calls the DLL's own
    DllRegisterServer. That difference is not cosmetic:

      MSI (default here)   vi-VN profile only, no TSF category registration
      -SelfRegister        regsvr32 -> DllRegisterServer, which ALSO adds an
                           en-US profile and registers the TIP_KEYBOARD and
                           DISPLAYATTRIBUTEPROVIDER categories

    Testing with regsvr32 therefore tests a MORE registered service than the
    one you ship. Use the default to reproduce what users will actually have;
    use -SelfRegister only to isolate whether a bug comes from the missing
    category registration.

    Requires Administrator: the install directory is under Program Files and
    every registry key lives in HKLM, exactly as in the MSI.

.PARAMETER Uninstall
    Remove the registration and the installed files, then exit.

.PARAMETER SelfRegister
    Register through regsvr32 (DllRegisterServer) instead of the MSI's
    registry writes. See the description for how this differs from a release.

.PARAMETER Msi
    Build a real .msi via installers\windows\build_installer.ps1 and stop
    without installing it. The most faithful test of all, and the slowest —
    needs cargo-wix.

.PARAMETER NoBuild
    Skip cargo and install whatever is already in target\release.

.PARAMETER InstallDir
    Override the install location. Defaults to the MSI's directory.

.EXAMPLE
    # Build, install and register like the release does:
    .\scripts\build-tsf.ps1

.EXAMPLE
    # Iterate on the DLL without rebuilding the world:
    .\scripts\build-tsf.ps1 -NoBuild

.EXAMPLE
    # Clean up:
    .\scripts\build-tsf.ps1 -Uninstall
#>
param(
    [switch]$Uninstall,
    [switch]$SelfRegister,
    [switch]$Msi,
    [switch]$NoBuild,
    [string]$InstallDir = "$env:ProgramFiles\buttre"
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path "$PSScriptRoot\.."

# Must match installers\windows\product.wxs and
# crates\buttre-platform\src\platforms\windows\tsf\registration.rs — three
# copies of these GUIDs exist and they have to agree or the TIP silently
# fails to load.
$CLSID      = "{E6B8A6C0-1234-5678-9ABC-DEF012345678}"
$ProfileGuid = "{B7447743-7652-4AB6-8D82-250D935EBCC0}"
$LangId     = "0x0000042A"  # vi-VN
$ClsidKey   = "HKLM:\SOFTWARE\Classes\CLSID\$CLSID"
$TipKey     = "HKLM:\SOFTWARE\Microsoft\CTF\TIP\$CLSID"

function Assert-Admin {
    $principal = New-Object Security.Principal.WindowsPrincipal(
        [Security.Principal.WindowsIdentity]::GetCurrent())
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        Write-Host "ERROR: run this from an Administrator PowerShell." -ForegroundColor Red
        Write-Host "       HKLM and Program Files are both required — same as the MSI." -ForegroundColor Gray
        exit 1
    }
}

function Assert-Bitness {
    # A 32-bit PowerShell would write CLSID under WOW6432Node, where a 64-bit
    # host app can never find it — the failure looks like "registered fine but
    # buttre never appears in the language bar".
    if (-not [Environment]::Is64BitProcess) {
        Write-Host "ERROR: this is a 32-bit PowerShell." -ForegroundColor Red
        Write-Host "       The registration would land in WOW6432Node and no 64-bit app would see it." -ForegroundColor Gray
        exit 1
    }
}

<# Release the DLL so it can be overwritten.

Every TSF client process has it loaded, so a plain copy fails. Unregistering
first stops NEW loads; ctfmon/TextInputHost are restarted automatically by
Windows. Apps already running keep the OLD DLL in memory until they restart —
that is why the summary tells you to restart the app you test in. #>
function Stop-TsfHosts {
    foreach ($name in @("ctfmon", "TextInputHost")) {
        Get-Process -Name $name -ErrorAction SilentlyContinue |
            Stop-Process -Force -ErrorAction SilentlyContinue
    }
    Start-Sleep -Milliseconds 800
}

function Remove-Registration {
    Remove-Item $TipKey   -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item $ClsidKey -Recurse -Force -ErrorAction SilentlyContinue
}

<# Write exactly the keys installers\windows\product.wxs writes. #>
function Add-MsiRegistration([string]$dllPath) {
    New-Item -Path $ClsidKey -Force | Out-Null
    New-ItemProperty -Path $ClsidKey -Name "(default)" -Value "buttre Text Service" `
        -PropertyType String -Force | Out-Null

    $inproc = "$ClsidKey\InProcServer32"
    New-Item -Path $inproc -Force | Out-Null
    New-ItemProperty -Path $inproc -Name "(default)" -Value $dllPath `
        -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $inproc -Name "ThreadingModel" -Value "Apartment" `
        -PropertyType String -Force | Out-Null

    $profileKey = "$TipKey\LanguageProfile\$LangId\$ProfileGuid"
    New-Item -Path $profileKey -Force | Out-Null
    New-ItemProperty -Path $profileKey -Name "Description" -Value "buttre Vietnamese IME" `
        -PropertyType String -Force | Out-Null
    New-ItemProperty -Path $profileKey -Name "Enable" -Value 1 `
        -PropertyType DWord -Force | Out-Null
}

<# Copy over a DLL that may still be mapped by a running process.

Windows refuses to overwrite a loaded image but DOES allow renaming it, so the
rename frees the name for the new file and the stale one is deleted on the next
run (it cannot be deleted now — still mapped). #>
function Copy-OverLockedFile([string]$source, [string]$destination) {
    Get-ChildItem (Split-Path $destination) -Filter "*.old-*" -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue
    try {
        Copy-Item $source $destination -Force -ErrorAction Stop
    } catch {
        $stale = "$destination.old-$(Get-Random)"
        Move-Item $destination $stale -Force -ErrorAction Stop
        Copy-Item $source $destination -Force -ErrorAction Stop
        Write-Host "      (old DLL was in use — renamed, close TSF apps to reclaim it)" -ForegroundColor DarkYellow
    }
}

<# True when a real MSI install owns this machine's buttre.

Staging over it leaves Windows believing the MSI version is still installed —
its Add/Remove entry survives, its file list no longer matches, and a later
MSI repair or uninstall will act on files this script replaced. Worth saying
out loud before touching Program Files. #>
function Test-MsiInstalled {
    $roots = @(
        "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
    )
    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        $hit = Get-ChildItem $root -ErrorAction SilentlyContinue |
            ForEach-Object { Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue } |
            Where-Object { $_.DisplayName -like "*buttre*" } |
            Select-Object -First 1
        if ($hit) { return $hit.DisplayName }
    }
    return $null
}

function Warn-IfMsiInstalled {
    $installed = Test-MsiInstalled
    if ($installed) {
        Write-Host "  WARNING: '$installed' is installed via MSI." -ForegroundColor Yellow
        Write-Host "           This script overwrites its files but leaves its Add/Remove entry." -ForegroundColor Gray
        Write-Host "           Uninstall it from Settings first for a clean test." -ForegroundColor Gray
        Write-Host ""
    }
}

# ── MSI passthrough (build only — no admin needed) ───────────────────────────
if ($Msi) {
    & (Join-Path $repoRoot "installers\windows\build_installer.ps1")
    Write-Host ""
    Write-Host "  Install it with:  msiexec /i <path-to-msi> /l*v install.log" -ForegroundColor Cyan
    Write-Host ""
    exit $LASTEXITCODE
}

Assert-Admin
Assert-Bitness

# ── Uninstall ────────────────────────────────────────────────────────────────
if ($Uninstall) {
    Write-Host ""
    Write-Host "  Removing buttre TSF" -ForegroundColor Cyan
    Write-Host ""
    Warn-IfMsiInstalled

    $dll = Join-Path $InstallDir "buttre_platform.dll"
    if (Test-Path $dll) { & regsvr32.exe /u /s $dll 2>$null }
    Remove-Registration
    Stop-TsfHosts
    if (Test-Path $InstallDir) {
        Remove-Item $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
    }

    Write-Host "  Done. Also remove it from Settings > Time & Language >" -ForegroundColor Green
    Write-Host "  Vietnamese > Options if it still shows there." -ForegroundColor Gray
    Write-Host ""
    exit 0
}

Push-Location $repoRoot
try {
    $mode = if ($SelfRegister) { "regsvr32 (DllRegisterServer — NOT what the MSI does)" }
            else { "registry writes (same as the release MSI)" }
    Write-Host ""
    Write-Host "  buttre TSF local install" -ForegroundColor Cyan
    Write-Host "  Target: $InstallDir" -ForegroundColor Gray
    Write-Host "  Register via: $mode" -ForegroundColor Gray
    Write-Host ""
    Warn-IfMsiInstalled

    # ── Build ────────────────────────────────────────────────────────────
    if ($NoBuild) {
        Write-Host "[1/4] Skipped build (-NoBuild)" -ForegroundColor DarkGray
    } else {
        Write-Host "[1/4] Building release..." -ForegroundColor Yellow
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        & cargo build -p buttre-platform --release
        if ($LASTEXITCODE -ne 0) {
            Write-Host "ERROR: cargo build failed." -ForegroundColor Red
            Write-Host "       If it failed writing buttre_platform.dll, an app still has the" -ForegroundColor Gray
            Write-Host "       old one loaded — close Word/VS Code/browsers and retry." -ForegroundColor Gray
            exit 1
        }
        $sw.Stop()
        Write-Host "      OK ($([math]::Round($sw.Elapsed.TotalSeconds, 1))s)" -ForegroundColor Green
    }

    $targetDir = Join-Path $repoRoot "target\release"
    $dllSource = Join-Path $targetDir "buttre_platform.dll"
    $exeSource = Join-Path $targetDir "buttre.exe"
    foreach ($required in @($dllSource, $exeSource)) {
        if (-not (Test-Path $required)) {
            Write-Host "ERROR: missing $required — build first (drop -NoBuild)." -ForegroundColor Red
            exit 1
        }
    }

    # ── Unregister + free the DLL ────────────────────────────────────────
    Write-Host "[2/4] Unregistering the previous install..." -ForegroundColor Yellow
    $oldDll = Join-Path $InstallDir "buttre_platform.dll"
    if (Test-Path $oldDll) { & regsvr32.exe /u /s $oldDll 2>$null }
    Remove-Registration
    Stop-TsfHosts
    Write-Host "      Done" -ForegroundColor Gray

    # ── Stage the MSI's payload ──────────────────────────────────────────
    Write-Host "[3/4] Installing files..." -ForegroundColor Yellow
    New-Item -ItemType Directory -Force $InstallDir | Out-Null

    Copy-OverLockedFile $dllSource (Join-Path $InstallDir "buttre_platform.dll")
    Write-Host "      buttre_platform.dll ($([math]::Round((Get-Item $dllSource).Length / 1MB, 1)) MB)" -ForegroundColor Gray
    Copy-OverLockedFile $exeSource (Join-Path $InstallDir "buttre.exe")
    Write-Host "      buttre.exe" -ForegroundColor Gray

    # keyboards\*.toml — the MSI ships these as individual components.
    $kbSource = Join-Path $repoRoot "keyboards"
    if (Test-Path $kbSource) {
        $kbDest = Join-Path $InstallDir "keyboards"
        New-Item -ItemType Directory -Force $kbDest | Out-Null
        Copy-Item "$kbSource\*.toml" $kbDest -Force
        Write-Host "      keyboards\ ($((Get-ChildItem "$kbDest\*.toml").Count) layouts)" -ForegroundColor Gray
    }

    # buttre_nom.db — conditional in the MSI (IncludeNomDb), so conditional here.
    $nomDb = @(
        (Join-Path $targetDir "buttre_nom.db"),
        (Join-Path $repoRoot "buttre_nom.db")
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    if ($nomDb) {
        Copy-Item $nomDb (Join-Path $InstallDir "buttre_nom.db") -Force
        Write-Host "      buttre_nom.db" -ForegroundColor Gray
    } else {
        Write-Host "      buttre_nom.db not found — Nôm unavailable, as in an MSI built without it" -ForegroundColor DarkYellow
    }

    # ── Register ─────────────────────────────────────────────────────────
    Write-Host "[4/4] Registering..." -ForegroundColor Yellow
    $installedDll = Join-Path $InstallDir "buttre_platform.dll"
    if ($SelfRegister) {
        & regsvr32.exe /s $installedDll
        if ($LASTEXITCODE -ne 0) {
            Write-Host "ERROR: regsvr32 failed (code $LASTEXITCODE)." -ForegroundColor Red
            exit 1
        }
    } else {
        Add-MsiRegistration $installedDll
    }

    # Verify what actually landed, rather than trusting the writes above.
    $inprocValue = (Get-ItemProperty "$ClsidKey\InProcServer32" -ErrorAction SilentlyContinue).'(default)'
    $enabled = (Get-ItemProperty "$TipKey\LanguageProfile\$LangId\$ProfileGuid" `
        -Name Enable -ErrorAction SilentlyContinue).Enable
    if (-not $inprocValue -or $enabled -ne 1) {
        Write-Host "ERROR: registration did not take." -ForegroundColor Red
        Write-Host "       InProcServer32='$inprocValue' Enable='$enabled'" -ForegroundColor Gray
        exit 1
    }
    Write-Host "      CLSID -> $inprocValue" -ForegroundColor Gray
    Write-Host "      vi-VN profile enabled" -ForegroundColor Gray

    Write-Host ""
    Write-Host "  Installed." -ForegroundColor Green
    Write-Host ""
    Write-Host "  1. Settings > Time & Language > Language & Region >" -ForegroundColor White
    Write-Host "     Vietnamese > Options > Add a keyboard > buttre Vietnamese IME" -ForegroundColor White
    Write-Host "  2. Switch to it with Win+Space" -ForegroundColor White
    Write-Host "  3. Start the tray app:  $InstallDir\buttre.exe" -ForegroundColor White
    Write-Host "  4. RESTART the app you test in — Word, VS Code and browsers keep the" -ForegroundColor White
    Write-Host "     old DLL mapped until they exit" -ForegroundColor White
    Write-Host ""
    Write-Host "  Status:    .\scripts\check-tsf-status.ps1" -ForegroundColor Gray
    Write-Host "  Remove:    .\scripts\build-tsf.ps1 -Uninstall" -ForegroundColor Gray
    Write-Host ""
}
finally {
    Pop-Location
}
