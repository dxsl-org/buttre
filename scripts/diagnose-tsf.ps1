# Comprehensive TSF diagnosis
# Run as Administrator

Write-Host "=== buttre TSF Diagnosis ===" -ForegroundColor Cyan
Write-Host ""

$clsid = "{E6B8A6C0-1234-5678-9ABC-DEF012345678}"

# 1. Check which DLL is registered
Write-Host "1. Registered DLL location:" -ForegroundColor Yellow
$clsidPath = "HKLM:\SOFTWARE\Classes\CLSID\$clsid\InprocServer32"
if (Test-Path $clsidPath) {
    $registeredDll = (Get-ItemProperty $clsidPath).'(default)'
    Write-Host "  $registeredDll" -ForegroundColor White
    
    if (Test-Path $registeredDll) {
        $dll = Get-Item $registeredDll
        Write-Host "  ✓ File exists" -ForegroundColor Green
        Write-Host "  Size: $($dll.Length) bytes" -ForegroundColor Gray
        Write-Host "  Modified: $($dll.LastWriteTime)" -ForegroundColor Gray
        
        # Check if debug or release
        if ($registeredDll -like "*\debug\*") {
            Write-Host "  Type: DEBUG (good for logging)" -ForegroundColor Green
        } else {
            Write-Host "  Type: RELEASE (no debug logs!)" -ForegroundColor Red
        }
    } else {
        Write-Host "  ✗ File NOT found!" -ForegroundColor Red
    }
} else {
    Write-Host "  ✗ Not registered" -ForegroundColor Red
}

Write-Host ""
Write-Host "2. Language Profiles:" -ForegroundColor Yellow
$tipPath = "HKLM:\SOFTWARE\Microsoft\CTF\TIP\$clsid"
if (Test-Path $tipPath) {
    $langPath = "$tipPath\LanguageProfile"
    if (Test-Path $langPath) {
        $profiles = Get-ChildItem $langPath
        foreach ($profile in $profiles) {
            $langId = $profile.PSChildName
            Write-Host "  Language: $langId" -ForegroundColor White
            
            $subProfiles = Get-ChildItem $profile.PSPath
            foreach ($sub in $subProfiles) {
                $guid = $sub.PSChildName
                Write-Host "    Profile GUID: $guid" -ForegroundColor Gray
                
                $desc = (Get-ItemProperty $sub.PSPath -Name "Description" -ErrorAction SilentlyContinue).Description
                if ($desc) {
                    Write-Host "    Description: $desc" -ForegroundColor Gray
                }
            }
        }
    }
}

Write-Host ""
Write-Host "3. Check for duplicate registrations:" -ForegroundColor Yellow
$allTips = Get-ChildItem "HKLM:\SOFTWARE\Microsoft\CTF\TIP" -ErrorAction SilentlyContinue
$buttreTips = $allTips | Where-Object { 
    $profiles = Get-ChildItem "$($_.PSPath)\LanguageProfile" -Recurse -ErrorAction SilentlyContinue
    $hasbuttre = $profiles | Get-ItemProperty -Name "Description" -ErrorAction SilentlyContinue | 
                Where-Object { $_.Description -like "*buttre*" }
    $hasbuttre -ne $null
}

if ($buttreTips) {
    Write-Host "  Found buttre-related TIPs:" -ForegroundColor White
    foreach ($tip in $buttreTips) {
        $tipGuid = $tip.PSChildName
        Write-Host "    $tipGuid" -ForegroundColor Gray
        if ($tipGuid -ne $clsid) {
            Write-Host "      ⚠ Different GUID! This might conflict" -ForegroundColor Yellow
        }
    }
} else {
    Write-Host "  No duplicates found" -ForegroundColor Green
}

Write-Host ""
Write-Host "4. Test DLL manually:" -ForegroundColor Yellow
Write-Host "  Run this to test if DLL loads:" -ForegroundColor White
Write-Host "  regsvr32.exe `"$registeredDll`"" -ForegroundColor Cyan
Write-Host ""
Write-Host "  Should show: 'DllRegisterServer succeeded'" -ForegroundColor Gray

Write-Host ""
Write-Host "5. Restart CTF:" -ForegroundColor Yellow
Write-Host "  To force Windows reload TSF services:" -ForegroundColor White
Write-Host "  taskkill /f /im ctfmon.exe; start ctfmon.exe" -ForegroundColor Cyan

Write-Host ""
Write-Host "6. Check log file:" -ForegroundColor Yellow
$logPath = Join-Path $env:TEMP "buttre_tsf_debug.log"
if (Test-Path $logPath) {
    $log = Get-Content $logPath -Tail 20
    if ($log) {
        Write-Host "  ✓ Log file exists with content:" -ForegroundColor Green
        Write-Host "  $logPath" -ForegroundColor Gray
        Write-Host ""
        Write-Host "  Last 10 lines:" -ForegroundColor Gray
        Get-Content $logPath -Tail 10 | ForEach-Object { Write-Host "    $_" -ForegroundColor DarkGray }
    } else {
        Write-Host "  ⚠ Log file empty - DLL never loaded" -ForegroundColor Yellow
    }
} else {
    Write-Host "  ✗ No log file - DLL never loaded" -ForegroundColor Red
}

Write-Host ""
Write-Host "=== Next Steps ===" -ForegroundColor Cyan
Write-Host "If DLL is RELEASE (not DEBUG):" -ForegroundColor Yellow
Write-Host "  1. Run: .\scripts\build-tsf.ps1 -Debug" -ForegroundColor White
Write-Host ""
Write-Host "If log file is empty/missing:" -ForegroundColor Yellow
Write-Host "  1. Restart CTF: taskkill /f /im ctfmon.exe; start ctfmon.exe" -ForegroundColor White
Write-Host "  2. Or logout/login" -ForegroundColor White
Write-Host "  3. Type in Notepad again" -ForegroundColor White
Write-Host ""
Write-Host "If still no logs:" -ForegroundColor Yellow
Write-Host "  Windows might not be loading our DLL." -ForegroundColor White
Write-Host "  Check Event Viewer > Application for errors" -ForegroundColor White
