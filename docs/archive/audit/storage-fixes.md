# Storage / IPC / skills remediation

## Scope dan hasil

Berkas implementasi yang dimodifikasi: `src-tauri/src/agent/storage.rs`, `skills.rs`, `src-tauri/src/lib.rs`. Tidak mengedit runner/provider/tools/tasks/types/frontend. Schema `workspaceCwd` dan `provider_messages` ditambahkan oleh parent/worker lain; literal storage dan binding IPC disesuaikan. Tidak membaca kredensial/config pengguna atau menjalankan aplikasi. Test skill save lama yang dahulu menulis direktori global kini memakai explicit fixture root.

| Temuan | Perbaikan dalam slice ini | Batas/residual |
|---|---|---|
| F03 | IPC mengikat `request.workspace_cwd` sebelum `begin` dan spawn; canonical existing directory. `Some("")` berarti tanpa workspace, tidak fallback. Legacy `None` masih fallback global. Council menolak run/session/execution-mode/workspace berbeda sebelum begin. | Frontend wajib menyertakan workspace sesi; parent menangani schema/frontend dan lifecycle restore. Tidak mengklaim seluruh race workspace selesai. |
| F04 | Local/URL opener memakai `ShellExecuteW` Windows langsung, null parameters, tanpa `cmd`/rundll32. Builder memvalidasi scheme, keberadaan/type, menolak UNC serta executable/shortcut/unknown extension; dokumen di-allowlist. Terminal Windows langsung `powershell.exe -NoLogo -NoExit`, working directory lewat `current_dir`, tanpa script/interpreter shell perantara. | Test builder literal, bukan meluncurkan file/browser/terminal nyata. File association pengguna masih menentukan aplikasi pembuka dokumen. Reveal path legacy tidak direfaktor. |
| F05 | Ordinary snapshot tidak mengubah pin/title native existing (kecuali judul awal `New Chat` saat disk masih kosong); rename/pin memakai authoritative save. Durable `.deleted` marker dibuat sebelum delete dan save menolak ID bertombstone. Toggle mengembalikan readback metadata. Comparison updatedAt sekarang mencakup seluruh messages/artifacts/model override. | Bukan revision/CAS atau lock lintas proses. Multi-instance racing write tetap mungkin. Tombstone dipertahankan; restore harus ID baru. Tombstone dibuat sebelum remove: remove gagal tetap error dan marker dapat membuat sesi read-only sampai pemulihan operator. Clear title via snapshot tidak lagi mereset judul existing—gunakan explicit rename bila diinginkan. |
| F06 | Checkpoint terdahulu dimasukkan sebagai quoted historical data (budget 12.000 chars), summary total 23.000 chars; summary assistant membaca ordered narrative termasuk preamble/segments. | Bounded deterministic compaction tetap lossy; riwayat besar/compaction sangat berulang dapat terpotong. Tidak mengklaim ringkasan tanpa kehilangan semantik. |
| F07 | Shared `pub(crate) ordered_transcript` mempertahankan segment text/tool order, memakai latest flattened tool state berdasarkan ID, dedupe mirror; fallback preamble→tools→final. Markdown/HTML memuat arguments/status/result; HTML escape tetap berlaku. Summary memakai helper sama. | Integrasi replay runner milik parent/runner worker; tidak diubah di sini. Export belum full structured council/attachment/status transcript; JSON tetap format terlengkap. |
| F08 | Clear workspace hanya mengabaikan NotFound; remove errors dipropagasi ke IPC sehingga backend tidak mengaku berhasil. Canonical workspace helper dipakai persistence dan run binding. | Transaksi session metadata/global workspace/frontend milik worker lain. |
| F10 | List sessions/artifacts menjadi `Result`; directory/read/parse/schema/filename-ID errors terlihat beserta path. Semua broken entry diagregasi, jumlah valid file yang dipertahankan dilaporkan. Save create hanya pada NotFound; corrupt/read errors tidak diabaikan/ditimpa. Legacy JSON wajib object+safe ID. | Kebijakan fail-whole-catalog: satu corrupt membuat IPC Err (bukan partial vector), valid files tetap utuh tetapi UI perlu recovery untuk tetap menampilkannya. PermissionDenied tidak di-inject dengan ACL; error path diuji memakai directory-as-file/not-directory. |
| F14 | Discovery/activation membaca maksimal 256 KiB+1; regular-file checks sebelum/sesudah open, metadata identity (name/description) diperiksa ulang. Activation canonical containment root. Attachment dan global skill writes memakai existing tools resolver sebelum dan sesudah membuat parent; selected missing workspace ditolak, bukan fallback global. | Validate-then-open masih TOCTOU; bukan handle/no-follow guarantee. Body swap dengan metadata identik tidak dideteksi; ancestor root global/config dianggap trust anchor. `overwrite=false` skill save masih exists-check lalu replace (race create bersamaan belum fixed). |

