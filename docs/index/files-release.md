# release (26 files, 1925 lines, 17 tests)

.github/workflows/promote-release.yml | 76L
.github/workflows/release-quality.yml | 43L
.github/workflows/signed-release.yml | 141L
scripts/build-signed-release.ps1 | 108L
scripts/build-signed-release.sh | 26L
scripts/project-index.mjs | 170L
scripts/release-archive-policy.ps1 | 44L
scripts/release-build-config.ps1 | 12L
scripts/release-native-command.ps1 | 22L
scripts/release-pipeline.mjs | 297L | sha256 validateKey publicUrl preflight verifySignature expectedArtifacts inspectLinux createCandidate prepareRelease verifyPrepared promoteLocal
scripts/release-policy-tests/Cargo.toml | 14L
scripts/release-policy-tests/README.md | 45L
scripts/release-policy-tests/src/main.rs | 50L
scripts/release-policy-tests/src/signature_tests.rs | 40L | 2T
scripts/release-signature-check.ps1 | 15L
scripts/run-aether.ps1 | 11L
scripts/run-dev.bat | 9L
scripts/test-release-archive.ps1 | 45L
scripts/test-release-linux.sh | 68L
scripts/test-release-pipeline.mjs | 332L | 15T
scripts/test-release-powershell.ps1 | 29L
scripts/test-release-preflight.ps1 | 158L
scripts/test-release-signature.ps1 | 37L
scripts/test-release.ps1 | 24L
scripts/test-rpm-dependency-semantics.mjs | 90L
scripts/verify-installer.ps1 | 19L
