# App update pipeline

## Current status

The source includes update checks, bounded downloads, progress reporting, Ed25519 signature and signed-version verification, install/restart confirmation, session readiness checks, Windows/Linux release candidates, and a separate promotion step.

Production still needs a public endpoint, operator-owned signing keys, a Windows publisher certificate, signing runners, and protected GitHub environments. Local builds are not signed production releases.

A binary built without an endpoint and public key keeps updates disabled. Users must manually install a configured bootstrap release first. Publishing `latest.json` does not enable updates in an older, unconfigured binary.

## In the app

Open **Settings → About AetherAgent → Check for updates**. Installation requires confirmation. The app saves idle sessions, downloads and verifies the package, then starts installation. Platforms that need a restart ask for one explicitly.

- The endpoint and public key are embedded at build time. The webview cannot supply an installer URL or replace the signing identity.
- `requireSignedVersion` stays enabled. Only a newer semantic version is accepted; changing build metadata alone does not count as an update.
- The frontend checks all cached sessions, including background runs. It waits for pending session/settings saves, flushes buffers, and saves dirty sessions through the normal canonical persistence path. Navigation during preparation cancels readiness.
- The native update permit rejects active runs/reviews and blocks new ones during installation. Users must stop active background tasks; the updater does not silently kill them.
- Package downloads are limited to 512 MiB, with a 15-second connection timeout, 30-second header/chunk idle timeout, and 15-minute total timeout. HTTPS URLs and redirects are validated; embedded credentials, query strings, and fragments are rejected.
- The official plugin verifies the signature and signed version before installation. Checking for updates never starts an installer by itself.
- Windows may close the app when handing off to the installer. Linux requires an explicit relaunch after installation. DEB/RPM installation needs OS authorization and testing on the target distribution.

## Signing and hosting

Public configuration:

- `AETHER_UPDATE_BASE_URL`: a static HTTPS hosting origin/path without credentials, a query string, or a fragment.
- `AETHER_UPDATE_ENDPOINT`: `BASE_URL/latest.json`, embedded at build time.
- `AETHER_UPDATE_PUBLIC_KEY`: the base64-encoded minisign public-key file, not raw Ed25519 bytes.
- `AETHER_WINDOWS_UPDATE_PUBLIC_KEY` and `AETHER_LINUX_UPDATE_PUBLIC_KEY`: platform key pins used during verification and promotion.
- `AETHER_CERTIFICATE_THUMBPRINT` and `AETHER_TIMESTAMP_URL`: the Windows publisher certificate and RFC 3161 timestamp service.

Keep these private inputs in the signing environment only:

- `TAURI_SIGNING_PRIVATE_KEY` and its optional password. The key must match the platform's pinned public key.
- The Windows code-signing certificate and private key, installed in the runner's CurrentUser certificate store under the configured thumbprint. The script does not import certificates or generate replacement keys.

Never print secrets or commit them to Git. The bundler and native runtime receive the same public key. Scripts reject missing keys or endpoints rather than producing an unsigned update.

The current URL policy expects static HTTPS hosting. GitHub Releases downloads usually redirect to CDN URLs with query strings, which this policy rejects. Do not use those URLs without a narrowly scoped, tested policy change.

## Pipeline stages

### 1. Source and checks

`.github/workflows/release-quality.yml` runs the checks without signing secrets. Release source must be an immutable commit/tag, with all required implementation files committed before CI runs.

The npm, Cargo, Tauri, and `vVERSION` tag versions must match. Use a stable Tauri v2 CLI version of at least 2.12.0 for signed-version support. Builds use lockfiles; old build output must not serve as a fallback.

### 2. Protected signed candidates

`.github/workflows/signed-release.yml`:

- Accepts a tag push or a manual run whose selected tag points exactly to the workflow SHA/HEAD and belongs to `main` history.
- Waits for the quality gate.
- Uses Windows runner labels `self-hosted`, `Windows`, `X64`, `aether-release-windows`, with environment `release-sign-windows`.
- Uses Linux runner labels `self-hosted`, `Linux`, `X64`, `aether-release-linux-24.04`, with environment `release-sign-linux`. The Linux build must not run as root.
- Has no `contents:write` permission and does not publish the update endpoint.

