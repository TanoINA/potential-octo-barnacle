[CmdletBinding()]
param(
    [string]$ArchiveRoot = (Join-Path $PSScriptRoot '../release-artifacts'),
    [string]$PreviousReleaseRecord,
    [Parameter(Mandatory=$true)][string]$Tag,
    [string]$CandidateDirectory
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-native-command.ps1')
# Identitas penandatanganan wajib tersedia; jangan membuat kunci pengganti.
foreach ($name in @('AETHER_UPDATE_BASE_URL','AETHER_UPDATE_PUBLIC_KEY','TAURI_SIGNING_PRIVATE_KEY','AETHER_CERTIFICATE_THUMBPRINT','AETHER_TIMESTAMP_URL')) {
    if (![Environment]::GetEnvironmentVariable($name)) { throw "Required release environment variable missing: $name" }
}
if ($env:AETHER_CERTIFICATE_THUMBPRINT -notmatch '^[A-Fa-f0-9]{40}$') { throw 'Invalid Authenticode certificate thumbprint' }
foreach ($value in @($env:AETHER_UPDATE_BASE_URL, $env:AETHER_TIMESTAMP_URL)) {
    $uri = [Uri]$value
    if ($uri.Scheme -ne 'https' -or $uri.UserInfo -or $uri.Query -or $uri.Fragment -or [Uri]::CheckHostName($uri.Host) -ne 'Dns' -or $uri.Host -notmatch '\.' -or $uri.Host -match '(^|\.)(localhost|local|internal)$') { throw 'Release services require public HTTPS URLs without credentials' }
}
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
. (Join-Path $PSScriptRoot 'release-build-config.ps1')
$override = Get-SignedReleaseBuildConfig -Root $root -Tag $Tag -PublicKey $env:AETHER_UPDATE_PUBLIC_KEY -Thumbprint $env:AETHER_CERTIFICATE_THUMBPRINT -TimestampUrl $env:AETHER_TIMESTAMP_URL
$env:AETHER_UPDATE_ENDPOINT = $env:AETHER_UPDATE_BASE_URL.TrimEnd('/') + '/latest.json'
$sourceSha = (Invoke-ReleaseNative -FilePath 'git' -ArgumentList @('-C', $root, 'rev-parse', 'HEAD'))
if ($sourceSha.ExitCode -ne 0 -or ($sourceSha.Output -join '').Trim() -notmatch '^[a-f0-9]{40}$') { throw 'Full source SHA required' }
$sourceSha = ($sourceSha.Output -join '').Trim()
if ($env:CARGO_TARGET_DIR -or $env:CARGO_BUILD_TARGET) { throw 'Custom Cargo target/output is not supported by exact release artifact paths' }
. (Join-Path $PSScriptRoot 'release-archive-policy.ps1')
$previous = $null
if ($PreviousReleaseRecord) {
    $previous = Assert-RetainedReleasePath -Root $root -Path (Resolve-Path -LiteralPath $PreviousReleaseRecord).Path
    $old = Get-Content -LiteralPath $previous -Raw | ConvertFrom-Json
    foreach ($artifact in $old.artifacts) {
        if (!$artifact.Installer -or ![IO.Path]::IsPathRooted($artifact.Installer)) { throw 'Previous installer must have an explicit absolute retained path' }
        $retained = Assert-RetainedReleasePath -Root $root -Path $artifact.Installer
        & (Join-Path $PSScriptRoot 'verify-installer.ps1') -Installer $retained -ExpectedSHA256 $artifact.SHA256 -TrustedPublisherThumbprint $env:AETHER_CERTIFICATE_THUMBPRINT | Out-Null
    }
    if (!$old.artifacts -or !$old.artifacts.Count) { throw 'Previous release record contains no retained installers' }
}
$certificate = Get-Item -LiteralPath ('Cert:\CurrentUser\My\' + $env:AETHER_CERTIFICATE_THUMBPRINT)
if (!$certificate.HasPrivateKey -or $certificate.NotAfter -le [DateTime]::Now) { throw 'Configured signing certificate is unusable' }
$package = Get-Content (Join-Path $root 'package.json') -Raw | ConvertFrom-Json
$config = Get-Content (Join-Path $root 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
if ($config.version -ne $package.version) { throw 'Package and Tauri versions disagree' }
$expected = @(Get-ExpectedReleaseInstallers -Root $root -Product $config.productName -Version $package.version)
$ArchiveRoot = Assert-RetainedReleasePath -Root $root -Path $ArchiveRoot
Get-Command cargo -ErrorAction Stop | Out-Null
$result = Invoke-ReleaseNative -FilePath 'cargo' -ArgumentList @('build', '--offline', '--locked', '--quiet', '--manifest-path', (Join-Path $PSScriptRoot 'release-policy-tests/Cargo.toml'), '--bin', 'aether-release-policy-tests')
$result.Output | Write-Output
if ($result.ExitCode -ne 0) { throw 'Release signature verifier is unavailable; refusing build' }
$overridePath = Join-Path ([IO.Path]::GetTempPath()) ('aether-signed-release-' + [guid]::NewGuid().ToString('N') + '.json')
$overrideCreated = $false
Push-Location $root
try {
    # PS 5.1 strips inline JSON quotes through npm.cmd. Persist only public preflight
    # fields; private signing keys/passwords remain exclusively in the environment.
    $stream = [IO.File]::Open($overridePath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    $overrideCreated = $true
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes($override)
        $stream.Write($bytes, 0, $bytes.Length)
    } finally { $stream.Dispose() }
    $startedUTC = [DateTime]::UtcNow
    $result = Invoke-ReleaseNative -FilePath 'npm.cmd' -ArgumentList @('run', 'tauri', '--', 'build', '--bundles', 'nsis,msi', '--config', $overridePath, '--', '--locked')
    $result.Output | Write-Output
    if ($result.ExitCode -ne 0) { throw 'Signed release build failed; nothing published' }
    Assert-FreshReleaseArtifacts -Paths $expected -StartedUTC $startedUTC
    # Semua keluaran harus terverifikasi sebelum disalin ke arsip.
    $verified = @()
    foreach ($path in $expected) {
        $installer = Get-Item -LiteralPath $path
        $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
        & (Join-Path $PSScriptRoot 'verify-installer.ps1') -Installer $path -ExpectedSHA256 $hash -TrustedPublisherThumbprint $env:AETHER_CERTIFICATE_THUMBPRINT | Out-Null
        & (Join-Path $PSScriptRoot 'release-signature-check.ps1') -Artifact $path -Signature ($path + '.sig') -Version $package.version | Out-Null
        $verified += [pscustomobject]@{ Item = $installer; Hash = $hash }
    }
    if ($CandidateDirectory) {
        $candidateArguments = @((Join-Path $PSScriptRoot 'release-pipeline.mjs'), 'candidate', '--source', (Join-Path $root 'src-tauri/target/release/bundle'), '--out', $CandidateDirectory, '--platform', 'windows-x86_64', '--version', $package.version, '--sha', $sourceSha, '--started-at', $startedUTC.ToString('o'))
        $candidateResult = Invoke-ReleaseNative -FilePath 'node' -ArgumentList $candidateArguments
        if ($candidateResult.ExitCode -ne 0) { throw 'Candidate artifact verification failed' }
        $candidateResult.Output | Write-Output
    }
    $stamp = [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffffffZ')
    $archive = Join-Path $ArchiveRoot ($package.version + '-' + $stamp + '-' + [guid]::NewGuid().ToString('N'))
    New-ReleaseArchive -Root $root -Path $archive | Out-Null
    $records = @()
    foreach ($entry in $verified) {
        $installer = $entry.Item
        $destination = Join-Path $archive ($installer.Directory.Name + '-' + $installer.Name)
        [IO.File]::Copy($installer.FullName, $destination, $false)
        [IO.File]::Copy(($installer.FullName + '.sig'), ($destination + '.sig'), $false)
        # Periksa ulang salinan untuk mendeteksi perubahan setelah verifikasi.
        $records += & (Join-Path $PSScriptRoot 'verify-installer.ps1') -Installer $destination -ExpectedSHA256 $entry.Hash -TrustedPublisherThumbprint $env:AETHER_CERTIFICATE_THUMBPRINT
        & (Join-Path $PSScriptRoot 'release-signature-check.ps1') -Artifact $destination -Signature ($destination + '.sig') -Version $package.version | Out-Null
    }
    $record = @{ version = $package.version; startedUTC = $startedUTC.ToString('o'); builtAtUTC = [DateTime]::UtcNow.ToString('o'); artifacts = @($records); previousReleaseRecord = $previous; published = $false }
    $recordPath = Join-Path $archive 'release-record.json'
    $stream = [IO.File]::Open($recordPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($record | ConvertTo-Json -Depth 8))
        $stream.Write($bytes, 0, $bytes.Length)
    } finally { $stream.Dispose() }
    Write-Output "Verified artifacts retained at $archive. Not published. Independently test installation and rollback, then publish the endpoint manifest and signatures."
} finally {
    try {
        if ($overrideCreated) { Remove-Item -LiteralPath $overridePath -Force -ErrorAction Stop }
    } finally { Pop-Location }
}
