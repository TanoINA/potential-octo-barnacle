#!/bin/bash
set -u
set -o pipefail
WORK=/root/aether-build-20261108T141140Z
OUT=/mnt/d/AetherAgent/release/linux/0.3.0/20261108T142121Z
exec > >(tee "$OUT/verification.log") 2>&1
for ((i=0;i<240;i++)); do [[ -f "$OUT/build-status.txt" ]] && break; sleep 5; done
cat "$OUT/build-status.txt" || exit 2
grep -qx 'build_exit=0' "$OUT/build-status.txt" || { echo 'Packaging blocked, see tauri logs'; exit 3; }
cd "$WORK" || exit 4
BIN="$WORK/src-tauri/target/release/aether-agent"
[[ -x "$BIN" ]] || { find src-tauri/target/release -maxdepth 1 -type f -executable; exit 5; }
cp "$BIN" "$OUT/aether-agent"
find src-tauri/target/release/bundle -type f \( -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' \) -exec cp -t "$OUT" {} +
mapfile -t DEBS < <(find "$OUT" -maxdepth 1 -name '*.deb')
mapfile -t RPMS < <(find "$OUT" -maxdepth 1 -name '*.rpm')
mapfile -t APPS < <(find "$OUT" -maxdepth 1 -name '*.AppImage')
[[ ${#DEBS[@]} == 1 && ${#RPMS[@]} == 1 && ${#APPS[@]} == 1 ]] || exit 6
{ file "$OUT/aether-agent" "${DEBS[0]}" "${RPMS[0]}" "${APPS[0]}"; readelf -h "$BIN"; readelf -l "$BIN"; readelf -d "$BIN"; readelf --version-info "$BIN"; ldd "$BIN"; } > "$OUT/native-elf-verification.txt" 2>&1
readelf --version-info "$BIN" | grep -o 'GLIBC_[0-9.]*' | sort -Vu > "$OUT/native-glibc-requirements.txt"
{ dpkg-deb --info "${DEBS[0]}"; dpkg-deb --field "${DEBS[0]}"; dpkg-deb --contents "${DEBS[0]}"; } > "$OUT/deb-metadata.txt" 2>&1
{ rpm -qip "${RPMS[0]}"; rpm -qpR "${RPMS[0]}"; rpm -qp --scripts "${RPMS[0]}"; rpm -qpl "${RPMS[0]}"; } > "$OUT/rpm-metadata.txt" 2>&1
SMOKE=$(mktemp -d /root/aether-linux-smoke-XXXXXXXX)
echo "$SMOKE" > "$OUT/smoke-workspace.txt"
dpkg-deb --extract "${DEBS[0]}" "$SMOKE/deb"
dpkg-deb --control "${DEBS[0]}" "$SMOKE/deb-control"
find "$SMOKE/deb-control" -type f -exec sh -c 'echo "--- $1"; cat "$1"' sh {} \; > "$OUT/deb-control-and-scripts.txt"
sha256sum "$BIN" "$OUT/aether-agent" "$SMOKE/deb/usr/bin/aether-agent" > "$OUT/elf-identity.txt"
chmod +x "${APPS[0]}"
"${APPS[0]}" --appimage-version > "$OUT/appimage-runtime-version.txt" 2>&1
mkdir "$SMOKE/appimage"
(cd "$SMOKE/appimage" && "${APPS[0]}" --appimage-extract) > "$OUT/appimage-extraction.log" 2>&1
APPDIR="$SMOKE/appimage/squashfs-root"
{ find "$APPDIR" -iname '*webkit*' -o -iname '*javascriptcore*' -o -iname '*libsoup*'; find "$APPDIR" -name '*libc.so*'; ls -l "$APPDIR"; } > "$OUT/appimage-runtime-library-inventory.txt" 2>&1
Xvfb :93 -screen 0 1280x850x24 -nolisten tcp > "$OUT/xvfb.log" 2>&1 &
XVFB_PID=$!
trap 'kill "$XVFB_PID" 2>/dev/null || true' EXIT
sleep 2
smoke() {
  name=$1; executable=$2
  profile="$SMOKE/profile-$name"
  mkdir -p "$profile/home" "$profile/config" "$profile/data" "$profile/cache" "$profile/run" "$profile/tmp"
  chmod 700 "$profile/run"
  echo "Starting $name: $executable"
  timeout --signal=TERM --kill-after=3s 15s dbus-run-session -- env HOME="$profile/home" USERPROFILE="$profile/home" XDG_CONFIG_HOME="$profile/config" XDG_DATA_HOME="$profile/data" XDG_CACHE_HOME="$profile/cache" XDG_RUNTIME_DIR="$profile/run" TMPDIR="$profile/tmp" DISPLAY=:93 GDK_BACKEND=x11 WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 APPDIR="$APPDIR" APPIMAGE="${APPS[0]}" "$executable" > "$OUT/smoke-$name.log" 2>&1 &
  pid=$!
  sleep 10
  if kill -0 "$pid" 2>/dev/null; then echo "$name alive_at_10s=yes"; else echo "$name alive_at_10s=no"; fi
  DISPLAY=:93 xwininfo -root -tree > "$OUT/smoke-$name-windows.txt" 2>&1
  DISPLAY=:93 xwd -root -silent -out "$OUT/smoke-$name.xwd" 2>> "$OUT/smoke-$name.log"
  wait "$pid"; rc=$?
  echo "$name timeout_exit=$rc" | tee -a "$OUT/smoke-status.txt"
}
smoke native "$BIN"
smoke deb-extracted "$SMOKE/deb/usr/bin/aether-agent"
smoke appimage-extracted "$APPDIR/AppRun"
export CARGO_BUILD_JOBS=3 CARGO_INCREMENTAL=0
(cd src-tauri && cargo test --locked --lib) > "$OUT/cargo-test-lib.log" 2>&1
printf 'cargo_test_lib_exit=%s\n' "$?" > "$OUT/test-status.txt"
sha256sum -c "$OUT/source-sha256.txt" > "$OUT/final-source-check.txt"
{ echo 'Arch Linux x86_64 build host: glibc 2.44. Modern Linux only; NOT verified or supported for Ubuntu 22.04.'; printf 'Maximum native ELF GLIBC symbol version: '; tail -n 1 "$OUT/native-glibc-requirements.txt"; echo 'Native and distro packages require system WebKitGTK 4.1 / GTK3 and dependencies. See actual DEB/RPM metadata and ldd.'; echo 'AppImage runtime inventory and isolated extracted-AppRun smoke are recorded; glibc remains a host prerequisite.'; echo 'Unsigned local build. No generated package installed; no Windows GUI launched. Root WebKit sandbox override used ONLY in isolated Xvfb smoke profiles.'; } > "$OUT/COMPATIBILITY.txt"
(cd "$OUT" && sha256sum aether-agent *.deb *.rpm *.AppImage > SHA256SUMS && sha256sum -c SHA256SUMS)
printf 'Verification complete %s\n' "$(date -u -Iseconds)"