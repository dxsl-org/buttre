# Check buttre TSF status
Write-Host "=== buttre TSF Status Check ===" -ForegroundColor Cyan
Write-Host ""

# Check registry
Write-Host "1. COM Registration:" -ForegroundColor Yellow
$clsid = "{E6B8A6C0-1234-5678-9ABC-DEF012345678}"
$clsidPath = "HKLM:\SOFTWARE\Classes\CLSID\$clsid"

if (Test-Path $clsidPath) {
    Write-Host "  ✓ CLSID registered" -ForegroundColor Green
    $inprocPath = "$clsidPath\InprocServer32"
    if (Test-Path $inprocPath) {
        $dllPath = (Get-ItemProperty $inprocPath).'(default)'
        Write-Host "  DLL: $dllPath" -ForegroundColor Gray
        if (Test-Path $dllPath) {
            $dll = Get-Item $dllPath
            Write-Host "  Size: $($dll.Length) bytes" -ForegroundColor Gray
            Write-Host "  Modified: $($dll.LastWriteTime)" -ForegroundColor Gray
        } else {
            Write-Host "  ✗ DLL not found!" -ForegroundColor Red
        }
    }
} else {
    Write-Host "  ✗ Not registered" -ForegroundColor Red
}

Write-Host ""
Write-Host "2. TSF Service Registration:" -ForegroundColor Yellow
$tipPath = "HKLM:\SOFTWARE\Microsoft\CTF\TIP\$clsid"
if (Test-Path $tipPath) {
    Write-Host "  ✓ TIP registered" -ForegroundColor Green
    
    # Check language profiles
    $profiles = Get-ChildItem "$tipPath\LanguageProfile" -ErrorAction SilentlyContinue
    if ($profiles) {
        Write-Host "  Registered for languages:" -ForegroundColor Gray
        foreach ($lang in $profiles) {
            $langId = $lang.PSChildName
            Write-Host "    - $langId" -ForegroundColor Gray
        }
    }
} else {
    Write-Host "  ✗ TIP not registered" -ForegroundColor Red
}

Write-Host ""
Write-Host "3. Which backend will the tray use:" -ForegroundColor Yellow
# Ask the product, not the registry. An earlier version of this section read
# HKCU\...\CTF\SortOrder to infer "is buttre added as an input method" and
# answered NO while the text service was demonstrably running — SortOrder names
# the DEFAULT service per language, not every added one. `--tsf-status` calls the
# same function the tray decides with, so this can no longer disagree with it.
$buttreExe = @(
    (Join-Path ${env:ProgramFiles} "buttre\buttre.exe"),
    (Join-Path (Split-Path $PSScriptRoot -Parent) "target\release\buttre.exe")
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $buttreExe) {
    Write-Host "  ?   buttre.exe not found - install it, or build it first." -ForegroundColor DarkYellow
} else {
    # Start-Process with explicit redirection, NOT `& $exe`. buttre.exe is a
    # GUI-subsystem binary (it must be, or the tray would flash a console), and a
    # GUI child's stdout is only sometimes inherited by a capturing shell — so
    # `&` returned the report on one run and nothing on the next. Real pipes to
    # files are deterministic.
    $outFile = Join-Path $env:TEMP "buttre-tsf-status.out"
    $errFile = Join-Path $env:TEMP "buttre-tsf-status.err"
    $proc = Start-Process $buttreExe -ArgumentList "--tsf-status" -Wait -PassThru -NoNewWindow `
        -RedirectStandardOutput $outFile -RedirectStandardError $errFile
    $exitCode = $proc.ExitCode
    $status = @(Get-Content $outFile -ErrorAction SilentlyContinue)
    # An older buttre.exe does not know this flag and falls through to launching
    # the tray, which then exits non-zero on the single-instance lock. Treat a
    # missing verdict line as "cannot tell" — reporting HOOK there would be a
    # second lying diagnostic, which is what this section was rewritten to stop.
    $answered = ($status -join "`n") -match 'tray will use:'
    $status | ForEach-Object { Write-Host "  $_" -ForegroundColor Gray }

    if (-not $answered) {
        Write-Host "  ?   $buttreExe is too old for --tsf-status." -ForegroundColor DarkYellow
        Write-Host "      Reinstall (.\scripts\build-tsf.ps1 -Install) to get this check." -ForegroundColor Gray
    } elseif ($exitCode -eq 0) {
        Write-Host "  OK  TSF backend - the Nom candidate window is reachable." -ForegroundColor Green
    } else {
        Write-Host ""
        Write-Host "  Everything you type goes through the HOOK, so TSF-only features" -ForegroundColor Yellow
        Write-Host "  (the Nom candidate window) never appear." -ForegroundColor Gray
        Write-Host ""
        Write-Host "  Fix: Settings > Time and language > Language and region >" -ForegroundColor Cyan
        Write-Host "       (a language) > Options > Add a keyboard > buttre" -ForegroundColor Cyan
        Write-Host "       Then Win+Space to switch to it, and RESTART buttre.exe." -ForegroundColor Cyan
    }
}

