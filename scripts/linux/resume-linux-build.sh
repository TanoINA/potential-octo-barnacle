#!/bin/bash
set -u
set -o pipefail
WORK=/root/aether-build-20261108T141140Z
OLD=/mnt/d/AetherAgent/release/linux/0.3.0/20261108T141140Z
STAMP=$(date -u +%Y%m%dT%H%M%SZ)
OUT=/mnt/d/AetherAgent/release/linux/0.3.0/$STAMP
mkdir "$OUT" || exit 1
printf '%s\n' "$OUT" > /mnt/d/AetherAgent/release/linux/0.3.0/resume-output-location.txt
exec > >(tee "$OUT/linux-resume.log") 2>&1
printf 'Started %s\nWORK=%s\nOUT=%s\n' "$(date -u -Iseconds)" "$WORK" "$OUT"
cd "$WORK" || exit 2
sha256sum -c "$OLD/source-sha256.txt" > "$OUT/cached-source-before-copy.txt" || exit 3
cp /mnt/d/AetherAgent/src-tauri/src/lib.rs "$WORK/src-tauri/src/lib.rs" || exit 4
: > "$OUT/source-sha256.txt"
count=0
while read -r hash rel; do
  current=$(sha256sum "$rel" | cut -d' ' -f1)
  windows=$(sha256sum "/mnt/d/AetherAgent/${rel#./}" | cut -d' ' -f1)
  if [[ "$current" != "$windows" ]]; then echo "CURRENT SOURCE MISMATCH: $rel"; exit 5; fi
  if [[ "$rel" != './src-tauri/src/lib.rs' && "$current" != "$hash" ]]; then echo "UNEXPECTED CHANGE: $rel"; exit 6; fi
  printf '%s  %s\n' "$current" "$rel" >> "$OUT/source-sha256.txt"
  count=$((count+1))
done < "$OLD/source-sha256.txt"
[[ "$count" == 224 ]] || exit 7
echo "PASS: all 224 source hashes match live source; only lib.rs copied and allowed changed" | tee "$OUT/source-freshness-check.txt"
{ diff -u <(cat /mnt/d/AetherAgent/src-tauri/src/lib.rs) src-tauri/src/lib.rs; sha256sum src-tauri/src/lib.rs; grep -n -A65 'let valid_dir = if target_dir' src-tauri/src/lib.rs; } > "$OUT/portability-fix-verification.txt"
{ uname -a; cat /etc/os-release; getconf GNU_LIBC_VERSION; rustc -Vv; cargo -V; node -v; npm -v; pacman -Q; } > "$OUT/build-environment.txt"
unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD TAURI_PRIVATE_KEY TAURI_KEY_PASSWORD
export CARGO_BUILD_JOBS=3 CARGO_INCREMENTAL=0
npm run tauri -- build --bundles deb,rpm,appimage -- --locked 2>&1 | tee "$OUT/tauri-build.log"
rc=${PIPESTATUS[0]}
echo "Initial build exit=$rc"
if [[ $rc != 0 ]] && grep -Ei 'strip|RELR' "$OUT/tauri-build.log" >/dev/null; then
  echo 'Retrying with NO_STRIP=1 after exact strip/RELR diagnostic (original log preserved)'
  NO_STRIP=1 npm run tauri -- build --bundles deb,rpm,appimage -- --locked 2>&1 | tee "$OUT/tauri-build-no-strip.log"
  rc=${PIPESTATUS[0]}
fi
printf 'build_exit=%s\n' "$rc" > "$OUT/build-status.txt"
find src-tauri/target/release/bundle -maxdepth 4 -type f -printf '%p %s bytes\n' > "$OUT/bundle-inventory.txt" 2>/dev/null
if [[ $rc != 0 ]]; then echo "BUILD BLOCKED ($rc), evidence $OUT"; exit "$rc"; fi
find src-tauri/target/release -maxdepth 1 -type f -executable -print
printf 'BUILD COMPLETE %s\n' "$OUT"