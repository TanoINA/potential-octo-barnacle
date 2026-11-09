[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$helper = Join-Path $PSScriptRoot 'release-build-config.ps1'
if (!(Test-Path -LiteralPath $helper)) { throw 'Missing factored release build preflight' }
. $helper
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('aether-pipeline-preflight-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path (Join-Path $fixture 'src-tauri') -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $fixture 'node_modules/@tauri-apps/cli') -Force | Out-Null
function Write-Fixture([string]$CargoVersion = '0.3.0', [string]$CliVersion = '2.12.0') {
    [IO.File]::WriteAllText((Join-Path $fixture 'package.json'), '{"version":"0.3.0"}')
    [IO.File]::WriteAllText((Join-Path $fixture 'src-tauri/tauri.conf.json'), '{"version":"0.3.0","productName":"AetherAgent"}')
    [IO.File]::WriteAllText((Join-Path $fixture 'src-tauri/Cargo.toml'), "[package]`nname = `"fixture`"`nversion = `"$CargoVersion`"`n")
    [IO.File]::WriteAllText((Join-Path $fixture 'node_modules/@tauri-apps/cli/package.json'), ('{"version":"' + $CliVersion + '"}'))
}
function Assert-Rejected([scriptblock]$Action, [string]$Name) {
    $rejected = $false
    try { & $Action | Out-Null } catch { $rejected = $true }
    if (!$rejected) { throw "Accepted invalid preflight: $Name" }
}
Write-Fixture
$key = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("untrusted comment: fixture`n" + [Convert]::ToBase64String(([byte[]](69,100) + [byte[]](1..40))) + "`n"))
$config = Get-SignedReleaseBuildConfig -Root $fixture -Tag 'v0.3.0' -PublicKey $key -Thumbprint ('A' * 40) -TimestampUrl 'https://timestamp.example.com'
$parsed = $config | ConvertFrom-Json
if ($parsed.plugins.updater.pubkey -cne $key -or !$parsed.plugins.updater.requireSignedVersion -or !$parsed.bundle.createUpdaterArtifacts) { throw 'Bundler lacks exact public key or signed-version settings' }
foreach ($version in @('2.10.0','2.11.9','2.12.0-beta.1','3.0.0')) {
    Write-Fixture -CliVersion $version
    Assert-Rejected { Get-SignedReleaseBuildConfig -Root $fixture -Tag 'v0.3.0' -PublicKey $key } "CLI $version"
}
Write-Fixture -CargoVersion '0.2.0'
Assert-Rejected { Get-SignedReleaseBuildConfig -Root $fixture -Tag 'v0.3.0' -PublicKey $key } 'Cargo mismatch'
Write-Fixture
Assert-Rejected { Get-SignedReleaseBuildConfig -Root $fixture -Tag 'v0.3.1' -PublicKey $key } 'tag mismatch'
Assert-Rejected { Get-SignedReleaseBuildConfig -Root $fixture -Tag '' -PublicKey $key } 'missing tag'
Assert-Rejected { Get-SignedReleaseBuildConfig -Root $fixture -Tag 'v0.3.0' -PublicKey 'invalid' } 'invalid key'
[IO.File]::WriteAllText((Join-Path $fixture 'src-tauri/tauri.conf.json'), '{"version":"0.2.0","productName":"AetherAgent"}')
Assert-Rejected { Get-SignedReleaseBuildConfig -Root $fixture -Tag 'v0.3.0' -PublicKey $key } 'Tauri mismatch'
Write-Output 'PASS: bundler public key, signed version, CLI >=2.12 stable v2, npm/Cargo/Tauri/tag preflight; isolated fixtures only'

# Exercise the actual signed-build call through PS 5.1 -> npm.cmd -> Node argv.
# Only disposable command stubs run; no certificate, build, signing or publish tools.
if ($PSVersionTable.PSVersion.Major -ne 5 -or $PSVersionTable.PSVersion.Minor -ne 1) { throw 'Run this regression in Windows PowerShell 5.1' }
Write-Fixture
$nativeRoot = Join-Path $fixture 'native argv fixture'
$bin = Join-Path $fixture 'stub-commands'
$temp = Join-Path $nativeRoot 'temp config with spaces'
New-Item -ItemType Directory -Path $bin -Force | Out-Null
foreach ($directory in @('scripts', 'src-tauri', 'node_modules/@tauri-apps/cli', 'temp config with spaces')) {
    New-Item -ItemType Directory -Path (Join-Path $nativeRoot $directory) -Force | Out-Null
}
foreach ($name in @('build-signed-release.ps1', 'release-native-command.ps1', 'release-build-config.ps1', 'release-archive-policy.ps1', 'release-pipeline.mjs')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination (Join-Path $nativeRoot ('scripts/' + $name))
}
foreach ($name in @('package.json', 'src-tauri/tauri.conf.json', 'src-tauri/Cargo.toml', 'node_modules/@tauri-apps/cli/package.json')) {
    Copy-Item -LiteralPath (Join-Path $fixture $name) -Destination (Join-Path $nativeRoot $name)
}
$utf8 = [Text.UTF8Encoding]::new($false)
$node = (Get-Command node.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
[IO.File]::WriteAllText((Join-Path $bin 'git.cmd'), ("@echo off`r`necho " + ('a' * 40) + "`r`nexit /b 0`r`n"), $utf8)
[IO.File]::WriteAllText((Join-Path $bin 'cargo.cmd'), "@echo off`r`nexit /b 0`r`n", $utf8)
[IO.File]::WriteAllText((Join-Path $bin 'npm.cmd'), ('@echo off' + "`r`n" + '"' + $node + '" "%~dp0capture.cjs" %*' + "`r`nexit /b %errorlevel%`r`n"), $utf8)
[IO.File]::WriteAllText((Join-Path $bin 'capture.cjs'), @'
const fs = require('node:fs');
const { isDeepStrictEqual } = require('node:util');
const argv = process.argv.slice(2);
const argument = argv[argv.indexOf('--config') + 1];
let configIsFile = false, inlineJsonValid = false, publicConfigExact = false;
let utf8Exact = false, containsSecretMarkers = false;
try { configIsFile = fs.statSync(argument).isFile(); } catch {}
try { JSON.parse(argument); inlineJsonValid = true; } catch {}
if (configIsFile) {
  const bytes = fs.readFileSync(argument);
  const text = bytes.toString('utf8');
  try { publicConfigExact = isDeepStrictEqual(JSON.parse(text), JSON.parse(process.env.AETHER_SIGNING_TEST_EXPECTED_CONFIG)); } catch {}
  utf8Exact = bytes.equals(Buffer.from(process.env.AETHER_SIGNING_TEST_EXPECTED_CONFIG, 'utf8'));
  containsSecretMarkers = [process.env.TAURI_SIGNING_PRIVATE_KEY, process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD].some(value => value && text.includes(value));
}
// Capture public argv and booleans only, never private environment values.
fs.writeFileSync(process.env.AETHER_SIGNING_TEST_CAPTURE, JSON.stringify({ argv, argument, configIsFile, inlineJsonValid, publicConfigExact, utf8Exact, containsSecretMarkers }));
process.exit(Number(process.env.AETHER_SIGNING_TEST_EXIT_CODE));
'@, $utf8)
$environment = @{
    PATH = $bin + ';' + $env:PATH
    TEMP = $temp
    TMP = $temp
    AETHER_UPDATE_BASE_URL = 'https://updates.example.com/aether'
    AETHER_UPDATE_PUBLIC_KEY = $key
    AETHER_UPDATE_ENDPOINT = 'https://updates.example.com/aether/latest.json'
    AETHER_CERTIFICATE_THUMBPRINT = ('A' * 40)
    AETHER_TIMESTAMP_URL = 'https://timestamp.example.com'
    TAURI_SIGNING_PRIVATE_KEY = ('fixture-private-marker-' + [guid]::NewGuid())
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ('fixture-password-marker-' + [guid]::NewGuid())
    CARGO_TARGET_DIR = $null
    CARGO_BUILD_TARGET = $null
    AETHER_SIGNING_TEST_EXPECTED_CONFIG = $config
    AETHER_SIGNING_TEST_CAPTURE = $null
    AETHER_SIGNING_TEST_EXIT_CODE = $null
}
$savedEnvironment = @{}
foreach ($name in $environment.Keys) { $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name) }
$paths = @()
try {
    foreach ($name in $environment.Keys) { [Environment]::SetEnvironmentVariable($name, $environment[$name]) }
    foreach ($name in @('git', 'cargo', 'npm.cmd')) {
        if ((Get-Command $name -CommandType Application | Select-Object -First 1).Source -notlike ($bin + '\*')) { throw "Fixture did not isolate $name" }
    }
    foreach ($code in @(23, 0)) {
        $env:AETHER_SIGNING_TEST_CAPTURE = Join-Path $nativeRoot ("capture-$code.json")
        $env:AETHER_SIGNING_TEST_EXIT_CODE = [string]$code
        $beforeLocation = (Get-Location).Path
        $invocation = @{ Failure = $null }
        $output = @(& {
            # Fake only the certificate provider; normal filesystem checks stay real.
            function Get-Item {
                [CmdletBinding()]
                param([string]$LiteralPath, [switch]$Force)
                if ($LiteralPath -eq ('Cert:\CurrentUser\My\' + $env:AETHER_CERTIFICATE_THUMBPRINT)) {
                    [pscustomobject]@{ HasPrivateKey = $true; NotAfter = [DateTime]::Now.AddDays(1) }
                } else { Microsoft.PowerShell.Management\Get-Item @PSBoundParameters }
            }
            try { & (Join-Path $nativeRoot 'scripts/build-signed-release.ps1') -Tag 'v0.3.0' }
            catch { $script:nativeBuildFailure = $_ }
        })
        $failure = $script:nativeBuildFailure
        $script:nativeBuildFailure = $null
        if (!(Test-Path -LiteralPath $env:AETHER_SIGNING_TEST_CAPTURE)) {
            $reason = [string]$failure.Exception.Message + ' ' + ($output -join ' ')
            foreach ($marker in @($env:TAURI_SIGNING_PRIVATE_KEY, $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD)) { $reason = $reason.Replace($marker, '[redacted]') }
            throw "Fixture did not reach npm.cmd/Node capture: $reason"
        }
        $capture = Get-Content -LiteralPath $env:AETHER_SIGNING_TEST_CAPTURE -Raw | ConvertFrom-Json
        if (!$capture.configIsFile) { throw "REGRESSION: --config reached Node as inline JSON, not a readable .json path; inline JSON parses=$($capture.inlineJsonValid)" }
        if (![IO.Path]::IsPathRooted($capture.argument) -or [IO.Path]::GetExtension($capture.argument) -ne '.json' -or !$capture.argument.StartsWith(($temp + '\'), [StringComparison]::OrdinalIgnoreCase) -or !$capture.argument.Contains(' ')) { throw 'Expected an absolute temporary JSON path containing spaces' }
        if (!$capture.publicConfigExact -or !$capture.utf8Exact -or $capture.containsSecretMarkers) { throw 'Temporary config must be exact BOM-less UTF-8 public preflight JSON, without private key/password values' }
        $expectedArguments = @('run', 'tauri', '--', 'build', '--bundles', 'nsis,msi', '--config', $capture.argument, '--', '--locked')
        if ($capture.argv.Count -ne $expectedArguments.Count) { throw 'npm.cmd changed the number of build arguments' }
        for ($i = 0; $i -lt $expectedArguments.Count; $i++) {
            if ($capture.argv[$i] -cne $expectedArguments[$i]) { throw "npm.cmd changed build argument $i" }
        }
        if ($paths -contains $capture.argument) { throw 'Temporary config path reused between invocations' }
        $paths += $capture.argument
        if (Test-Path -LiteralPath $capture.argument) { throw 'Temporary config was not cleaned up after build failure' }
        if ((Get-Location).Path -ne $beforeLocation) { throw 'Signed build did not restore its working directory' }
        if (!$failure) { throw 'Disposable build unexpectedly passed installer verification' }
        if ($code -eq 23 -and $failure.Exception.Message -ne 'Signed release build failed; nothing published') { throw 'Native build failure did not fail closed' }
        if ($code -eq 0 -and $failure.Exception.Message -notlike '*AetherAgent_0.3.0_x64-setup.exe*') { throw 'Successful npm stub did not reach the existing missing-installer guard' }
        $diagnostics = ($output -join "`n") + $failure.ToString()
        foreach ($marker in @($env:TAURI_SIGNING_PRIVATE_KEY, $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD)) {
            if ($diagnostics.Contains($marker)) { throw 'Private fixture marker appeared in signed-build output' }
        }
        if (Test-Path -LiteralPath (Join-Path $nativeRoot 'release-artifacts')) { throw 'Fixture unexpectedly created a release archive' }
        Write-Output "PASS: PS5.1 -> npm.cmd -> Node public --config JSON path with spaces; exit=$code; exact signing flags, UTF-8, no private markers, cleanup and fail-closed artifact guard"
    }
} finally {
    foreach ($name in $savedEnvironment.Keys) { [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name]) }
}
Write-Output 'PASS: fresh config path per invocation; command stubs only, no real build/sign/publish'
