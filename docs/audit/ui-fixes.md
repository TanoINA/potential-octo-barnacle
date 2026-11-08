# Perbaikan UI terverifikasi — handoff integrasi

Laporan audit asal: `docs/audit/layer-ui-review.md`. Tahap awal read-only sudah selesai sebelum izin implementasi diberikan. Audit mencakup 26 komponen produksi, 18 file test komponen, CSS, util, dan wiring App/hook/native terkait. **Layout tetap source-derived; tidak ada UI live yang dijalankan worker ini.**

## Perbaikan yang sudah mendarat

| Finding | Perubahan konkret | Regresi |
|---|---|---|
| UI-02 | `/plan` tidak lagi ditelan dispatcher lokal; dikirim ke hook dengan goal/opsi run. Draft rejected tetap tersimpan. | Enter, Send, rejected draft |
| UI-05/06 | Queue menyimpan snapshot executionMode/Council/provider+model serta origin session ID. Dequeue hanya setelah sender accepted; rejection/throw mempertahankan item untuk Resume. Navigasi ke sesi idle tidak mengirim queue sesi lama. Review menunda dispatch, Stop menahan sampai Resume. Command lokal antrean memakai dispatcher lokal dan tidak membersihkan draft baru pengguna. | Pending acceptance, mode+Council, reject/retry, A→B navigation, review→idle local `/new`, queued `/model` menjaga draft baru, Stop/Resume |
| UI-03 mitigasi aman | FilesChangedCard tanpa provenance workspace/revision menjadi **review-only**: Accept, Revert, dan Commit disabled; guard handler menolak mutation; alasan tampil. Review artifact tetap bekerja. Semua receipt saat ini belum punya provenance sehingga mutation kartu dinonaktifkan untuk semuanya, termasuk yang baru. Git workspace sidebar tetap tersedia dan tidak diubah. | Active Git status tidak mengaktifkan mutation; klik disabled tidak mengirim IPC mutation, Review tetap memilih artifact |
| UI-04 | Context menu Edit memicu tombol inline editor pesan asli, bukan langsung memotong riwayat dan resend. | Real MessageRow + ContextMenu: nol resend sebelum Save & Submit, lalu teks hasil edit dikirim |
| UI-08 | Paste/Paste as Code menggunakan native prototype value setter sebelum input event agar React controlled state benar-benar berubah; fokus/selection dikembalikan ke input. | Controlled textarea + output state, kedua aksi paste |
| UI-15 | Split diff membaca ukuran hunk old/new; source `---flag`/`+++counter` dalam hunk dipertahankan; metadata, trailing blank, no-newline marker tidak menjadi baris palsu; pasangan deletion/addition tidak dipisah marker. | Tiga fixture parser termasuk multi-file dan zero-range hunk; virtualizer dimock untuk materialisasi row, bukan klaim geometri |
| UI-16 | TerminalOutput merekam sticky-bottom pada scroll sebelum append, lalu mengikuti chunk besar hanya saat sticky. | Simulasi geometri deterministik: bottom→chunk besar, scroll-up→tidak ditarik, kembali bottom→tail |
| UI-18 | MessageRow memakai segment branch hanya jika ada segment yang renderable; start failure di content tetap tampil saat placeholder segments kosong. | Error content + empty segment terlihat |
| Catatan native drop | Async drag-drop registration yang selesai sesudah unmount segera unlisten; callback mengabaikan event ketika disposed. | Register pending→unmount→resolve→unlisten tepat sekali |

## Bukti RED→GREEN

Seluruh fix di atas memiliki test yang ditulis dan dijalankan gagal dahulu sebelum perubahan source:

- ChatCanvas plan: **3 failed / 8 passed** → **11 passed**.
- Queue contract awal: **4 failed / 11 passed** → **15 passed**; tambahan queued local draft **1 failed / 16 passed** → **17 passed**. Stop/Resume sudah lulus dan menjadi guard tambahan.
- ContextMenu: edit benar-benar memanggil resend sebelum perbaikan; controlled paste state tetap `draft`. Setelah fix: **15 passed**. Query label Paste diperbaiki untuk memperhitungkan accessible shortcut; rerun RED khusus memastikan dua paste tetap gagal karena state, bukan query typo.
- Split diff: **2 failed / 1 passed** → **3 passed**. Matcher teks source disesuaikan setelah word-highlight memecah span; gejala awal hilangnya baris tetap terbukti dari output RED.
- Terminal tail: **1 failed** (`800` bukan `1600`) → **1 passed**.
- MessageRow error: **1 failed / 8 passed** → **9 passed**.
- Native drop cleanup: **1 failed / 2 passed** → **3 passed**.
- Receipt safety: **1 failed / 2 passed** → **3 passed**; test tidak dihapus saat central suite parent kebetulan berjalan di tengah edit.

