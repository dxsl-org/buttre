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
Write-Host "3. Current Input Method:" -ForegroundColor Yellow
# This requires additional APIs, showing alternative
Write-Host "  Check manually:" -ForegroundColor Gray
Write-Host "  - Press Win+Space to see keyboard list" -ForegroundColor White
Write-Host "  - Look for 'buttre - Vietnamese Input'" -ForegroundColor White

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
    Write-Host "  Run: .\scripts\build-tsf.ps1 -Debug" -ForegroundColor Yellow
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
