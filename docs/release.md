# Release and recovery

Product version stays synced at **0.3.0** across npm, Cargo, and Tauri. `build.rs` records build metadata separately (UTC timestamp, git SHA, and clean/dirty status) using only allowlisted compile-time variables and never reads host environment secrets. Cargo re-runs the build script only when source files, config, or `HEAD` change, so cached identical builds keep their original stamp.

## Current status

Standard local builds ship without updater endpoints, publisher certificates, or updater private keys, so the About dialog lists installer signing as **Unknown (not verified at runtime)**. Note that an Ed25519 updater key only verifies update archives; it does not replace a Windows Authenticode certificate.

## Automatic updates

To turn on update checks during compilation, export two variables:
- `AETHER_UPDATE_ENDPOINT`: HTTPS URL pointing to your update manifest (such as `https://updates.example.com/latest.json`).
- `AETHER_UPDATE_PUBLIC_KEY`: base64-encoded minisign Ed25519 public key from Tauri.

Before trusting a response, the updater validates the endpoint and signature:
- Endpoints must be public HTTPS URLs (no raw IP addresses, local hostnames, query strings, or `#` fragments).
- Frontend webview code cannot override endpoints or swap the public key.
- Remote versions must be strictly newer by semver, ignoring plain metadata tweaks.
- Downloaded signatures must bind both the binary archive and its advertised version comment to stop downgrade attacks.
- Checking for updates pulls only metadata JSON and never auto-downloads or executes an installer on its own.

## Building signed releases

On Windows, run `scripts/build-signed-release.ps1` with:
- A code-signing certificate loaded in the `CurrentUser` store.
- `AETHER_CERTIFICATE_THUMBPRINT`: SHA-1 thumbprint of that certificate.
- `AETHER_TIMESTAMP_URL`: RFC 3161 timestamp server URL.
- `TAURI_SIGNING_PRIVATE_KEY`: private minisign key (and password if encrypted).
- `AETHER_UPDATE_ENDPOINT` and `AETHER_UPDATE_PUBLIC_KEY`.

Keep private keys inside CI secrets or an HSM; never pass them on command lines, commit them, or echo them in build logs. The release script forces updater artifact generation, checks installer signatures and SHA-256 hashes, stops immediately if anything comes out unsigned or incomplete, and archives outputs locally without uploading them automatically.

## Offline installation and rollback

1. Download the installer (EXE or MSI) from your distribution source. Get its expected SHA-256 hash and publisher certificate thumbprint through an independent channel.
2. Verify the installer on Windows before running it:
   ```powershell
   scripts/verify-installer.ps1 -Installer <path-to-installer> -ExpectedSHA256 <64-hex> -TrustedPublisherThumbprint <40-hex>
   ```
   The script checks the SHA-256 hash, validates the Authenticode signature chain, and confirms the pinned certificate thumbprint. If any check fails, it stops without launching the file.
3. Close AetherAgent, back up your `.aether-agent` profile directory, and run the verified installer manually.
4. For rollbacks, keep previous signed installers and their `release-record.json` outside the `target/` directory. Pass `-PreviousReleaseRecord <record.json>` when running the signed build script so new releases link back to verified rollback artifacts.
5. To downgrade, verify the older installer using its original thumbprint and hash, then run it manually. If the database schema changed between versions, restore your profile backup before launching the older version.

## Verification checks

Run the automated release policy checks:

```powershell
# Verify signature and manifest policies
cargo test --manifest-path scripts/release-policy-tests/Cargo.toml --locked

# Verify frontend update and about modal logic
npm test -- src/components/AboutModal.test.tsx src/components/release-wiring.test.ts src/components/SettingsModal.test.tsx

# Verify installer verification logic against unsigned and tampered fixtures
powershell -File scripts/test-release.ps1
```

These fixture tests verify error handling and signature parsing. End-to-end upgrade and rollback tests with genuine signing keys must still be run by the release operator on test machines.
