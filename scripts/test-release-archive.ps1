$ErrorActionPreference = 'Stop'
function Assert-Rejected([scriptblock]$Action, [string]$Message) {
    $rejected = $false
    try { & $Action | Out-Null } catch { $rejected = $true }
    if (!$rejected) { throw $Message }
}
$root = Join-Path ([IO.Path]::GetTempPath()) ('aether-release-policy-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path (Join-Path $root 'src-tauri/target'), (Join-Path $root 'dist'), (Join-Path $root 'retained') -Force | Out-Null
$helper = Join-Path $PSScriptRoot 'release-archive-policy.ps1'
if (!(Test-Path -LiteralPath $helper)) { throw 'FAIL: release archive path policy is missing' }
. $helper
foreach ($path in @('src-tauri/target/old.exe', 'dist/old.msi', 'retained/../dist/old.exe')) {
    Assert-Rejected { Assert-RetainedReleasePath -Root $root -Path (Join-Path $root $path) } 'Build output accepted as retained installer'
}
Assert-RetainedReleasePath -Root $root -Path (Join-Path $root 'retained/old.exe') | Out-Null
$alias = Join-Path $root 'retained/target-alias'
New-Item -ItemType Junction -Path $alias -Target (Join-Path $root 'src-tauri/target') | Out-Null
Assert-Rejected { Assert-RetainedReleasePath -Root $root -Path (Join-Path $alias 'old.exe') } 'Junction alias accepted as retained installer'
$expected = @(Get-ExpectedReleaseInstallers -Root $root -Product 'AetherAgent' -Version '0.3.0')
if ($expected.Count -ne 2 -or $expected[0] -notlike '*AetherAgent_0.3.0_x64-setup.exe' -or $expected[1] -notlike '*AetherAgent_0.3.0_x64_en-US.msi') { throw 'Expected exact versioned NSIS/MSI paths' }
$stale = Join-Path $root 'src-tauri/target/release/bundle/nsis/AetherAgent_0.2.0_x64-setup.exe'
New-Item -ItemType Directory -Path (Split-Path $stale) -Force | Out-Null
[IO.File]::WriteAllText($stale, 'old release')
if ($expected -contains $stale) { throw 'Stale version selected' }
$started = [DateTime]::UtcNow
Assert-Rejected { Assert-FreshReleaseArtifacts -Paths $expected -StartedUTC $started } 'Missing expected installers accepted'
foreach ($path in $expected) {
    New-Item -ItemType Directory -Path (Split-Path $path) -Force | Out-Null
    [IO.File]::WriteAllText($path, 'fresh')
    [IO.File]::WriteAllText(($path + '.sig'), 'signature')
}
Assert-FreshReleaseArtifacts -Paths $expected -StartedUTC $started
(Get-Item -LiteralPath $expected[0]).LastWriteTimeUtc = $started.AddMinutes(-1)
Assert-Rejected { Assert-FreshReleaseArtifacts -Paths $expected -StartedUTC $started } 'Stale installer accepted'
(Get-Item -LiteralPath $expected[0]).LastWriteTimeUtc = [DateTime]::UtcNow
(Get-Item -LiteralPath ($expected[1] + '.sig')).LastWriteTimeUtc = $started.AddMinutes(-1)
Assert-Rejected { Assert-FreshReleaseArtifacts -Paths $expected -StartedUTC $started } 'Stale signature accepted'
$archive = Join-Path $root 'retained/archive'
New-ReleaseArchive -Root $root -Path $archive | Out-Null
[IO.File]::WriteAllText((Join-Path $archive 'sentinel'), 'retain')
Assert-Rejected { New-ReleaseArchive -Root $root -Path $archive } 'Existing archive overwritten'
if ([IO.File]::ReadAllText((Join-Path $archive 'sentinel')) -ne 'retain') { throw 'Archive changed' }
Write-Output 'PASS: retained paths, exact installers, freshness, immutable archive'

