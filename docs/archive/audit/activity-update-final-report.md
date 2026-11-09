# Hasil final: pemulihan interupsi, aktivitas ringkas, updater, dan build

Tanggal: 9 November 2026. Workspace: D:/AetherAgent. Cakupan React/Tauri; Rune tidak diubah. Working tree memang sudah dirty sebelum pekerjaan ini. Tidak dilakukan reset, commit, push, publikasi update, penggantian kunci, instalasi paket ke OS pengguna, atau penggantian aplikasi pengguna yang sedang berjalan.

## Status singkat

| Pekerjaan | Hasil aktual |
| --- | --- |
| Pemeriksaan sisa operasi agent yang dibatalkan | Source terkait diperiksa, compile dan regression suite lulus. Tidak ada klaim bahwa semua kemungkinan korupsi profil pengguna sudah diuji. |
| Aktivitas memenuhi composer | Diganti satu Activity tray, detail dibuka manual dan scroll berbatas. |
| Queue hilang saat pindah chat | Queue/pause/pending acceptance per sesi dipertahankan dari perbaikan sebelumnya; full frontend suite mencakup regresinya. |
| Bugfix substantif | Retry checkpoint, recovery interrupted journal, save acknowledgment, export internal-state protection, updater lifetime/readiness/transport dan packaging RPM. |
| Update melalui aplikasi | Source UI/native/pipeline tersedia dan diuji; belum aktif pada binary lokal karena endpoint/key produksi belum diprovisikan. |
| Build Windows | EXE standalone, installer NSIS, dan MSI selesai, checksum diverifikasi, startup terisolasi lulus. |
| Build Linux | Native ELF, DEB, RPM, AppImage selesai, metadata/checksum/GLIBC diperiksa, tiga startup smoke lulus. |

## Bugfix yang relevan

1. **Provider retry dan journal:** ID tool UI dibedakan antar attempt, pending tool attempt yang ditinggalkan diterminalkan tanpa mengubah provider protocol ID. Retry tidak lagi menimpa checkpoint tool yang sudah durable.
2. **Recovery operasi terputus:** hanya journal abandoned yang dinormalisasi; status assistant/tool yang terputus tidak menghalangi Clear/Undo setelah recovery. Journal aktif dan seal committed tetap dilindungi.
3. **Canonical save berhasil tetapi acknowledgment gagal:** frontend memvalidasi receipt dan memakai recover_save_ack untuk cursor/committed baseline. Tidak menimpa UI stream yang lebih baru atau menganggap snapshot berbeda sudah tersimpan.
4. **Ekspor ke state internal:** destination yang mengarah ke config/session/checkpoint internal ditolak; path tervalidasi dan byte internal dipertahankan, termasuk pengujian traversal/alias.
5. **Native update lifetime:** lease memblokir admission run/review baru dan menunggu pekerjaan native terlepas yang masih menyimpan hasil, bukan sekadar menunggu UI menampilkan done.
6. **Update network:** metadata dibatasi 1 MiB, paket 512 MiB, connect/stale/total deadline, pemeriksaan content length/chunk dan URL/redirect, kemudian signature/signed-version resmi sebelum installer.
7. **Install UI:** konfirmasi eksplisit, readiness save semua sesi, progress/notes berbatas, tidak membuka installer otomatis saat check. Close/Escape tidak menyingkirkan barrier ketika operasi installer native sedang berjalan.
8. **Pipeline dan paket:** perbaikan PS5.1 inline JSON quoting memakai file konfigurasi publik, RPM memakai rich dependency yang benar, fixture Linux diperbarui agar membaca tag dependency semantik.

## UI aktivitas

