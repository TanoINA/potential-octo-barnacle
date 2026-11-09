$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-native-command.ps1')
$cli = Join-Path $PSScriptRoot '../node_modules/.bin/tauri.cmd'
if (!(Test-Path -LiteralPath $cli)) { throw 'Local Tauri CLI required for disposable signature fixtures' }
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('aether-signature-fixture-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $fixture | Out-Null
$keyPath = Join-Path $fixture 'disposable.key'
$artifact = Join-Path $fixture 'fixture.txt'
[IO.File]::WriteAllText($artifact, 'disposable release signature test')
# This test-only key is generated independently; never loads any production private key.
$result = Invoke-ReleaseNative -FilePath $cli -ArgumentList @('signer', 'generate', '--ci', '-p', 'disposable-fixture-only', '-w', $keyPath)
if ($result.ExitCode -ne 0) { throw "Disposable fixture key generation failed: $($result.Output -join [Environment]::NewLine)" }
$result = Invoke-ReleaseNative -FilePath $cli -ArgumentList @('signer', 'sign', '-k', (Get-Content -LiteralPath $keyPath -Raw), '-p', 'disposable-fixture-only', '--app-version', '0.3.0', $artifact)
if ($result.ExitCode -ne 0) { throw "Disposable fixture signing failed: $($result.Output -join [Environment]::NewLine)" }
$savedPublicKey = $env:AETHER_UPDATE_PUBLIC_KEY
try {
    $env:AETHER_UPDATE_PUBLIC_KEY = (Get-Content -LiteralPath ($keyPath + '.pub') -Raw).Trim()
    $checker = Join-Path $PSScriptRoot 'release-signature-check.ps1'
    & $checker -Artifact $artifact -Signature ($artifact + '.sig') -Version '0.3.0'
    $rejected = $false
    try { & $checker -Artifact $artifact -Signature ($artifact + '.sig') -Version '0.3.1' } catch { $rejected = $true }
    if (!$rejected) { throw 'Wrong product version accepted' }
    # A different, correctly signed payload must not authenticate this artifact.
    $otherArtifact = Join-Path $fixture 'other.txt'
    [IO.File]::WriteAllText($otherArtifact, 'different disposable payload')
    $result = Invoke-ReleaseNative -FilePath $cli -ArgumentList @('signer', 'sign', '-k', (Get-Content -LiteralPath $keyPath -Raw), '-p', 'disposable-fixture-only', '--app-version', '0.3.0', $otherArtifact)
    if ($result.ExitCode -ne 0) { throw "Wrong-signature fixture signing failed: $($result.Output -join [Environment]::NewLine)" }
    $rejected = $false
    try { & $checker -Artifact $artifact -Signature ($otherArtifact + '.sig') -Version '0.3.0' } catch { $rejected = $true }
    if (!$rejected) { throw 'Wrong signature accepted' }
    [IO.File]::AppendAllText($artifact, 'tampered')
    $rejected = $false
    try { & $checker -Artifact $artifact -Signature ($artifact + '.sig') -Version '0.3.0' } catch { $rejected = $true }
    if (!$rejected) { throw 'Tampered fixture accepted' }
    Write-Output 'PASS: disposable Tauri signed-version fixture, wrong version, wrong signature rejection and tampering rejection'
} finally { $env:AETHER_UPDATE_PUBLIC_KEY = $savedPublicKey }
