# Pipeline update aplikasi

## Status dan batas nyata

Implementasi check, download berbatas, progress, verifikasi Ed25519/signed-version, konfirmasi install/restart, readiness sesi, kandidat rilis Windows/Linux, dan promosi terpisah tersedia di source. **Infrastruktur produksi belum diprovisikan**: endpoint publik, private/public key operator, sertifikat publisher Windows, runner signing, dan GitHub protected environments belum tersedia pada mesin audit. Jangan menyebut build lokal sebagai signed production release.

Binary tanpa endpoint/public key compile-time tetap fail-closed: tombol update tidak aktif. Satu rilis bootstrap yang sudah dikonfigurasi perlu dipasang manual terlebih dahulu; menerbitkan latest.json saja tidak mengaktifkan binary lama.

## Alur aplikasi

Settings → About AetherAgent → Check for updates → konfirmasi → simpan sesi idle → download → verifikasi → install → restart eksplisit bila platform memerlukannya.

- Endpoint/public key hanya disematkan saat build; webview tidak dapat memasok URL installer atau mengganti identitas signing.
- requireSignedVersion tetap true; hanya semver yang lebih baru diterima. Build metadata saja tidak dianggap update.
- Frontend memeriksa semua sesi cached, termasuk run tersembunyi, menunggu antrean save/settings, flush buffer, lalu menyimpan sesi dirty melalui jalur persistence canonical yang sama. Perubahan navigasi selama persiapan membatalkan readiness.
- Native update permit menolak active run/review dan menahan admission run/review baru selama install. Task aktif global harus dihentikan, bukan dimatikan diam-diam.
- Download dibatasi 512 MiB, connect 15 detik, idle header/chunk 30 detik, total 15 menit. HTTPS dan redirect tervalidasi; tidak menerima credential/query/fragment.
- Signature dan signed-version diverifikasi oleh plugin resmi sebelum installer. Instalasi tidak otomatis terjadi ketika check.
- Windows dapat menutup proses ketika handoff installer. Linux memerlukan relaunch eksplisit setelah paket terpasang. DEB/RPM memerlukan otorisasi OS dan pengujian distro tersendiri.

## Identitas dan hosting

Nilai publik:

- AETHER_UPDATE_BASE_URL: origin/path HTTPS static hosting tanpa credential, query, atau fragment.
- AETHER_UPDATE_ENDPOINT: BASE_URL/latest.json, disematkan saat build.
- AETHER_UPDATE_PUBLIC_KEY: base64 file minisign public key, bukan raw Ed25519 bytes.
- AETHER_WINDOWS_UPDATE_PUBLIC_KEY dan AETHER_LINUX_UPDATE_PUBLIC_KEY: pin platform untuk verifikasi/promosi.
- AETHER_CERTIFICATE_THUMBPRINT dan AETHER_TIMESTAMP_URL: publisher Windows dan RFC3161 timestamp service.

Secrets hanya pada signing environment:

- TAURI_SIGNING_PRIVATE_KEY dan password opsional, identitas milik operator yang cocok dengan public key platform.
- Sertifikat code-signing Windows beserta private key berada pada CurrentUser certificate store runner, sesuai thumbprint. Script tidak mengimpor atau membuat pengganti otomatis.

Jangan mencetak secret atau menaruhnya di Git. Public key disuntikkan ke konfigurasi bundler dan native dengan nilai yang sama. Skrip menolak key/endpoint kosong daripada menghasilkan update unsigned.

Policy saat ini sengaja memilih static HTTPS hosting. GitHub Releases download biasanya redirect ke CDN ber-query dan tidak sesuai policy URL ini; jangan memakai URL tersebut sebagai endpoint tanpa perubahan policy yang diuji secara sempit.

## Tahapan pipeline

### 1. Source dan quality

Berkas `.github/workflows/release-quality.yml` menjalankan quality matrix tanpa signing secrets. Source release harus commit/tag immutable dan semua file implementasi lokal harus masuk version control sebelum CI digunakan. Audit ini tidak melakukan commit/push.

Versi npm, Cargo, Tauri, dan tag vVERSION harus sama. CLI wajib stable Tauri v2 >=2.12.0 agar signed-version tersedia. Lockfile dipakai; jangan mengambil build output lama sebagai fallback.

### 2. Kandidat signed terproteksi

`.github/workflows/signed-release.yml`:

- Tag push atau manual yang tag-nya menunjuk persis workflow SHA/HEAD, serta berada pada history main.
- Menunggu quality gate.
- Windows runner label: self-hosted, Windows, X64, aether-release-windows; environment release-sign-windows.
- Linux runner label: self-hosted, Linux, X64, aether-release-linux-24.04; environment release-sign-linux. Jalankan bukan root.
- Job tidak mempunyai contents:write dan tidak memublikasikan endpoint.

