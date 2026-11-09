#!/usr/bin/env bash
set -euo pipefail
# Fixture terisolasi memakai alat palsu; tidak memasang paket atau memakai kunci asli.
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aether-pipeline-shell.XXXXXXXX")"
mkdir -p "$fixture/scripts" "$fixture/bin" "$fixture/src-tauri" "$fixture/node_modules/@tauri-apps/cli"
cp "$root/scripts/build-signed-release.sh" "$root/scripts/release-pipeline.mjs" "$fixture/scripts/"
printf '{"version":"0.3.0"}' > "$fixture/package.json"
printf '{"version":"0.3.0","productName":"AetherAgent"}' > "$fixture/src-tauri/tauri.conf.json"
printf '[package]\nversion = "0.3.0"\n' > "$fixture/src-tauri/Cargo.toml"
printf '{"version":"2.12.0"}' > "$fixture/node_modules/@tauri-apps/cli/package.json"
cat > "$fixture/bin/cargo" <<'STUB'
#!/usr/bin/env bash
printf 'cargo %s\n' "$*" >> "$TRACE"
[[ " $* " == *" --locked "* ]]
STUB
cat > "$fixture/bin/git" <<'STUB'
#!/usr/bin/env bash
printf '%040d\n' 0
STUB
cat > "$fixture/bin/npm" <<'STUB'
#!/usr/bin/env bash
printf 'npm %s\n' "$*" >> "$TRACE"
[[ " $* " == *" --locked "* && " $* " == *'"pubkey"'* && " $* " == *'"requireSignedVersion":true'* ]]
node --input-type=module <<'JS'
import fs from 'node:fs';
const names = [['appimage', 'AetherAgent_0.3.0_amd64.AppImage'], ['deb', 'AetherAgent_0.3.0_amd64.deb'], ['rpm', 'AetherAgent-0.3.0-1.x86_64.rpm']];
for (const [kind, name] of names) {
 const dir = `src-tauri/target/release/bundle/${kind}`;
 fs.mkdirSync(dir, {recursive:true});
 const data = Buffer.alloc(64, 1);
 if (kind === 'appimage') { data.set([127,69,76,70,2,1],0); data.set([65,73,2],8); data.writeUInt16LE(62,18); }
 fs.writeFileSync(`${dir}/${name}`, data, {mode:0o755});
 if (!(process.env.CASE === 'missing' && kind === 'rpm')) fs.writeFileSync(`${dir}/${name}.sig`, Buffer.from('fixture signature only').toString('base64'));
 if (process.env.CASE === 'stale') fs.utimesSync(`${dir}/${name}`, new Date(0), new Date(0));
}
JS
STUB
cat > "$fixture/bin/dpkg-deb" <<'STUB'
#!/usr/bin/env bash
case "${!#}" in Package) echo aether-agent;; Version) echo 0.3.0;; Architecture) echo amd64;; Depends) echo 'libc6 (>= 2.39), libwebkit2gtk-4.1-0';; *) exit 1;; esac
STUB
cat > "$fixture/bin/rpm" <<'STUB'
#!/usr/bin/env bash
if [[ " $* " == *' --requires '* ]]; then
 printf '(glibc >= 2.39)\nlibwebkit2gtk-4.1.so.0()(64bit)\n'
elif [[ " $* " == *'REQUIRENAME'* ]]; then
 printf '(glibc >= 2.39)|0|\nlibwebkit2gtk-4.1.so.0()(64bit)|0|\n'
else
 printf '0.3.0\n1\nx86_64\n'
fi
STUB
chmod +x "$fixture/bin/"*
export PATH="$fixture/bin:$PATH" TRACE="$fixture/trace"
export AETHER_RELEASE_TAG=v0.3.0 AETHER_UPDATE_BASE_URL=https://updates.example.com/aether
export AETHER_UPDATE_PUBLIC_KEY="$(node -e 'const b=Buffer.from([69,100,...Array.from({length:40},(_,i)=>i+1)]);process.stdout.write(Buffer.from(`untrusted comment: fixture\n${b.toString("base64")}\n`).toString("base64"))')"
export TAURI_SIGNING_PRIVATE_KEY=fixture-marker-not-a-key
export AETHER_CANDIDATE_DIRECTORY="$fixture/candidate-good"
bash "$fixture/scripts/build-signed-release.sh"
[[ -s "$AETHER_CANDIDATE_DIRECTORY/candidate-record.json" ]]
for CASE in missing stale; do
 export CASE AETHER_CANDIDATE_DIRECTORY="$fixture/candidate-$CASE"
 if bash "$fixture/scripts/build-signed-release.sh" > "$fixture/$CASE.log" 2>&1; then printf 'Accepted %s artifacts\n' "$CASE" >&2; exit 1; fi
 [[ ! -e "$AETHER_CANDIDATE_DIRECTORY/candidate-record.json" ]]
done
! grep -E 'apt|sudo|signer generate|npm install|npm ci' "$fixture/trace"
printf 'PASS: Linux build command, locked builds, current bundler public key, exact artifact metadata/signatures, stale/missing rejection. Stubs only: %s\n' "$fixture"
