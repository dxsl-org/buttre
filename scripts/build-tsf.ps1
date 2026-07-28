#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Build the TSF text service, and optionally install it exactly the way the
    release MSI does, for local testing before shipping.

.DESCRIPTION
    BUILDS ONLY by default — no admin, nothing touched outside target\. It
    reports the payload the MSI would ship and the command to install it.

    Add -Install to actually put it on the machine. That step mirrors
    installers\windows\product.wxs: the same files into the same directory,
    the same registry keys. What you test is then what users get, and it needs
    Administrator for the same reason the MSI does (Program Files + HKLM).

    Registration under -Install is REGISTRY-ONLY, matching the MSI — the MSI
    has no CustomAction, so it never calls the DLL's own DllRegisterServer.
    That difference is not cosmetic:

      MSI (default here)   vi-VN profile only, no TSF category registration
      -SelfRegister        regsvr32 -> DllRegisterServer, which ALSO adds an
                           en-US profile and registers the TIP_KEYBOARD and
                           DISPLAYATTRIBUTEPROVIDER categories

    Testing with regsvr32 therefore tests a MORE registered service than the
    one you ship. Use the default to reproduce what users will actually have;
    use -SelfRegister only to isolate whether a bug comes from the missing
    category registration.

.PARAMETER Install
    After building, copy the payload into place and register it. Requires
    Administrator.

.PARAMETER Uninstall
    Remove the registration and the installed files, then exit. Requires
    Administrator.

.PARAMETER SelfRegister
    With -Install, register through regsvr32 (DllRegisterServer) instead of
    the MSI's registry writes. See the description for how the two differ.

.PARAMETER Msi
    Build a real .msi via installers\windows\build_installer.ps1 and stop
    without installing it. The most faithful test of all, and the slowest —
    needs cargo-wix. No admin needed.

.PARAMETER Debug
    Build the DEBUG DLL. The only reason to: TSF logging is compiled at WARN
    in release and at DEBUG in debug builds (see tsf\logging.rs), so this is
    the only way to see per-keystroke traces. NOT a shippable build — it is
    several times larger and slower, and it loads into every application on
    the machine.

.PARAMETER NoBuild
    Skip cargo and use whatever is already built.

.PARAMETER InstallDir
    Override the install location. Defaults to the MSI's directory.

.EXAMPLE
    # Just build (no admin, nothing installed):
    .\scripts\build-tsf.ps1

.EXAMPLE
    # Build and install like the release does (Administrator):
    .\scripts\build-tsf.ps1 -Install

.EXAMPLE
    # Reinstall the DLL you already built (Administrator):
    .\scripts\build-tsf.ps1 -Install -NoBuild

.EXAMPLE
    # Clean up (Administrator):
    .\scripts\build-tsf.ps1 -Uninstall
