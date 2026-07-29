<#
.SYNOPSIS
    Remove every trace of buttre from THIS USER's Windows input-method list, so
    the keyboard can be added again from a known-clean state.

.DESCRIPTION
    Registration and selection live in different hives, and this script only
    touches the second one:

      HKLM\SOFTWARE\Microsoft\CTF\TIP\<clsid>   the installer's: "it EXISTS"
      HKCU\...                                  Windows': "this user WANTS it"

    Deleting the HKLM half is what broke things before: the profile vanished,
    Windows pruned the user's HKCU entry, and a dangling Preload value was left
    behind that still appeared in the keyboard list and silently resolved to the
    plain layout. So the default here is HKCU only, which needs no admin and
    cannot damage the install.

    It also restarts ctfmon. Windows caches the input-method list in ctfmon and
    TextInputHost, so entries keep showing in Win+Space after their registry
    keys are gone — the "ghost buttre entries" that look like a failed cleanup
    but are only a stale cache.

.PARAMETER All
    Also unregister the machine-wide install (HKLM), for a truly from-scratch
    state. Requires Administrator. Reinstall afterwards before adding the
    keyboard again.

.PARAMETER DryRun
    Report what would be removed and change nothing.

.EXAMPLE
    # Clean this user's selection, then add the keyboard by hand (no admin):
    .\scripts\reset-tsf-input.ps1

.EXAMPLE
    # See what it would touch first:
    .\scripts\reset-tsf-input.ps1 -DryRun
#>
[CmdletBinding()]
param(
    [switch]$All,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$Clsid = "{E6B8A6C0-1234-5678-9ABC-DEF012345678}"
$removed = 0

function Write-Action([string]$message) {
    if ($DryRun) {
        Write-Host "  would remove  $message" -ForegroundColor DarkYellow
    } else {
        Write-Host "  removed       $message" -ForegroundColor Green
    }
    $script:removed++
}

<# True when a Preload value is a text service (d<index><langid>) belonging to
buttre, or one that resolves to nothing at all.

A dangling entry is treated as ours on purpose: it is the exact wreckage an
unregister-then-register cycle leaves behind, it is unusable by definition, and
leaving it in place is what produced "pick Vietnamese - buttre, get
Vietnamese - US". #>
function Test-ButtreOrDanglingTip([string]$value) {
    if ($value -notmatch '^[dD](?<idx>[0-9a-fA-F]{3})(?<lang>[0-9a-fA-F]{4})$') { return $false }
    $index = '{0:x8}' -f [Convert]::ToInt32($Matches.idx, 16)
    $langKey = "0x0000{0}" -f $Matches.lang.ToLower()
    $assembly = Join-Path "HKCU:\Software\Microsoft\CTF\SortOrder\AssemblyItem" $langKey

    $resolved = Get-ChildItem $assembly -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.PSChildName -eq $index } |
        ForEach-Object { (Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue).CLSID } |
        Select-Object -First 1

    return (-not $resolved) -or ($resolved -eq $Clsid)
}

Write-Host ""
Write-Host "=== Reset buttre input-method state ===" -ForegroundColor Cyan
if ($DryRun) { Write-Host "    (dry run - nothing will change)" -ForegroundColor DarkYellow }
Write-Host ""

# ── 1. The user's own enable record ──────────────────────────────────────────
Write-Host "1. HKCU CTF\TIP (your enable/disable choice):" -ForegroundColor Yellow
$userTip = "HKCU:\Software\Microsoft\CTF\TIP\$Clsid"
if (Test-Path $userTip) {
    Write-Action $userTip
    if (-not $DryRun) { Remove-Item $userTip -Recurse -Force }
} else {
    Write-Host "  clean" -ForegroundColor Gray
}

# ── 2. Preload / Substitutes ─────────────────────────────────────────────────
# Preload values MUST stay numbered 1..N with no gaps, so survivors are
# rewritten rather than the removed ones simply deleted.
Write-Host ""
Write-Host "2. HKCU Keyboard Layout\Preload:" -ForegroundColor Yellow
$preloadKey = "HKCU:\Keyboard Layout\Preload"
$doomed = @()
if (Test-Path $preloadKey) {
    $preload = Get-ItemProperty $preloadKey
    $keep = @()
    foreach ($name in ((Get-Item $preloadKey).GetValueNames() | Sort-Object)) {
        $value = $preload.$name
        if (Test-ButtreOrDanglingTip $value) {
            Write-Action "Preload[$name] = $value"
            $doomed += $value
        } else {
            $keep += $value
        }
    }

    if ($doomed -and -not $DryRun) {
        foreach ($name in (Get-Item $preloadKey).GetValueNames()) {
            Remove-ItemProperty $preloadKey -Name $name
        }
        for ($i = 0; $i -lt $keep.Count; $i++) {
            New-ItemProperty $preloadKey -Name ($i + 1) -Value $keep[$i] -PropertyType String | Out-Null
        }
        Write-Host "  renumbered    $($keep.Count) surviving entry/entries" -ForegroundColor Gray
    }
    if (-not $doomed) { Write-Host "  clean" -ForegroundColor Gray }
}

