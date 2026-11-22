# Security policy

## Reporting a vulnerability

If you find a security issue (for example in workspace path confinement, web fetch/SSRF filtering, iframe sandboxing, native openers, or credential handling), please report it privately through GitHub Security Advisories on this repository instead of opening a public issue.

Include the affected file or feature, reproduction steps, and what happens versus what you expected.

## Scope notes for pre-release builds

- Pre-release Windows installers (`v*-beta*`) are built unsigned, so Windows SmartScreen will show an unknown-publisher warning. Compare the downloaded file's SHA-256 hash against the checksum published with the release before running it.
- Code inside the experimental `rune/` directory is not part of the Tauri desktop release build.
