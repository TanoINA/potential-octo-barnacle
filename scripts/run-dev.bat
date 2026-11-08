@echo off
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
cd /d "%~dp0.."
echo ==========================================
echo    Starting AetherAgent (Rust + Tauri)
echo ==========================================
npm run tauri dev
pause
