# Release and CI Automation Files

This index lists the scripts, workflows, and test harnesses supporting packaging, signing, and verification.

## GitHub Actions Workflows (`.github/workflows/`)

- **`release-quality.yml`**: Runs linting, type checks, frontend tests, and Rust tests on PRs and commits.
- **`signed-release.yml`**: Builds and packages signed candidates for Windows and Linux in protected environments.
- **`promote-release.yml`**: Verified promotion workflow that publishes update manifests to the hosting server.

## Release and Build Scripts (`scripts/`)

- **`build-signed-release.ps1` & `.sh`**: Builds production installers (NSIS, MSI, AppImage, DEB, RPM) with Authenticode and minisign updater signatures.
- **`release-pipeline.mjs`**: Pipeline orchestration script for preflight validation, checksum creation, signature checks, and manifest generation.
- **`verify-installer.ps1`**: Offline Windows utility to verify an installer's SHA-256 hash and Authenticode certificate thumbprint.
- **`release-signature-check.ps1`**: Verifies Tauri minisign signatures and version comments against public keys.

## Release Test Suites (`scripts/`)

- **`release-policy-tests/`**: Rust test harness verifying signature policies, version tokens, and URL safety.
- **`test-release.ps1` & `test-release-powershell.ps1`**: Verifies release script behavior against unsigned or tampered fixtures.
- **`test-release-pipeline.mjs`**: End-to-end unit tests for the Node.js release pipeline logic.
- **`test-release-linux.sh`**: Linux candidate package validation and GLIBC dependency verification.
