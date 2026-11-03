@echo off
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
echo ==========================================
echo    Starting AetherAgent (Rust + Tauri)
echo ==========================================
npm run tauri dev
pause
