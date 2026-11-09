# 0.3.0 — timeline fix, Windows and Linux packages

## Source changes

Includes the right-hand timeline measured-position fix, source-index marker selection, late virtual-row resampling and keyboard controls. Final frontend verification: 59 files, 574 passing tests; production timeline + roadmap browser verification: 198/198. Browser evidence uses actual production React/TanStack with fixture IPC, not native compositor validation.

Linux compilation exposed a pre-existing moved-path error in open_terminal. Linux/macOS branches now use the validated directory already used on Windows. Linux tests also exposed Unix symlink cleanup assumptions and IPv6 literal DNS fallback. Test cleanup is platform-specific, and validate_public_url classifies literals through typed url::Host before DNS/private-address filtering.

Linux configuration adds explicit glibc >=2.39 requirements to DEB/RPM metadata. No installation, profile migration, distro reset, user-data deletion, commit or push was performed.

## Build/test gates

- Windows native suite after portability fixes: **361 passed, 0 failed, 2 ignored subprocess fixtures**, main/doc tests pass.
- Linux native library suite: **351 passed, 0 failed, 2 ignored fixtures**. Counts differ due to target-specific tests.
- Windows Tauri bundle command: `npm run tauri -- build --bundles nsis,msi -- --locked`.
- Linux Tauri bundle command: `npm run tauri -- build --bundles deb,rpm,appimage -- --locked`.
- Linux build uses an isolated native WSL directory, three Cargo build jobs, Linux-native npm dependencies and no signing secrets. Source snapshot excludes agent profiles, verification/build output, Git metadata and common secret-file patterns.
- All 225 copied source/config files match the live workspace at Linux completion. Existing Windows artifacts are built independently from the same source, with target-specific build identities.

## Linux outputs

Final immutable output directory:

`release/linux/0.3.0/20261108T144648Z-final/`

| Artifact | Size |
|---|---:|
| Native ELF `aether-agent` | 19.02 MiB |
| `AetherAgent_0.3.0_amd64.deb` | 8.49 MiB |
| `AetherAgent-0.3.0-1.x86_64.rpm` | 8.49 MiB |
| `AetherAgent_0.3.0_amd64.AppImage` | 105.76 MiB |

Hashes were computed in Linux, checked by sha256sum, and independently recomputed on Windows. See final-directory `SHA256SUMS`, `windows-artifact-verification.txt`, and `verification/cross-platform-linux-artifacts.json`.

### Compatibility is restricted

Built natively on current Arch Linux x86_64 with glibc 2.44, not an old-Linux compatibility image:

- Native/DEB/RPM executable requires GLIBC symbols through **2.39**, plus system WebKitGTK 4.1 / GTK3 and transitive runtime dependencies.
- AppImage bundled ELF files require GLIBC symbols through **2.44**. glibc itself is not bundled. AppImage therefore **requires host glibc >=2.44**, despite the native executable's lower symbol floor.
- DEB and RPM metadata explicitly enforce libc6/glibc >=2.39. Tauri's automatic GTK/WebKit dependencies appear alongside manual entries; duplicate dependencies are harmless.
- Tested only on current Arch WSL. These packages are not certified for Ubuntu, Debian or Fedora, and are not suitable for Ubuntu 22.04. Package extensions do not imply tested distro support.

### Linux startup smoke

Native ELF, the exact extracted DEB ELF and extracted AppImage AppRun each created an AetherAgent 1280x850 window under Xvfb and remained alive after ten seconds. Each ended via deliberate 15-second timeout (exit124), not a crash. Separate temporary HOME/XDG profiles were used, with a root-only WebKit sandbox override confined to smoke processes. The AppImage outer runtime version check also passed; a FUSE-mounted GUI launch was not tested.

Initial post-build Xvfb binding failed because of WSL shared X sockets. That failure is retained. Verification was rerun successfully using a private mount namespace, without modifying or rebuilding artifacts. No installed package or complete feature workflow is claimed by these startup tests.

Detailed Linux evidence: final-directory `BUILD-REPORT.md`, `COMPATIBILITY.txt`, `tauri-build.log`, `cargo-test-lib.log`, `smoke-status.txt`, `verification-resume-status.txt`, raw XWD captures and source hash inventories.

## Windows outputs

Latest exact paths:

- `src-tauri/target/release/aether-agent.exe`
- `src-tauri/target/release/bundle/nsis/AetherAgent_0.3.0_x64-setup.exe`
- `src-tauri/target/release/bundle/msi/AetherAgent_0.3.0_x64_en-US.msi`

The final Windows rebuild follows portability changes, rather than presenting the earlier intermediate Windows build as the final common source. Version/freshness/PE/MSI metadata and SHA-256 are verified by `verification/verify-release.ps1`.

Evidence: `verification/cross-platform-windows-tests.log`, `cross-platform-windows-build.log`, `cross-platform-windows-artifacts.json`, and `cross-platform-windows-sha256.txt`.

## Release limitations

All packages are unsigned and production automatic update remains unconfigured. Windows may show SmartScreen. No installer was executed and no active installation replaced. Linux GUI smoke confirms startup/window creation only. The Linux build still reports three unrelated warnings in mcp.rs/mod.rs; the passing Linux tests and build should not be described as a strict zero-warning Linux lint gate.
