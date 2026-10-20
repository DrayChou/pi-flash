@echo off
rem Double-click to sync GitHub issues into docs\issues.md
chcp 65001 >nul
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\sync-issues.ps1"
set ERR=%ERRORLEVEL%
echo.
pause
exit /b %ERR%