### Gate fokus terakhir (exit 0)

```sh
npm test -- src/components/ChatCanvas.commands.test.tsx src/components/ChatCanvas.dropPaste.test.tsx src/components/ChatCanvas.reflection.test.tsx src/components/ContextMenu.test.tsx src/components/FilesChangedCard.test.tsx src/components/MessageRow.test.tsx src/components/SideBySideDiff.test.tsx src/components/TerminalOutput.test.tsx
```

Hasil nyata: **8 file passed, 53 tests passed**, durasi 47,35 s. `git diff --check` juga exit 0. Ada warning Git LF→CRLF dari worktree Windows, bukan test failure. Tidak menjalankan full suite, build/native build, atau commit; gate integrasi menjadi milik parent.

## File milik worker ini

Source yang diubah:
- `src/components/ChatCanvas.tsx`
- `src/components/ContextMenu.tsx`
- `src/components/FilesChangedCard.tsx`
- `src/components/MessageRow.tsx`
- `src/components/SideBySideDiff.tsx`
- `src/components/TerminalOutput.tsx`

Test yang diubah:
- `src/components/ChatCanvas.commands.test.tsx`
- `src/components/ChatCanvas.dropPaste.test.tsx`
- `src/components/ContextMenu.test.tsx`
- `src/components/FilesChangedCard.test.tsx`
- `src/components/MessageRow.test.tsx`

Test baru:
- `src/components/SideBySideDiff.test.tsx`
- `src/components/TerminalOutput.test.tsx`

Dokumen baru: `docs/audit/layer-ui-review.md`, `docs/audit/ui-fixes.md` ini.

Tidak mengubah App, hooks, shared types, backend, CSS, atau utils. Worktree lain berubah paralel oleh worker/parent; jangan atribusikan perubahan itu ke handoff UI ini. File bukti tools milik parent tidak disentuh.

## Residual eksplisit / integrasi berikutnya

1. **Safety enforcement UI-01** tetap membutuhkan backend approval gate; queue kini menjaga mode yang dipilih tetapi itu tidak mengubah prompt-only policy menjadi gate.
2. **Provenance Git:** mutation receipt sengaja disabled sampai workspace/revision berasal dari metadata pesan yang persisted dan command native memvalidasi expected workspace. Mengirim workspace view saat ini ke MessageRow tidak menyelesaikan receipt lama. Logic mutation lama masih guarded agar patch tetap minimum; jangan sekadar mengubah canMutate menjadi true tanpa kontrak end-to-end.
3. **Memory workspace change UI-07:** belum diubah worker ini. Parent memperbaiki App modal shortcut guard secara terpisah; Memory read/save masih perlu origin workspace binding. Hook multi-session routing juga dikerjakan worker lain.
4. **Queue batas scope:** queue sesi lama dibuang saat navigasi (perilaku semula dipertahankan), bukan disimpan per sesi. Tidak ada cross-session accidental dispatch. Item run menjaga options snapshot; command lokal yang mempunyai operasi async internal masih mengikuti acceptance/feedback dispatcher lokal yang lama. Queue `/retry`/`tasks` dan jalur prompt dari Spotlight/ContextMenu masih perlu konsolidasi opsi run di integrasi berikutnya. `/undo` memang sengaja mengisi composer; draft preservation test saat ini untuk command tanpa output composer.
5. **UI-09/10/11/13/14/17:** trace error/result transparency, task bubble inspector, shell artifact auto-switch, total-height composer, context-menu height/keyboard capture, library adapter swallowing errors belum dikerjakan worker ini. Parent memiliki App shell minimum-width overlay/alert/modal guard, bukan worker UI.
6. Hook-order plan branch, markdown code fence preservation, clipboard lifecycle tambahan, speech unsupported UX, plan streaming state: tetap observasi audit yang belum diperbaiki; tidak diklaim selesai.
7. **Verifikasi live:** test terminal memakai geometri stub dan diff memakai virtualizer stub; tidak membuktikan flex/clipping/virtual scroll native. Parent melaporkan browser startup bermasalah dan mencoba jalur native; worker ini tidak membuat klaim hasil UI live.

Scope dihentikan setelah slice test hijau sesuai permintaan bounded finish. Parent perlu rerun full matrix sesudah seluruh worker selesai; tidak ada proses background milik worker ini.
