[CmdletBinding()]
param()
. (Join-Path $PSScriptRoot 'release-native-command.ps1')
function Get-SignedReleaseBuildConfig {
    param([string]$Root, [string]$Tag, [string]$PublicKey, [string]$Thumbprint, [string]$TimestampUrl)
    # Satu preflight dipakai Windows dan Linux agar kebijakan versi tidak menyimpang.
    $arguments = @((Join-Path $PSScriptRoot 'release-pipeline.mjs'), 'preflight', '--root', $Root, '--tag', $Tag, '--public-key', $PublicKey, '--platform', 'windows-x86_64')
    if ($Thumbprint) { $arguments += @('--thumbprint', $Thumbprint, '--timestamp-url', $TimestampUrl) }
    $result = Invoke-ReleaseNative -FilePath 'node' -ArgumentList $arguments
    if ($result.ExitCode -ne 0) { throw 'Release preflight rejected configuration, CLI, public key or version/tag agreement' }
    $result.Output -join [Environment]::NewLine
}