Write-Host ""
Write-Host "4. Do your input-method entries still resolve:" -ForegroundColor Yellow
# A Preload value of the form d<index><langid> is a text service, and <index>
# has to name a subkey under CTF\SortOrder\AssemblyItem\0x0000<langid>. Our
# install flow unregisters and re-registers, which can leave HKCU pointing at an
# index that no longer exists. Windows then falls back through Substitutes to
# the plain keyboard layout — so the entry is still listed, still selectable,
# and silently gives you "Vietnamese - US" every time you pick it.
$assemblyRoot = "HKCU:\Software\Microsoft\CTF\SortOrder\AssemblyItem"
$dangling = @()
if (Test-Path "HKCU:\Keyboard Layout\Preload") {
    $preload = Get-ItemProperty "HKCU:\Keyboard Layout\Preload"
    foreach ($name in (Get-Item "HKCU:\Keyboard Layout\Preload").GetValueNames()) {
        $value = $preload.$name
        if ($value -notmatch '^[dD](?<idx>[0-9a-fA-F]{3})(?<lang>[0-9a-fA-F]{4})$') { continue }
        $index = '{0:x8}' -f [Convert]::ToInt32($Matches.idx, 16)
        $langKey = "0x0000{0}" -f $Matches.lang.ToLower()
        $resolved = Get-ChildItem (Join-Path $assemblyRoot $langKey) -Recurse -ErrorAction SilentlyContinue |
            Where-Object { $_.PSChildName -eq $index } |
            ForEach-Object { (Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue).CLSID } |
            Select-Object -First 1

        if (-not $resolved) {
            Write-Host "  BROKEN  $value ($langKey, assembly $index) resolves to nothing." -ForegroundColor Red
            $dangling += $value
        } elseif ($resolved -eq $clsid) {
            Write-Host "  OK      $value -> buttre" -ForegroundColor Green
        } else {
            Write-Host "  (other) $value -> $resolved" -ForegroundColor Gray
        }
    }
}

# The OTHER direction, and the one this section used to miss entirely — leaving
# it blank on the very run where it held the answer. A routing entry under
# SortOrder with no matching Preload value means buttre is installed, added, and
# correctly wired, but is NOT in the Win+Space cycle: Windows cycles Preload. It
# cannot be selected, with nothing anywhere saying why.
$routed = Get-ChildItem $assemblyRoot -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.PSChildName -match '^[0-9a-fA-F]{8}$' } |
    Where-Object { (Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue).CLSID -eq $clsid } |
    ForEach-Object {
        $parts = ($_.Name -split 'AssemblyItem\\')[1] -split '\\'
        # 0x0000<langid>\<category>\<8-digit index>  ->  d<3-digit index><langid>
        [pscustomobject]@{
            Expected = "d{0}{1}" -f $parts[2].Substring(5), $parts[0].Substring(6)
            Where    = $parts[0]
        }
    }

$inPreload = if (Test-Path $preloadKey) {
    $p = Get-ItemProperty $preloadKey
    (Get-Item $preloadKey).GetValueNames() | ForEach-Object { $p.$_ }
} else { @() }

foreach ($entry in $routed) {
    if ($inPreload -contains $entry.Expected) {
        Write-Host "  OK      $($entry.Expected) is in the Win+Space cycle" -ForegroundColor Green
    } else {
        Write-Host "  MISSING $($entry.Expected) - buttre is wired up under $($entry.Where)" -ForegroundColor Red
        Write-Host "          but absent from Preload, so Win+Space cannot reach it." -ForegroundColor Gray
        Write-Host ""
        Write-Host "  Fix: sign out and back in (Windows rebuilds Preload from the" -ForegroundColor Cyan
        Write-Host "       language list). If it still does not appear, add it directly:" -ForegroundColor Cyan
        # Literal path, not interpolated: this line is meant to be copy-pasted,
        # and a variable that happens to be out of scope would print an empty
        # path into a command the reader then runs.
        Write-Host '         $k = "HKCU:\Keyboard Layout\Preload"' -ForegroundColor White
        Write-Host '         $n = (((Get-Item $k).GetValueNames() | ForEach-Object { [int]$_ } | Measure-Object -Maximum).Maximum) + 1' -ForegroundColor White
        Write-Host "         New-ItemProperty `$k -Name `$n -Value '$($entry.Expected)' -PropertyType String" -ForegroundColor White
    }
}

