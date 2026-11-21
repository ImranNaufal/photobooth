# Skrip PowerShell untuk memulakan Photobooth RS
$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot
Set-Location -Path $projectRoot

$msvcrtBin = "C:\Users\it-01995\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.MSVCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"
$env:Path = "$msvcrtBin;$HOME\.cargo\bin;$env:Path"

Write-Host "===================================================" -ForegroundColor Cyan
Write-Host "      MEMULAKAN PHOTOBOOTH-RS (WINDOWS KIOSK)      " -ForegroundColor Green
Write-Host "===================================================" -ForegroundColor Cyan

if (Test-Path "target\debug\photobooth-rs.exe") {
    Write-Host "Melancarkan target\debug\photobooth-rs.exe..." -ForegroundColor Yellow
    Start-Process -FilePath "target\debug\photobooth-rs.exe"
} else {
    Write-Host "Mengkompilasi dan memulakan via cargo run..." -ForegroundColor Yellow
    cargo run
}
