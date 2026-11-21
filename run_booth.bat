@echo off
title Photobooth RS
cd /d "%~dp0"

echo ===================================================
echo       MEMULAKAN PHOTOBOOTH-RS (WINDOWS KIOSK)
echo ===================================================

set "MSVCRT_BIN=C:\Users\it-01995\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.MSVCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"
set "PATH=%MSVCRT_BIN%;%USERPROFILE%\.cargo\bin;%PATH%"

if exist "target\debug\photobooth-rs.exe" (
    echo Melancarkan photobooth-rs.exe...
    start "" "target\debug\photobooth-rs.exe"
    exit
) else (
    echo Mengkompilasi dan memulakan via cargo run...
    cargo run
)