$subKey = "HKCU:\Keyboard Layout\Substitutes"
if ($doomed -and (Test-Path $subKey)) {
    foreach ($value in $doomed) {
        if ((Get-Item $subKey).GetValueNames() -contains $value) {
            Write-Action "Substitutes[$value]"
            if (-not $DryRun) { Remove-ItemProperty $subKey -Name $value }
        }
    }
}

# ── 3. Per-language input-method lists and the assembly order ────────────────
Write-Host ""
Write-Host "3. HKCU per-language lists and SortOrder:" -ForegroundColor Yellow
$found = $false
foreach ($root in @(
    "HKCU:\Control Panel\International\User Profile",
    "HKCU:\Software\Microsoft\CTF\SortOrder\AssemblyItem",
    "HKCU:\Software\Microsoft\CTF\Assemblies"
)) {
    if (-not (Test-Path $root)) { continue }
    Get-ChildItem $root -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
        $item = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
        # Our CLSID can appear as a value NAME ("0409:{clsid}{profile}") or as
        # value DATA (SortOrder's CLSID field) — check both.
        foreach ($name in ($item.PSObject.Properties.Name | Where-Object { $_ -notlike "PS*" })) {
            if ($name -like "*$Clsid*" -or "$($item.$name)" -eq $Clsid) {
                Write-Action "$($_.Name -replace '^HKEY_CURRENT_USER','HKCU') -> $name"
                if (-not $DryRun) { Remove-ItemProperty $_.PSPath -Name $name -ErrorAction SilentlyContinue }
                $found = $true
            }
        }
    }
}
if (-not $found) { Write-Host "  clean" -ForegroundColor Gray }

# ── 4. Machine-wide registration (opt-in) ────────────────────────────────────
Write-Host ""
Write-Host "4. HKLM registration:" -ForegroundColor Yellow
if (-not $All) {
    Write-Host "  kept (pass -All to remove it too, needs Administrator)" -ForegroundColor Gray
} else {
    $exe = Join-Path ${env:ProgramFiles} "buttre\buttre.exe"
    if (-not (Test-Path $exe)) {
        Write-Host "  buttre.exe not installed - nothing to unregister." -ForegroundColor Gray
    } elseif ($DryRun) {
        Write-Host "  would run  $exe --unregister-tsf" -ForegroundColor DarkYellow
    } else {
        & $exe --unregister-tsf
        if ($LASTEXITCODE -eq 0) {
            Write-Host "  unregistered - REINSTALL before adding the keyboard again." -ForegroundColor Green
        } else {
            Write-Host "  --unregister-tsf failed (code $LASTEXITCODE)" -ForegroundColor Red
        }
    }
}

# ── 5. Flush the cached keyboard list ────────────────────────────────────────
Write-Host ""
Write-Host "5. Rebuilding the input-method cache:" -ForegroundColor Yellow
if ($DryRun) {
    Write-Host "  would restart ctfmon" -ForegroundColor DarkYellow
} else {
    # ctfmon owns the list Win+Space draws. Without this, deleted entries keep
    # appearing and look like the cleanup failed.
    Get-Process ctfmon -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Get-Process TextInputHost -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500
    Start-Process "$env:WINDIR\System32\ctfmon.exe" -ErrorAction SilentlyContinue
    Write-Host "  ctfmon restarted" -ForegroundColor Green
}

Write-Host ""
if ($removed -eq 0) {
    Write-Host "Nothing to clean - this user's input list has no buttre entries." -ForegroundColor Green
} else {
    Write-Host "$removed item(s) $(if ($DryRun) { 'would be' } else { '' }) removed." -ForegroundColor Green
}
Write-Host ""
Write-Host "Next: Settings > Time and language > Language and region >" -ForegroundColor Cyan
Write-Host "      English (United States) > Options > Add a keyboard > buttre" -ForegroundColor Cyan
Write-Host "Then: .\scripts\check-tsf-status.ps1   (section 3 must list a language)" -ForegroundColor Gray
Write-Host ""
