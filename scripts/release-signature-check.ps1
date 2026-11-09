[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$Artifact,
    [Parameter(Mandatory=$true)][string]$Signature,
    [Parameter(Mandatory=$true)][string]$Version
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-native-command.ps1')
if (!$env:AETHER_UPDATE_PUBLIC_KEY) { throw 'Missing AETHER_UPDATE_PUBLIC_KEY' }
Get-Command cargo -ErrorAction Stop | Out-Null
# Isolated harness, never the application crate; no network or installer launch.
$result = Invoke-ReleaseNative -FilePath 'cargo' -ArgumentList @('run', '--offline', '--locked', '--quiet', '--manifest-path', (Join-Path $PSScriptRoot 'release-policy-tests/Cargo.toml'), '--bin', 'aether-release-policy-tests', '--', 'verify', $Artifact, $Signature, $Version)
$result.Output | Write-Output
if ($result.ExitCode -ne 0) { throw 'Updater signature/public-key/version verification failed; refusing archive' }
