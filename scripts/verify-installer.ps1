[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$Installer,
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ExpectedSHA256,
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-fA-F0-9]{40}$')][string]$TrustedPublisherThumbprint
)
$ErrorActionPreference = 'Stop'
$item = Get-Item -LiteralPath $Installer
if ($item.PSIsContainer -or $item.Extension -notin @('.exe', '.msi')) { throw 'Select an explicit EXE or MSI installer file' }
if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Installer must not be a symbolic link' }
if ($item.FullName.StartsWith('\\')) { throw 'Copy the installer to a local drive before verification' }
# Verification only: no URL opening, process launch or automatic installation.
$hash = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash
if ($hash -ne $ExpectedSHA256) { throw 'Installer SHA-256 mismatch; do not install' }
$signature = Get-AuthenticodeSignature -LiteralPath $item.FullName
if ($signature.Status -ne 'Valid' -or !$signature.SignerCertificate) { throw 'Installer lacks a valid trusted Authenticode signature; do not install' }
if ($signature.SignerCertificate.Thumbprint -ne $TrustedPublisherThumbprint) { throw 'Installer publisher certificate does not match the independently trusted publisher' }
[pscustomobject]@{ Installer = $item.FullName; SHA256 = $hash; SignatureStatus = 'Valid'; PublisherThumbprint = $signature.SignerCertificate.Thumbprint; Publisher = $signature.SignerCertificate.Subject; VerifiedAtUTC = [DateTime]::UtcNow.ToString('o'); InstallationStarted = $false }
