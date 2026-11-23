# Security policy

## Reporting a vulnerability

If you find a security issue (for example in workspace path confinement, web fetch/SSRF filtering, iframe sandboxing, native openers, or credential handling), please report it privately through GitHub Security Advisories on this repository instead of opening a public issue.

Include the affected file or feature, reproduction steps, and what happens versus what you expected.

## Scope notes for pre-release builds

- Pre-release installers (`v*-beta*`) on Windows and Linux are built unsigned; Windows SmartScreen will show an unknown-publisher warning. Compare downloaded files against the `SHA256SUMS.txt` checksums published with the release before installing or running them.
- Code inside the experimental `rune/` directory is not part of the Tauri desktop release build.
