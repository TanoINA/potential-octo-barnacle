[CmdletBinding()]
param([string]$LogPath = (Join-Path ([IO.Path]::GetTempPath()) ('aether-release-ps5-' + [guid]::NewGuid() + '.log')))
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-native-command.ps1')
# Both streams must be captured without converting informational stderr into failure.
foreach ($code in @(0, 23)) {
    $result = Invoke-ReleaseNative -FilePath 'cmd.exe' -ArgumentList @('/d', '/c', "echo native-stdout & echo native-stderr 1>&2 & exit /b $code")
    if ($result.ExitCode -ne $code) { throw "Native exit code lost: expected $code, got $($result.ExitCode)" }
    if (($result.Output -join "`n") -notmatch 'native-stdout' -or ($result.Output -join "`n") -notmatch 'native-stderr') { throw 'Native streams not captured' }
    if ($ErrorActionPreference -ne 'Stop') { throw 'Caller error preference changed' }
}
$missingRejected = $false
try { Invoke-ReleaseNative -FilePath ('missing-native-' + [guid]::NewGuid() + '.exe') } catch { $missingRejected = $true }
if (!$missingRejected -or $ErrorActionPreference -ne 'Stop') { throw 'Missing executable accepted or preference changed' }
$cargo = Invoke-ReleaseNative -FilePath 'cargo' -ArgumentList @('test', '--offline', '--locked', '--manifest-path', (Join-Path $PSScriptRoot 'release-policy-tests/Cargo.toml'))
if ($cargo.ExitCode -ne 0 -or ($cargo.Output -join "`n") -notmatch 'test result: ok\.') { throw 'Captured Cargo test did not complete successfully' }
# Check the actual Windows PowerShell process, not a stale parent LASTEXITCODE.
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Push-Location $root
try {
    $process = Start-Process -FilePath 'powershell.exe' -ArgumentList @('-NoProfile', '-File', 'scripts/test-release.ps1') -Wait -PassThru -NoNewWindow -RedirectStandardOutput $LogPath -RedirectStandardError ($LogPath + '.stderr')
    if ($process.ExitCode -ne 0) { throw "Release subprocess exited $($process.ExitCode); see $LogPath and $LogPath.stderr" }
    $text = Get-Content -LiteralPath $LogPath -Raw
    foreach ($marker in @('PASS: SHA-256 fixture', 'PASS: retained paths', 'test result: ok.', 'PASS: disposable Tauri signed-version fixture', 'wrong signature rejection')) {
        if (!$text.Contains($marker)) { throw "Release subprocess did not complete $marker; see $LogPath" }
    }
    Write-Output $text
    Write-Output "PASS: Windows PowerShell -NoProfile -File release subprocess, native streams, nonzero exit and preference restoration. Log: $LogPath"
} finally { Pop-Location }