Configure protected environments with reviewer approval and branch/tag restrictions. Runner labels alone do not enforce protection. Use ephemeral or operator-controlled signing runners; never run untrusted pull-request code on them.

The build scripts can also run directly:

```powershell
./scripts/build-signed-release.ps1 -ArchiveRoot D:/retained-releases -CandidateDirectory D:/candidates/unique-run
```

```bash
AETHER_RELEASE_TAG=vVERSION AETHER_CANDIDATE_DIRECTORY=/srv/candidates/unique-run bash scripts/build-signed-release.sh
```

Replace the example version and paths with the actual values. Scripts require configured signing inputs and do not generate keys. On Windows, Authenticode signing and timestamping happen first; the updater signature is then computed over the final bytes. Linux candidates verify the exact AppImage/DEB/RPM filenames and metadata. Candidate output must use a fresh directory, not overwrite an existing one.

The intended Linux release baseline is Ubuntu 24.04 x86_64. An Arch WSL build does not prove compatibility with that baseline. Inspect GLIBC requirements across all bundled AppImage ELF files and test installation on the target distribution.

### 3. Records and manifest

`scripts/release-pipeline.mjs` provides `preflight`, `candidate`, `prepare`, `verify`, and `promote-local`. The Rust verifier in `scripts/release-policy-tests` checks the signature, global trusted comment, and exactly one matching signed version.

- Candidate records contain the exact artifact set, SHA-256 hashes, signature hashes, version, full source SHA, and public-key digest.
- A signature in `latest.json` is the **contents of the `.sig` file**, not its path, URL, or hash.
- Platform keys distinguish `windows-x86_64-nsis`/`msi` and `linux-x86_64-appimage`/`deb`/`rpm`. Generic fallbacks select NSIS on Windows and AppImage on Linux.
- Artifact URLs are immutable: `BASE_URL/releases/VERSION/FULL_SOURCE_SHA/FILENAME`.
- Verification rejects stale, missing, or tampered artifacts; swapped platform keys; version mismatches; symlinks/junctions; and immutable-path collisions.

### 4. Separate manual promotion

`.github/workflows/promote-release.yml` uses the protected `release-promotion` environment and `aether-release-promotion` runner:

1. The operator selects a successful candidate run ID and tag.
2. Run metadata must match the repository, workflow, event, status, and selected release SHA/tag/branch.
3. The job downloads the CI candidates and verifies them again without a private key.
4. It copies the immutable files exclusively into the publish directory.
5. It verifies the destination.
6. It replaces `latest.json` through a same-directory rename, **last**.

The publish directory and all its ancestors must have a trusted single writer. Other users and web-server uploads must not be able to modify them. Path checks are not descriptor-relative protection against filesystem races. Set `AETHER_UPDATE_PUBLISH_DIRECTORY` only on a trusted runner. Do not use junctions, symlinks, UNC paths, or device paths to bypass these checks.

If promotion fails after copying some immutable files, the previous `latest.json` remains active. The partial tree is retained, and retrying the same identity is rejected. The operator must inspect and handle that tree explicitly. Scripts never delete production data automatically or overwrite an artifact that has already been released.

## Tests and release checks

Run the local fixtures with:

```text
node --test scripts/test-release-pipeline.mjs
powershell -NoProfile -File scripts/test-release-preflight.ps1
bash scripts/test-release-linux.sh
cargo test --locked --manifest-path scripts/release-policy-tests/Cargo.toml
```

The orchestration fixtures use an injected verifier. The Rust tests cover valid and tampered signature vectors and signed-version checks. Neither proves that production certificates or hosting are configured correctly.

Before the first promotion, test the full Windows and Linux upgrade flow in disposable VMs/profiles: check, confirm, download, verify, upgrade, and relaunch. Cover invalid signatures, changed bytes, a newer manifest pointing to an older package, unsafe URLs/redirects, network interruption, active tasks/runs, failed saves, cancellation/UAC, and manual rollback from a backup. The app does not downgrade automatically.

## Local builds without signing

Unsigned local builds can be retained for compilation and testing. Do not put them in the production manifest or label them as trusted signed releases. Retain hashes, the source snapshot, compiler/platform details, package metadata, smoke-test results, and GLIBC requirements. Archive old artifacts before replacing them.
