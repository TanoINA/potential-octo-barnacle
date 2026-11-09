#!/bin/bash
set -u
OUT=/mnt/d/AetherAgent/release/linux/0.3.0/20261108T142121Z
WORK=/root/aether-build-20261108T141140Z
for ((i=0;i<90;i++)); do [[ -f "$OUT/test-status.txt" && -f "$OUT/SHA256SUMS" ]] && break; sleep 2; done
SMOKE=$(cat "$OUT/smoke-workspace.txt")
APPDIR="$SMOKE/appimage/squashfs-root"
{ while IFS= read -r -d '' f; do
  req=$(readelf --version-info "$f" 2>/dev/null | sed -n '/Version needs section/,$p' | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -n1)
  [[ -n "$req" ]] && printf '%s %s\n' "$req" "${f#$APPDIR/}"
done < <(find "$APPDIR" -type f -print0); } > "$OUT/appimage-elf-glibc-per-file.txt"
cut -d' ' -f1 "$OUT/appimage-elf-glibc-per-file.txt" | sort -Vu > "$OUT/appimage-glibc-requirements.txt"
NATIVE_MAX=$(tail -n1 "$OUT/native-glibc-requirements.txt")
APP_MAX=$(tail -n1 "$OUT/appimage-glibc-requirements.txt")
cd "$WORK" || exit 2
: > "$OUT/final-live-source-check.txt"
while read -r hash rel; do
  live=$(sha256sum "/mnt/d/AetherAgent/${rel#./}" | cut -d' ' -f1)
  [[ "$hash" == "$live" ]] && echo "$rel: OK" >> "$OUT/final-live-source-check.txt" || echo "$rel: CHANGED" >> "$OUT/final-live-source-check.txt"
done < "$OUT/source-sha256.txt"
{ echo 'Arch Linux x86_64 build host: glibc 2.44; modern Linux only.'; echo "Native ELF maximum required GLIBC symbol version: $NATIVE_MAX"; echo "AppImage bundled ELF maximum required GLIBC symbol version: $APP_MAX"; echo 'NOT an Ubuntu 22.04-compatible or universally portable Linux release.'; echo 'Native/DEB/RPM need system WebKitGTK 4.1 / GTK3 plus transitive shared libraries. AppImage bundles WebKitGTK, JavaScriptCoreGTK, libsoup, and WebKit processes, but does not bundle glibc.'; echo 'PACKAGING CAVEAT: DEB Depends declares libwebkit2gtk-4.1-0, libgtk-3-0 only; RPM declares corresponding shared-library sonames. Neither metadata explicitly enforces the actual minimum glibc requirement. Older systems may accept installation and then fail at load time. No production metadata was changed.'; echo 'Unsigned local build; only current Arch WSL runtime validated. No generated package installed. No Windows GUI launched.'; } > "$OUT/COMPATIBILITY.txt"
(cd "$OUT" && sha256sum aether-agent *.deb *.rpm *.AppImage > SHA256SUMS && sha256sum -c SHA256SUMS) > "$OUT/artifact-hash-check.txt"
{
  echo '# Linux 0.3.0 resumed build report'
  echo
  echo '## Result'
  echo 'All four requested artifacts created successfully, unsigned, using the existing native Linux cache. First requested Tauri command succeeded (exit 0), no NO_STRIP retry or source/configuration edits required.'
  echo "Release evidence: $OUT"
  echo "Workspace retained: $WORK"
  echo 'Prior failed directory 20261108T141140Z was not modified. Only updated src-tauri/src/lib.rs copied into cached source. Original 224-file manifest checked before copy; all 224 current hashes matched live Windows source after copy.'
  echo
  echo '## Build'
  echo 'npm run tauri -- build --bundles deb,rpm,appimage -- --locked'
  echo 'CARGO_BUILD_JOBS=3 CARGO_INCREMENTAL=0; signing keys unset. Native release compilation completed in 3m35s. Pinned linuxdeploy-07333c6 and AppRun downloads completed without workarounds.'
  cat "$OUT/build-status.txt"
  echo
  echo '## Compatibility and package metadata caveat'
  cat "$OUT/COMPATIBILITY.txt"
  echo 'See native-elf-verification.txt, deb-metadata.txt, rpm-metadata.txt, deb-control-and-scripts.txt, appimage-runtime-library-inventory.txt, appimage-elf-glibc-per-file.txt.'
  echo 'DEB contains only control/md5sums, no maintainer scripts. RPM query reported no install/uninstall scripts. RPM signature: none.'
  echo
  echo '## Isolated GUI smoke'
  echo "Smoke workspace retained: $SMOKE"
  echo 'Xvfb :93, 1280x850x24, TCP disabled, temporary HOME/USERPROFILE/XDG/TMPDIR per run, dbus-run-session, GDK_BACKEND=x11. Root WebKit sandbox override was set only for these smoke processes. Own app processes terminated by 15s timeout; Xvfb stopped afterward.'
  cat "$OUT/smoke-status.txt"
  for name in native deb-extracted appimage-extracted; do echo "### $name"; grep '"AetherAgent"' "$OUT/smoke-$name-windows.txt"; done
  echo 'Each run was alive after 10s and created an AetherAgent 1280x850 X window. Exit 124 means deliberate timeout, not a crash. Raw XWD captures retained. Only startup/liveness is validated, not feature interaction or visual correctness. Mesa software-rendering/DRI warnings were nonfatal.'
  echo 'AppImage outer runtime --appimage-version succeeded. GUI tested through extracted AppRun (no FUSE mount/install required), not through mounted AppImage launch. Native final ELF differs from DEB ELF because Tauri patches bundle-type information separately; exact DEB payload was smoked independently.'
  echo
  echo '## Rust library tests'
  if [[ -f "$OUT/test-status.txt" ]]; then cat "$OUT/test-status.txt"; else echo 'Test process still pending; do not claim tests passed.'; fi
  tail -n 45 "$OUT/cargo-test-lib.log"
  echo
  echo '## Artifact sizes and SHA-256'
  stat -c '%n %s bytes' "$OUT/aether-agent" "$OUT"/*.deb "$OUT"/*.rpm "$OUT"/*.AppImage
  cat "$OUT/SHA256SUMS"
  echo
  echo '## Source verification'
  cat "$OUT/source-freshness-check.txt"
  printf 'Final live source OK count: '; grep -c ': OK$' "$OUT/final-live-source-check.txt"
  echo 'Source fingerprints: source-sha256.txt; lib.rs fingerprint and validated branch lines: portability-fix-verification.txt.'
  echo 'No production source changed, no built package installed, no Windows outputs touched, and no previous failed evidence overwritten.'
  echo 'Nonfatal build warnings: unused id in agent/mcp.rs, unnecessary mut in agent/mod.rs, and Vite chunk-size warning.'
} > "$OUT/BUILD-REPORT.md"
(cd "$OUT" && find . -maxdepth 1 -type f ! -name EVIDENCE-SHA256SUMS ! -name linux-resume.log ! -name verification.log -print0 | sort -z | xargs -0 sha256sum > EVIDENCE-SHA256SUMS)
cat "$OUT/BUILD-REPORT.md"
echo '--- Final artifact validation'; cat "$OUT/artifact-hash-check.txt"
echo '--- lib.rs SHA'; head -n1 "$OUT/portability-fix-verification.txt"
echo '--- Missing native libraries'; grep 'not found' "$OUT/native-elf-verification.txt" || true