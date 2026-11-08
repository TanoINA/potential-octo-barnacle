# Ensure Cargo & Rust are on PATH
$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "   Starting AetherAgent (Rust + Tauri 2.0)" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

npm run tauri dev
