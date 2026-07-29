#!/usr/bin/env pwsh
<#
.SYNOPSIS
    Tail the TSF text service's logs while you type.

.DESCRIPTION
    The text service runs INSIDE the host application (Word, a browser, the
    shell), so it writes one log file per process to
    %LOCALAPPDATA%\buttre\tsf-<pid>.log. This follows all of them at once and
    prefixes each line with the process it came from.

    Level is WARN by default — enough to see a failing edit session, and it
    carries no typed text. -Verbose creates the opt-in marker
    %LOCALAPPDATA%\buttre\tsf-debug, which raises every process to DEBUG. That
    DOES record the characters being typed, which is why it is opt-in; the
    marker is removed again when this script exits.

    Applications must be RESTARTED after the marker changes: the level is
    chosen once per process, when the text service activates.

.PARAMETER Clear
    Delete existing log files before watching.

.EXAMPLE
    .\scripts\watch-tsf-logs.ps1 -Verbose -Clear
#>
[CmdletBinding()]
param(
    [switch]$Clear
)

$ErrorActionPreference = "Stop"
$logDir = Join-Path $env:LOCALAPPDATA "buttre"
$marker = Join-Path $logDir "tsf-debug"
$wantVerbose = $VerbosePreference -ne "SilentlyContinue"

New-Item -ItemType Directory -Force $logDir | Out-Null

if ($Clear) {
    Get-ChildItem $logDir -Filter "tsf-*.log" -ErrorAction SilentlyContinue | Remove-Item -Force
    Write-Host "Cleared old logs." -ForegroundColor Gray
}

$markerCreatedHere = $false
if ($wantVerbose -and -not (Test-Path $marker)) {
    New-Item -ItemType File $marker | Out-Null
    $markerCreatedHere = $true
    Write-Host "DEBUG logging ON (records typed characters)." -ForegroundColor Yellow
    Write-Host "RESTART the application you are testing — the level is fixed" -ForegroundColor Yellow
    Write-Host "when the text service activates in that process." -ForegroundColor Gray
}

Write-Host ""
Write-Host "Watching $logDir\tsf-<pid>.log  (Ctrl+C to stop)" -ForegroundColor Cyan
Write-Host ""

# Polled rather than `Get-Content -Wait`: every new host PROCESS creates a new
# file, and -Wait would only ever follow the one that existed at startup.
$linesSeen = @{}
try {
    while ($true) {
        foreach ($file in Get-ChildItem $logDir -Filter "tsf-*.log" -ErrorAction SilentlyContinue) {
            $lines = @(Get-Content $file.FullName -ErrorAction SilentlyContinue)
            $seen = if ($linesSeen.ContainsKey($file.Name)) { $linesSeen[$file.Name] } else { 0 }
            if ($lines.Count -lt $seen) { $seen = 0 }  # file was truncated
            if ($lines.Count -le $seen) { continue }

            $procId = $file.BaseName -replace '^tsf-', ''
            $owner = (Get-Process -Id $procId -ErrorAction SilentlyContinue).ProcessName
            $tag = if ($owner) { "$owner/$procId" } else { $procId }

            foreach ($line in ($lines | Select-Object -Skip $seen)) {
                $colour = if ($line -match "ERROR") { "Red" }
                          elseif ($line -match "WARN") { "Yellow" }
                          else { "Gray" }
                Write-Host "[$tag] $line" -ForegroundColor $colour
            }
            $linesSeen[$file.Name] = $lines.Count
        }
        Start-Sleep -Milliseconds 300
    }
}
finally {
    if ($markerCreatedHere) {
        Remove-Item $marker -Force -ErrorAction SilentlyContinue
        Write-Host ""
        Write-Host "DEBUG logging marker removed." -ForegroundColor Gray
    }
}
