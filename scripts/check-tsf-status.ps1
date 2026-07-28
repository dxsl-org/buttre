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
Write-Host "3. Added as one of YOUR input methods:" -ForegroundColor Yellow
# The decisive check, and the one this script used to punt on ("requires
# additional APIs"). Registration only makes the IME available in Windows'
# picker; until the user ADDS it, no application activates the text service and
# buttre.exe falls back to the global-hook backend — which looks like a bug in
# TSF but is the tray correctly refusing a backend that cannot receive keys.
$enabled = @()
Get-ChildItem "HKCU:\Software\Microsoft\CTF\SortOrder\AssemblyItem" -Recurse -ErrorAction SilentlyContinue |
    ForEach-Object {
        $item = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
        if ($item.CLSID -eq $clsid) {
            # .../AssemblyItem\<langid>\<category>\<index>
            $enabled += ($_.Name -split '\\')[-3]
        }
    }

if ($enabled) {
    foreach ($langId in ($enabled | Select-Object -Unique)) {
        Write-Host "  OK  added under $langId" -ForegroundColor Green
    }
    Write-Host "  buttre.exe will use the TSF backend." -ForegroundColor Gray
} else {
    Write-Host "  NO  buttre is registered but NOT added to your input methods." -ForegroundColor Red
    Write-Host ""
    Write-Host "  buttre.exe therefore runs the HOOK backend, not TSF." -ForegroundColor Yellow
    Write-Host "  Everything you type goes through the hook, so TSF-only features" -ForegroundColor Gray
    Write-Host "  (the Nom candidate window) never appear." -ForegroundColor Gray
    Write-Host ""
    Write-Host "  Fix: Settings > Time and language > Language and region >" -ForegroundColor Cyan
    Write-Host "       (a language) > Options > Add a keyboard > buttre" -ForegroundColor Cyan
    Write-Host "       Then Win+Space to switch to it, and RESTART buttre.exe." -ForegroundColor Cyan
}

Write-Host ""
Write-Host "4. Debug DLL Check:" -ForegroundColor Yellow
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
Write-Host "5. Processes:" -ForegroundColor Yellow
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
