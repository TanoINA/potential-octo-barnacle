param(
  [switch]$SkipBundle
)

$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $Root

Write-Host '==> Frontend type-check and production build' -ForegroundColor Cyan
npm run build
if ($LASTEXITCODE -ne 0) { throw "npm run build failed ($LASTEXITCODE)" }

Write-Host '==> Rust tests' -ForegroundColor Cyan
cargo test --manifest-path src-tauri/Cargo.toml
if ($LASTEXITCODE -ne 0) { throw "cargo test failed ($LASTEXITCODE)" }

Write-Host '==> Rust check' -ForegroundColor Cyan
cargo check --manifest-path src-tauri/Cargo.toml
if ($LASTEXITCODE -ne 0) { throw "cargo check failed ($LASTEXITCODE)" }

if (-not $SkipBundle) {
  Write-Host '==> Tauri release build and installer packaging' -ForegroundColor Cyan
  npm run tauri build
  if ($LASTEXITCODE -ne 0) { throw "npm run tauri build failed ($LASTEXITCODE)" }
}
else {
  Write-Host '==> Rust release executable (installers skipped)' -ForegroundColor Cyan
  cargo build --release --manifest-path src-tauri/Cargo.toml
  if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed ($LASTEXITCODE)" }
}

$Source = Join-Path $Root 'src-tauri/target/release/aether-agent.exe'
$Target = Join-Path $Root 'AetherAgent.exe'
if (-not (Test-Path $Source -PathType Leaf)) { throw "Release executable not found: $Source" }
Copy-Item -LiteralPath $Source -Destination $Target -Force

$SourceHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $Source).Hash
$TargetHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $Target).Hash
if ($SourceHash -ne $TargetHash) { throw 'Copied executable hash does not match the release binary' }

$Item = Get-Item -LiteralPath $Target
Write-Host "==> Verified $Target" -ForegroundColor Green
Write-Host "    Size:   $($Item.Length) bytes"
Write-Host "    SHA256: $TargetHash"
