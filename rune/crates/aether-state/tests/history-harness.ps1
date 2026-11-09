# Isolated verification while controller.rs is owned by the lifecycle agent.
# Only copies are patched; the working-tree controller/lib are never written.
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '../../../..')).Path
$state = Join-Path $root 'rune/crates/aether-state'
$harness = Join-Path ([IO.Path]::GetTempPath()) ('aether-history-harness-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path (Join-Path $harness 'src'), (Join-Path $harness 'tests') | Out-Null
Copy-Item (Join-Path $state 'src/*.rs') (Join-Path $harness 'src')
Copy-Item (Join-Path $state 'tests/history_commands.rs') (Join-Path $harness 'tests')
$utf8 = New-Object Text.UTF8Encoding($false)
$manifest = [IO.File]::ReadAllText((Join-Path $state 'Cargo.toml')).Replace('../aether-core', ((Join-Path $root 'rune/crates/aether-core').Replace('\','/')))
[IO.File]::WriteAllText((Join-Path $harness 'Cargo.toml'), $manifest, $utf8)
$libPath = Join-Path $harness 'src/lib.rs'
$lib = [IO.File]::ReadAllText($libPath)
foreach ($mod in @('history', 'commands')) {
    if ((Test-Path (Join-Path $harness "src/$mod.rs")) -and $lib -notmatch "mod $mod;") { $lib += "`npub mod $mod;`n" }
}
[IO.File]::WriteAllText($libPath, $lib, $utf8)
$path = Join-Path $harness 'src/controller.rs'
$text = [IO.File]::ReadAllText($path)
foreach ($field in @('live','selected','generation','published','refresh')) {
    $text = [regex]::Replace($text, "(?m)^    ${field}:", "    pub(crate) ${field}:")
}
foreach ($fn in @('publish','catalog')) { $text = $text.Replace("    fn $fn(", "    pub(crate) fn $fn(") }
if ($text -notmatch 'history::execute') {
    $text = $text.Replace('let result = if command == "load_session" {', 'let result = if command == crate::history::HISTORY_COMMAND { crate::history::execute(&backend, args).await } else if command == "load_session" {')
}
if ($text -notmatch 'self.complete_history') {
    $text = $text.Replace('        let id = completion.session_id.clone();', '        if completion.command == crate::history::HISTORY_COMMAND { self.complete_history(completion.session_id.as_deref(), completion.generation, completion.result); return; }' + "`n" + '        let id = completion.session_id.clone();')
}
[IO.File]::WriteAllText($path, $text, $utf8)
Write-Host "Isolated source: $harness"
& cargo test --manifest-path (Join-Path $harness 'Cargo.toml') --target-dir (Join-Path $state 'target') --test history_commands @args
exit $LASTEXITCODE
