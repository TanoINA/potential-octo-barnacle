# Release verification harness

Run the test suite from PowerShell:

```powershell
& ./scripts/test-release.ps1
```

The tests require Cargo, the local Tauri CLI, and cached dependencies. All Cargo commands run offline with `--locked`. Missing tools or dependencies result in errors rather than silent skips. The isolated manifest uses `minisign-verify 0.3`, matching the application's updater lockfile.

The suite runs purely local checks and never launches installers or connects to remote services. It validates:
- Rejection of unsigned Authenticode binaries
- Retained path restrictions (including junction aliases)
- Exact versioned NSIS and MSI selection
- Detection of missing or stale build outputs
- Archive collision rejection
- Cryptographic tampering and wrong-key rejection
- Strict signed-version enforcement

For the positive signature test, a disposable password-protected test key is generated in a unique temporary directory. It is used only for fixture validation and is never reused or published.

## Manual signature check

You can test artifact verification directly:

```powershell
./scripts/release-signature-check.ps1 -Artifact <file> -Signature <file.sig> -Version 0.3.0
```

This verifies the signature against `AETHER_UPDATE_PUBLIC_KEY` (in Tauri's base64 public key file format). It validates both the artifact bytes and the trusted comment to confirm the signed version matches the advertised release.

## Build expectations

The release script accepts only the configured product and version targets:
- `nsis/AetherAgent_0.3.0_x64-setup.exe`
- `msi/AetherAgent_0.3.0_x64_en-US.msi`

Both installers and their `.sig` files must exist, have valid non-zero sizes, and carry a write timestamp at or after the build start.

## Rollback records

When passing `-PreviousReleaseRecord <path>`, store the previous record and its installer files outside `src-tauri/target` and `dist/`. All installer paths must be absolute; paths are canonicalized, and reparse-point ancestors or UNC paths are rejected. New releases verify the retained files before linking them in the new release record.