if ($dangling) {
    $sub = Get-ItemProperty "HKCU:\Keyboard Layout\Substitutes" -ErrorAction SilentlyContinue
    foreach ($value in $dangling) {
        if ($sub.$value) {
            Write-Host "          Windows substitutes it with layout $($sub.$value)" -ForegroundColor Gray
        }
    }
    Write-Host ""
    Write-Host "  This is why picking that entry lands on the plain layout instead" -ForegroundColor Yellow
    Write-Host "  of buttre. Remove it and add buttre again:" -ForegroundColor Gray
    Write-Host "    Settings > Time and language > Language and region >" -ForegroundColor Cyan
    Write-Host "    (that language) > Options > Keyboards > remove the buttre entry," -ForegroundColor Cyan
    Write-Host "    then Add a keyboard > buttre. Sign out and back in to be sure." -ForegroundColor Cyan
}

Write-Host ""
Write-Host "5. Windows layout-switch hotkey:" -ForegroundColor Yellow
# Ctrl+Shift is Windows' own "Switch Keyboard Layout" chord by default, and it
# cycles through every Preload entry. buttre puts Ctrl+Shift+Z (word toggle) and
# Ctrl+Shift+1/2/3 (method switch) on top of it, and Ctrl+Shift+Left/Right is how
# everyone selects words — so the layout can flip away from buttre mid-sentence,
# with nothing anywhere to explain why.
$toggle = "HKCU:\Keyboard Layout\Toggle"
$layoutHotkey = if (Test-Path $toggle) {
    (Get-ItemProperty $toggle -ErrorAction SilentlyContinue).'Layout Hotkey'
} else { $null }

$entries = @()
if (Test-Path "HKCU:\Keyboard Layout\Preload") {
    $preload = Get-ItemProperty "HKCU:\Keyboard Layout\Preload"
    $entries = (Get-Item "HKCU:\Keyboard Layout\Preload").GetValueNames() |
        ForEach-Object { $preload.$_ }
}

if ($layoutHotkey -eq 3) {
    Write-Host "  OK  layout switching is unassigned - nothing fights buttre's chords." -ForegroundColor Green
} elseif ($entries.Count -le 1) {
    Write-Host "  OK  only one input entry, so there is nothing to cycle to." -ForegroundColor Green
} else {
    $shown = if ($null -eq $layoutHotkey) { "not set (Windows default: Ctrl+Shift)" } else { "value $layoutHotkey" }
    Write-Host "  WARN  Switch Keyboard Layout = $shown" -ForegroundColor Yellow
    Write-Host "        $($entries.Count) entries in the cycle: $($entries -join ', ')" -ForegroundColor Gray
    Write-Host "        Ctrl+Shift will flip away from buttre - including the" -ForegroundColor Gray
    Write-Host "        Ctrl+Shift+Left/Right everyone uses to select words." -ForegroundColor Gray
    Write-Host ""
    Write-Host "  Fix: Settings > Time and language > Typing > Advanced keyboard" -ForegroundColor Cyan
    Write-Host "       settings > Input language hot keys > Switch Keyboard Layout" -ForegroundColor Cyan
    Write-Host "       > Change Key Sequence > Not Assigned" -ForegroundColor Cyan
}

Write-Host ""
Write-Host "6. Debug DLL Check:" -ForegroundColor Yellow
$debugDll = "target\debug\buttre_platform.dll"
if (Test-Path $debugDll) {
    $dll = Get-Item $debugDll
    Write-Host "  ✓ Debug DLL exists" -ForegroundColor Green
    Write-Host "  Size: $($dll.Length) bytes" -ForegroundColor Gray
    Write-Host "  Modified: $($dll.LastWriteTime)" -ForegroundColor Gray
} else {
    Write-Host "  ✗ Debug DLL not found" -ForegroundColor Red
    Write-Host "  Run: .\scripts\build-tsf.ps1 -Install -Debug" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "7. Processes:" -ForegroundColor Yellow
$processes = @("TextInputHost", "ctfmon")
foreach ($proc in $processes) {
    $running = Get-Process -Name $proc -ErrorAction SilentlyContinue
    if ($running) {
        Write-Host "  ✓ $proc running (PID: $($running.Id))" -ForegroundColor Green
    } else {
        Write-Host "  ✗ $proc not running" -ForegroundColor Red
    }
}

Write-Host ""
Write-Host "=== Troubleshooting ===" -ForegroundColor Cyan
Write-Host "If buttre not in keyboard list:" -ForegroundColor Yellow
Write-Host "1. Restart TSF: taskkill /f /im ctfmon.exe && start ctfmon.exe" -ForegroundColor White
Write-Host "2. Or logout/login" -ForegroundColor White
Write-Host "3. Or restart computer" -ForegroundColor White
