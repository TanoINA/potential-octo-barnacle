# Resolve dot segments and reject reparse-point ancestors rather than trusting aliases.
function Get-CanonicalReleasePath([string]$Path) {
    $full = [IO.Path]::GetFullPath($Path)
    if ($full.StartsWith('\\')) { throw 'Release paths must be on a local drive' }
    $cursor = $full
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            $item = Get-Item -LiteralPath $cursor -Force
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Release paths must not traverse reparse points' }
        }
        $parent = Split-Path -Parent $cursor
        if ($parent -eq $cursor) { break }
        $cursor = $parent
    }
    $full.TrimEnd([IO.Path]::DirectorySeparatorChar)
}
function Assert-RetainedReleasePath([string]$Root, [string]$Path) {
    $canonical = Get-CanonicalReleasePath $Path
    foreach ($relative in @('src-tauri/target', 'dist')) {
        $forbidden = Get-CanonicalReleasePath (Join-Path $Root $relative)
        if ($canonical.Equals($forbidden, [StringComparison]::OrdinalIgnoreCase) -or $canonical.StartsWith(($forbidden + [IO.Path]::DirectorySeparatorChar), [StringComparison]::OrdinalIgnoreCase)) { throw 'Retain previous installers outside src-tauri/target and dist before building' }
    }
    $canonical
}
function Get-ExpectedReleaseInstallers([string]$Root, [string]$Product, [string]$Version) {
    if ($Product -notmatch '^[A-Za-z0-9._-]+$' -or $Version -notmatch '^\d+\.\d+\.\d+([+-][A-Za-z0-9.-]+)?$') { throw 'Unsafe product/version for installer paths' }
    Join-Path $Root "src-tauri/target/release/bundle/nsis/${Product}_${Version}_x64-setup.exe"
    Join-Path $Root "src-tauri/target/release/bundle/msi/${Product}_${Version}_x64_en-US.msi"
}
function Assert-FreshReleaseArtifacts([string[]]$Paths, [DateTime]$StartedUTC) {
    foreach ($path in $Paths) {
        foreach ($file in @($path, ($path + '.sig'))) {
            $canonical = Get-CanonicalReleasePath $file
            $item = Get-Item -LiteralPath $canonical
            if ($item.PSIsContainer -or $item.Length -eq 0 -or $item.LastWriteTimeUtc -lt $StartedUTC.ToUniversalTime()) { throw "Missing, empty or stale release artifact: $canonical" }
        }
    }
}
function New-ReleaseArchive([string]$Root, [string]$Path) {
    $canonical = Assert-RetainedReleasePath -Root $Root -Path $Path
    if (Test-Path -LiteralPath $canonical) { throw 'Release archive already exists; refusing overwrite' }
    New-Item -ItemType Directory -Path $canonical -ErrorAction Stop
}