#>
param(
    [switch]$Install,
    [switch]$Uninstall,
    [switch]$SelfRegister,
    [switch]$Msi,
    [switch]$Debug,
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

function Show-MsiInstallWarning {
    # Name chosen for PSUseApprovedVerbs; it only prints.
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

# Elevation is demanded only by the steps that genuinely touch the machine —
# building never does, so a plain `.\build-tsf.ps1` runs in any shell.
if ($Install -or $Uninstall) {
    Assert-Admin
    Assert-Bitness
}

# ── Uninstall ────────────────────────────────────────────────────────────────
if ($Uninstall) {
    Write-Host ""
    Write-Host "  Removing buttre TSF" -ForegroundColor Cyan
    Write-Host ""
    Show-MsiInstallWarning

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
    $profileDir = if ($Debug) { "debug" } else { "release" }
    $steps = if ($Install) { 4 } else { 2 }
    Write-Host ""
    Write-Host "  buttre TSF $(if ($Install) { 'build + install' } else { 'build' })" -ForegroundColor Cyan
    if ($Install) {
        $mode = if ($SelfRegister) { "regsvr32 (DllRegisterServer — NOT what the MSI does)" }
                else { "registry writes (same as the release MSI)" }
        Write-Host "  Target: $InstallDir" -ForegroundColor Gray
        Write-Host "  Register via: $mode" -ForegroundColor Gray
    }
    if ($Debug) {
        Write-Host "  Profile: DEBUG — for TSF logs only, never ship this" -ForegroundColor Yellow
    }
    Write-Host ""
    if ($Install) { Show-MsiInstallWarning }

    # ── Build ────────────────────────────────────────────────────────────
    if ($NoBuild) {
        Write-Host "[1/$steps] Skipped build (-NoBuild)" -ForegroundColor DarkGray
    } else {
        Write-Host "[1/$steps] Building $profileDir..." -ForegroundColor Yellow
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $buildArgs = @("build", "-p", "buttre-platform")
        if (-not $Debug) { $buildArgs += "--release" }
        & cargo @buildArgs
        if ($LASTEXITCODE -ne 0) {
            Write-Host "ERROR: cargo build failed." -ForegroundColor Red
            Write-Host "       If it failed writing buttre_platform.dll, an app still has the" -ForegroundColor Gray
            Write-Host "       old one loaded — close Word/VS Code/browsers and retry." -ForegroundColor Gray
            exit 1
        }
        $sw.Stop()
        Write-Host "      OK ($([math]::Round($sw.Elapsed.TotalSeconds, 1))s)" -ForegroundColor Green
    }

    $targetDir = Join-Path $repoRoot "target\$profileDir"
    $dllSource = Join-Path $targetDir "buttre_platform.dll"
    $exeSource = Join-Path $targetDir "buttre.exe"
    foreach ($required in @($dllSource, $exeSource)) {
        if (-not (Test-Path $required)) {
            Write-Host "ERROR: missing $required — build first (drop -NoBuild)." -ForegroundColor Red
            exit 1
        }
    }

    # buttre_nom.db is conditional in the MSI (IncludeNomDb), so it is here too.
    $nomDb = @(
        (Join-Path $targetDir "buttre_nom.db"),
        (Join-Path $repoRoot "buttre_nom.db")
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
    $kbSource = Join-Path $repoRoot "keyboards"

    # ── Report the payload ───────────────────────────────────────────────
    Write-Host "[2/$steps] Payload the MSI would ship:" -ForegroundColor Yellow
    Write-Host "      buttre_platform.dll ($([math]::Round((Get-Item $dllSource).Length / 1MB, 1)) MB)" -ForegroundColor Gray
    Write-Host "      buttre.exe          ($([math]::Round((Get-Item $exeSource).Length / 1MB, 1)) MB)" -ForegroundColor Gray
    if (Test-Path $kbSource) {
        Write-Host "      keyboards\          ($((Get-ChildItem "$kbSource\*.toml").Count) layouts)" -ForegroundColor Gray
    }
    if ($nomDb) {
        Write-Host "      buttre_nom.db" -ForegroundColor Gray
    } else {
        Write-Host "      buttre_nom.db not found — Nôm unavailable, as in an MSI built without it" -ForegroundColor DarkYellow
    }

    if (-not $Install) {
        Write-Host ""
        Write-Host "  Built. Nothing installed — this step needs no admin." -ForegroundColor Green
        Write-Host "  From:  $targetDir" -ForegroundColor Gray
        Write-Host ""
        Write-Host "  To install it (Administrator PowerShell):" -ForegroundColor Cyan
        $installCmd = ".\scripts\build-tsf.ps1 -Install -NoBuild"
        if ($Debug) { $installCmd += " -Debug" }
        Write-Host "    $installCmd" -ForegroundColor White
        Write-Host ""
        exit 0
    }

    # ── Unregister + free the DLL ────────────────────────────────────────
    Write-Host "[3/$steps] Unregistering the previous install..." -ForegroundColor Yellow
    $oldDll = Join-Path $InstallDir "buttre_platform.dll"
    if (Test-Path $oldDll) { & regsvr32.exe /u /s $oldDll 2>$null }
    Remove-Registration
    Stop-TsfHosts
    Write-Host "      Done" -ForegroundColor Gray

    # ── Copy the MSI's payload into place ────────────────────────────────
    Write-Host "[4/$steps] Installing files..." -ForegroundColor Yellow
    New-Item -ItemType Directory -Force $InstallDir | Out-Null

    Copy-OverLockedFile $dllSource (Join-Path $InstallDir "buttre_platform.dll")
    Copy-OverLockedFile $exeSource (Join-Path $InstallDir "buttre.exe")

    # keyboards\*.toml — the MSI ships these as individual components.
    if (Test-Path $kbSource) {
        $kbDest = Join-Path $InstallDir "keyboards"
        New-Item -ItemType Directory -Force $kbDest | Out-Null
        Copy-Item "$kbSource\*.toml" $kbDest -Force
    }
    if ($nomDb) { Copy-Item $nomDb (Join-Path $InstallDir "buttre_nom.db") -Force }
    Write-Host "      Copied to $InstallDir" -ForegroundColor Gray

    # ── Register ─────────────────────────────────────────────────────────
    Write-Host "      Registering..." -ForegroundColor Yellow
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

    # Report what actually landed, rather than trusting the writes above.
    #
    # Enumerated, never assumed: the two registration paths disagree about WHICH
    # language the profile goes under. The MSI hard-codes vi-VN; the DLL's own
    # register_tsf_service uses GetUserDefaultLangID plus en-US, so on an
    # English Windows it registers under en-US and vi-VN is absent. Checking a
    # fixed LCID reported a failure for a registration that had in fact
    # succeeded — under a language the check never looked at.
    $inprocValue = (Get-ItemProperty "$ClsidKey\InProcServer32" -ErrorAction SilentlyContinue).'(default)'
    if (-not $inprocValue) {
        Write-Host "ERROR: the COM server did not register — CLSID\InProcServer32 is empty." -ForegroundColor Red
        exit 1
    }
    Write-Host "      CLSID -> $inprocValue" -ForegroundColor Gray

    $profiles = @()
    Get-ChildItem "$TipKey\LanguageProfile" -ErrorAction SilentlyContinue | ForEach-Object {
        $lcid = Split-Path $_.Name -Leaf
        $p = Get-ItemProperty "$($_.PSPath)\$ProfileGuid" -ErrorAction SilentlyContinue
        if ($p -and $p.Enable -eq 1) {
            $profiles += [pscustomobject]@{ Lcid = $lcid; Description = $p.Description }
        }
    }
    if (-not $profiles) {
        Write-Host "ERROR: no enabled language profile — the IME cannot be added to any language." -ForegroundColor Red
        exit 1
    }
    foreach ($p in $profiles) {
        $lang = switch ($p.Lcid) {
            "0x0000042A" { "vi-VN" }
            "0x00000409" { "en-US" }
            default      { $p.Lcid }
        }
        Write-Host "      profile $lang enabled — '$($p.Description)'" -ForegroundColor Gray
    }

    # The TIP_KEYBOARD category is what makes Windows treat this as a named
    # keyboard IME rather than a bare COM object; without it the language
    # switcher shows the LANGUAGE instead of the service. RegisterCategory
    # writes it, the MSI's registry-only path does not.
    $catKey = "$TipKey\Category\Category"
    if (Test-Path $catKey) {
        Write-Host "      categories registered" -ForegroundColor Gray
    } else {
        Write-Host "      WARNING: no TSF category registered." -ForegroundColor Yellow
        Write-Host "               Windows will not show this as a named IME." -ForegroundColor Gray
    }

    $addUnder = ($profiles | ForEach-Object {
        switch ($_.Lcid) { "0x0000042A" { "Vietnamese" } "0x00000409" { "English (United States)" } default { $_.Lcid } }
    }) -join " or "

    Write-Host ""
    Write-Host "  Installed." -ForegroundColor Green
    Write-Host ""
    Write-Host "  1. Settings > Time & Language > Language & Region >" -ForegroundColor White
    Write-Host "     $addUnder > Options > Add a keyboard" -ForegroundColor White
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