[Implementasi ActivityTray](file:///D:/AetherAgent/src/components/ActivityTray.tsx) merangkum subagent, background task, skill, running dan failed menjadi satu baris. Detail mempertahankan aksi log/refresh/kill/artifact serta identitas skill. Pembaruan status tidak memaksa panel terbuka; session switch mereset tampilan panel.

Pengujian production Chromium dengan fixture IPC 100 skill, 100 subagent, 100 task:

- Ringkasan collapsed: 32 px pada 1280x850 dan 900x600.
- Panel expanded: 297,5 px pada 1280x850; 210 px pada 900x600.
- Composer tetap terlihat; tidak ada horizontal page overflow.
- Keyboard Enter/Space berfungsi, update status tidak membuka kembali panel.
- 15 checks passed, 0 failed; 0 JS exceptions, 0 console errors, 0 unexpected IPC.

[Bukti browser](file:///D:/AetherAgent/verification/activity-updater/browser/report.md)

[Screenshot 900x600 collapsed](file:///D:/AetherAgent/verification/activity-updater/browser/activity-900x600-collapsed.png) | [900x600 expanded](file:///D:/AetherAgent/verification/activity-updater/browser/activity-900x600-expanded.png)

[Screenshot 1280x850 collapsed](file:///D:/AetherAgent/verification/activity-updater/browser/activity-1280x850-collapsed.png) | [1280x850 expanded](file:///D:/AetherAgent/verification/activity-updater/browser/activity-1280x850-expanded.png)

Harness awal gagal karena endpoint preview mati dan selector mengambil tombol aria-controls lain. Keyboard CDP juga perlu payload input lengkap. Kegagalan tersebut diselidiki pada diagnostic DOM/IPC, bukan disembunyikan dengan melonggarkan assertion geometri. Source aplikasi tidak diubah untuk membuat harness lulus.

## Gate verifikasi

| Gate | Hasil | Bukti |
| --- | --- | --- |
| Full frontend final | 680 passed, 63 files | [Log](file:///D:/AetherAgent/verification/activity-updater/frontend-final.log) |
| Full native final Windows | 424 passed, 0 failed, 2 ignored fixture subprocess | [Log](file:///D:/AetherAgent/verification/activity-updater/native-final.log) |
| TypeScript/Vite + Windows release | Build exit 0, EXE/MSI/NSIS | [Log](file:///D:/AetherAgent/verification/activity-updater/windows-build.log) |
| Release orchestration Node | 35 passed, 0 failed | [Log](file:///D:/AetherAgent/verification/activity-updater/pipeline-complete-final.log) |
| PowerShell preflight/signing config stubs | Lulus, termasuk path spasi PS5.1 ke npm.cmd | [Log](file:///D:/AetherAgent/verification/activity-updater/preflight-complete-final.log) |
| Signature policy Rust | 3 library tests dan 2 binary tests passed | [Log](file:///D:/AetherAgent/verification/activity-updater/signature-complete-final.log) |
| Linux pipeline fixture | Lulus setelah tag-query fixture diperbaiki | [Log](file:///D:/AetherAgent/verification/activity-updater/linux-fixture-green.log) |
| Production browser aktivitas | 15 passed, 0 failed | [JSON](file:///D:/AetherAgent/verification/activity-updater/browser/report.json) |
| Windows isolated startup | Alive 12 detik, window title AetherAgent | [JSON](file:///D:/AetherAgent/release/windows/0.3.0/20261109T202518-activity-update/startup-smoke.json) |
| Linux isolated startup | Native, DEB extracted, AppImage extracted: alive 10 detik, deliberate timeout 124 | [Log](file:///D:/AetherAgent/release/linux/0.3.0/20261109T132223Z-activity-update/startup-smoke.log) |
| Linux snapshot terhadap source live | 239 files cocok | [Manifest](file:///D:/AetherAgent/release/linux/0.3.0/20261109T132223Z-activity-update/source-sha256.txt) |
| Paket hash | 3 Windows dan 4 Linux cocok | Manifest/SHA256SUMS di direktori paket |
| Whitespace | git diff --check exit 0 | [Log](file:///D:/AetherAgent/verification/activity-updater/final-diff-check.log) |

Warning non-blocking tetap dilaporkan: dead-code/helper/import native yang ada dan bundle frontend >500 kB. Tidak dilakukan cleanup style atau menaikkan warning limit demi tampak bersih.

## Paket Windows 0.3.0 x64

Direktori immutable lokal: [Windows build](file:///D:/AetherAgent/release/windows/0.3.0/20261109T202518-activity-update).

| Paket | Bytes | SHA-256 |
| --- | ---: | --- |
| [NSIS setup](file:///D:/AetherAgent/release/windows/0.3.0/20261109T202518-activity-update/AetherAgent_0.3.0_x64-setup.exe) | 6396138 | B2FBB9D929F2B832DB590D90226EB19BE50EFD495440880AE1CE784D283CDE29 |
| [MSI](file:///D:/AetherAgent/release/windows/0.3.0/20261109T202518-activity-update/AetherAgent_0.3.0_x64_en-US.msi) | 9646080 | 2941D2D2506D67EE57951116A28BF14D505890E7CFF4ABC826DCF341A803273E |
| [Executable](file:///D:/AetherAgent/release/windows/0.3.0/20261109T202518-activity-update/aether-agent.exe) | 17731584 | F83C63699ACA66888101F98FE55ECAB01F25FA880320815B3B3075FF163353CE |

Ketiga file berstatus Authenticode NotSigned. Installer tidak dijalankan. Standalone EXE membutuhkan WebView2 runtime; startup telah diuji memakai temporary USERPROFILE/APPDATA/LOCALAPPDATA/WebView data, bukan profil aplikasi pengguna.

## Paket Linux 0.3.0 x86_64

Direktori immutable lokal: [Linux build](file:///D:/AetherAgent/release/linux/0.3.0/20261109T132223Z-activity-update).

| Paket | Bytes | SHA-256 |
| --- | ---: | --- |
| [DEB](file:///D:/AetherAgent/release/linux/0.3.0/20261109T132223Z-activity-update/AetherAgent_0.3.0_amd64.deb) | 8946256 | 9f130cbbd03eb914fd2cee8542c51bbe56caafc155b21c1ca2bc045b9f309aba |
| [RPM](file:///D:/AetherAgent/release/linux/0.3.0/20261109T132223Z-activity-update/AetherAgent-0.3.0-1.x86_64.rpm) | 8950869 | 3ec6b790000057c527d8d63205cf6d2623c6c42394ec04c7648b732da6966341 |
| [AppImage](file:///D:/AetherAgent/release/linux/0.3.0/20261109T132223Z-activity-update/AetherAgent_0.3.0_amd64.AppImage) | 110930424 | fc06659dd3b18802acdeee7e23a224ac1d46c4e59e1284ac854a355ef85794c5 |
| [Native ELF](file:///D:/AetherAgent/release/linux/0.3.0/20261109T132223Z-activity-update/aether-agent) | 20073560 | 102c129827a7f1b5ecee5e68f7565f7c9ce4dc1c296676ec64f47794fb8b2ae5 |

**Batas kompatibilitas:** build Arch Linux WSL, bukan Linux universal. Native ELF membutuhkan simbol sampai GLIBC_2.39 serta WebKitGTK4.1/GTK3 dan dependency transitif distro. AppImage membawa library dengan kebutuhan maksimum GLIBC_2.44 dan belum diuji di distro lain. Jangan mengklaim AppImage ini kompatibel Ubuntu24.04/22.04. Pipeline signing produksi mendefinisikan baseline Ubuntu24.04 terpisah yang masih harus diprovisikan/dijalankan.

RPM actual tag memuat rich dependency (glibc >= 2.39), bukan nama capability literal. [Metadata verifier](file:///D:/AetherAgent/release/linux/0.3.0/20261109T132223Z-activity-update/package-verification.json).

Smoke menggunakan Xvfb, profil temporary dan mount namespace terpisah; root WebKit sandbox override hanya pada proses smoke. Native, extracted DEB dan extracted AppImage membuka window dan bertahan 10 detik; dihentikan sengaja pada 15 detik. Tidak ada instalasi DEB/RPM, FUSE AppImage, atau interaksi provider/update end-to-end. Full native test suite final dijalankan di Windows, bukan klaim full Linux suite.

## Status updater produksi

[Runbook lengkap](file:///D:/AetherAgent/docs/release-update-pipeline.md).

Tersedia source workflow quality, signed candidate, serta promosi terpisah dengan pemeriksaan provenance/signature/hash dan pointer latest.json ditulis terakhir. Belum dilakukan commit/push/workflow run GitHub atau publikasi. Protected environments/runner labels di YAML bukan bukti infrastruktur sudah dikonfigurasi.

Yang masih wajib disediakan operator:

1. Endpoint static HTTPS publik sesuai kebijakan URL.
2. Identitas signing updater tetap dan public key platform yang cocok.
3. Sertifikat Authenticode/timestamp Windows dan protected signing runners.
4. Protected promotion environment/hosting directory.
5. Satu build bootstrap bertanda tangan yang menyematkan endpoint dan key, dipasang manual; uji upgrade sesungguhnya pada VM disposable.

Build lokal ini tidak menyematkan endpoint/key dan tombol update tetap fail-closed. File unsigned tidak dipublikasikan sebagai signed update. Tidak dibuat kunci pengganti atau bypass verifikasi demi membuat tombol tampak aktif.
