$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-native-command.ps1')
$verify = Join-Path $PSScriptRoot 'verify-installer.ps1'
if (!(Test-Path -LiteralPath $verify)) { throw 'Missing fail-closed installer verifier' }
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('aether-release-' + [guid]::NewGuid() + '.exe')
[IO.File]::WriteAllText($fixture, 'abc', [Text.UTF8Encoding]::new($false))
$knownHash = 'BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD'
if ((Get-FileHash -LiteralPath $fixture -Algorithm SHA256).Hash -ne $knownHash) { throw 'Hash fixture is invalid' }
foreach ($hash in @(('0' * 64), $knownHash)) {
    $rejected = $false
    try { & $verify -Installer $fixture -ExpectedSHA256 $hash -TrustedPublisherThumbprint ('1' * 40) } catch { $rejected = $true }
    if (!$rejected) { throw 'Unsigned installer incorrectly accepted' }
}
Write-Output 'PASS: SHA-256 fixture, mismatched hash rejection and unsigned installer rejection'


& (Join-Path $PSScriptRoot 'test-release-archive.ps1')
Get-Command cargo -ErrorAction Stop | Out-Null
$result = Invoke-ReleaseNative -FilePath 'cargo' -ArgumentList @('test', '--offline', '--locked', '--manifest-path', (Join-Path $PSScriptRoot 'release-policy-tests/Cargo.toml'))
$result.Output | Write-Output
if ($result.ExitCode -ne 0) { throw 'Release cryptographic/policy tests failed' }

& (Join-Path $PSScriptRoot 'test-release-signature.ps1')
