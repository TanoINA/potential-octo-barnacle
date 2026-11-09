# Release verification harness

Run from PowerShell: `& ./scripts/test-release.ps1`.

Tests require Cargo, the locally installed Tauri CLI, and cached dependencies. All
Cargo invocations are offline and locked; missing tools/dependencies fail rather
than skip verification. No global verifier is installed. The isolated manifest
uses minisign-verify 0.3, matching the application's existing updater lockfile;
application source and dependencies are not modified by these scripts.

The suite never launches an installer or contacts release services. It tests
unsigned Authenticode rejection, retained-path restrictions (including junction
aliases), exact versioned NSIS/MSI selection, missing/stale outputs, archive
collision rejection, cryptographic tampering/wrong-key rejection, and exact
signed-version enforcement. A disposable password-protected test key is generated
in a unique temporary directory for a positive Tauri signed-version fixture.
This is a test-only identity, never a production replacement; it is not printed,
archived as a release, or reused. Temporary fixtures are intentionally retained.

`release-signature-check.ps1 -Artifact <file> -Signature <file.sig> -Version 0.3.0`
verifies against `AETHER_UPDATE_PUBLIC_KEY` (Tauri base64 public-key-file format).
It authenticates both artifact and trusted comment before accepting exactly one
matching version token. The Tauri signer CLI has no verify subcommand.

The release build accepts only AetherAgent's configured product/version paths:
`nsis/AetherAgent_0.3.0_x64-setup.exe` and
`msi/AetherAgent_0.3.0_x64_en-US.msi` for the current configuration. Version changes
are derived from matching package.json and tauri.conf.json values. Both installers
and their signatures must be nonempty and have LastWriteTimeUtc at or after the
build start; other versions are never selected. Non-x64/localized/custom target
layouts fail closed rather than scanning for substitutes.

When supplying PreviousReleaseRecord, first retain its record and installers
outside src-tauri/target and dist. Absolute installer paths are mandatory; paths
are normalized and reparse-point ancestors/UNC paths rejected before the build.
No previous record is required for an initial release. Archives must also remain
outside build output. Unique archive directories, exclusive file copies, and
CreateNew release records prevent overwrites. All expected outputs pass
Authenticode and updater verification before archive creation; retained copies
are reverified. A later failure can leave an incomplete directory without a
release-record.json; it must not be published. The script never publishes.

A real signed release, timestamp service, installation, and rollback still require
an independent credentialed operator validation; these tests do not claim that.