## Regression dan bukti aktual

RED awal: `cargo test --manifest-path src-tauri/Cargo.toml regression_ --lib` exit 101, helper storage baru belum diimplementasikan. Beberapa percobaan integrasi juga tertahan unfinished helpers/fixtures worker lain; tidak dianggap bug storage. Setelah compile tersedia, 10/11 regression pass dan fixture Windows junction gagal karena slash campuran; fixture diperbaiki memakai chained Path::join, lalu seluruh focused runs pass.

Command final memakai `TMPDIR=C:/Users/Tano/AppData/Local/hermes/cache/scratch`:

- `cargo test --manifest-path src-tauri/Cargo.toml regression_ --lib`: **11 passed, 0 failed**.
- `cargo test --manifest-path src-tauri/Cargo.toml agent::storage::tests --lib`: **18 passed, 0 failed**.
- `cargo test --manifest-path src-tauri/Cargo.toml agent::skills::tests --lib`: **10 passed, 0 failed**.
- `rustfmt --edition 2021 --config skip_children=true` pada tiga berkas owned berhasil.
- `git diff --check -- <tiga berkas owned>` exit 0.

Regression baru: repeated checkpoint+preamble, ordered export+HTML escape+single tool result, corrupt/schema preservation+list diagnostics, stale rename/unpin/tombstone, clear remove error, Windows attachment junction keluar workspace+missing workspace, skill grow/swap/directory activation, Windows global skill ancestor junction, opener encoded shell metachar/doc allowlist, explicit no-workspace binding, council scope. Junction dibuat hanya di fixture dan dihapus setelah test; tidak ada destructive exploit atau aplikasi pengguna yang dibuka. Seluruh skill save success tests diarahkan ke fixture root, tidak global home. Fixture memilih TMPDIR saat disediakan, fallback temp_dir untuk test runner biasa.

Caller inventory seluruh repo: `storage::list_sessions` dan `storage::list_all_artifacts` hanya wrapper `lib.rs`, keduanya sudah propagasi Result. `ordered_transcript` sengaja tersedia untuk replay worker, tanpa cross-file edit.

## Yang belum diselesaikan oleh slice ini

F01/F02/F09/F11/F12/F13/F15/F16/F17/F18 dan tambahan audit lain tidak dinyatakan fixed. Full native/frontend/build/clippy/release gates milik parent, tidak dijalankan worker ini. Native-opener interactive smoke, handle-bound confinement, CAS multi-process, partial-list recovery UI, export internal-target protection, keyring/trust/CSP membutuhkan slice terpisah.

## Evaluasi singkat

Akurasi 4/5 (real Windows fixture tests, opener tidak diluncurkan); kelengkapan 3/5 (residual eksplisit, bukan full audit remediation); kejelasan 4/5 (tabel fixed/residual); actionability 4/5 (command repeatable dan helper tersedia); keringkasan 4/5 (laporan memisahkan klaim dan bukti). Perbaikan berikutnya paling bernilai: full integration gates parent, replay canonical helper, kemudian CAS/handle-bound IO. Tidak mengklaim siap rilis sebelum gate parent.
