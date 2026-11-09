#!/usr/bin/env bash
set -euo pipefail
# Tidak ada instalasi paket, pembuatan kunci, atau publikasi dari skrip ini.
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
: "${AETHER_RELEASE_TAG:?Set exact vVERSION tag}"
: "${AETHER_UPDATE_BASE_URL:?Set public static HTTPS base URL}"
: "${AETHER_UPDATE_PUBLIC_KEY:?Set existing Linux public key}"
: "${TAURI_SIGNING_PRIVATE_KEY:?Provide existing signing identity through protected environment}"
: "${AETHER_CANDIDATE_DIRECTORY:?Set new absolute candidate directory}"
[[ ${EUID:-$(id -u)} -ne 0 ]] || { printf '%s\n' 'Refusing release build as root' >&2; exit 1; }
[[ -z ${CARGO_TARGET_DIR:-} && -z ${CARGO_BUILD_TARGET:-} ]] || { printf '%s\n' 'Custom Cargo output/target not supported' >&2; exit 1; }
[[ $AETHER_CANDIDATE_DIRECTORY = /* && ! -e $AETHER_CANDIDATE_DIRECTORY ]] || { printf '%s\n' 'Candidate directory must be absolute and new' >&2; exit 1; }
for tool in node npm cargo git dpkg-deb rpm; do command -v "$tool" >/dev/null || { printf 'Required tool missing: %s\n' "$tool" >&2; exit 1; }; done
cd "$root"
config="$(node scripts/release-pipeline.mjs preflight --root "$root" --tag "$AETHER_RELEASE_TAG" --platform linux-x86_64)"
version="${AETHER_RELEASE_TAG#v}"
sha="$(git rev-parse HEAD)"
[[ $sha =~ ^[a-f0-9]{40}$ ]] || { printf '%s\n' 'Full source SHA required' >&2; exit 1; }
export AETHER_UPDATE_ENDPOINT="${AETHER_UPDATE_BASE_URL%/}/latest.json"
node --input-type=module -e 'import { publicUrl } from "./scripts/release-pipeline.mjs"; publicUrl(process.env.AETHER_UPDATE_ENDPOINT)'
cargo build --locked --quiet --manifest-path scripts/release-policy-tests/Cargo.toml --bin aether-release-policy-tests
started="$(node -e 'process.stdout.write(new Date().toISOString())')"
npm run tauri -- build --config src-tauri/tauri.linux.conf.json --config "$config" --bundles appimage,deb,rpm -- --locked
node scripts/release-pipeline.mjs candidate --source "$root/src-tauri/target/release/bundle" --out "$AETHER_CANDIDATE_DIRECTORY" --platform linux-x86_64 --version "$version" --sha "$sha" --started-at "$started"
printf 'Verified Linux candidate retained at %s; not published\n' "$AETHER_CANDIDATE_DIRECTORY"