Provision protected environments dengan approval reviewer dan pembatasan branch/tag. Label runner saja tidak mengonfigurasi protection. Runner harus ephemeral atau dimiliki operator tepercaya; jangan menjalankan source PR tidak dipercaya pada runner signing.

Perintah reusable:

```powershell
./scripts/build-signed-release.ps1 -ArchiveRoot D:/retained-releases -CandidateDirectory D:/candidates/unique-run
```

```bash
AETHER_RELEASE_TAG=vVERSION AETHER_CANDIDATE_DIRECTORY=/srv/candidates/unique-run bash scripts/build-signed-release.sh
```

Placeholder VERSION/path harus diganti operator dengan identitas aktual. Skrip memerlukan environment yang valid dan tidak membuat key. Windows: Authenticode/timestamp dahulu, signature updater pada byte final sesudahnya. Linux: AppImage/DEB/RPM exact filenames dan metadata diverifikasi. Candidate output harus baru, bukan overwrite.

Baseline rilis CI Linux yang dimaksud Ubuntu 24.04 x86_64. Build lokal di Arch WSL tidak membuktikan kompatibilitas distro itu; periksa GLIBC seluruh bundled ELF AppImage dan uji install nyata pada baseline.

### 3. Record dan manifest

`scripts/release-pipeline.mjs` mempunyai preflight, candidate, prepare, verify, promote-local. Verifier Rust di scripts/release-policy-tests memeriksa signature/global trusted comment dan tepat satu version yang sama.

- Kandidat memuat exact artifact set, SHA-256, signature hash, versi, full source SHA, dan public-key digest.
- Signature pada latest.json adalah **isi file .sig**, bukan path/URL signature atau hash.
- Key platform spesifik: windows-x86_64-nsis/msi; linux-x86_64-appimage/deb/rpm. Generic fallback Windows menunjuk NSIS, Linux AppImage.
- URL immutable: BASE_URL/releases/VERSION/FULL_SOURCE_SHA/FILENAME.
- Verifikasi menolak stale/missing/tampered artifacts, key platform tertukar, version mismatch, symlink/junction, dan immutable collision.

### 4. Promosi manual terpisah

`.github/workflows/promote-release.yml`, protected environment release-promotion, runner aether-release-promotion:

1. Operator memilih successful candidate run ID dan tag.
2. Run metadata harus cocok repository/workflow/event/status dan head_sha/tag/branch release yang dipilih.
3. Download kandidat CI dan verifikasi ulang tanpa private key.
4. Salin byte immutable eksklusif ke publish directory.
5. Verifikasi destination.
6. Ganti pointer latest.json melalui rename satu direktori, **paling akhir**.

Publish directory/semua ancestor wajib trusted single-writer, bukan writable oleh pengguna lain atau web server upload. Path-based checks bukan descriptor-relative anti-race filesystem. Set AETHER_UPDATE_PUBLISH_DIRECTORY hanya pada runner tepercaya. Jangan gunakan junction/symlink/UNC/device path sebagai bypass.

Apabila gagal setelah sebagian immutable files tersalin, latest lama tetap aktif. Partial tree dipertahankan dan retry identitas sama ditolak; operator harus audit dan menangani tree itu secara eksplisit. Script tidak menghapus data production otomatis. Jangan menimpa artifact versi yang sudah dirilis.

## Uji dan gate operator

Fixture tests lokal:

```text
node --test scripts/test-release-pipeline.mjs
powershell -NoProfile -File scripts/test-release-preflight.ps1
bash scripts/test-release-linux.sh
cargo test --locked --manifest-path scripts/release-policy-tests/Cargo.toml
```

Fixtures orchestration memakai injected verifier; tests Rust membuktikan signature vector yang sah/tampered/signed-version. Keduanya bukan bukti bahwa sertifikat produksi atau hosting sudah benar.

Sebelum promosi pertama, VM/profil disposable harus membuktikan Windows dan Linux check → explicit confirm → download → signature verification → upgrade → relaunch. Uji signature salah, byte berubah, manifest versi baru dengan paket lama, URL/redirect tidak aman, jaringan putus, task/run aktif, failed save, cancel/UAC, dan rollback manual dengan backup. Aplikasi tidak melakukan automatic downgrade.

## Build lokal saat signing belum tersedia

Build lokal unsigned boleh diarsipkan sebagai hasil kompilasi/pengujian, tetapi tidak boleh dimasukkan ke manifest production atau diberi label trusted signed. Simpan record hash, source snapshot, compiler/platform, metadata paket, hasil smoke, dan batas GLIBC. Jangan mengganti artifact lama tanpa arsip.
