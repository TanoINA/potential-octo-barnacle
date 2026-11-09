# Bukti perbaikan integrasi parent

## Verifikasi yang telah dieksekusi

- Baseline frontend: 21 files / 113 tests lulus; build tsc+Vite lulus, warning chunk >500 kB.
- Perintah wajib awal `npm test -- --fileParallelism=false` gagal karena opsi serial dua kali. Default dipindahkan dari npm script ke konfigurasi Vitest; perintah persis tersebut sekarang benar-benar menjalankan suite (gate akhir masih berjalan saat catatan ini ditulis).
- Baseline native paralel: 117 lulus / 2 gagal timeout proses. Ulang masing-masing filter pada mesin lebih idle: cancellation PowerShell 1/1 lulus, spawn task 1/1 lulus. Ini dicatat sebagai flaky baseline, bukan klaim 100% gate.

## Provider SSE

Perubahan di provider.rs dan provider_audit_tests.rs:

1. H — EOF dengan delta tetapi tanpa completion marker bukan sukses. Regression HTTP awal benar-benar RED (`unfinished reply` dianggap sukses), lalu 21 test provider existing+baru GREEN.
2. H — Data line kosong turut dihitung dalam event budget; jumlah line dibatasi 8192. Tidak lagi bisa tumbuh tanpa batas dengan `data:` kosong.
3. M — Decoder mendukung CR, LF, CRLF termasuk split chunk. Invalid UTF-8 menjadi error eksplisit, bukan line dibuang.
4. M — Event sesudah [DONE] pada packet yang sama tidak mengubah hasil. Anthropic message_stop menjadi terminal.
5. H — Total SSE wire bytes dibatasi 10 MiB, selain batas per event 1 MiB. Regression stream berisi banyak delta kecil benar-benar RED sebelum perbaikan, lalu GREEN sesudahnya.

Bukti decoder: tiga pure decoder test RED (CR-only, empty data budget, invalid UTF-8) lewat harness yang mengekstrak source produksi persis; ketiganya GREEN sesudah fix. Gate crate asli `cargo test agent::provider::audit_tests -- --nocapture`: 5 passed, 0 failed. Test HTTP EOF ada di module tests existing, dijalankan terpisah di awal. Perubahan provider payload/state milik worker lain dicatat di laporan masing-masing; full suite integrasi tetap wajib.

## App shell

- H — Shortcut navigasi global tidak menembus modal aria-modal aktif atau event yang sudah preventDefault.
- M — Live Shell memakai fixed overlay, tidak memakan lebar kolom chat ketika sidebar terbuka.
- L — Toast error punya role=alert dan tombol Dismiss error yang bernama aksesibel.

Tiga regression App ditulis/dijalankan RED dulu. Gate focused terakhir: 3 passed, 13 skipped. Skipped adalah test lain di file karena filter, bukan failure yang disembunyikan.

## Browser UI nyata, IPC mocked

Helper browser bawaan gagal menjalankan Chrome. Alternatif executable Chrome headless terisolasi + CDP berhasil; tidak memakai profil/cookie pengguna. Probe source di scratch `aether_ui_probe.mjs`, hasil akhir di `docs/audit/ui-browser-evidence.json`.

- Chromium headless 157, Vite dev server 127.0.0.1:1425, ukuran 900/1024/1440 × 600; sidebar dipastikan terbuka.
- 900 px: main x=256, width=644; Send right=875, bottom=577; tidak ada document horizontal overflow.
- 1024 px: main width=768; Send right=999, bottom=577; tidak overflow.
- 1440 px: main width=1184; shell opened position=fixed dan main tetap width=1184.
- Settings modal aktif; Ctrl+N tidak menambah IPC call (before=30, after=30); satu dialog tetap aktif.
- Error browser akhir: array kosong. Probe pertama gagal karena mock Tauri belum menyediakan unregisterListener; mock diperbaiki dan probe diulang. Error mock tersebut bukan bug aplikasi.

Batas: ini bukan smoke WebView Tauri terpaket, bukan live LLM/MCP; belum membuktikan semua kontrol dalam task-card padat, virtualized long history, scaling 125/150%, dan process-tree OS. Test browser ini tidak menggantikan cargo/full frontend suites.
