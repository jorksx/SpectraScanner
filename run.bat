@echo off
cd /d "%~dp0"
if exist "%~dp0spectrascanner.exe" (
    start "" "%~dp0spectrascanner.exe"
) else (
    start "" "%~dp0target\release\spectrascanner.exe"
)
